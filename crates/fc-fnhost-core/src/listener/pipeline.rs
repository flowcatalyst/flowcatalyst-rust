//! The request pipeline (Java `FnHttpServer.handle` / `handlePublic` and
//! everything below them; spec `function-host-listener.md` §2-§4 and
//! `function-public-routes.md` §3-§4).

use std::net::SocketAddr;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use fc_function_abi::{permission_matches, Caller, MultiMap};
use fc_function_model::{EndpointAuth, HttpMethod, PathParams, RoutePattern};
use futures::FutureExt;
use http::request::Parts;
use http::HeaderMap;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use indexmap::IndexMap;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::answer::{outcome_for, HttpAnswer};
use super::bearer::TokenClaims;
use super::public_routes::{self, LIVE};
use super::route_path::RoutePath;
use super::{cors, latin1, webhook, Shared};
use crate::desired::{DesiredDocument, Entry, Role};
use crate::invoke::{self, InvocationContext, InvokeError};
use crate::loader::LoadedFunction;
use crate::logging::keys;
use crate::metrics::ListenerEntry;

/// A versioned call's body is buffered under this fixed cap before
/// authentication (its entry, and so its own cap, may not be looked at
/// yet); the endpoint's own cap is checked afterwards.
const VERSIONED_BODY_CAP_BYTES: u64 = 1_048_576;

/// `platform:function:version:invoke`.
pub const FUNCTION_VERSION_INVOKE: &str = "platform:function:version:invoke";

/// Headers the host consumes when it authenticates.
const CONSUMED_AUTH_HEADERS: [&str; 3] = [
    "authorization",
    "x-flowcatalyst-signature",
    "x-flowcatalyst-timestamp",
];

/// Always dropped: caller-controllable, and it names the internal routing
/// outcome.
const INBOUND_ROUTING_HEADER: &str = "x-flowcatalyst-function";

/// One entry of the current document, held by reference.
#[derive(Clone)]
pub(crate) struct EntryRef {
    document: Arc<DesiredDocument>,
    index: usize,
}

impl std::ops::Deref for EntryRef {
    type Target = Entry;

    fn deref(&self) -> &Entry {
        &self.document.functions[self.index]
    }
}

impl EntryRef {
    fn find(document: Arc<DesiredDocument>, predicate: impl Fn(&Entry) -> bool) -> Option<Self> {
        let index = document.functions.iter().position(predicate)?;
        Some(Self { document, index })
    }
}

/// The parts of the request the pipeline needs after the body is split off.
struct Call {
    parts: Parts,
    entry_kind: ListenerEntry,
    peer: SocketAddr,
}

impl Call {
    fn method(&self) -> &str {
        self.parts.method.as_str()
    }

    fn header(&self, name: &str) -> Option<String> {
        self.parts.headers.get(name).map(|v| latin1(v.as_bytes()))
    }

    /// `:authority` (HTTP/2, absolute-form) or `Host`.
    fn authority(&self) -> Option<String> {
        self.parts
            .uri
            .authority()
            .map(|a| a.as_str().to_owned())
            .or_else(|| self.header("host"))
    }
}

pub(crate) async fn handle(
    shared: Arc<Shared>,
    entry_kind: ListenerEntry,
    peer: SocketAddr,
    request: http::Request<Incoming>,
) -> HttpAnswer {
    let (parts, body) = request.into_parts();
    let call = Call {
        parts,
        entry_kind,
        peer,
    };
    let pipeline = async {
        match entry_kind {
            ListenerEntry::Private => handle_private(&shared, &call, body).await,
            ListenerEntry::Public => handle_public(&shared, &call, body).await,
        }
    };
    // A safety net: an unexpected panic still answers, never with its text.
    match AssertUnwindSafe(pipeline).catch_unwind().await {
        Ok(answer) => answer,
        Err(_) => {
            tracing::error!("unexpected failure before/around invocation");
            HttpAnswer::error(500, "INTERNAL", "internal error")
        }
    }
}

fn draining() -> HttpAnswer {
    HttpAnswer::error(503, "DRAINING", "the host is draining").with_header("Retry-After", "5")
}

fn not_found() -> HttpAnswer {
    HttpAnswer::error(404, "NOT_FOUND", "not found")
}

// ── private entry ─────────────────────────────────────────────────────────

async fn handle_private(shared: &Arc<Shared>, call: &Call, body: Incoming) -> HttpAnswer {
    if shared.is_draining() {
        return draining();
    }
    match RoutePath::parse(call.parts.uri.path()) {
        RoutePath::NotFunctionsRoute => not_found(),
        RoutePath::AddressInvalid => {
            HttpAnswer::error(400, "ADDRESS_INVALID", "invalid function address")
        }
        RoutePath::VersionInvalid => {
            HttpAnswer::error(400, "VERSION_INVALID", "version must be a positive integer")
        }
        RoutePath::Matched {
            address,
            version: None,
            function_path,
        } => {
            let entry = shared.reconciler.document().and_then(|document| {
                EntryRef::find(document, |e| e.role == Role::Live && e.address == address)
            });
            let Some(entry) = entry else {
                // An unknown address is never a metrics label value.
                shared
                    .metrics
                    .refused("not_found", None, ListenerEntry::Private);
                return HttpAnswer::error(404, "FUNCTION_NOT_FOUND", "no such function");
            };
            handle_entry(shared, call, body, entry, function_path, None, false).await
        }
        RoutePath::Matched {
            address,
            version: Some(version),
            function_path,
        } => {
            // Nothing about the entry may be looked at before authentication.
            let body = match read_body(&call.parts.headers, body, VERSIONED_BODY_CAP_BYTES).await {
                Ok(body) => body,
                Err(answer) => return answer,
            };
            handle_versioned(shared, call, address, version, function_path, body).await
        }
    }
}

// ── public entry ──────────────────────────────────────────────────────────

async fn handle_public(shared: &Arc<Shared>, call: &Call, body: Incoming) -> HttpAnswer {
    if shared.is_draining() {
        return draining();
    }
    let refuse = || {
        shared
            .metrics
            .refused("not_found", None, ListenerEntry::Public);
        not_found()
    };
    let Some(hostname) = public_routes::public_hostname(call.authority().as_deref()) else {
        return refuse();
    };
    let Some(document) = shared.reconciler.document() else {
        return refuse();
    };
    let table = shared.route_table(&document);
    let Some(matched) = table.resolve(&hostname, call.parts.uri.path()) else {
        return refuse();
    };
    // `live` is the exact-hostname match; any other alias runs over the
    // versioned load path, so the aliased version's own manifest drives
    // endpoint matching, auth and limits.
    let is_live = matched.alias == LIVE;
    let entry = EntryRef::find(document, |e| {
        e.address == matched.address
            && if is_live {
                e.role == Role::Live
            } else {
                e.role != Role::Candidate && e.aliases.contains(&matched.alias)
            }
    });
    let Some(entry) = entry else {
        return refuse();
    };
    let remote =
        public_routes::remote_address(call.peer.ip(), &call.parts.headers, &shared.trusted_proxies);
    handle_entry(
        shared,
        call,
        body,
        entry,
        matched.function_path,
        Some(remote),
        !is_live,
    )
    .await
}

// ── shared: CORS preflight → endpoint → body cap → auth → invoke ─────────

enum EndpointMatch {
    Ok(usize, PathParams),
    NotFound,
    MethodNotAllowed(Vec<HttpMethod>),
}

fn match_endpoint(entry: &Entry, function_path: &str, method: &str) -> EndpointMatch {
    let endpoints = &entry.manifest.endpoints;
    let Some(matched) = RoutePattern::first_match(endpoints.iter().map(|e| &e.path), function_path)
    else {
        return EndpointMatch::NotFound;
    };
    // The first endpoint declaring the matched pattern.
    let Some(index) = endpoints.iter().position(|e| &e.path == matched.pattern) else {
        return EndpointMatch::NotFound;
    };
    let effective = endpoints[index].effective_methods();
    if !effective.is_empty()
        && !method
            .parse::<HttpMethod>()
            .is_ok_and(|m| effective.contains(&m))
    {
        return EndpointMatch::MethodNotAllowed(effective);
    }
    EndpointMatch::Ok(index, matched.params)
}

fn method_not_allowed(allowed: &[HttpMethod]) -> HttpAnswer {
    let allow: Vec<&str> = allowed.iter().map(|m| m.as_str()).collect();
    HttpAnswer::error(
        405,
        "METHOD_NOT_ALLOWED",
        "method not allowed on this endpoint",
    )
    .with_header("Allow", &allow.join(", "))
}

fn endpoint_not_found() -> HttpAnswer {
    HttpAnswer::error(404, "ENDPOINT_NOT_FOUND", "no endpoint matches this path")
}

fn body_too_large() -> HttpAnswer {
    HttpAnswer::error(
        413,
        "BODY_TOO_LARGE",
        "request body exceeds the endpoint's limit",
    )
}

async fn handle_entry(
    shared: &Arc<Shared>,
    call: &Call,
    body: Incoming,
    entry: EntryRef,
    function_path: String,
    remote_override: Option<String>,
    versioned: bool,
) -> HttpAnswer {
    if cors::is_preflight(call.method(), &call.parts.headers) {
        let by_path = RoutePattern::first_match(
            entry.manifest.endpoints.iter().map(|e| &e.path),
            &function_path,
        )
        .and_then(|m| {
            entry
                .manifest
                .endpoints
                .iter()
                .find(|e| &e.path == m.pattern)
        });
        if let Some(endpoint) = by_path {
            if let Some(policy) = &endpoint.cors {
                let answer = cors::preflight(endpoint, policy, &call.parts.headers);
                shared
                    .metrics
                    .refused("preflight", Some(&entry.address), call.entry_kind);
                return answer;
            }
        }
        // Not a CORS-declaring endpoint: ordinary handling (very likely 405).
    }

    let (index, params) = match match_endpoint(&entry, &function_path, call.method()) {
        EndpointMatch::Ok(index, params) => (index, params),
        EndpointMatch::NotFound => return endpoint_not_found(),
        EndpointMatch::MethodNotAllowed(allowed) => return method_not_allowed(&allowed),
    };
    let endpoint = &entry.manifest.endpoints[index];
    let body = match read_body(&call.parts.headers, body, endpoint.max_body_bytes as u64).await {
        Ok(body) => body,
        Err(answer) => return answer,
    };

    let origin = call.header("origin");
    let caller = match authenticate(shared, call, &entry, endpoint.auth, &body).await {
        Ok(caller) => caller,
        Err(failure) => {
            shared
                .metrics
                .refused("unauthorized", Some(&entry.address), call.entry_kind);
            return cors::apply_to_actual_response(endpoint, origin.as_deref(), failure);
        }
    };
    let strip_auth_headers = endpoint.auth != EndpointAuth::None;
    let answer = invoke(
        shared,
        call,
        &entry,
        index,
        function_path,
        params,
        body,
        caller,
        strip_auth_headers,
        versioned,
        remote_override,
    )
    .await;
    cors::apply_to_actual_response(endpoint, origin.as_deref(), answer)
}

/// The endpoint's own `auth` (spec §3), identical on both listeners.
async fn authenticate(
    shared: &Arc<Shared>,
    call: &Call,
    entry: &Entry,
    auth: EndpointAuth,
    body: &[u8],
) -> Result<Caller, HttpAnswer> {
    match auth {
        EndpointAuth::Webhook => {
            let current = shared.reconciler.current_webhook_secret(&entry.address);
            let previous = shared.reconciler.previous_webhook_secret(&entry.address);
            webhook::verify(
                body,
                call.header("x-flowcatalyst-signature").as_deref(),
                call.header("x-flowcatalyst-timestamp").as_deref(),
                current.as_deref(),
                previous.as_deref(),
                shared.clock.now().timestamp(),
            )
            .map(|()| Caller::Platform)
            .map_err(|reason| HttpAnswer::error(401, "UNAUTHORIZED", reason))
        }
        EndpointAuth::Platform => shared
            .bearer
            .authenticate(call.header("authorization").as_deref())
            .await
            .map(|claims| Caller::Principal(claims.principal()))
            .map_err(|reason| unauthorized_bearer(&reason)),
        EndpointAuth::None => Ok(Caller::Anonymous),
    }
}

fn unauthorized_bearer(reason: &str) -> HttpAnswer {
    HttpAnswer::error(401, "UNAUTHORIZED", reason).with_header("WWW-Authenticate", "Bearer")
}

// ── versioned: token → permission → entry and reach → endpoint → cap ─────

async fn handle_versioned(
    shared: &Arc<Shared>,
    call: &Call,
    address: fc_function_abi::FunctionAddress,
    version: i32,
    function_path: String,
    body: Bytes,
) -> HttpAnswer {
    // Refusals carry no address: an unauthenticated caller must not learn
    // from the answer, or from /metrics, whether a version exists.
    let claims = match shared
        .bearer
        .authenticate(call.header("authorization").as_deref())
        .await
    {
        Ok(claims) => claims,
        Err(reason) => {
            shared
                .metrics
                .refused("unauthorized", None, ListenerEntry::Private);
            return unauthorized_bearer(&reason);
        }
    };
    if !claims
        .permissions
        .iter()
        .any(|held| permission_matches(held, FUNCTION_VERSION_INVOKE))
    {
        shared
            .metrics
            .refused("unauthorized", None, ListenerEntry::Private);
        return HttpAnswer::error(
            403,
            "PERMISSION_REQUIRED",
            "platform:function:version:invoke required",
        );
    }
    let entry = shared.reconciler.document().and_then(|document| {
        EntryRef::find(document, |e| e.address == address && e.version == version)
    });
    let Some(entry) = entry.filter(|entry| has_reach(&claims, entry)) else {
        shared
            .metrics
            .refused("not_found", None, ListenerEntry::Private);
        return HttpAnswer::error(404, "VERSION_NOT_AVAILABLE", "no such version");
    };
    let (index, params) = match match_endpoint(&entry, &function_path, call.method()) {
        EndpointMatch::Ok(index, params) => (index, params),
        EndpointMatch::NotFound => return endpoint_not_found(),
        EndpointMatch::MethodNotAllowed(allowed) => return method_not_allowed(&allowed),
    };
    if body.len() as u64 > entry.manifest.endpoints[index].max_body_bytes as u64 {
        return body_too_large();
    }
    // The endpoint's own auth is not applied.
    invoke(
        shared,
        call,
        &entry,
        index,
        function_path,
        params,
        body,
        Caller::Principal(claims.principal()),
        true,
        true,
        None,
    )
    .await
}

/// Anchor, or the token's clients hold the entry's client (a platform-owned
/// function needs anchor) and, when restricted, its applications hold the
/// entry's application.
fn has_reach(claims: &TokenClaims, entry: &Entry) -> bool {
    if claims
        .tier
        .as_deref()
        .is_some_and(|t| t.eq_ignore_ascii_case("ANCHOR"))
    {
        return true;
    }
    let Some(client_id) = &entry.client_id else {
        return false;
    };
    if !claims.clients.contains(client_id) {
        return false;
    }
    match &entry.application_id {
        None => true,
        Some(application_id) => {
            claims.all_applications || claims.applications.contains(application_id)
        }
    }
}

// ── permits → load → invoke → respond ────────────────────────────────────

/// Held by the invocation task until the invocation future actually
/// finishes: the permits, the in-flight mark, and `fc_fn_active`.
struct Worker {
    shared: Arc<Shared>,
    address: fc_function_abi::FunctionAddress,
    _grant: super::permits::Grant,
    _in_flight: crate::loader::InFlight,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.metrics.exited(&self.address);
    }
}

#[allow(clippy::too_many_arguments)]
async fn invoke(
    shared: &Arc<Shared>,
    call: &Call,
    entry: &EntryRef,
    endpoint_index: usize,
    function_path: String,
    path_params: PathParams,
    body: Bytes,
    caller: Caller,
    strip_auth_headers: bool,
    versioned: bool,
    remote_override: Option<String>,
) -> HttpAnswer {
    let entry_kind = call.entry_kind;
    let endpoint = &entry.manifest.endpoints[endpoint_index];

    // Step 7: permits, never queued.
    let Some(grant) = shared
        .permits
        .try_acquire(&entry.address, entry.manifest.limits.max_concurrency)
    else {
        shared
            .metrics
            .refused("busy", Some(&entry.address), entry_kind);
        return HttpAnswer::error(429, "BUSY", "the function is at capacity")
            .with_header("Retry-After", "1");
    };

    // Step 8: load.
    let loaded = load(shared, entry, versioned).await;
    let Some((function, in_flight)) = loaded else {
        drop(grant);
        if versioned {
            shared.metrics.refused("not_found", None, entry_kind);
            return HttpAnswer::error(404, "VERSION_NOT_AVAILABLE", "version is not loadable");
        }
        shared
            .metrics
            .refused("unavailable", Some(&entry.address), entry_kind);
        return unavailable();
    };

    // Step 9: invoke, with the deadline.
    let invocation_id = crate::tsid::generate();
    let headers = collect_headers(&call.parts.headers, strip_auth_headers);
    let (correlation_id, causation_id) =
        invoke::emit_defaults(&invocation_id, &headers, &caller, &body);
    let timeout = Duration::from_millis(endpoint.timeout_ms as u64);
    let interrupted = CancellationToken::new();
    let context = InvocationContext {
        invocation_id: invocation_id.clone(),
        address: entry.address.clone(),
        version: function.version(),
        method: call.method().to_owned(),
        path: function_path,
        original_host: call.authority(),
        original_path: Some(call.parts.uri.path().to_owned()),
        path_params: path_params.into_iter().collect::<IndexMap<_, _>>(),
        query: collect_query(call.parts.uri.query()),
        raw_query: call.parts.uri.query().map(str::to_owned),
        headers,
        body,
        remote_address: Some(remote_override.unwrap_or_else(|| call.peer.ip().to_string())),
        caller,
        deadline: Instant::now() + timeout,
        interrupted: interrupted.clone(),
        correlation_id,
        causation_id,
    };
    let span = tracing::info_span!(
        "invocation",
        { keys::FUNCTION } = %entry.address,
        { keys::VERSION } = function.version(),
        { keys::EXECUTION_ID } = %invocation_id,
        { keys::CORRELATION_ID } = tracing::field::Empty,
    );
    if let Some(correlation) = call.header("x-correlation-id") {
        span.record(keys::CORRELATION_ID, correlation.as_str());
    }

    shared.metrics.entered(&entry.address);
    let worker = Worker {
        shared: shared.clone(),
        address: entry.address.clone(),
        _grant: grant,
        _in_flight: in_flight,
    };
    let version = function.version();
    let started = Instant::now();
    let mut task = tokio::spawn(
        async move {
            let _worker = worker; // released only when the invocation really ends
            function.instance().invoke(context).await
        }
        .instrument(span.clone()),
    );

    let outcome = tokio::time::timeout(timeout, &mut task)
        .instrument(span.clone())
        .await;
    let _entered = span.enter();
    let (answer, outcome_label) = match outcome {
        Ok(Ok(Ok(response))) => {
            let answer = HttpAnswer::from_response(response);
            let label = outcome_for(answer.status);
            (answer, label)
        }
        Ok(Ok(Err(InvokeError::Timeout))) => (timed_out(), "timeout"),
        Ok(Ok(Err(InvokeError::Failed(detail)))) => {
            tracing::warn!(address = %entry.address, invocation_id = %invocation_id, err = %detail, "function invocation failed");
            (function_error(), "error")
        }
        Ok(Ok(Err(InvokeError::Unavailable(detail)))) => {
            tracing::warn!(address = %entry.address, invocation_id = %invocation_id, err = %detail, "function could not run the invocation");
            (unavailable(), "unavailable")
        }
        Ok(Err(join_error)) => {
            tracing::warn!(address = %entry.address, invocation_id = %invocation_id, err = %join_error, "function invocation panicked");
            (function_error(), "error")
        }
        Err(_elapsed) => {
            // Interrupt, and answer now; the permits stay with the task
            // until the invocation actually returns.
            interrupted.cancel();
            (timed_out(), "timeout")
        }
    };
    shared.metrics.completed(
        &entry.address,
        version,
        outcome_label,
        started.elapsed(),
        entry_kind,
    );
    answer
}

/// The live registry (unversioned), or the pinned LRU (versioned, and
/// alias-prefixed public calls), plus the in-flight mark. A version closing
/// under us (a promote) is retried once against what replaced it.
async fn load(
    shared: &Arc<Shared>,
    entry: &Entry,
    versioned: bool,
) -> Option<(Arc<LoadedFunction>, crate::loader::InFlight)> {
    for _ in 0..2 {
        let function = if versioned {
            shared.pinned.get_or_load(entry).await
        } else {
            shared.reconciler.ensure_loaded(&entry.address).await
        }?;
        if let Some(in_flight) = function.retain() {
            return Some((function, in_flight));
        }
    }
    None
}

fn unavailable() -> HttpAnswer {
    HttpAnswer::error(
        503,
        "FUNCTION_UNAVAILABLE",
        "the function could not be loaded",
    )
    .with_header("Retry-After", "15")
}

fn timed_out() -> HttpAnswer {
    HttpAnswer::error(
        504,
        "FUNCTION_TIMEOUT",
        "the invocation exceeded its deadline",
    )
}

fn function_error() -> HttpAnswer {
    HttpAnswer::error(500, "FUNCTION_ERROR", "the function failed")
}

// ── the request, as the function sees it ─────────────────────────────────

/// Headers in arrival order, repeated names grouped; the routing header is
/// always dropped, the auth headers when the host consumed them.
fn collect_headers(headers: &HeaderMap, strip_auth_headers: bool) -> MultiMap {
    let mut out = MultiMap::new();
    for (name, value) in headers {
        let name = name.as_str();
        if name == INBOUND_ROUTING_HEADER
            || (strip_auth_headers && CONSUMED_AUTH_HEADERS.contains(&name))
        {
            continue;
        }
        out.entry(name.to_owned())
            .or_default()
            .push(latin1(value.as_bytes()));
    }
    out
}

/// `+` is a space here (a query, not a path); repeated keys kept in order.
fn collect_query(query: Option<&str>) -> MultiMap {
    let mut out = MultiMap::new();
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or("").as_bytes()) {
        out.entry(key.into_owned())
            .or_default()
            .push(value.into_owned());
    }
    out
}

/// Buffers the body under `cap`: a `Content-Length` over it is refused
/// before anything is read; a chunked body is cut off the moment it
/// crosses it. A refused body is drained in the background (bounded) so
/// the caller reads the `413` rather than a connection reset.
async fn read_body(headers: &HeaderMap, body: Incoming, cap: u64) -> Result<Bytes, HttpAnswer> {
    if let Some(declared) = headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        if declared > cap {
            discard(body);
            return Err(body_too_large());
        }
    }
    let mut body = body;
    let mut buffer = BytesMut::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|e| {
            tracing::debug!(err = %e, "request body read failed");
            HttpAnswer::error(400, "BAD_REQUEST", "failed reading the request body")
        })?;
        if let Ok(data) = frame.into_data() {
            if (buffer.len() + data.len()) as u64 > cap {
                discard(body);
                return Err(body_too_large());
            }
            buffer.extend_from_slice(&data);
        }
    }
    Ok(buffer.freeze())
}

/// The most a refused body is read (and thrown away) for, in bytes and time.
const DISCARD_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const DISCARD_LIMIT: Duration = Duration::from_secs(10);

fn discard(mut body: Incoming) {
    tokio::spawn(async move {
        let _ = tokio::time::timeout(DISCARD_LIMIT, async {
            let mut read = 0usize;
            while let Some(Ok(frame)) = body.frame().await {
                read += frame.data_ref().map_or(0, |d| d.len());
                if read > DISCARD_LIMIT_BYTES {
                    return;
                }
            }
        })
        .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_decoding_keeps_repeats_and_reads_plus_as_space() {
        let query = collect_query(Some("y=hello+world&y=again&z=%41&flag&&"));
        assert_eq!(query["y"], ["hello world", "again"]);
        assert_eq!(query["z"], ["A"]);
        assert_eq!(query["flag"], [""]);
        assert_eq!(query.keys().collect::<Vec<_>>(), ["y", "z", "flag"]);
    }

    #[test]
    fn header_collection_strips_only_what_was_consumed() {
        let mut headers = HeaderMap::new();
        headers.append("authorization", "Bearer x".parse().unwrap());
        headers.append("x-flowcatalyst-function", "a.b.c".parse().unwrap());
        headers.append("x-multi", "1".parse().unwrap());
        headers.append("x-multi", "2".parse().unwrap());
        headers.append(
            "x-latin",
            http::HeaderValue::from_bytes(b"caf\xe9").unwrap(),
        );
        let kept = collect_headers(&headers, false);
        assert_eq!(
            kept.keys().collect::<Vec<_>>(),
            ["authorization", "x-multi", "x-latin"]
        );
        assert_eq!(kept["x-multi"], ["1", "2"]);
        assert_eq!(kept["x-latin"], ["café"]);
        let stripped = collect_headers(&headers, true);
        assert!(!stripped.contains_key("authorization"));
    }
}
