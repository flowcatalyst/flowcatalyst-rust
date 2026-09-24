//! The runtime-agnostic invocation seam between the listeners and a loaded
//! function (Java's `LoadedFunction.invoke(Request, FunctionContext)` plus
//! `InvocationRunner`, with the guest contract left open).
//!
//! The listener builds one [`InvocationContext`] per call (everything Java
//! puts in `Request`, plus the deadline, an interrupt signal and the emit
//! defaults) and hands it to the loaded version's [`Invoker`]. The runtime
//! decides how the guest sees it: Java's Extism JSON envelope, a
//! `wasi:http` request, or anything else. What comes back is a
//! [`Response`] (status, headers, body) or an [`InvokeError`].
//!
//! The listener owns everything around the call: permits, the deadline
//! (504 `FUNCTION_TIMEOUT`), and keeping the permits until the invocation
//! future actually finishes. An invoker should treat
//! [`InvocationContext::interrupted`] as Java's thread interrupt: stop as
//! soon as it can. One that ignores it keeps its permits until it returns.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use fc_function_abi::{Caller, FunctionAddress, MultiMap, Response, Webhook};
use indexmap::IndexMap;
use tokio_util::sync::CancellationToken;

/// How the host calls a loaded version. Implemented by every runtime's
/// [`FunctionInstance`](crate::loader::FunctionInstance).
#[async_trait]
pub trait Invoker: Send + Sync {
    /// Runs one invocation. A panic is contained by the listener and
    /// answered like [`InvokeError::Failed`].
    async fn invoke(&self, context: InvocationContext) -> Result<Response, InvokeError>;
}

/// Why an invocation produced no [`Response`]. The listener never shows
/// the detail to the caller; it is logged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvokeError {
    /// The runtime stopped the guest at the deadline (for example an epoch
    /// interrupt): `504 FUNCTION_TIMEOUT`, outcome `timeout`.
    #[error("the invocation exceeded its deadline")]
    Timeout,
    /// The guest failed (trapped, threw, answered something that is not a
    /// response): `500 FUNCTION_ERROR`, outcome `error`.
    #[error("the function failed: {0}")]
    Failed(String),
    /// The runtime could not run the guest right now (for example no
    /// instance, or the version is closing): `503 FUNCTION_UNAVAILABLE` with
    /// `Retry-After: 15`, outcome `unavailable`.
    #[error("the function is unavailable: {0}")]
    Unavailable(String),
}

/// One invocation, as the listener hands it to the runtime. The fields are
/// Java's `Request` (spec `function-host-listener.md` §2), plus what
/// `InvocationRunner` binds around the call.
#[derive(Clone)]
pub struct InvocationContext {
    /// A fresh TSID, unique per attempt (Java `Request.invocationId`, the
    /// `execution_id` log field).
    pub invocation_id: String,
    pub address: FunctionAddress,
    /// The loaded version handling the call.
    pub version: i32,
    pub method: String,
    /// The function path: the request path with the
    /// `/functions/{address}[:{version}]` prefix, or the public route's
    /// prefix, stripped. Raw (not percent-decoded); always starts with `/`.
    pub path: String,
    /// The `Host` (or `:authority`) the call arrived on.
    pub original_host: Option<String>,
    /// The full request path as it arrived.
    pub original_path: Option<String>,
    /// Bound by the matched endpoint's pattern, percent-decoded.
    pub path_params: IndexMap<String, String>,
    /// Query parameters: `+` decoded as a space, repeated keys kept in order.
    pub query: MultiMap,
    /// The query string exactly as received (not decoded), without the `?`;
    /// `None` when the request had none. A runtime that hands the guest a
    /// raw HTTP request (`wasi:http`) uses this rather than re-encoding
    /// [`query`](Self::query).
    pub raw_query: Option<String>,
    /// Request headers as received, minus the ones the host consumed
    /// (`Authorization`, `X-FlowCatalyst-Signature`, `X-FlowCatalyst-Timestamp`
    /// on a `webhook` or `platform` endpoint and on a versioned call) and,
    /// always, `X-FlowCatalyst-Function`. Values are the received bytes read
    /// as ISO-8859-1, as Java's (Netty's) header decoding does.
    pub headers: MultiMap,
    pub body: Bytes,
    /// The TCP peer, or on the public listener the right-most
    /// `X-Forwarded-For` entry when the peer is a trusted proxy.
    pub remote_address: Option<String>,
    pub caller: Caller,
    /// When the listener stops waiting and answers `504`.
    pub deadline: Instant,
    /// Cancelled at the deadline: the invoker should stop.
    pub interrupted: CancellationToken,
    /// The default `correlationId` for events this invocation emits.
    pub correlation_id: String,
    /// The default `causationId` for events this invocation emits.
    pub causation_id: Option<String>,
}

impl InvocationContext {
    /// The first value of the first header whose name matches `name`
    /// case-insensitively (Java `Request.header`).
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }

    /// Time left until the deadline (zero once past it).
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
}

/// Body bytes and secrets never reach a log line.
impl std::fmt::Debug for InvocationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvocationContext")
            .field("invocation_id", &self.invocation_id)
            .field("address", &self.address.render())
            .field("version", &self.version)
            .field("method", &self.method)
            .field("path", &self.path)
            .field("original_host", &self.original_host)
            .field("original_path", &self.original_path)
            .field("path_params", &self.path_params)
            .field("query", &self.query)
            .field("raw_query", &self.raw_query)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("body", &format_args!("{} bytes", self.body.len()))
            .field("remote_address", &self.remote_address)
            .field("caller", &self.caller)
            .field("correlation_id", &self.correlation_id)
            .field("causation_id", &self.causation_id)
            .finish()
    }
}

pub(crate) fn header<'a>(headers: &'a MultiMap, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, values)| values.first())
        .map(String::as_str)
}

/// The emit defaults (Java `InvocationRunner.emitDefaults`, spec
/// `function-context.md` §3): `(correlationId, causationId)`.
///
/// `correlationId`: the inbound event's own `correlationId` when the call
/// is a verified webhook delivery ([`Caller::Platform`]) whose body parses
/// as an event envelope and carries a non-blank one; else the
/// `X-Correlation-Id` header; else the invocation id. `causationId`: that
/// inbound event's id, and nothing otherwise. An unverified caller's
/// envelope-shaped body never steers either.
pub fn emit_defaults(
    invocation_id: &str,
    headers: &MultiMap,
    caller: &Caller,
    body: &[u8],
) -> (String, Option<String>) {
    let mut correlation_id = header(headers, "X-Correlation-Id")
        .unwrap_or(invocation_id)
        .to_owned();
    let mut causation_id = None;
    if *caller == Caller::Platform {
        if let Ok(inbound) = Webhook::event(body) {
            causation_id = Some(inbound.id);
            if let Some(correlation) = inbound.correlation_id {
                if !crate::java::is_blank(&correlation) {
                    correlation_id = correlation;
                }
            }
        }
    }
    (correlation_id, causation_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(correlation: &str) -> Vec<u8> {
        format!(r#"{{"id":"evt-9","type":"a:b:c:d","attemptNumber":1{correlation},"data":{{}}}}"#)
            .into_bytes()
    }

    fn headers(pairs: &[(&str, &str)]) -> MultiMap {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), vec![(*v).to_owned()]))
            .collect()
    }

    // Java EmitDefaultsTest
    #[test]
    fn the_inbound_events_correlation_id_wins_over_the_header_and_the_invocation_id() {
        let (correlation, causation) = emit_defaults(
            "inv-1",
            &headers(&[("X-Correlation-Id", "hdr-1")]),
            &Caller::Platform,
            &envelope(r#","correlationId":"flow-7""#),
        );
        assert_eq!(correlation, "flow-7");
        assert_eq!(causation.as_deref(), Some("evt-9"));
    }

    #[test]
    fn without_an_inbound_correlation_id_the_header_is_used() {
        let (correlation, causation) = emit_defaults(
            "inv-1",
            &headers(&[("x-correlation-id", "hdr-1")]),
            &Caller::Platform,
            &envelope(""),
        );
        assert_eq!(correlation, "hdr-1");
        assert_eq!(causation.as_deref(), Some("evt-9"));
    }

    #[test]
    fn with_neither_the_invocation_id_is_used() {
        let (correlation, _) =
            emit_defaults("inv-1", &MultiMap::new(), &Caller::Platform, &envelope(""));
        assert_eq!(correlation, "inv-1");
    }

    #[test]
    fn an_unverified_callers_envelope_shaped_body_offers_neither_value() {
        let (correlation, causation) = emit_defaults(
            "inv-1",
            &MultiMap::new(),
            &Caller::Anonymous,
            &envelope(r#","correlationId":"forged""#),
        );
        assert_eq!(correlation, "inv-1");
        assert_eq!(causation, None);
    }

    #[test]
    fn a_verified_delivery_that_is_not_an_event_envelope_offers_no_causation() {
        let (correlation, causation) = emit_defaults(
            "inv-1",
            &MultiMap::new(),
            &Caller::Platform,
            br#"{"jobId":"sjb_1"}"#,
        );
        assert_eq!(causation, None);
        assert_eq!(correlation, "inv-1");
    }
}
