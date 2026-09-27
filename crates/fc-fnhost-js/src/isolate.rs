//! One isolate, created from the process's base snapshot for one request
//! and dropped after it (the `wasi:http` instance-per-request model): it
//! loads the version's bundle (from V8's code cache), runs its top-level
//! code, then handles the request.
//!
//! **Many isolates per thread.** A worker thread interleaves the isolates
//! of every request it holds, so one waiting on I/O (`fetch`, `emit`, a
//! timer) holds no thread. V8 requires an isolate to be *entered* on its
//! thread while it is used, and rusty_v8 enters an isolate when it is
//! created and exits it when it is dropped, which only works for strictly
//! nested lifetimes. So an [`Isolate`] is exited right after it is
//! created, entered around every use ([`Isolate::enter`], every poll of
//! the event loop) and entered again just before it is dropped. At any
//! moment at most one isolate is entered on a thread.
//!
//! **Limits.** The V8 heap is capped at the function's memory limit; as
//! the heap nears it, V8 calls back, the isolate's execution is terminated
//! and the call answers 500 (rather than V8 aborting the process). Its
//! `ArrayBuffer` storage has its own cap ([`crate::allocator`]). The
//! deadline is the watchdog's: [`Isolate::terminator`] stops running
//! JavaScript from another thread.

use std::future::Future;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::Poll;

use deno_core::{v8, JsRuntime, ModuleId, PollEventLoopOptions, RuntimeOptions};

use crate::allocator::{self, Budget};
use crate::modules::FunctionModules;
use crate::ops::{fc_function, HostState};

/// The smallest V8 heap an isolate gets, whatever the function's memory
/// limit: below it, restoring the snapshot alone would hit the limit.
pub const MIN_HEAP_BYTES: usize = 8 << 20;

/// What caps one isolate.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The V8 heap's maximum (at least [`MIN_HEAP_BYTES`]).
    pub heap_bytes: usize,
    /// All `ArrayBuffer` backing stores together.
    pub array_buffer_bytes: usize,
}

impl Limits {
    /// Both caps at `bytes` (the manifest's `limits.wasmMemoryMb`).
    pub fn of(bytes: usize) -> Self {
        Self {
            heap_bytes: bytes.max(MIN_HEAP_BYTES),
            array_buffer_bytes: bytes,
        }
    }

    pub fn create_params(&self, budget: &Arc<Budget>) -> v8::CreateParams {
        v8::CreateParams::default()
            .heap_limits(0, self.heap_bytes)
            .array_buffer_allocator(allocator::capped(budget))
    }
}

/// Why the isolate stopped, set from wherever it was stopped.
#[derive(Debug, Default)]
pub struct Stops {
    /// The heap reached its limit.
    pub out_of_memory: AtomicBool,
    /// The watchdog stopped it (deadline, or the call was abandoned).
    pub stopped: AtomicBool,
}

/// Stops an isolate's JavaScript from any thread.
#[derive(Clone)]
pub struct Terminator {
    handle: v8::IsolateHandle,
    stops: Arc<Stops>,
}

impl Terminator {
    /// Stops the isolate: running JavaScript throws an uncatchable
    /// termination, and the call's future sees [`Stops::stopped`].
    pub fn stop(&self) {
        self.stops.stopped.store(true, Ordering::Release);
        self.handle.terminate_execution();
    }
}

/// Enters an isolate for as long as it lives.
struct Entered(v8::Isolate);

impl Entered {
    fn new(raw: v8::UnsafeRawIsolatePtr) -> Self {
        // SAFETY: `raw` is the isolate an `Isolate` owns and keeps alive
        // for as long as this guard (a borrow of it) lives.
        let isolate = unsafe { v8::Isolate::from_raw_isolate_ptr(raw) };
        unsafe { isolate.enter() };
        Self(isolate)
    }
}

impl Drop for Entered {
    fn drop(&mut self) {
        // SAFETY: entered in `new`, on this thread.
        unsafe { self.0.exit() };
    }
}

pub struct Isolate {
    js: ManuallyDrop<JsRuntime>,
    raw: v8::UnsafeRawIsolatePtr,
    stops: Arc<Stops>,
    handle: v8::IsolateHandle,
    budget: Arc<Budget>,
    /// The main module, once [`Isolate::start`] has run it.
    main: Option<ModuleId>,
}

/// Why [`Isolate::start`] failed.
#[derive(Debug)]
pub enum StartError {
    /// The module graph did not load (an import refused, a syntax error).
    Load(String),
    /// The top-level code threw, or was stopped.
    Evaluate(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Load(why) | StartError::Evaluate(why) => f.write_str(why),
        }
    }
}

impl Isolate {
    /// An isolate from the process's base snapshot, loading modules through
    /// `modules`, with `host` in its op state. Returns exited.
    pub fn from_base(
        base: &'static [u8],
        limits: Limits,
        host: Rc<HostState>,
        modules: Rc<FunctionModules>,
    ) -> Result<Self, String> {
        let budget = Budget::new(limits.array_buffer_bytes);
        let mut js = JsRuntime::try_new(RuntimeOptions {
            extensions: vec![fc_function::init()],
            module_loader: Some(modules),
            startup_snapshot: Some(base),
            create_params: Some(limits.create_params(&budget)),
            ..Default::default()
        })
        .map_err(|e| format!("the isolate did not start: {e}"))?;
        let stops = Arc::new(Stops::default());
        let handle = js.v8_isolate().thread_safe_handle();
        {
            let stops = stops.clone();
            let handle = handle.clone();
            js.add_near_heap_limit_callback(move |current, _initial| {
                stops.out_of_memory.store(true, Ordering::Release);
                handle.terminate_execution();
                // Room for the termination to unwind; the isolate is
                // dropped right after.
                current.saturating_mul(2)
            });
        }
        js.op_state().borrow_mut().put(host);
        // `Atomics.wait` would block the worker thread (and every isolate
        // sharing it): off, it throws.
        js.v8_isolate().set_allow_atomics_wait(false);
        // SAFETY: the pointer is only used while `js` lives (the `Isolate`
        // owns both).
        let raw = unsafe { js.v8_isolate().as_raw_isolate_ptr() };
        // SAFETY: created entered, on this thread (see the module docs).
        unsafe { v8::Isolate::from_raw_isolate_ptr(raw).exit() };
        Ok(Isolate {
            js: ManuallyDrop::new(js),
            raw,
            stops,
            handle,
            budget,
            main: None,
        })
    }

    /// Loads the main module (the host's seal, then the bundle) and runs
    /// the top-level code, driving the event loop until it settles.
    pub async fn start(&mut self) -> Result<ModuleId, StartError> {
        let raw = self.raw;
        let specifier =
            deno_core::ModuleSpecifier::parse(crate::modules::MAIN).expect("a valid specifier");
        let id = {
            let js = &mut *self.js;
            let mut load = Box::pin(js.load_main_es_module(&specifier));
            std::future::poll_fn(|cx| {
                let _entered = Entered::new(raw);
                load.as_mut().poll(cx)
            })
            .await
            .map_err(|e| StartError::Load(e.to_string()))?
        };
        let evaluated = {
            let _entered = Entered::new(raw);
            Box::pin(self.js.mod_evaluate(id))
        };
        self.drive(evaluated).await.map_err(StartError::Evaluate)?;
        // A top-level throw settles the evaluation `Ok` and comes back as an
        // unhandled rejection, which the event loop reports.
        let raw = self.raw;
        let js = &mut *self.js;
        std::future::poll_fn(|cx| {
            let _entered = Entered::new(raw);
            match js.poll_event_loop(cx, PollEventLoopOptions::default()) {
                Poll::Ready(Err(e)) => Poll::Ready(Err(StartError::Evaluate(e.to_string()))),
                _ => Poll::Ready(Ok(())),
            }
        })
        .await?;
        self.main = Some(id);
        Ok(id)
    }

    /// Replaces the host state the ops see (the request's, once the
    /// top-level code has run without it).
    pub fn set_host(&mut self, host: Rc<HostState>) {
        self.js.op_state().borrow_mut().put(host);
    }

    /// Polls `future` and the event loop together, entered, until `future`
    /// is ready. An event loop that finishes first leaves `future` pending
    /// forever: an error.
    async fn drive<T, E: std::fmt::Display>(
        &mut self,
        mut future: std::pin::Pin<Box<impl Future<Output = Result<T, E>>>>,
    ) -> Result<T, String> {
        let raw = self.raw;
        let js = &mut *self.js;
        std::future::poll_fn(|cx| {
            let _entered = Entered::new(raw);
            if let Poll::Ready(result) = future.as_mut().poll(cx) {
                return Poll::Ready(result.map_err(|e| e.to_string()));
            }
            match js.poll_event_loop(cx, PollEventLoopOptions::default()) {
                Poll::Ready(Err(e)) => Poll::Ready(Err(e.to_string())),
                Poll::Ready(Ok(())) => match future.as_mut().poll(cx) {
                    Poll::Ready(result) => Poll::Ready(result.map_err(|e| e.to_string())),
                    Poll::Pending => Poll::Ready(Err(
                        "a promise never settled: nothing was left for it to wait on".into(),
                    )),
                },
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }

    pub fn terminator(&self) -> Terminator {
        Terminator {
            handle: self.handle.clone(),
            stops: self.stops.clone(),
        }
    }

    pub fn stops(&self) -> &Stops {
        &self.stops
    }

    /// `ArrayBuffer` bytes in use.
    pub fn array_buffer_bytes(&self) -> usize {
        self.budget.used()
    }

    /// Calls the main module's `invoke` with the request, and drives the
    /// event loop until its promise settles. `Ok` is the dispatcher's
    /// `[status, headers, body]`. [`Isolate::start`] must have run.
    pub async fn call(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<(u16, Vec<(String, String)>, Vec<u8>), String> {
        let raw = self.raw;
        let main = self.main.ok_or("the main module has not run")?;
        let promise = {
            let _entered = Entered::new(raw);
            let namespace = self
                .js
                .get_module_namespace(main)
                .map_err(|e| format!("the main module is missing: {e}"))?;
            deno_core::scope!(scope, &mut *self.js);
            let namespace = v8::Local::new(scope, namespace);
            let key = v8::String::new(scope, "invoke").expect("a short string");
            let invoke = namespace
                .get(scope, key.into())
                .and_then(|f| v8::Local::<v8::Function>::try_from(f).ok())
                .ok_or("the main module does not export invoke")?;
            let method = v8::String::new(scope, method).ok_or("the method is too long")?;
            let url = v8::String::new(scope, url).ok_or("the URL is too long")?;
            let headers = deno_core::serde_v8::to_v8(scope, headers)
                .map_err(|e| format!("the headers did not convert: {e}"))?;
            let store = v8::ArrayBuffer::new_backing_store_from_vec(body.to_vec()).make_shared();
            let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
            let body = v8::Uint8Array::new(scope, buffer, 0, body.len())
                .ok_or("the body did not convert")?;
            let undefined = v8::undefined(scope);
            let returned = invoke.call(
                scope,
                undefined.into(),
                &[method.into(), url.into(), headers, body.into()],
            );
            match returned {
                Some(value) => v8::Global::new(scope, value),
                None => return Err("the call threw before it returned a promise".into()),
            }
        };
        let settled = {
            let _entered = Entered::new(raw);
            Box::pin(self.js.resolve(promise))
        };
        let value = self.drive(settled).await?;
        let _entered = Entered::new(raw);
        deno_core::scope!(scope, &mut *self.js);
        let value = v8::Local::new(scope, value);
        let (status, headers, body): (u16, Vec<(String, String)>, deno_core::JsBuffer) =
            deno_core::serde_v8::from_v8(scope, value)
                .map_err(|e| format!("the dispatcher's answer did not convert: {e}"))?;
        Ok((status, headers, body.to_vec()))
    }
}

impl Drop for Isolate {
    fn drop(&mut self) {
        // Entered once more: dropping a rusty_v8 isolate exits it.
        // SAFETY: the isolate is alive (dropped just below) and on this
        // thread; nothing else is entered here now.
        unsafe { v8::Isolate::from_raw_isolate_ptr(self.raw).enter() };
        // SAFETY: dropped exactly once, here.
        unsafe { ManuallyDrop::drop(&mut self.js) };
    }
}
