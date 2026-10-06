//! The host side of a JS function: the ops the bootstrap (`js/bootstrap.js`)
//! builds the `flowcatalyst:function/*` modules, `console` and `fetch` from.
//! They are the whole of what a function can reach outside its isolate,
//! and each one applies the host's policy itself, exactly as the WASM
//! runtime's host interfaces do:
//!
//! - `config` / `secrets`: the manifest's declared keys only; a secret is
//!   never logged;
//! - `log` and `console`: lines on `fn.<address>`, inside the invocation's
//!   span;
//! - `events`: through the shared [`Emitter`], with the invocation's emit
//!   defaults;
//! - `fetch`: `httpAllow` (exact host, or `*.suffix`), `https` only except
//!   loopback, redirects never followed, the timeout capped by the
//!   invocation's deadline and 30 s ([`fc_fnhost_core::wasm::egress::decide`],
//!   the WASM runtime's own check); a refusal is the `wasi:http` error code
//!   `HTTP-request-denied`.
//!
//! The host APIs other than logging work only while a request is being
//! handled: a bundle's top-level code (run at load, and again at the start
//! of every request's isolate) gets an error.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use deno_core::{op2, JsBuffer, OpState, ToJsBuffer};
use deno_error::JsErrorBox;
use fc_fnhost_core::emit::{EmitFailure, Emitter, OutboundEvent};
use fc_fnhost_core::invoke::InvocationContext;
use fc_fnhost_core::wasm::egress::{self, Decision, HttpAllowlist};
use fc_fnhost_core::wasm::output::{GuestLogger, GuestOutput};
use fc_function_abi::{Caller, FunctionAddress};
use reqwest::header::HeaderMap;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use std::error;
use std::str;
use tokio::runtime::Handle;

deno_core::extension!(
    fc_function,
    ops = [
        op_fc_config_get,
        op_fc_secret_get,
        op_fc_log,
        op_fc_invocation,
        op_fc_emit,
        op_fc_fetch,
        op_fc_random_fill,
        op_fc_url_parse,
        op_fc_url_set,
        op_fc_utf8_valid,
    ],
);

/// What every invocation of one loaded version shares. Holds secret
/// values: deliberately not `Debug`.
pub struct VersionShared {
    pub address: FunctionAddress,
    pub version: i32,
    pub logger: GuestLogger,
    /// Only the keys the manifest declares.
    pub config: HashMap<String, String>,
    /// Only the keys the manifest declares, and only non-empty values.
    pub secrets: HashMap<String, String>,
    pub allow: Arc<HttpAllowlist>,
    pub emitter: Emitter,
    /// The largest body `fetch` reads and the handler may answer with:
    /// `limits.wasmMemoryMb`.
    pub body_cap: usize,
    pub http: reqwest::Client,
    /// The host's own runtime, where outbound calls and emits run.
    pub host_runtime: Option<Handle>,
}

/// The current request, as the host APIs see it.
pub struct InvocationState {
    pub context: ContextOut,
    pub deadline: Instant,
    /// `(correlationId, causationId)` for emitted events.
    pub defaults: (String, Option<String>),
}

/// An isolate's view of the host (in its `OpState`).
pub struct HostState {
    pub version: Arc<VersionShared>,
    /// `None` while the bundle's top-level code runs.
    pub invocation: Option<InvocationState>,
}

fn host(state: &OpState) -> Rc<HostState> {
    state.borrow::<Rc<HostState>>().clone()
}

/// The invocation being handled, or the error a call made outside a request
/// (a bundle's top-level code) gets.
fn during_request<'a>(host: &'a HostState, api: &str) -> Result<&'a InvocationState, JsErrorBox> {
    match &host.invocation {
        Some(invocation) => Ok(invocation),
        None => Err(JsErrorBox::generic(format!(
            "{api} is available only while a request is handled, not in a bundle's top-level code"
        ))),
    }
}

// ── config, secrets ─────────────────────────────────────────────────────

#[op2]
#[string]
fn op_fc_config_get(
    state: &mut OpState,
    #[string] key: String,
) -> Result<Option<String>, JsErrorBox> {
    let host = host(state);
    during_request(&host, "config.get")?;
    Ok(host.version.config.get(&key).cloned())
}

/// Never logs: a secret value must not reach a log line.
#[op2]
#[string]
fn op_fc_secret_get(
    state: &mut OpState,
    #[string] key: String,
) -> Result<Option<String>, JsErrorBox> {
    let host = host(state);
    during_request(&host, "secrets.get")?;
    Ok(host.version.secrets.get(&key).cloned())
}

// ── log, console ────────────────────────────────────────────────────────

fn level(name: &str) -> tracing::Level {
    match name {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    }
}

/// `log.log` writes the message as one line, as the WIT `log` interface
/// does; `console.*` splits it the way WASI standard output is split (one
/// line per `\n`, at most 8 KiB each).
#[op2(fast)]
fn op_fc_log(
    state: &mut OpState,
    #[string] level_name: &str,
    #[string] message: &str,
    console: bool,
) {
    let host = host(state);
    let level = level(level_name);
    if console {
        let output = GuestOutput::new(host.version.logger.clone(), level);
        output.push(message.as_bytes());
        output.flush();
    } else {
        host.version.logger.line(level, message);
    }
}

// ── invocation ──────────────────────────────────────────────────────────

/// The WIT `invocation-context` record, camelCased.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOut {
    pub invocation_id: String,
    pub address: String,
    pub version: i32,
    pub caller: CallerOut,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub original_host: Option<String>,
    pub original_path: Option<String>,
    pub remote_address: Option<String>,
    pub path_params: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CallerOut {
    Platform,
    Anonymous,
    Principal { principal: PrincipalOut },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalOut {
    pub id: String,
    pub principal_type: String,
    pub tier: Option<String>,
    pub clients: Vec<String>,
    pub roles: Vec<String>,
    pub applications: Vec<String>,
    pub all_applications: bool,
    /// Sorted.
    pub permissions: Vec<String>,
}

impl ContextOut {
    pub fn from(context: &InvocationContext) -> Self {
        let caller = match &context.caller {
            Caller::Platform => CallerOut::Platform,
            Caller::Anonymous => CallerOut::Anonymous,
            Caller::Principal(p) => CallerOut::Principal {
                principal: PrincipalOut {
                    id: p.id.clone(),
                    principal_type: p.principal_type.clone(),
                    tier: p.tier.clone(),
                    clients: p.clients.clone(),
                    roles: p.roles.clone(),
                    applications: p.applications.clone(),
                    all_applications: p.all_applications,
                    permissions: p.permissions.iter().cloned().collect(),
                },
            },
        };
        Self {
            invocation_id: context.invocation_id.clone(),
            address: context.address.render(),
            version: context.version,
            caller,
            correlation_id: context.correlation_id.clone(),
            causation_id: context.causation_id.clone(),
            original_host: context.original_host.clone(),
            original_path: context.original_path.clone(),
            remote_address: context.remote_address.clone(),
            path_params: context
                .path_params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }
}

#[op2]
#[serde]
fn op_fc_invocation(state: &mut OpState) -> Result<ContextOut, JsErrorBox> {
    let host = host(state);
    let invocation = during_request(&host, "invocation.context")?;
    Ok(invocation.context.clone())
}

// ── events ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventIn {
    #[serde(rename = "type")]
    event_type: String,
    source: Option<String>,
    subject: Option<String>,
    data_content_type: Option<String>,
    /// JSON text (the bootstrap stringifies the author's value).
    data: Option<String>,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    message_group: Option<String>,
    dedup_id: String,
}

/// `result<string, emit-event-error>` as `{ ok: true, id }` or
/// `{ ok: false, error }`.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum EmitOut {
    Ok { ok: bool, id: String },
    Err { ok: bool, error: EmitErrorOut },
}

/// The WIT `emit-event-error` variant.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum EmitErrorOut {
    Invalid {
        code: String,
    },
    Refused {
        code: String,
        status: u16,
        message: String,
    },
    Unavailable {
        message: String,
    },
}

#[op2]
#[serde]
async fn op_fc_emit(
    state: Rc<RefCell<OpState>>,
    #[serde] event: EventIn,
) -> Result<EmitOut, JsErrorBox> {
    let (version, defaults) = {
        let host = host(&state.borrow());
        let invocation = during_request(&host, "events.emit")?;
        (host.version.clone(), invocation.defaults.clone())
    };
    let event = OutboundEvent {
        event_type: event.event_type,
        source: event.source,
        subject: event.subject,
        data_content_type: event.data_content_type,
        data: event.data,
        correlation_id: event.correlation_id,
        causation_id: event.causation_id,
        message_group: event.message_group,
        dedup_id: event.dedup_id,
    };
    let result = version
        .emitter
        .emit(&version.address, version.version, event, defaults)
        .await;
    let error = match result {
        Ok(id) => return Ok(EmitOut::Ok { ok: true, id }),
        Err(EmitFailure::Invalid(code)) => EmitErrorOut::Invalid { code },
        Err(failure @ EmitFailure::Platform(_)) if failure.is_unavailable() => {
            let EmitFailure::Platform(e) = failure else {
                unreachable!()
            };
            EmitErrorOut::Unavailable {
                message: e.message().to_owned(),
            }
        }
        Err(EmitFailure::Platform(e)) => EmitErrorOut::Refused {
            code: e.code().to_owned(),
            status: e.status(),
            message: e.message().to_owned(),
        },
    };
    Ok(EmitOut::Err { ok: false, error })
}

// ── fetch ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct FetchIn {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<JsBuffer>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct FetchOut {
    ok: bool,
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    body: Option<ToJsBuffer>,
    /// The `wasi:http` error-code a failure is (`HTTP-request-denied`, …).
    code: Option<String>,
    message: Option<String>,
}

impl FetchOut {
    fn failed(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: Some(code.to_owned()),
            message: Some(message.into()),
            ..Self::default()
        }
    }
}

/// Headers the host sets itself (or that only make sense hop by hop); a
/// function's value for one is dropped.
const HOST_OWNED_HEADERS: [&str; 10] = [
    "connection",
    "content-length",
    "host",
    "keep-alive",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// A received response header value, as ISO-8859-1 text (one char per
/// byte), the way the listener hands request headers to functions.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

#[op2]
#[serde]
async fn op_fc_fetch(
    state: Rc<RefCell<OpState>>,
    #[serde] request: FetchIn,
) -> Result<FetchOut, JsErrorBox> {
    let (version, deadline) = {
        let host = host(&state.borrow());
        let invocation = during_request(&host, "fetch")?;
        (host.version.clone(), invocation.deadline)
    };
    let url = match url::Url::parse(&request.url) {
        Ok(url) => url,
        Err(e) => return Ok(FetchOut::failed("HTTP-request-URI-invalid", e.to_string())),
    };
    let host_name = url.host_str().unwrap_or("").to_owned();
    let timeout = match egress::decide(&version.allow, url.scheme(), &host_name, deadline) {
        Decision::Deny(why) => {
            tracing::debug!(host = %host_name, reason = %why, "outbound call refused");
            return Ok(FetchOut::failed("HTTP-request-denied", why));
        }
        Decision::Allow { timeout } => timeout,
    };
    let method = match reqwest::Method::from_bytes(request.method.as_bytes()) {
        Ok(method) => method,
        Err(e) => {
            return Ok(FetchOut::failed(
                "HTTP-request-method-invalid",
                e.to_string(),
            ))
        }
    };
    let mut headers = HeaderMap::new();
    for (name, value) in &request.headers {
        if HOST_OWNED_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
            continue;
        }
        let bytes: Option<Vec<u8>> = value.chars().map(|c| u8::try_from(c as u32).ok()).collect();
        let (Ok(name), Some(Ok(value))) = (
            HeaderName::from_bytes(name.as_bytes()),
            bytes.map(|b| HeaderValue::from_bytes(&b)),
        ) else {
            return Ok(FetchOut::failed(
                "HTTP-request-header-invalid",
                format!("the header '{name}' is not legal on the wire"),
            ));
        };
        headers.append(name, value);
    }
    let mut builder = version
        .http
        .request(method, url)
        .headers(headers)
        .timeout(timeout);
    if let Some(body) = request.body {
        builder = builder.body(body.to_vec());
    }
    let cap = version.body_cap;
    let call = async move {
        let mut response = match builder.send().await {
            Ok(response) => response,
            Err(e) => return failure(&e),
        };
        let status = response.status();
        let mut out = FetchOut {
            ok: true,
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or("").to_owned(),
            headers: response
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_owned(), latin1(v.as_bytes())))
                .collect(),
            ..FetchOut::default()
        };
        let mut body = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    if body.len() + chunk.len() > cap {
                        return FetchOut::failed(
                            "HTTP-response-body-size",
                            format!("the response body is over the {cap}-byte cap"),
                        );
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => return failure(&e),
            }
        }
        out.body = Some(body.into());
        out
    };
    let out = match &version.host_runtime {
        Some(runtime) => runtime
            .spawn(call)
            .await
            .unwrap_or_else(|e| FetchOut::failed("internal-error", e.to_string())),
        None => call.await,
    };
    Ok(out)
}

/// A client error as its `wasi:http` error code.
fn failure(e: &reqwest::Error) -> FetchOut {
    let code = if e.is_timeout() {
        if e.is_connect() {
            "connection-timeout"
        } else {
            "HTTP-response-timeout"
        }
    } else if e.is_connect() {
        "connection-refused"
    } else if e.is_body() || e.is_decode() {
        "HTTP-protocol-error"
    } else {
        "internal-error"
    };
    let mut message = e.to_string();
    let mut source = error::Error::source(e);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    FetchOut::failed(code, message)
}

/// The shared client outbound calls use: no redirects followed, no proxy
/// from the environment (the WASM runtime's `wasi:http` uses none either),
/// rustls with the web PKI roots.
pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .build()
        .map_err(|e| format!("the outbound HTTP client did not build: {e}"))
}

// ── crypto, URL, TextDecoder ────────────────────────────────────────────

#[op2(fast)]
fn op_fc_random_fill(#[buffer] buffer: &mut [u8]) {
    use rand::RngCore;
    rand::rng().fill_bytes(buffer);
}

/// `[href, origin, protocol, username, password, host, hostname, port,
/// pathname, search, hash]`: the WHATWG URL getters (`url::quirks`).
fn parts(url: &url::Url) -> [String; 11] {
    use url::quirks;
    [
        quirks::href(url).to_owned(),
        quirks::origin(url),
        quirks::protocol(url).to_owned(),
        quirks::username(url).to_owned(),
        quirks::password(url).to_owned(),
        quirks::host(url).to_owned(),
        quirks::hostname(url).to_owned(),
        quirks::port(url).to_owned(),
        quirks::pathname(url).to_owned(),
        quirks::search(url).to_owned(),
        quirks::hash(url).to_owned(),
    ]
}

#[op2]
#[serde]
fn op_fc_url_parse(#[string] href: String, #[string] base: String) -> Option<[String; 11]> {
    // An empty base is no base (the bootstrap passes "" for undefined).
    let url = if base.is_empty() {
        url::Url::parse(&href).ok()?
    } else {
        url::Url::parse(&base).ok()?.join(&href).ok()?
    };
    Some(parts(&url))
}

/// A WHATWG URL setter (`url::quirks`): the new parts, or `None` when
/// `href` itself does not parse (a setter that ignores its value leaves
/// the URL as it was).
#[op2]
#[serde]
fn op_fc_url_set(
    #[string] href: String,
    #[string] part: String,
    #[string] value: String,
) -> Option<[String; 11]> {
    use url::quirks;
    let mut url = url::Url::parse(&href).ok()?;
    match part.as_str() {
        "href" => quirks::set_href(&mut url, &value).ok()?,
        "protocol" => {
            let _ = quirks::set_protocol(&mut url, &value);
        }
        "username" => {
            let _ = quirks::set_username(&mut url, &value);
        }
        "password" => {
            let _ = quirks::set_password(&mut url, &value);
        }
        "host" => {
            let _ = quirks::set_host(&mut url, &value);
        }
        "hostname" => {
            let _ = quirks::set_hostname(&mut url, &value);
        }
        "port" => {
            let _ = quirks::set_port(&mut url, &value);
        }
        "pathname" => quirks::set_pathname(&mut url, &value),
        "search" => quirks::set_search(&mut url, &value),
        "hash" => quirks::set_hash(&mut url, &value),
        _ => return None,
    }
    Some(parts(&url))
}

#[op2(fast)]
fn op_fc_utf8_valid(#[buffer] bytes: &[u8]) -> bool {
    str::from_utf8(bytes).is_ok()
}
