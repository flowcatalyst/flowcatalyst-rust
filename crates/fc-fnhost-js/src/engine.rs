//! The one V8 platform and the worker threads every JS function shares.
//!
//! **Where functions run.** Not on the listener's tokio workers: on
//! `FC_FN_MAX_EXECUTING` threads of their own, each a current-thread tokio
//! runtime driving many isolates at once ([`crate::isolate`] explains how
//! they share a thread). A new call goes to the worker with the fewest
//! calls in flight, and one waiting on I/O holds no thread.
//!
//! **What caps execution** is not these threads but the host-wide
//! [`ExecBudget`]: `FC_FN_MAX_EXECUTING` permits that the JS isolates and
//! the WASM guests share, so a host running both kinds executes at most
//! that many guests at once, not that many of each. An isolate holds a
//! permit only while its future is polled, one event-loop turn at a time
//! (see [`crate::function`]); awaiting a `fetch`, an emit or a timer, it
//! holds none. Each worker is a [`Lane`]: an isolate takes its worker's
//! lane before it queues for a permit, so a permit is never handed to an
//! isolate whose thread is busy with another. There are
//! `FC_FN_MAX_EXECUTING` workers so that JS alone can use the whole budget;
//! more could never execute at once.
//!
//! **What a worker cannot do** that the WASM runtime's epoch ticks do:
//! preempt. JavaScript runs to its next `await`; a function that computes
//! for 200 ms holds its worker, and its permit, for 200 ms (the other calls
//! on that worker wait; the WASM guests and the other workers share what
//! permits are left). Its deadline still stops it (the watchdog terminates
//! the isolate from another thread).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once, OnceLock};

use deno_core::{JsRuntime, JsRuntimeForSnapshot, RuntimeOptions};

use fc_fnhost_core::exec::{ExecBudget, Lane};

use crate::ops::fc_function;
use crate::prepare::BOOTSTRAP;
use tokio::sync::mpsc;

/// A job: made on the caller's thread, run (as a `!Send` future) on a
/// worker.
type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// Starts V8, once per process, with deno_core's own (non-snapshotting)
/// flags and this crate's platform ([`crate::platform`]), and, when
/// [`use_snapshot`], builds the [`base_snapshot`] every isolate is made
/// from. (WebAssembly is removed from a function's globals by the bootstrap,
/// not by a flag.)
pub fn init_v8() -> Result<Option<&'static [u8]>, String> {
    static INIT: Once = Once::new();
    INIT.call_once(|| JsRuntime::init_platform(Some(crate::platform::new())));
    if use_snapshot() {
        base_snapshot().map(Some)
    } else {
        Ok(None)
    }
}

/// Whether isolates are made from the base snapshot: on Linux, unless
/// `FC_FN_JS_SNAPSHOT=false`; elsewhere only with `FC_FN_JS_SNAPSHOT=true`.
///
/// An isolate from the snapshot starts in about 0.7 ms, one without in about
/// 2.5 ms (deno_core's own start-up and the bootstrap script, every time).
/// On macOS arm64, creating and disposing thousands of isolates from a
/// snapshot aborted the process in most runs (`pointer being freed was not
/// allocated` in `BackingStore::~BackingStore`, from V8's heap teardown;
/// reproduced with deno_core alone and its own snapshot, never without a
/// snapshot, and never on Linux arm64 in 16 runs of 2,000 requests). Windows
/// is untested here, and deno_core itself serialises the first snapshot
/// deserialisation there for a crash of its own. Production hosts run Linux.
pub fn use_snapshot() -> bool {
    match std::env::var("FC_FN_JS_SNAPSHOT").ok().as_deref() {
        Some(v) if v.eq_ignore_ascii_case("true") => true,
        Some(v) if v.eq_ignore_ascii_case("false") => false,
        _ => cfg!(target_os = "linux"),
    }
}

/// The process's **base snapshot**: deno_core's own JavaScript plus the
/// bootstrap (`js/bootstrap.js`), built once, before any other isolate
/// exists; every function isolate is created from it. It is the only
/// snapshot the host ever makes: a snapshot creator writes the read-only
/// space V8 shares between all isolates of the process, and isolates
/// running meanwhile crash (seen: a request isolate allocating into the
/// read-only space, `StringForwardingTable` checks failing, and a
/// protection fault in `ReadOnlySpace::RepairFreeSpacesBeforeSerialization`
/// for a creator started from this very snapshot).
///
/// Before it is taken, [`WARM_UP`] runs one request through the dispatcher,
/// so the bootstrap's `Request`, `Response`, `Headers`, `URL` and friends
/// are compiled and their bytecode is in the snapshot (deno_core keeps
/// function code): a request's isolate does not parse them again.
pub fn base_snapshot() -> Result<&'static [u8], String> {
    static BASE: OnceLock<Result<Box<[u8]>, String>> = OnceLock::new();
    BASE.get_or_init(|| {
        // On a thread of its own: the snapshot creator is entered on it for
        // its whole life, and leaves nothing entered behind.
        std::thread::Builder::new()
            .name("fn-js-base".into())
            .stack_size(WORKER_STACK_BYTES)
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| format!("no runtime for the base snapshot: {e}"))?;
                let mut js = JsRuntimeForSnapshot::try_new(RuntimeOptions {
                    extensions: vec![fc_function::init()],
                    ..Default::default()
                })
                .map_err(|e| format!("the base snapshot's isolate did not start: {e}"))?;
                js.execute_script("[fc:bootstrap]", BOOTSTRAP.to_owned())
                    .map_err(|e| format!("the bootstrap failed: {e}"))?;
                let warmed = js
                    .execute_script("[fc:warm-up]", WARM_UP.to_owned())
                    .map_err(|e| format!("the warm-up failed: {e}"))?;
                let settled = js.resolve(warmed);
                runtime
                    .block_on(js.with_event_loop_promise(
                        Box::pin(settled),
                        deno_core::PollEventLoopOptions::default(),
                    ))
                    .map_err(|e| format!("the warm-up failed: {e}"))?;
                // The creator's foreground tasks go while it still exists
                // (see `crate::platform`).
                // SAFETY: only the pointer's value is used, as a key.
                let key = crate::platform::key(unsafe { js.v8_isolate().as_raw_isolate_ptr() });
                crate::platform::close(key);
                let snapshot = js.snapshot();
                crate::platform::forget(key);
                Ok(snapshot)
            })
            .map_err(|e| format!("no thread for the base snapshot: {e}"))?
            .join()
            .unwrap_or_else(|_| Err("building the base snapshot panicked".into()))
    })
    .as_ref()
    .map(|snapshot| &**snapshot)
    .map_err(Clone::clone)
}

/// One request through the dispatcher and the web subset, before the base
/// snapshot is taken (see [`base_snapshot`]). No host API: there is no
/// function behind it.
const WARM_UP: &str = r#"
(async () => {
  const invoke = globalThis.__fcHost.dispatcher({
    default: async (request) => {
      const url = new URL(request.url);
      url.searchParams.get("q");
      request.headers.get("content-type");
      const body = request.method === "POST" ? await request.json() : await request.text();
      const headers = new Headers({ "x-a": "b" });
      headers.append("set-cookie", "a=1");
      return Response.json({ path: url.pathname, body }, { status: 201, headers });
    },
  }, "default");
  await invoke("POST", "http://localhost/warm/up?q=1", [["content-type", "application/json"]],
    new TextEncoder().encode(JSON.stringify({ n: 1 })));
  await invoke("GET", "http://localhost/", [], new Uint8Array(0));
  new Response("text").text();
  new Request("http://localhost/", { method: "POST", body: "x" }).arrayBuffer();
  new TextDecoder().decode(new Uint8Array([104, 105]));
  btoa("hi");
  atob("aGk=");
  new URLSearchParams("a=1&b=2").toString();
})()
"#;

struct Worker {
    jobs: mpsc::UnboundedSender<Job>,
    load: Arc<AtomicUsize>,
    /// Which of this worker's isolates may run or queue for a permit.
    lane: Lane,
}

pub struct Workers {
    workers: Vec<Worker>,
    next: AtomicUsize,
    budget: ExecBudget,
}

/// A worker's stack: V8 limits its own stack use to about 1 MiB, and the
/// event loop runs on top of it.
const WORKER_STACK_BYTES: usize = 8 << 20;

impl Workers {
    /// `count` workers whose isolates execute on `budget`'s permits.
    pub fn start(count: usize, budget: ExecBudget) -> Result<Self, String> {
        let mut workers = Vec::with_capacity(count.max(1));
        for i in 0..count.max(1) {
            let (jobs, mut queue) = mpsc::unbounded_channel::<Job>();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("a JS worker's runtime did not start: {e}"))?;
            std::thread::Builder::new()
                .name(format!("fn-js-{i}"))
                .stack_size(WORKER_STACK_BYTES)
                .spawn(move || {
                    let local = tokio::task::LocalSet::new();
                    local.block_on(&runtime, async move {
                        while let Some(job) = queue.recv().await {
                            tokio::task::spawn_local(job());
                        }
                    });
                })
                .map_err(|e| format!("a JS worker did not start: {e}"))?;
            workers.push(Worker {
                jobs,
                load: Arc::new(AtomicUsize::new(0)),
                lane: Lane::new(),
            });
        }
        Ok(Self {
            workers,
            next: AtomicUsize::new(0),
            budget,
        })
    }

    /// The executing permits the isolates share with the host's other
    /// runtimes.
    pub fn budget(&self) -> &ExecBudget {
        &self.budget
    }

    pub fn len(&self) -> usize {
        self.workers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.workers.is_empty()
    }

    /// Runs `make(lane)`'s future on the least-loaded worker (ties go round
    /// robin); `lane` is that worker's, for metering the future on
    /// ([`ExecBudget::run_on`]). `false` when the workers are gone (the
    /// engine is shutting down).
    pub fn run<F, Fut>(&self, make: F) -> bool
    where
        F: FnOnce(Lane) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let start = self.next.fetch_add(1, Ordering::Relaxed);
        let n = self.workers.len();
        let worker = (0..n)
            .map(|i| &self.workers[(start + i) % n])
            .min_by_key(|w| w.load.load(Ordering::Relaxed))
            .expect("at least one worker");
        let load = worker.load.clone();
        let lane = worker.lane.clone();
        load.fetch_add(1, Ordering::Relaxed);
        let job: Job = Box::new(move || {
            Box::pin(async move {
                struct Done(Arc<AtomicUsize>);
                impl Drop for Done {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::Relaxed);
                    }
                }
                let _done = Done(load);
                make(lane).await;
            })
        });
        worker.jobs.send(job).is_ok()
    }
}
