//! The platform surface the host talks to (Java
//! `fnhost/reconcile/{ControlPlane,ControlPlaneException,HttpControlPlane}.java`):
//! `/control/functions/{desired-state,heartbeat,events}` with a bearer from
//! [`TokenSource`]. A 401 re-mints the token once and retries the same
//! request once; a second 401 is `UNAUTHORIZED`. Every other failure is
//! `UNAVAILABLE`, which the reconciler treats as "keep serving what is
//! loaded". The wire formats are Java's, unchanged, so this client works
//! against the Java platform as-is.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fc_function_abi::{EventEmitError, FunctionAddress};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use reqwest::{Client, Response, StatusCode};
use serde::Serialize;
use serde_json::Value;

use crate::desired::DesiredDocument;
use crate::heartbeat::HeartbeatReport;
use crate::log_throttle::LogThrottle;
use crate::token::TokenSource;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPlaneErrorReason {
    /// Non-2xx/304, an I/O error or a timeout. Routine: a platform outage
    /// never unloads a function.
    Unavailable,
    /// A second 401 after one token refresh and one retry.
    Unauthorized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPlaneError {
    pub reason: ControlPlaneErrorReason,
    pub message: String,
}

impl ControlPlaneError {
    pub fn new(reason: ControlPlaneErrorReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ControlPlaneErrorReason::Unavailable, message)
    }
}

impl fmt::Display for ControlPlaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.reason, self.message)
    }
}

impl std::error::Error for ControlPlaneError {}

/// The outcome of one desired-state call.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    /// 304: the known ETag is still current.
    NotModified,
    /// 200 with a fresh ETag and the parsed document.
    Changed {
        etag: String,
        document: DesiredDocument,
    },
}

/// One `POST /control/functions/events` call: always a batch of one, spoken
/// for the loaded `version` of `address`.
#[derive(Debug, Clone, PartialEq)]
pub struct EmitRequest {
    pub host_id: String,
    pub address: FunctionAddress,
    pub version: i32,
    pub events: Vec<EmitItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmitItem {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub dedup_id: String,
    /// The event's `data` member, as JSON.
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireEmit<'a> {
    host_id: &'a str,
    address: String,
    version: i32,
    events: &'a [EmitItem],
}

impl EmitRequest {
    pub fn to_json(&self) -> String {
        serde_json::to_string(&WireEmit {
            host_id: &self.host_id,
            address: self.address.render(),
            version: self.version,
            events: &self.events,
        })
        .expect("an emit request always serialises")
    }
}

#[async_trait]
pub trait ControlPlane: Send + Sync {
    /// `GET /control/functions/desired-state?pool=<pool>`, with
    /// `If-None-Match` when an ETag is known.
    async fn desired_state(
        &self,
        pool: &str,
        known_etag: Option<&str>,
    ) -> Result<Fetched, ControlPlaneError>;

    /// `POST /control/functions/heartbeat`; 204 expected.
    async fn heartbeat(&self, report: &HeartbeatReport) -> Result<(), ControlPlaneError>;

    /// `POST /control/functions/events` on a function's behalf (used by the
    /// runtime's `fc_emit_event`). Accepted: the id the platform stored the
    /// event under (`""` when its answer could not be read). A non-2xx
    /// carries the platform's `error` code, status and `message`; a
    /// transport failure is `UNAVAILABLE`/503 with what failed.
    async fn emit(&self, request: &EmitRequest) -> Result<String, EventEmitError>;
}

/// [`ControlPlane`] over HTTP.
pub struct HttpControlPlane {
    client: Client,
    platform_url: String,
    token_source: Arc<TokenSource>,
}

impl HttpControlPlane {
    pub fn new(
        client: Client,
        platform_url: impl Into<String>,
        token_source: Arc<TokenSource>,
    ) -> Self {
        Self {
            client,
            platform_url: platform_url.into(),
            token_source,
        }
    }

    /// A client with Java's control-plane timeouts: 5 s connect, 30 s request.
    pub fn default_client() -> Client {
        Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("a plain reqwest client builds")
    }

    /// Sends `build(token)`; on a 401, refreshes once and resends once.
    async fn send_with_auth(
        &self,
        what: &str,
        build: impl Fn(&str) -> reqwest::RequestBuilder,
    ) -> Result<Response, ControlPlaneError> {
        let token = self.token_source.token().await?;
        let response = send(what, build(&token)).await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        tracing::debug!("control plane rejected the bearer token; refreshing and retrying once");
        let token = self.token_source.refresh().await?;
        let response = send(what, build(&token)).await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ControlPlaneError::new(
                ControlPlaneErrorReason::Unauthorized,
                "control plane rejected the refreshed bearer token",
            ));
        }
        Ok(response)
    }
}

async fn send(what: &str, request: reqwest::RequestBuilder) -> Result<Response, ControlPlaneError> {
    request.send().await.map_err(|_| {
        ControlPlaneError::unavailable(format!("control plane request failed: {what}"))
    })
}

fn unavailable(what: &str, status: StatusCode) -> ControlPlaneError {
    ControlPlaneError::unavailable(format!("{what}: unexpected status {}", status.as_u16()))
}

#[async_trait]
impl ControlPlane for HttpControlPlane {
    async fn desired_state(
        &self,
        pool: &str,
        known_etag: Option<&str>,
    ) -> Result<Fetched, ControlPlaneError> {
        let url = format!(
            "{}/control/functions/desired-state?pool={pool}",
            self.platform_url
        );
        let response = self
            .send_with_auth("GET /control/functions/desired-state", |token| {
                let mut request = self
                    .client
                    .get(&url)
                    .header(AUTHORIZATION, format!("Bearer {token}"));
                if let Some(etag) = known_etag {
                    request = request.header(IF_NONE_MATCH, etag);
                }
                request
            })
            .await?;
        let status = response.status();
        if status == StatusCode::NOT_MODIFIED {
            return Ok(Fetched::NotModified);
        }
        if status != StatusCode::OK {
            return Err(unavailable("desired-state", status));
        }
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| unavailable("desired-state response carried no ETag", status))?;
        let body = response.text().await.map_err(|_| {
            ControlPlaneError::unavailable("desired-state response body could not be read")
        })?;
        let document = DesiredDocument::parse(&body).map_err(|_| {
            ControlPlaneError::unavailable("desired-state response was not a readable document")
        })?;
        Ok(Fetched::Changed { etag, document })
    }

    async fn heartbeat(&self, report: &HeartbeatReport) -> Result<(), ControlPlaneError> {
        let url = format!("{}/control/functions/heartbeat", self.platform_url);
        let body = report.to_json();
        let response = self
            .send_with_auth("POST /control/functions/heartbeat", |token| {
                self.client
                    .post(&url)
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header(CONTENT_TYPE, "application/json")
                    .body(body.clone())
            })
            .await?;
        if response.status() != StatusCode::NO_CONTENT {
            return Err(unavailable("heartbeat", response.status()));
        }
        Ok(())
    }

    async fn emit(&self, request: &EmitRequest) -> Result<String, EventEmitError> {
        let url = format!("{}/control/functions/events", self.platform_url);
        let body = request.to_json();
        let post = |token: &str| {
            self.client
                .post(&url)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .header(CONTENT_TYPE, "application/json")
                .body(body.clone())
        };
        let token = self
            .token_source
            .token()
            .await
            .map_err(|e| emit_unavailable("minting a control-plane token failed", &e))?;
        let mut response = post(&token).send().await.map_err(|e| {
            emit_unavailable(
                "control plane request failed: POST /control/functions/events",
                &e,
            )
        })?;
        if response.status() == StatusCode::UNAUTHORIZED {
            tracing::debug!(
                "control plane rejected the bearer token on emit; refreshing and retrying once"
            );
            let token = self
                .token_source
                .refresh()
                .await
                .map_err(|e| emit_unavailable("refreshing a control-plane token failed", &e))?;
            response = post(&token).send().await.map_err(|e| {
                emit_unavailable(
                    "control plane request failed: POST /control/functions/events",
                    &e,
                )
            })?;
        }
        let status = response.status();
        // An unreadable body reads as empty: a 2xx is still an accepted event.
        let body = response.bytes().await.unwrap_or_default();
        if status.is_success() {
            return Ok(emitted_id(&body));
        }
        Err(refusal(status.as_u16(), &body))
    }
}

/// Spec §3: a 2xx body is the ingest routes' `{results: [{id, status}]}`,
/// one item per event, and this host always sends a batch of one. An
/// unreadable body still means the platform accepted the event, so the id
/// falls back to `""` rather than turning an accepted emit into a refusal
/// the function might retry into a duplicate (Java 571fdff1).
fn emitted_id(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|body| {
            body.pointer("/results/0/id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

/// The platform's own `{error, message}` envelope read back into a refusal
/// naming its code, this response's status and the platform's reason. An
/// unreadable body still carries a real status, so the code falls back to
/// `UNKNOWN` rather than losing it (Java 67b04a51).
fn refusal(status: u16, body: &[u8]) -> EventEmitError {
    let body = serde_json::from_slice::<Value>(body).ok();
    let text = |field: &str| {
        body.as_ref()
            .and_then(|b| b.get(field))
            .and_then(Value::as_str)
            .filter(|v| !crate::java::is_blank(v))
            .map(str::to_owned)
    };
    EventEmitError::new(
        text("error").unwrap_or_else(|| "UNKNOWN".to_owned()),
        status,
    )
    .with_message(text("message").unwrap_or_default())
}

/// A transport or token failure: the function gets the `UNAVAILABLE` code it
/// branches on, and the host operator gets the cause, logged at WARN and
/// throttled (a platform outage fails every emit).
fn emit_unavailable(what: &str, cause: &dyn std::fmt::Display) -> EventEmitError {
    static EMIT_FAILURE_LOG: LogThrottle = LogThrottle::new(Duration::from_secs(10));
    if let Some(suppressed) = EMIT_FAILURE_LOG.admit() {
        tracing::warn!(
            what,
            error = %cause,
            suppressed_since_last = suppressed,
            "a function's event emit could not reach the platform"
        );
    }
    EventEmitError::unavailable().with_message(format!("{what}: {cause}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn emit_wire_shape_matches_java() {
        let request = EmitRequest {
            host_id: "h".into(),
            address: FunctionAddress::parse("a.b.c").unwrap(),
            version: 4,
            events: vec![EmitItem {
                event_type: "app:thing:happened".into(),
                subject: None,
                dedup_id: "d-1".into(),
                data: json!({"z": 1, "a": 2}),
                correlation_id: Some("corr".into()),
                causation_id: None,
                message_group: None,
            }],
        };
        assert_eq!(
            request.to_json(),
            r#"{"hostId":"h","address":"a.b.c","version":4,"events":[{"type":"app:thing:happened","dedupId":"d-1","data":{"z":1,"a":2},"correlationId":"corr"}]}"#
        );
    }
}
