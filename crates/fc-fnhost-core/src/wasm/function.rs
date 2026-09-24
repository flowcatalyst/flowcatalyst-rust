//! One loaded version of a component function, and how an invocation runs
//! it (Java `fnhost/wasm/WasmFunction.java`, for `wasi:http`).
//!
//! **Instance per request.** Every invocation instantiates the version's
//! `ProxyPre` into a fresh store (a pooled slot: microseconds) and drops it
//! afterwards. That is the `wasi:http` model (`wasmtime serve` does the
//! same), and it means no state leaks from one call to the next, and a
//! trapped or interrupted instance never needs discarding. Java instead
//! pools instances per version and reuses them; a component function must
//! not rely on globals surviving between calls.
//!
//! **The call.** The listener's [`InvocationContext`] becomes a
//! `wasi:http` incoming request (method, path and raw query, headers,
//! body, the original `Host` as authority). The handler runs in its own
//! task on the guest runtime; this future awaits the response-outparam,
//! then buffers the body (capped at `limits.wasmMemoryMb`), then waits for
//! the handler to return. Running the handler in its own task and awaiting
//! the outparam, rather than polling both in one future, is the F0
//! spike's lesson: guests that finish their body only as the handler
//! returns need it.
//!
//! **Outcomes.** A response is the function's answer, verbatim. A trap
//! (including an allocation past the memory cap), a response the guest
//! never set, an error-code response, or a response body past the cap is
//! Java's `500 {"error":"the function failed"}`; the detail goes only to
//! the host's WARN line. The deadline (the endpoint's `timeoutMs`) stops the
//! guest wherever it is and is [`InvokeError::Timeout`] (504). No free pool
//! slot is [`InvokeError::Unavailable`] (503).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use bytes::Bytes;
use fc_function_abi::{MultiMap, Response};
use http::header::{HeaderName, HeaderValue};
use http_body_util::{BodyExt, Full};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use wasmtime::component::ResourceTable;
use wasmtime::{Store, StoreLimitsBuilder, Trap, UpdateDeadline};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi_http::p2::bindings::http::types::{ErrorCode, Scheme};
use wasmtime_wasi_http::p2::bindings::ProxyPre;
use wasmtime_wasi_http::p2::body::HyperOutgoingBody;
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

use super::guest::{DenyOutbound, FunctionShared, GuestState, InvocationData};
use super::output::GuestOutput;
use super::WasmRuntime;
use crate::invoke::{InvocationContext, InvokeError, Invoker};
use crate::loader::FunctionInstance;

pub struct WasmFunction {
    runtime: Arc<WasmRuntime>,
    pre: ProxyPre<GuestState>,
    shared: Arc<FunctionShared>,
    closed: AtomicBool,
}

impl WasmFunction {
    pub(super) fn new(
        runtime: Arc<WasmRuntime>,
        pre: ProxyPre<GuestState>,
        shared: Arc<FunctionShared>,
    ) -> Self {
        Self {
            runtime,
            pre,
            shared,
            closed: AtomicBool::new(false),
        }
    }

    fn state(
        &self,
        context: &InvocationContext,
        stdout: &GuestOutput,
        stderr: &GuestOutput,
    ) -> GuestState {
        let mut wasi = WasiCtxBuilder::new();
        wasi.stdout(stdout.clone())
            .stderr(stderr.clone())
            .allow_tcp(false)
            .allow_udp(false)
            .allow_ip_name_lookup(false);
        GuestState {
            wasi: wasi.build(),
            http: WasiHttpCtx::new(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(self.shared.memory_limit)
                .build(),
            hooks: DenyOutbound,
            function: self.shared.clone(),
            invocation: InvocationData::from(context),
        }
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
            address = %self.shared.address,
            version = self.shared.version,
            reason,
            detail = %detail,
            "wasm function call failed"
        );
    }

    fn failed(&self, reason: &str, detail: &str) -> Result<Response, InvokeError> {
        self.warn(reason, detail);
        Ok(Response::function_failed())
    }
}

/// How the guest task ended.
enum GuestEnd {
    /// The handler returned.
    Returned,
    /// The handler trapped (a panic, `unreachable`, a failed allocation…).
    Trapped(String),
    /// The epoch callback stopped it at the deadline.
    Interrupted,
    /// Dropped mid-run: the invocation was cancelled or timed out.
    Stopped,
    /// No pool slot free.
    NoInstance(String),
    /// Instantiation or request setup failed.
    Failed(String),
}

type Head = Result<hyper::Response<HyperOutgoingBody>, ErrorCode>;
type Outparam = tokio::sync::oneshot::Receiver<Head>;

async fn run_guest(
    engine: wasmtime::Engine,
    pre: ProxyPre<GuestState>,
    state: GuestState,
    request: hyper::Request<Full<Bytes>>,
    outparam: tokio::sync::oneshot::Sender<Head>,
    deadline: Instant,
    stop: CancellationToken,
) -> GuestEnd {
    let mut store = Store::new(&engine, state);
    store.limiter(|state| &mut state.limits);
    // Every tick: stop at the deadline, else yield so other guests (and
    // other tasks on the guest runtime) get the thread.
    store.set_epoch_deadline(1);
    store.epoch_deadline_callback(move |_| {
        if stop.is_cancelled() || Instant::now() >= deadline {
            Ok(UpdateDeadline::Interrupt)
        } else {
            Ok(UpdateDeadline::Yield(1))
        }
    });
    let proxy = match pre.instantiate_async(&mut store).await {
        Ok(proxy) => proxy,
        Err(e)
            if e.downcast_ref::<wasmtime::PoolConcurrencyLimitError>()
                .is_some() =>
        {
            return GuestEnd::NoInstance(format!("{e:#}"))
        }
        Err(e) if is_interrupt(&e) => return GuestEnd::Interrupted,
        Err(e) => return GuestEnd::Failed(format!("instantiation failed: {e:#}")),
    };
    let body = request.map(|b| {
        b.map_err(|never: std::convert::Infallible| -> wasmtime_wasi_http::Error { match never {} })
    });
    let incoming = match store
        .data_mut()
        .http()
        .new_incoming_request(Scheme::Http, body)
    {
        Ok(incoming) => incoming,
        Err(e) => return GuestEnd::Failed(format!("the request could not be handed over: {e:#}")),
    };
    let out = match store.data_mut().http().new_response_outparam(outparam) {
        Ok(out) => out,
        Err(e) => return GuestEnd::Failed(format!("no response outparam: {e:#}")),
    };
    let result = proxy
        .wasi_http_incoming_handler()
        .call_handle(&mut store, incoming, out)
        .await;
    match result {
        Ok(()) => GuestEnd::Returned,
        Err(e) if is_interrupt(&e) => GuestEnd::Interrupted,
        Err(e) => GuestEnd::Trapped(format!("{e:#}")),
    }
}

fn is_interrupt(e: &wasmtime::Error) -> bool {
    e.downcast_ref::<Trap>() == Some(&Trap::Interrupt)
}

#[async_trait]
impl Invoker for WasmFunction {
    async fn invoke(&self, context: InvocationContext) -> Result<Response, InvokeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(InvokeError::Unavailable("the version is closed".into()));
        }
        let request = match to_request(&context) {
            Ok(request) => request,
            Err(why) => return self.failed("bad_request", &why),
        };
        let deadline = context.deadline;
        let stop = context.interrupted.child_token();
        let stdout = GuestOutput::new(self.shared.logger.clone(), tracing::Level::INFO);
        let stderr = GuestOutput::new(self.shared.logger.clone(), tracing::Level::WARN);
        let state = self.state(&context, &stdout, &stderr);
        let (sender, outparam) = tokio::sync::oneshot::channel();
        let guest = {
            let stop = stop.clone();
            let run = run_guest(
                self.runtime.engine.clone(),
                self.pre.clone(),
                state,
                request,
                sender,
                deadline,
                stop.clone(),
            );
            self.runtime.spawn(
                async move {
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => GuestEnd::Stopped,
                        end = run => end,
                    }
                }
                .instrument(tracing::Span::current()),
            )
        };
        let mut guest = GuestTask {
            handle: Some(guest),
            end: None,
            stop,
        };
        let result = self.drive(&mut guest, outparam, deadline).await;
        if matches!(result, Driven::Failed(..)) {
            // The answer is decided; nothing the guest does now changes it.
            guest.stop.cancel();
        }
        // Whatever happened, the guest is over before the call is: the
        // listener's permits are held until this future returns.
        let end = guest.finish_by(deadline).await;
        stdout.flush();
        stderr.flush();
        match result {
            Driven::Answer(response) => {
                if let Some(GuestEnd::Trapped(detail)) = &end {
                    // The response was complete before the trap: it stands.
                    self.warn("trap_after_response", detail);
                }
                if matches!(end, Some(GuestEnd::Interrupted) | Some(GuestEnd::Stopped)) {
                    return Err(InvokeError::Timeout);
                }
                Ok(response)
            }
            Driven::Timeout => Err(InvokeError::Timeout),
            Driven::Failed(reason, detail) => self.failed(reason, &detail),
            Driven::NoResponse => match end {
                Some(GuestEnd::Interrupted) | Some(GuestEnd::Stopped) => Err(InvokeError::Timeout),
                Some(GuestEnd::NoInstance(detail)) => {
                    self.warn("no_instance", &detail);
                    Err(InvokeError::Unavailable(detail))
                }
                Some(GuestEnd::Trapped(detail)) => self.failed("trap", &detail),
                Some(GuestEnd::Failed(detail)) => self.failed("instantiation_failed", &detail),
                Some(GuestEnd::Returned) => self.failed(
                    "no_response",
                    "the handler returned without setting a response",
                ),
                None => self.failed("host_panic", "the guest task panicked"),
            },
        }
    }
}

/// What the caller-facing half saw.
enum Driven {
    Answer(Response),
    Timeout,
    Failed(&'static str, String),
    /// The outparam was dropped unset: the guest task's end explains why.
    NoResponse,
}

impl WasmFunction {
    async fn drive(&self, guest: &mut GuestTask, outparam: Outparam, deadline: Instant) -> Driven {
        let deadline = tokio::time::Instant::from_std(deadline);
        let head = match tokio::time::timeout_at(deadline, outparam).await {
            Err(_) => {
                guest.stop.cancel();
                return Driven::Timeout;
            }
            Ok(Err(_)) => return Driven::NoResponse,
            Ok(Ok(Err(code))) => {
                return Driven::Failed("error_response", format!("the guest answered {code:?}"))
            }
            Ok(Ok(Ok(head))) => head,
        };
        let (parts, body) = head.into_parts();
        let cap = self.shared.response_cap;
        let body = match tokio::time::timeout_at(deadline, collect_capped(body, cap)).await {
            Err(_) => {
                guest.stop.cancel();
                return Driven::Timeout;
            }
            Ok(Err(why)) => return Driven::Failed("response_body", why),
            Ok(Ok(body)) => body,
        };
        // The handler must return before the call ends; the deadline still
        // applies.
        if tokio::time::timeout_at(deadline, guest.wait())
            .await
            .is_err()
        {
            guest.stop.cancel();
            return Driven::Timeout;
        }
        let mut headers = MultiMap::new();
        for (name, value) in &parts.headers {
            headers
                .entry(name.as_str().to_owned())
                .or_default()
                .push(value.as_bytes().iter().map(|&b| b as char).collect());
        }
        match Response::http(parts.status.as_u16(), headers, body) {
            Ok(response) => Driven::Answer(response),
            Err(e) => Driven::Failed("bad_status", e.to_string()),
        }
    }
}

/// The spawned guest; `finish_by` always waits for it to be gone.
struct GuestTask {
    handle: Option<tokio::task::JoinHandle<GuestEnd>>,
    /// Set once the task is over (`None` inside: it panicked).
    end: Option<Option<GuestEnd>>,
    stop: CancellationToken,
}

impl GuestTask {
    /// Waits for the task to end. Cancel-safe: dropped mid-wait, the handle
    /// stays here for `finish_by`.
    async fn wait(&mut self) {
        if let Some(handle) = self.handle.as_mut() {
            let end = handle.await;
            self.handle = None;
            self.end = Some(end.ok());
        }
    }

    /// Waits for the task to end, stopping it at `deadline` if it has not.
    async fn finish_by(&mut self, deadline: Instant) -> Option<GuestEnd> {
        let deadline = tokio::time::Instant::from_std(deadline);
        if tokio::time::timeout_at(deadline, self.wait())
            .await
            .is_err()
        {
            self.stop.cancel();
            self.wait().await;
        }
        self.end.take().flatten()
    }
}

impl Drop for GuestTask {
    /// If the invocation future is dropped early, the guest must not run on.
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.stop.cancel();
        }
    }
}

/// The body, whole, or an error once it passes `cap` bytes.
async fn collect_capped<B>(mut body: B, cap: usize) -> Result<Vec<u8>, String>
where
    B: hyper::body::Body<Data = Bytes> + Unpin,
    B::Error: std::fmt::Debug,
{
    let mut out = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|e| format!("the response body failed: {e:?}"))?;
        if let Ok(data) = frame.into_data() {
            if out.len() + data.len() > cap {
                return Err(format!("the response body is over the {cap}-byte cap"));
            }
            out.extend_from_slice(&data);
        }
    }
    Ok(out)
}

/// The listener's request as a `wasi:http` incoming request. Header values
/// arrive as ISO-8859-1 text (one char per received byte) and go back to
/// those bytes; a name or value that is not legal on the wire is dropped.
fn to_request(c: &InvocationContext) -> Result<hyper::Request<Full<Bytes>>, String> {
    let mut path_and_query = c.path.clone();
    if let Some(query) = &c.raw_query {
        path_and_query.push('?');
        path_and_query.push_str(query);
    }
    let authority = c
        .original_host
        .as_deref()
        .filter(|h| h.parse::<http::uri::Authority>().is_ok())
        .unwrap_or("localhost");
    let uri = http::Uri::builder()
        .scheme("http")
        .authority(authority)
        .path_and_query(path_and_query)
        .build()
        .map_err(|e| format!("the request URI is not legal: {e}"))?;
    let method = http::Method::from_bytes(c.method.as_bytes())
        .map_err(|e| format!("the method is not legal: {e}"))?;
    let mut request = hyper::Request::builder()
        .method(method)
        .uri(uri)
        .body(Full::new(c.body.clone()))
        .map_err(|e| format!("the request is not legal: {e}"))?;
    let headers = request.headers_mut();
    for (name, values) in &c.headers {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        for value in values {
            let bytes: Option<Vec<u8>> = value
                .chars()
                .map(|ch| u8::try_from(ch as u32).ok())
                .collect();
            if let Some(value) = bytes.and_then(|b| HeaderValue::from_bytes(&b).ok()) {
                headers.append(name.clone(), value);
            }
        }
    }
    Ok(request)
}

#[async_trait]
impl FunctionInstance for WasmFunction {
    async fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
