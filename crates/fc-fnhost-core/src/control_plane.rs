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
use fc_function_abi::{emit_error, EventEmitError, FunctionAddress};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use reqwest::{Client, Response, StatusCode};
use serde::Serialize;
use serde_json::Value;

use crate::desired::DesiredDocument;
use crate::heartbeat::HeartbeatReport;
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
    /// runtime's `fc_emit_event`). A non-2xx carries the platform's `error`
    /// code and status; a transport failure is `UNAVAILABLE`/503.
    async fn emit(&self, request: &EmitRequest) -> Result<(), EventEmitError>;
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

    async fn emit(&self, request: &EmitRequest) -> Result<(), EventEmitError> {
        let url = format!("{}/control/functions/events", self.platform_url);
        let body = request.to_json();
        let post = |token: &str| {
            self.client
                .post(&url)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .header(CONTENT_TYPE, "application/json")
                .body(body.clone())
        };
        let unavailable = || EventEmitError::new(emit_error::UNAVAILABLE, 503);
        let token = self.token_source.token().await.map_err(|_| unavailable())?;
        let mut response = post(&token).send().await.map_err(|_| unavailable())?;
        if response.status() == StatusCode::UNAUTHORIZED {
            tracing::debug!(
                "control plane rejected the bearer token on emit; refreshing and retrying once"
            );
            let token = self
                .token_source
                .refresh()
                .await
                .map_err(|_| unavailable())?;
            response = post(&token).send().await.map_err(|_| unavailable())?;
        }
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let code = response
            .json::<Value>()
            .await
            .ok()
            .and_then(|body| body.get("error").and_then(Value::as_str).map(str::to_owned))
            .filter(|code| !crate::java::is_blank(code))
            .unwrap_or_else(|| "UNKNOWN".to_owned());
        Err(EventEmitError::new(code, status.as_u16()))
    }
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
