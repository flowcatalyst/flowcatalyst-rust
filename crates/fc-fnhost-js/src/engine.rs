//! The one V8 platform and the worker threads every JS function shares.
//!
//! **Where functions run.** Not on the listener's tokio workers: on
//! `FC_FN_MAX_EXECUTING` threads of their own (the variable the WASM
//! runtime sizes its guest runtime with), each a current-thread tokio
//! runtime driving many isolates at once ([`crate::isolate`] explains how
//! they share a thread). So at most that many functions execute JavaScript
//! at any moment, and one waiting on I/O holds no thread. A new call goes
//! to the worker with the fewest calls in flight.
//!
//! **What a worker cannot do** that the WASM runtime's epoch ticks do:
//! preempt. JavaScript runs to its next `await`; a function that computes
//! for 200 ms holds its worker, and the other calls on that worker, for
//! 200 ms. Its deadline still stops it (the watchdog terminates the
//! isolate from another thread).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once, OnceLock};

use deno_core::{JsRuntime, JsRuntimeForSnapshot, RuntimeOptions};

use crate::ops::fc_function;
use crate::prepare::BOOTSTRAP;
use tokio::sync::mpsc;

/// A job: made on the caller's thread, run (as a `!Send` future) on a
/// worker.
type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// Starts V8, once per process, with deno_core's own (non-snapshotting)
/// flags, and builds the [`base_snapshot`]. (WebAssembly is removed from a
/// function's globals by the bootstrap, not by a flag.)
pub fn init_v8() -> Result<&'static [u8], String> {
    static INIT: Once = Once::new();
    INIT.call_once(|| JsRuntime::init_platform(None));
    base_snapshot()
}

/// The process's **base snapshot**: deno_core's own JavaScript plus the
/// bootstrap (`js/bootstrap.js`), built once, before any other isolate
/// exists. Every version's snapshot is created from it, never from
/// scratch: a snapshot creator that bootstraps a heap from scratch writes
/// the read-only space V8 shares between all isolates of the process, and
/// isolates running meanwhile crash (seen: a request isolate allocating
/// into the read-only space, and `StringForwardingTable` checks failing).
/// Created from the base, a snapshot creator only reads that space.
pub fn base_snapshot() -> Result<&'static [u8], String> {
    static BASE: OnceLock<Result<Box<[u8]>, String>> = OnceLock::new();
    BASE.get_or_init(|| {
        // On a thread of its own: the snapshot creator is entered on it for
        // its whole life, and leaves nothing entered behind.
        std::thread::Builder::new()
            .name("fn-js-base".into())
            .stack_size(WORKER_STACK_BYTES)
            .spawn(|| {
                let mut js = JsRuntimeForSnapshot::try_new(RuntimeOptions {
                    extensions: vec![fc_function::init()],
                    ..Default::default()
                })
                .map_err(|e| format!("the base snapshot's isolate did not start: {e}"))?;
                js.execute_script("[fc:bootstrap]", BOOTSTRAP.to_owned())
                    .map_err(|e| format!("the bootstrap failed: {e}"))?;
                Ok(js.snapshot())
            })
            .map_err(|e| format!("no thread for the base snapshot: {e}"))?
            .join()
            .unwrap_or_else(|_| Err("building the base snapshot panicked".into()))
    })
    .as_ref()
    .map(|snapshot| &**snapshot)
    .map_err(Clone::clone)
}

struct Worker {
    jobs: mpsc::UnboundedSender<Job>,
    load: Arc<AtomicUsize>,
}

pub struct Workers {
    workers: Vec<Worker>,
    next: AtomicUsize,
}

/// A worker's stack: V8 limits its own stack use to about 1 MiB, and the
/// event loop runs on top of it.
const WORKER_STACK_BYTES: usize = 8 << 20;

impl Workers {
    pub fn start(count: usize) -> Result<Self, String> {
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
            });
        }
        Ok(Self {
            workers,
            next: AtomicUsize::new(0),
        })
    }

    pub fn len(&self) -> usize {
        self.workers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.workers.is_empty()
    }

    /// Runs `make()`'s future on the least-loaded worker (ties go round
    /// robin). `false` when the workers are gone (the engine is shutting
    /// down).
    pub fn run<F, Fut>(&self, make: F) -> bool
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let start = self.next.fetch_add(1, Ordering::Relaxed);
        let n = self.workers.len();
        let worker = (0..n)
            .map(|i| &self.workers[(start + i) % n])
            .min_by_key(|w| w.load.load(Ordering::Relaxed))
            .expect("at least one worker");
        let load = worker.load.clone();
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
                make().await;
            })
        });
        worker.jobs.send(job).is_ok()
    }
}
