//! One loaded version of a JS function, and how an invocation runs it.
//!
//! **Isolate per request.** Every invocation gets an isolate created from
//! the process's base snapshot, which loads the bundle from its code cache
//! and runs the top-level code, and is dropped afterwards, as a component
//! function gets a fresh instance: no state leaks from one call to the
//! next, and a stopped isolate never needs discarding.
//!
//! **The call.** The listener's [`InvocationContext`] becomes a `Request`
//! (method, `http://<original host><path>?<raw query>`, headers, body) for
//! the entrypoint; the `Response` it returns (or resolves to) is the
//! function's answer: status, headers (each value of a repeated header on
//! its own line), body. Bodies are buffered; the response's is capped at
//! `limits.wasmMemoryMb`.
//!
//! **Outcomes**, as the WASM runtime's:
//! - a `Response` is passed through verbatim;
//! - a throw or rejection, a value that is not a `Response`, a body over
//!   the cap, running out of memory, or a promise that can never settle is
//!   Java's `500 {"error":"the function failed"}`, the detail only on the
//!   host's WARN line;
//! - the deadline (the endpoint's `timeoutMs`) stops the isolate wherever
//!   it is, including queued for an executing permit:
//!   [`InvokeError::Timeout`] (504), and the call's permits are held until
//!   the isolate has stopped running JavaScript. (Its teardown, a bounded
//!   fraction of a millisecond, happens after the answer.)
//!
//! **Executing.** An isolate runs JavaScript only while it holds one of the
//! host's `FC_FN_MAX_EXECUTING` executing permits, which the WASM guests
//! share; see [`run`].

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use fc_fnhost_core::exec::{ExecBudget, Lane, Stopped};
use fc_fnhost_core::invoke::{InvocationContext, InvokeError, Invoker};
use fc_fnhost_core::loader::FunctionInstance;
use fc_function_abi::{MultiMap, Response};
use parking_lot::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::engine::Workers;
use crate::isolate::{Isolate, Limits, Terminator};
use crate::modules::{FunctionModules, VersionCode};
use crate::ops::{ContextOut, HostState, InvocationState, VersionShared};

pub struct JsFunction {
    workers: Arc<Workers>,
    base: Option<&'static [u8]>,
    version: Arc<VersionShared>,
    code: VersionCode,
    limits: Limits,
    closed: AtomicBool,
}

impl JsFunction {
    pub(crate) fn new(
        workers: Arc<Workers>,
        base: Option<&'static [u8]>,
        version: Arc<VersionShared>,
        code: VersionCode,
        limits: Limits,
    ) -> Self {
        Self {
            workers,
            base,
            version,
            code,
            limits,
            closed: AtomicBool::new(false),
        }
    }

    /// What this version holds while loaded: the bundle, its main module
    /// and its code cache.
    pub fn held_bytes(&self) -> usize {
        self.code.held_bytes()
    }

    /// Java `WasmFunction.warn`: the reason and a bounded detail, never to
    /// the caller.
    fn warn(&self, reason: &str, detail: &str) {
        let detail: String = if detail.len() > 1024 {
            let mut end = 1024;
            while !detail.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &detail[..end])
        } else {
            detail.to_owned()
        };
        tracing::warn!(
            address = %self.version.address,
            version = self.version.version,
            reason,
            detail = %detail,
            "js function call failed"
        );
    }

    fn failed(&self, reason: &str, detail: &str) -> Result<Response, InvokeError> {
        self.warn(reason, detail);
        Ok(Response::function_failed())
    }
}

/// The request as the dispatcher takes it.
struct RequestParts {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: bytes::Bytes,
}

impl RequestParts {
    fn from(c: &InvocationContext) -> Self {
        let authority = c
            .original_host
            .as_deref()
            .filter(|h| h.parse::<http::uri::Authority>().is_ok())
            .unwrap_or("localhost");
        let mut url = format!("http://{authority}{}", c.path);
        if let Some(query) = &c.raw_query {
            url.push('?');
            url.push_str(query);
        }
        let headers = c
            .headers
            .iter()
            .flat_map(|(name, values)| values.iter().map(move |v| (name.clone(), v.clone())))
            .collect();
        Self {
            method: c.method.clone(),
            url,
            headers,
            body: c.body.clone(),
        }
    }
}

/// How the worker's run ended.
enum Outcome {
    Answer(u16, Vec<(String, String)>, Vec<u8>),
    /// The handler threw, rejected, or answered something that is not a
    /// `Response`.
    Threw(String),
    OutOfMemory(String),
    Timeout,
    /// The isolate did not start.
    Failed(String),
}

/// Where the watchdog finds the running isolate.
type Slot = Arc<Mutex<Option<Terminator>>>;

struct Job {
    /// The invocation's usage meter: the isolate's peak memory (JS is not
    /// fuel-metered).
    usage: fc_fnhost_core::invoke::UsageMeter,
    base: Option<&'static [u8]>,
    code: VersionCode,
    limits: Limits,
    version: Arc<VersionShared>,
    invocation: InvocationState,
    request: RequestParts,
    stop: CancellationToken,
    slot: Slot,
}

/// Runs the request, answers through `answer`, then tears the isolate down:
/// the caller has its response before the isolate's teardown.
///
/// **The executing budget.** Everything that runs V8 on the worker (making
/// the isolate, the top-level code, the call, the teardown) holds one of
/// the host's executing permits, shared with the WASM guests, but only
/// while its future is polled: each poll is one turn of the isolate's event
/// loop, which runs JavaScript until every task awaits an op or a timer,
/// then returns, and the permit goes back. So a function awaiting a
/// `fetch`, an emit or a timer holds no permit, and one computing holds it
/// until its next `await`. Before each turn the isolate takes its worker's
/// lane and then queues (FIFO, with the WASM guests) for a permit. Stopped
/// (the deadline, the listener's interrupt) while it queues, it answers a
/// timeout at once rather than when a permit frees.
async fn run(job: Job, answer: oneshot::Sender<Outcome>, budget: ExecBudget, lane: Lane) {
    let stop = job.stop.clone();
    let unfinished = match budget.run_on(&lane, run_isolate(job)).until(stop).await {
        Ok((outcome, isolate)) => {
            let _ = answer.send(outcome);
            let Some(isolate) = isolate else { return };
            budget.run_on(&lane, async move { drop(isolate) }).await;
            return;
        }
        Err(Stopped(unfinished)) => unfinished,
    };
    let _ = answer.send(Outcome::Timeout);
    // Dropping the unfinished run tears its isolate down, if it had one.
    budget.run_on(&lane, async move { drop(unfinished) }).await;
}

async fn run_isolate(job: Job) -> (Outcome, Option<Isolate>) {
    if job.stop.is_cancelled() {
        return (Outcome::Timeout, None);
    }
    let at_start = Rc::new(HostState {
        version: job.version.clone(),
        invocation: None,
    });
    let modules = FunctionModules::new(job.code);
    let mut isolate = match Isolate::from_base(job.base, job.limits, at_start, modules) {
        Ok(isolate) => isolate,
        Err(why) => return (Outcome::Failed(why), None),
    };
    *job.slot.lock() = Some(isolate.terminator());
    // The watchdog may have fired before the isolate was in the slot.
    if job.stop.is_cancelled() {
        job.slot.lock().take();
        return (Outcome::Timeout, Some(isolate));
    }
    let r = &job.request;
    let during = Rc::new(HostState {
        version: job.version,
        invocation: Some(job.invocation),
    });
    let result = tokio::select! {
        biased;
        _ = job.stop.cancelled() => None,
        result = async {
            isolate.start().await.map_err(|e| e.to_string())?;
            isolate.set_host(during);
            isolate.call(&r.method, &r.url, &r.headers, &r.body).await
        } => Some(result),
    };
    job.slot.lock().take();
    job.usage.observe_memory(isolate.memory_in_use());
    let out_of_memory = isolate.stops().out_of_memory.load(Ordering::Acquire);
    let stopped = isolate.stops().stopped.load(Ordering::Acquire) || job.stop.is_cancelled();
    let outcome = match result {
        _ if stopped && !out_of_memory => Outcome::Timeout,
        None => Outcome::Timeout,
        Some(Err(why)) if out_of_memory => Outcome::OutOfMemory(why),
        Some(Err(why)) => Outcome::Threw(why),
        Some(Ok(_)) if out_of_memory => {
            Outcome::OutOfMemory("the heap reached its limit after the response".into())
        }
        Some(Ok((status, headers, body))) => Outcome::Answer(status, headers, body),
    };
    (outcome, Some(isolate))
}

/// Stops the isolate if the invocation future is dropped early, or when
/// the watchdog fires.
struct Stopper {
    stop: CancellationToken,
    slot: Slot,
    /// Cleared once the isolate is gone.
    armed: bool,
}

impl Stopper {
    fn stop(&self) {
        self.stop.cancel();
        if let Some(terminator) = self.slot.lock().as_ref() {
            terminator.stop();
        }
    }
}

impl Drop for Stopper {
    fn drop(&mut self) {
        if self.armed {
            self.stop();
        }
    }
}

#[async_trait]
impl Invoker for JsFunction {
    async fn invoke(&self, context: InvocationContext) -> Result<Response, InvokeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(InvokeError::Unavailable("the version is closed".into()));
        }
        let stop = context.interrupted.child_token();
        let slot: Slot = Arc::new(Mutex::new(None));
        let (sender, answer) = oneshot::channel();
        let job = Job {
            usage: context.usage.clone(),
            base: self.base,
            code: self.code.clone(),
            limits: self.limits,
            version: self.version.clone(),
            invocation: InvocationState {
                context: ContextOut::from(&context),
                deadline: context.deadline,
                defaults: (context.correlation_id.clone(), context.causation_id.clone()),
            },
            request: RequestParts::from(&context),
            stop: stop.clone(),
            slot: slot.clone(),
        };
        let span = tracing::Span::current();
        let budget = self.workers.budget().clone();
        let sent = self.workers.run(move |lane| {
            async move {
                run(job, sender, budget, lane).await;
            }
            .instrument(span)
        });
        if !sent {
            return Err(InvokeError::Unavailable(
                "the JS runtime is shutting down".into(),
            ));
        }
        // Whatever ends the call first: the deadline, the listener's
        // interrupt, or this future being dropped (the `Stopper`).
        let mut stopper = Stopper {
            stop: stop.clone(),
            slot,
            armed: true,
        };
        let deadline = tokio::time::Instant::from_std(context.deadline);
        let mut answer = answer;
        let outcome = tokio::select! {
            outcome = &mut answer => outcome,
            _ = tokio::time::sleep_until(deadline) => {
                stopper.stop();
                answer.await
            }
            _ = stop.cancelled() => {
                stopper.stop();
                answer.await
            }
        };
        // The isolate is gone: nothing left to stop.
        stopper.armed = false;
        let Ok(outcome) = outcome else {
            return self.failed("host_panic", "the JS worker dropped the call");
        };
        match outcome {
            Outcome::Timeout => Err(InvokeError::Timeout),
            Outcome::Threw(detail) => self.failed("threw", &detail),
            Outcome::OutOfMemory(detail) => self.failed("out_of_memory", &detail),
            Outcome::Failed(detail) => self.failed("isolate_failed", &detail),
            Outcome::Answer(status, headers, body) => {
                if body.len() > self.version.body_cap {
                    return self.failed(
                        "response_body",
                        &format!(
                            "the response body is over the {}-byte cap",
                            self.version.body_cap
                        ),
                    );
                }
                let mut map = MultiMap::new();
                for (name, value) in headers {
                    map.entry(name).or_default().push(value);
                }
                match Response::http(status, map, body) {
                    Ok(response) => Ok(response),
                    Err(e) => self.failed("bad_status", &e.to_string()),
                }
            }
        }
    }
}

#[async_trait]
impl FunctionInstance for JsFunction {
    async fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
