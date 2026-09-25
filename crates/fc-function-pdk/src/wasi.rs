//! The real host: `wasi:http` for the request, the response and outbound
//! calls, and the `flowcatalyst:function` imports (feature `flowcatalyst`).
//!
//! Every import is called lazily, from the API that needs it, so a
//! component imports exactly what it uses: one that never reads its
//! invocation context does not import `flowcatalyst:function/invocation`.

use std::rc::Rc;
use std::time::{Duration, SystemTime};

use wasip2::http::outgoing_handler;
use wasip2::http::types::{
    ErrorCode, Fields, IncomingBody, IncomingRequest, Method, OutgoingBody, OutgoingRequest,
    RequestOptions, Scheme,
};
use wasip2::io::streams::{InputStream, OutputStream, StreamError};

use crate::backend::{Backend, HttpFuture};
use crate::context::Level;
use crate::http::{target, HttpCall, HttpDenied, HttpError, HttpReply};
use crate::request::{header_map, Request};
use crate::runtime::wait;

#[cfg(feature = "flowcatalyst")]
mod bindings {
    wit_bindgen::generate!({
        path: "../../wit/flowcatalyst-function",
        world: "imports",
    });
}

#[cfg(feature = "flowcatalyst")]
use crate::context::{EmitError, Invocation};
#[cfg(feature = "flowcatalyst")]
use bindings::flowcatalyst::function as fc;
#[cfg(feature = "flowcatalyst")]
use fc_function_abi::OutboundEvent;

pub(crate) struct WasiBackend {
    #[cfg(feature = "flowcatalyst")]
    invocation: std::cell::OnceCell<Invocation>,
}

impl WasiBackend {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(feature = "flowcatalyst")]
            invocation: std::cell::OnceCell::new(),
        }
    }
}

impl Backend for WasiBackend {
    #[cfg(feature = "flowcatalyst")]
    fn invocation(&self) -> &Invocation {
        self.invocation
            .get_or_init(|| invocation(fc::invocation::context()))
    }

    #[cfg(feature = "flowcatalyst")]
    fn config(&self, key: &str) -> Option<String> {
        fc::config::get(key)
    }

    #[cfg(feature = "flowcatalyst")]
    fn secret(&self, key: &str) -> Option<String> {
        fc::secrets::get(key)
    }

    #[cfg(feature = "flowcatalyst")]
    fn emit(&self, event: &OutboundEvent) -> Result<(), EmitError> {
        use fc::events;
        let data = match event.data() {
            [] => None,
            bytes => Some(String::from_utf8(bytes.to_vec()).map_err(|_| {
                EmitError::Invalid(fc_function_abi::emit_error::INVALID_EVENT_DATA_NOT_JSON.into())
            })?),
        };
        let wire = events::OutboundEvent {
            type_: event.event_type().to_owned(),
            source: event.source().map(str::to_owned),
            subject: event.subject().map(str::to_owned),
            data_content_type: event.data_content_type().map(str::to_owned),
            data,
            correlation_id: event.correlation_id().map(str::to_owned),
            causation_id: event.causation_id().map(str::to_owned),
            message_group: event.message_group().map(str::to_owned),
            dedup_id: event.dedup_id().to_owned(),
        };
        events::emit(&wire).map_err(|e| match e {
            events::EmitError::Invalid(code) => EmitError::Invalid(code),
            events::EmitError::Refused(r) => EmitError::Refused {
                code: r.code,
                status: r.status,
            },
            events::EmitError::Unavailable => EmitError::Unavailable,
        })
    }

    fn log(&self, level: Level, message: &str) {
        log_line(level, message);
    }

    fn send(&self, call: HttpCall) -> HttpFuture {
        Box::pin(send(call))
    }

    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// One log line: the host's `log` interface, or without the `flowcatalyst`
/// feature standard output (TRACE to INFO) or standard error (WARN, ERROR).
pub(crate) fn log_line(level: Level, message: &str) {
    #[cfg(feature = "flowcatalyst")]
    {
        use fc::log::Level as L;
        let level = match level {
            Level::Trace => L::Trace,
            Level::Debug => L::Debug,
            Level::Info => L::Info,
            Level::Warn => L::Warn,
            Level::Error => L::Error,
        };
        fc::log::log(level, message);
    }
    #[cfg(not(feature = "flowcatalyst"))]
    {
        if level >= Level::Warn {
            eprintln!("{} {message}", level.as_str());
        } else {
            println!("{} {message}", level.as_str());
        }
    }
}

#[cfg(feature = "flowcatalyst")]
fn invocation(ctx: fc::invocation::InvocationContext) -> Invocation {
    use fc::invocation::Caller as C;
    use fc_function_abi::{Caller, FunctionAddress, Principal};
    let caller = match ctx.caller {
        C::Platform => Caller::Platform,
        C::Anonymous => Caller::Anonymous,
        C::Principal(p) => Caller::Principal(Principal {
            id: p.id,
            principal_type: p.principal_type,
            tier: p.tier,
            clients: p.clients,
            roles: p.roles,
            applications: p.applications,
            all_applications: p.all_applications,
            permissions: p.permissions.into_iter().collect(),
        }),
    };
    let address = FunctionAddress::parse(&ctx.address)
        .expect("the host names its function by a valid address");
    let mut invocation = Invocation::new(address);
    invocation.invocation_id = ctx.invocation_id;
    invocation.version = ctx.version;
    invocation.caller = caller;
    invocation.correlation_id = ctx.correlation_id;
    invocation.causation_id = ctx.causation_id;
    invocation.original_host = ctx.original_host;
    invocation.original_path = ctx.original_path;
    invocation.remote_address = ctx.remote_address;
    invocation.path_params = ctx.path_params;
    invocation
}

/// The incoming request, body read in full (the host has it buffered).
pub(crate) fn read_request(
    request: IncomingRequest,
    backend: Rc<dyn Backend>,
) -> Result<Request, String> {
    let method = method_name(request.method());
    let path_with_query = request.path_with_query().unwrap_or_default();
    let authority = request.authority();
    let headers = header_map(request.headers().entries());
    let body = request
        .consume()
        .map_err(|()| "the body was already taken")?;
    let bytes = read_blocking(body).map_err(|e| format!("reading the body: {e}"))?;
    Ok(Request::new(
        method,
        &path_with_query,
        headers,
        bytes,
        authority,
        backend,
    ))
}

fn method_name(method: Method) -> String {
    match method {
        Method::Get => "GET".into(),
        Method::Head => "HEAD".into(),
        Method::Post => "POST".into(),
        Method::Put => "PUT".into(),
        Method::Delete => "DELETE".into(),
        Method::Connect => "CONNECT".into(),
        Method::Options => "OPTIONS".into(),
        Method::Trace => "TRACE".into(),
        Method::Patch => "PATCH".into(),
        Method::Other(other) => other,
    }
}

fn method_value(name: &str) -> Method {
    match name {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "CONNECT" => Method::Connect,
        "OPTIONS" => Method::Options,
        "TRACE" => Method::Trace,
        "PATCH" => Method::Patch,
        other => Method::Other(other.to_owned()),
    }
}

fn read_blocking(body: IncomingBody) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let stream = body.stream().map_err(|()| "the stream was already taken")?;
        loop {
            match stream.blocking_read(64 * 1024) {
                Ok(chunk) => out.extend(chunk),
                Err(StreamError::Closed) => break,
                Err(StreamError::LastOperationFailed(e)) => return Err(e.to_debug_string()),
            }
        }
    }
    IncomingBody::finish(body);
    Ok(out)
}

/// One outbound call through `wasi:http/outgoing-handler`.
async fn send(call: HttpCall) -> Result<HttpReply, HttpError> {
    let invalid = |what: &str| HttpError::InvalidRequest(format!("{what} refused by wasi:http"));
    if call.timeout().is_some_and(|t| t.is_zero()) {
        return Err(HttpError::InvalidRequest(
            "the timeout must be positive".into(),
        ));
    }
    let target = target(call.url())?;
    let mut entries: Vec<(String, Vec<u8>)> = call
        .headers()
        .iter()
        .flat_map(|(name, values)| {
            values
                .iter()
                .map(move |v| (name.clone(), v.clone().into_bytes()))
        })
        .collect();
    // The body is in hand, so say how long it is rather than let it go out
    // chunked (which many APIs refuse).
    let sends_body = !call.body().is_empty() || matches!(call.method(), "POST" | "PUT" | "PATCH");
    if sends_body && call.header("content-length").is_none() {
        entries.push((
            "content-length".into(),
            call.body().len().to_string().into_bytes(),
        ));
    }
    let fields = Fields::from_list(&entries)
        .map_err(|e| HttpError::InvalidRequest(format!("a header was refused: {e:?}")))?;
    let request = OutgoingRequest::new(fields);
    request
        .set_method(&method_value(call.method()))
        .map_err(|()| invalid("the method"))?;
    let scheme = match target.scheme.to_ascii_lowercase().as_str() {
        "https" => Scheme::Https,
        "http" => Scheme::Http,
        other => Scheme::Other(other.to_owned()),
    };
    request
        .set_scheme(Some(&scheme))
        .map_err(|()| invalid("the scheme"))?;
    request
        .set_authority(Some(target.authority))
        .map_err(|()| invalid("the authority"))?;
    request
        .set_path_with_query(Some(target.path_with_query))
        .map_err(|()| invalid("the path"))?;
    let options = call.timeout().map(options);
    let body = request.body().expect("a new request's body is taken once");
    let denied = || HttpError::Denied(HttpDenied::new(target.host));
    let pending = outgoing_handler::handle(request, options).map_err(|e| error(e, &denied))?;
    // A body the host stopped taking (a refused call, a dead connection) is
    // reported only when the response itself does not say why.
    let written = if call.body().is_empty() {
        Ok(())
    } else {
        let stream = body.write().expect("a new body's stream is taken once");
        write_all(&stream, call.body()).await
    };
    let finished = OutgoingBody::finish(body, None);
    wait(pending.subscribe()).await;
    let response = pending
        .get()
        .expect("the response is ready once its pollable is")
        .expect("the response is taken once")
        .map_err(|e| error(e, &denied))?;
    written.map_err(|e| HttpError::Failed(format!("writing the request body: {e}")))?;
    finished.map_err(|e| error(e, &denied))?;
    let status = response.status();
    let headers = header_map(response.headers().entries());
    let incoming = response.consume().expect("the response body is taken once");
    let bytes = {
        let stream = incoming.stream().expect("the body stream is taken once");
        read_all(&stream)
            .await
            .map_err(|e| HttpError::Failed(format!("reading the response body: {e}")))?
    };
    drop(IncomingBody::finish(incoming));
    Ok(HttpReply::new(status, headers, bytes))
}

fn options(timeout: Duration) -> RequestOptions {
    let nanos = u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX);
    let options = RequestOptions::new();
    // The host caps each of them; one it does not support is left unset.
    let _ = options.set_connect_timeout(Some(nanos));
    let _ = options.set_first_byte_timeout(Some(nanos));
    let _ = options.set_between_bytes_timeout(Some(nanos));
    options
}

fn error(code: ErrorCode, denied: &dyn Fn() -> HttpError) -> HttpError {
    match code {
        ErrorCode::HttpRequestDenied => denied(),
        ErrorCode::DnsTimeout
        | ErrorCode::ConnectionTimeout
        | ErrorCode::ConnectionReadTimeout
        | ErrorCode::ConnectionWriteTimeout
        | ErrorCode::HttpResponseTimeout => HttpError::Timeout,
        other => HttpError::Failed(format!("{other:?}")),
    }
}

fn stream_error(e: StreamError) -> String {
    match e {
        StreamError::Closed => "the stream closed".into(),
        StreamError::LastOperationFailed(e) => e.to_debug_string(),
    }
}

async fn write_all(stream: &OutputStream, mut bytes: &[u8]) -> Result<(), String> {
    while !bytes.is_empty() {
        let permitted = stream.check_write().map_err(stream_error)?;
        if permitted == 0 {
            wait(stream.subscribe()).await;
            continue;
        }
        let take = bytes
            .len()
            .min(usize::try_from(permitted).unwrap_or(usize::MAX));
        stream.write(&bytes[..take]).map_err(stream_error)?;
        bytes = &bytes[take..];
    }
    // Every byte is with the host now. The host stops taking the body once
    // the declared `content-length` has gone out, so the stream may already
    // be closed here: that is the body sent, not a failure (a connection
    // that died instead shows in the response, which is checked first).
    let settled = |r: Result<u64, StreamError>| match r {
        Ok(_) | Err(StreamError::Closed) => Ok(()),
        Err(e) => Err(stream_error(e)),
    };
    settled(stream.flush().map(|()| 0))?;
    wait(stream.subscribe()).await;
    settled(stream.check_write())
}

async fn read_all(stream: &InputStream) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        match stream.read(64 * 1024) {
            Ok(chunk) if chunk.is_empty() => wait(stream.subscribe()).await,
            Ok(chunk) => out.extend(chunk),
            Err(StreamError::Closed) => return Ok(out),
            Err(e) => return Err(stream_error(e)),
        }
    }
}
