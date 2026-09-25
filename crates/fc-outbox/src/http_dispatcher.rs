//! HTTP Dispatcher for FlowCatalyst API
//!
//! Sends outbox items to the platform's batch endpoints and classifies each
//! item's outcome, as Go's `HTTPDispatcher.SendBatch`
//! (`flowcatalyst-go/internal/outbox/http_dispatcher.go`):
//!
//! - `/api/events/batch` for EVENT items
//! - `/api/dispatch-jobs/batch` for DISPATCH_JOB items
//! - `/api/audit-logs/batch` for AUDIT_LOG items
//!
//! The request is `{"items": [payload, …]}`. A 2xx answer (200 or 201) is not
//! a blanket success: its `results` are matched to the items by **position**
//! (the result id is the platform's resource id, not the outbox row id). Any
//! other answer applies to every item of the request:
//!
//! | answer | status | retried |
//! |---|---|---|
//! | 400 | BAD_REQUEST | no |
//! | 401 | UNAUTHORIZED | yes |
//! | 403 | FORBIDDEN | no |
//! | 502, 503, 504, network error | GATEWAY_ERROR | yes |
//! | anything else (404, 409, 422, 429, 500, …) | INTERNAL_ERROR | yes |
//!
//! Beyond Go, three whole-batch refusals are split rather than applied to
//! every item, so one item can't keep good ones from being accepted:
//!
//! - 409 `DUPLICATE_ID` (owner decision #24: a supplied dispatch-job id that
//!   already names a job refuses the whole batch) and 400 `BATCH_TOO_LARGE`
//!   and 413 are retried as two halves, down to single items;
//! - a single item refused 409 `DUPLICATE_ID` whose payload `id` is its own
//!   outbox row id is SUCCESS: the platform already holds the job an earlier
//!   attempt created (its answer was lost), and the SDKs set that id for
//!   exactly this purpose.
//!
//! Requests never carry more than [`MAX_PLATFORM_BATCH`] items.

use async_trait::async_trait;
use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

/// The platform refuses a batch of more than this many items (400
/// `BATCH_TOO_LARGE`, Go and Rust alike).
pub const MAX_PLATFORM_BATCH: usize = 1000;

/// How much of an error answer's body is kept in `error_message`.
const ERROR_BODY_LIMIT: usize = 500;

/// HTTP dispatcher configuration
#[derive(Debug, Clone)]
pub struct HttpDispatcherConfig {
    /// FlowCatalyst API base URL
    pub api_base_url: String,
    /// Optional Bearer token for authentication
    pub api_token: Option<String>,
    /// Connect timeout
    pub connect_timeout: Duration,
    /// Request timeout (Go: 30s)
    pub request_timeout: Duration,
}

impl Default for HttpDispatcherConfig {
    fn default() -> Self {
        Self {
            api_base_url: "http://localhost:8080".to_string(),
            api_token: None,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// Batch request payload: each item is the row's raw payload.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest<'a> {
    pub items: Vec<&'a serde_json::Value>,
}

/// Batch response from the API: `{results:[{id,status,error?}]}`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResponse {
    pub results: Vec<ItemResult>,
}

/// Result for a single item
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemResult {
    #[serde(default)]
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// The status a per-item wire status stands for (Go `parseItemStatus`).
/// SKIPPED — the platform deliberately did not store the item (e.g. an
/// audit log with an unknown application) — is an acknowledged, terminal
/// outcome, so it clears the row like SUCCESS.
pub fn parse_item_status(status: &str) -> Option<OutboxStatus> {
    Some(match status {
        "SUCCESS" | "SKIPPED" => OutboxStatus::Success,
        "BAD_REQUEST" => OutboxStatus::BadRequest,
        "INTERNAL_ERROR" => OutboxStatus::InternalError,
        "UNAUTHORIZED" => OutboxStatus::Unauthorized,
        "FORBIDDEN" => OutboxStatus::Forbidden,
        "GATEWAY_ERROR" => OutboxStatus::GatewayError,
        _ => return None,
    })
}

/// The outcome of sending one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchOutcome {
    pub status: OutboxStatus,
    /// Why it failed; empty on success.
    pub message: String,
}

impl DispatchOutcome {
    pub fn success() -> Self {
        Self {
            status: OutboxStatus::Success,
            message: String::new(),
        }
    }

    pub fn failed(status: OutboxStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn is_success(&self) -> bool {
        self.status == OutboxStatus::Success
    }
}

/// Sends a batch of same-type items and reports one outcome per item, in
/// item order. The processor depends on this, not on HTTP.
#[async_trait]
pub trait OutboxDispatcher: Send + Sync {
    async fn send_batch(&self, items: &[OutboxItem]) -> Vec<DispatchOutcome>;
}

/// A whole-batch refusal that is retried as smaller batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitReason {
    /// 409 `DUPLICATE_ID`.
    DuplicateId,
    /// 400 `BATCH_TOO_LARGE`.
    TooLarge,
    /// 413: the body is over the platform's size limit.
    PayloadTooLarge,
}

/// What one request's answer means.
enum Answer {
    /// One outcome per item (a 2xx with matching results).
    PerItem(Vec<DispatchOutcome>),
    /// The same outcome for every item.
    Whole(DispatchOutcome),
    /// Retry as halves; with one item, `fallback` applies.
    Split {
        reason: SplitReason,
        fallback: DispatchOutcome,
    },
}

/// HTTP dispatcher that sends outbox items to FlowCatalyst API
pub struct HttpDispatcher {
    config: HttpDispatcherConfig,
    client: reqwest::Client,
}

impl HttpDispatcher {
    pub fn new(config: HttpDispatcherConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .build()?;

        Ok(Self { config, client })
    }

    /// Get the API endpoint for a given item type
    fn endpoint_for_type(&self, item_type: OutboxItemType) -> String {
        format!(
            "{}{}",
            self.config.api_base_url.trim_end_matches('/'),
            item_type.api_path()
        )
    }

    /// Sends same-type items and returns one outcome per item, in order.
    pub async fn send_outbox_batch(&self, items: &[OutboxItem]) -> Vec<DispatchOutcome> {
        let mut outcomes: Vec<Option<DispatchOutcome>> = vec![None; items.len()];

        // Ranges still to send; popped from the back, so pushed last-first.
        let mut pending: Vec<(usize, usize)> = (0..items.len())
            .step_by(MAX_PLATFORM_BATCH)
            .map(|start| (start, (start + MAX_PLATFORM_BATCH).min(items.len())))
            .collect();
        pending.reverse();

        while let Some((start, end)) = pending.pop() {
            let slice = &items[start..end];
            match self.post(slice).await {
                Answer::PerItem(per_item) => {
                    for (slot, outcome) in outcomes[start..end].iter_mut().zip(per_item) {
                        *slot = Some(outcome);
                    }
                }
                Answer::Whole(outcome) => {
                    for slot in &mut outcomes[start..end] {
                        *slot = Some(outcome.clone());
                    }
                }
                Answer::Split { reason, .. } if slice.len() > 1 => {
                    let mid = start + slice.len() / 2;
                    debug!(
                        ?reason,
                        items = slice.len(),
                        "Outbox batch refused as a whole; retrying as two halves"
                    );
                    pending.push((mid, end));
                    pending.push((start, mid));
                }
                Answer::Split { reason, fallback } => {
                    let item = &slice[0];
                    let outcome = if reason == SplitReason::DuplicateId && carries_own_id(item) {
                        debug!(
                            id = %item.id,
                            "Platform already holds this outbox item's id; acknowledged"
                        );
                        DispatchOutcome::success()
                    } else {
                        fallback
                    };
                    outcomes[start] = Some(outcome);
                }
            }
        }

        outcomes
            .into_iter()
            .map(|o| {
                o.unwrap_or_else(|| {
                    DispatchOutcome::failed(OutboxStatus::InternalError, "no outcome for item")
                })
            })
            .collect()
    }

    /// One request for `items` (all the same type, at most
    /// [`MAX_PLATFORM_BATCH`]).
    async fn post(&self, items: &[OutboxItem]) -> Answer {
        let url = self.endpoint_for_type(items[0].item_type);
        let body = BatchRequest {
            items: items.iter().map(|item| &item.payload).collect(),
        };

        debug!(count = items.len(), item_type = %items[0].item_type, %url, "Sending outbox batch");

        let mut request = self.client.post(&url).json(&body);
        if let Some(token) = self.config.api_token.as_deref().filter(|t| !t.is_empty()) {
            request = request.bearer_auth(token);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                warn!(error = %e, %url, "Outbox batch request failed");
                return Answer::Whole(DispatchOutcome::failed(
                    OutboxStatus::GatewayError,
                    format!("request: {e}"),
                ));
            }
        };

        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();

        if (200..300).contains(&status) {
            return Answer::PerItem(match per_item_outcomes(&body, items.len()) {
                Ok(outcomes) => outcomes,
                Err(outcome) => vec![outcome; items.len()],
            });
        }

        warn!(status, body = %truncate(&body, ERROR_BODY_LIMIT), %url, "Outbox batch refused");
        let message = if body.trim().is_empty() {
            status.to_string()
        } else {
            format!("{status}: {}", truncate(body.trim(), ERROR_BODY_LIMIT))
        };
        let outcome = DispatchOutcome::failed(status_for_http(status), message);

        let reason = match (status, error_code(&body).as_deref()) {
            (409, Some("DUPLICATE_ID")) => Some(SplitReason::DuplicateId),
            (400, Some("BATCH_TOO_LARGE")) => Some(SplitReason::TooLarge),
            (413, _) => Some(SplitReason::PayloadTooLarge),
            _ => None,
        };
        match reason {
            Some(reason) => Answer::Split {
                reason,
                fallback: outcome,
            },
            None => Answer::Whole(outcome),
        }
    }
}

#[async_trait]
impl OutboxDispatcher for HttpDispatcher {
    async fn send_batch(&self, items: &[OutboxItem]) -> Vec<DispatchOutcome> {
        if items.is_empty() {
            return Vec::new();
        }
        self.send_outbox_batch(items).await
    }
}

/// The status a non-2xx answer gives every item (Go `SendBatch`).
pub fn status_for_http(status: u16) -> OutboxStatus {
    match status {
        400 => OutboxStatus::BadRequest,
        401 => OutboxStatus::Unauthorized,
        403 => OutboxStatus::Forbidden,
        502..=504 => OutboxStatus::GatewayError,
        _ => OutboxStatus::InternalError,
    }
}

/// Per-item outcomes from a 2xx body, matched by position. A body that
/// isn't a result list, or has the wrong number of results, fails every
/// item retryably rather than risk giving an outcome to the wrong row.
fn per_item_outcomes(body: &str, count: usize) -> Result<Vec<DispatchOutcome>, DispatchOutcome> {
    let parsed: BatchResponse = serde_json::from_str(body).map_err(|_| {
        DispatchOutcome::failed(
            OutboxStatus::InternalError,
            format!("parse results: {}", truncate(body, 200)),
        )
    })?;
    if parsed.results.len() != count {
        return Err(DispatchOutcome::failed(
            OutboxStatus::InternalError,
            format!(
                "result count mismatch: got {} for {} items",
                parsed.results.len(),
                count
            ),
        ));
    }
    Ok(parsed
        .results
        .into_iter()
        .map(|r| match parse_item_status(&r.status) {
            Some(OutboxStatus::Success) => DispatchOutcome::success(),
            Some(status) => DispatchOutcome::failed(status, r.error.unwrap_or_default()),
            None => DispatchOutcome::failed(
                OutboxStatus::InternalError,
                format!("unknown item status: {}", r.status),
            ),
        })
        .collect())
}

/// The `code` of a platform error body (`{"code": "...", ...}`), if any.
fn error_code(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("code")
        .and_then(|c| c.as_str())
        .map(str::to_string)
}

/// Whether the item's payload names the item's own outbox id as its `id`,
/// which the SDKs set on dispatch jobs so a resend can't create a second job.
fn carries_own_id(item: &OutboxItem) -> bool {
    item.payload.get("id").and_then(|v| v.as_str()) == Some(item.id.as_str())
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::{Arc, Mutex};

    fn item(id: &str, item_type: OutboxItemType, payload: serde_json::Value) -> OutboxItem {
        OutboxItem {
            id: id.to_string(),
            item_type,
            message_group: None,
            payload,
            status: OutboxStatus::InProgress,
            retry_count: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            error_message: None,
            client_id: None,
            payload_size: None,
            headers: None,
        }
    }

    fn job(id: &str) -> OutboxItem {
        item(
            id,
            OutboxItemType::DispatchJob,
            serde_json::json!({"id": id, "code": "c"}),
        )
    }

    type Handler = Arc<dyn Fn(&serde_json::Value) -> (StatusCode, serde_json::Value) + Send + Sync>;

    /// Each request's path and item ids.
    type Requests = Arc<Mutex<Vec<(String, Vec<String>)>>>;

    #[derive(Clone)]
    struct Platform {
        handler: Handler,
        /// Every request's item ids (payload `id`s), and its path.
        requests: Requests,
    }

    impl Platform {
        fn requests(&self) -> Vec<(String, Vec<String>)> {
            self.requests.lock().unwrap().clone()
        }
    }

    async fn answer(
        State(p): State<Platform>,
        uri: axum::http::Uri,
        Json(body): Json<serde_json::Value>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        let ids = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_str().unwrap_or_default().to_string())
            .collect();
        p.requests
            .lock()
            .unwrap()
            .push((uri.path().to_string(), ids));
        let (status, value) = (p.handler)(&body);
        (status, Json(value))
    }

    async fn platform(
        handler: impl Fn(&serde_json::Value) -> (StatusCode, serde_json::Value) + Send + Sync + 'static,
    ) -> (HttpDispatcher, Platform) {
        let platform = Platform {
            handler: Arc::new(handler),
            requests: Arc::default(),
        };
        let app = Router::new()
            .route("/api/events/batch", post(answer))
            .route("/api/dispatch-jobs/batch", post(answer))
            .route("/api/audit-logs/batch", post(answer))
            .with_state(platform.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dispatcher = HttpDispatcher::new(HttpDispatcherConfig {
            api_base_url: format!("http://{addr}"),
            api_token: Some("tok".into()),
            ..Default::default()
        })
        .unwrap();
        (dispatcher, platform)
    }

    fn all_success(body: &serde_json::Value) -> serde_json::Value {
        let n = body["items"].as_array().unwrap().len();
        serde_json::json!({
            "results": (0..n).map(|i| serde_json::json!({"id": format!("res{i}"), "status": "SUCCESS"})).collect::<Vec<_>>()
        })
    }

    #[test]
    fn item_statuses_parse_as_go() {
        assert_eq!(parse_item_status("SUCCESS"), Some(OutboxStatus::Success));
        assert_eq!(parse_item_status("SKIPPED"), Some(OutboxStatus::Success));
        assert_eq!(
            parse_item_status("BAD_REQUEST"),
            Some(OutboxStatus::BadRequest)
        );
        assert_eq!(
            parse_item_status("GATEWAY_ERROR"),
            Some(OutboxStatus::GatewayError)
        );
        assert_eq!(parse_item_status("success"), None);
    }

    #[test]
    fn http_statuses_classify_as_go() {
        assert_eq!(status_for_http(400), OutboxStatus::BadRequest);
        assert_eq!(status_for_http(401), OutboxStatus::Unauthorized);
        assert_eq!(status_for_http(403), OutboxStatus::Forbidden);
        for s in [502, 503, 504] {
            assert_eq!(status_for_http(s), OutboxStatus::GatewayError);
        }
        for s in [404, 409, 413, 422, 429, 500, 501] {
            assert_eq!(status_for_http(s), OutboxStatus::InternalError, "{s}");
        }
        // Retryability is the status's (fc-common, as Go's IsRetryable).
        assert!(!OutboxStatus::BadRequest.is_retryable());
        assert!(!OutboxStatus::Forbidden.is_retryable());
        assert!(OutboxStatus::Unauthorized.is_retryable());
        assert!(OutboxStatus::GatewayError.is_retryable());
        assert!(OutboxStatus::InternalError.is_retryable());
    }

    #[tokio::test]
    async fn a_201_is_matched_to_items_by_position() {
        let (d, p) = platform(|_| {
            (
                StatusCode::CREATED,
                serde_json::json!({"results": [
                    {"id": "evt1", "status": "SUCCESS"},
                    {"id": "", "status": "BAD_REQUEST", "error": "bad type"},
                    {"id": "evt3", "status": "SKIPPED"},
                ]}),
            )
        })
        .await;
        let items = vec![
            item("r1", OutboxItemType::Event, serde_json::json!({"id": "r1"})),
            item("r2", OutboxItemType::Event, serde_json::json!({"id": "r2"})),
            item("r3", OutboxItemType::Event, serde_json::json!({"id": "r3"})),
        ];
        let out = d.send_batch(&items).await;
        assert_eq!(out[0], DispatchOutcome::success());
        assert_eq!(
            out[1],
            DispatchOutcome::failed(OutboxStatus::BadRequest, "bad type")
        );
        assert_eq!(out[2], DispatchOutcome::success());
        assert_eq!(
            p.requests(),
            vec![(
                "/api/events/batch".to_string(),
                vec!["r1".into(), "r2".into(), "r3".into()]
            )]
        );
    }

    #[tokio::test]
    async fn a_result_count_mismatch_fails_every_item_retryably() {
        let (d, _) = platform(|_| {
            (
                StatusCode::OK,
                serde_json::json!({"results": [{"id": "x", "status": "SUCCESS"}]}),
            )
        })
        .await;
        let out = d.send_batch(&[job("a"), job("b")]).await;
        for o in out {
            assert_eq!(o.status, OutboxStatus::InternalError);
            assert!(o.message.contains("result count mismatch"));
        }
    }

    #[tokio::test]
    async fn an_unparseable_2xx_fails_every_item_retryably() {
        let (d, _) = platform(|_| (StatusCode::OK, serde_json::json!("ok"))).await;
        let out = d.send_batch(&[job("a")]).await;
        assert_eq!(out[0].status, OutboxStatus::InternalError);
        assert!(out[0].message.starts_with("parse results"));
    }

    #[tokio::test]
    async fn whole_batch_answers_apply_to_every_item() {
        for (status, expected) in [
            (StatusCode::BAD_REQUEST, OutboxStatus::BadRequest),
            (StatusCode::UNAUTHORIZED, OutboxStatus::Unauthorized),
            (StatusCode::FORBIDDEN, OutboxStatus::Forbidden),
            (StatusCode::NOT_FOUND, OutboxStatus::InternalError),
            (StatusCode::CONFLICT, OutboxStatus::InternalError),
            (StatusCode::TOO_MANY_REQUESTS, OutboxStatus::InternalError),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                OutboxStatus::InternalError,
            ),
            (StatusCode::BAD_GATEWAY, OutboxStatus::GatewayError),
            (StatusCode::SERVICE_UNAVAILABLE, OutboxStatus::GatewayError),
            (StatusCode::GATEWAY_TIMEOUT, OutboxStatus::GatewayError),
        ] {
            let (d, p) = platform(move |_| {
                (
                    status,
                    serde_json::json!({"code": "FORBIDDEN", "message": "No access to client: c1"}),
                )
            })
            .await;
            let out = d.send_batch(&[job("a"), job("b")]).await;
            assert_eq!(out.len(), 2);
            for o in &out {
                assert_eq!(o.status, expected, "{status}");
                assert!(o.message.starts_with(&status.as_u16().to_string()));
            }
            // Sent once, never split.
            assert_eq!(p.requests().len(), 1, "{status}");
        }
    }

    #[tokio::test]
    async fn a_network_error_is_a_gateway_error() {
        let d = HttpDispatcher::new(HttpDispatcherConfig {
            api_base_url: "http://127.0.0.1:1".into(),
            ..Default::default()
        })
        .unwrap();
        let out = d.send_batch(&[job("a")]).await;
        assert_eq!(out[0].status, OutboxStatus::GatewayError);
        assert!(out[0].message.starts_with("request: "));
    }

    /// The platform as it answers dispatch-job batches: 409 DUPLICATE_ID for
    /// the whole batch when any id is already stored.
    fn jobs_platform(
        stored: Arc<Mutex<Vec<String>>>,
    ) -> impl Fn(&serde_json::Value) -> (StatusCode, serde_json::Value) + Send + Sync + 'static
    {
        move |body| {
            let ids: Vec<String> = body["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["id"].as_str().unwrap_or_default().to_string())
                .collect();
            let mut stored = stored.lock().unwrap();
            let taken: Vec<&String> = ids.iter().filter(|id| stored.contains(id)).collect();
            if !taken.is_empty() {
                return (
                    StatusCode::CONFLICT,
                    serde_json::json!({"code": "DUPLICATE_ID", "message": "dispatch job id already exists"}),
                );
            }
            stored.extend(ids.iter().cloned());
            (StatusCode::CREATED, all_success(body))
        }
    }

    #[tokio::test]
    async fn a_duplicate_id_is_isolated_and_the_good_items_are_accepted() {
        let stored = Arc::new(Mutex::new(vec!["b".to_string()]));
        let (d, p) = platform(jobs_platform(stored.clone())).await;
        let items = vec![job("a"), job("b"), job("c"), job("d")];
        let out = d.send_batch(&items).await;
        // "b" was already accepted (an earlier attempt whose answer was
        // lost): acknowledged. The rest are stored now.
        assert!(out.iter().all(DispatchOutcome::is_success), "{out:?}");
        let mut stored = stored.lock().unwrap().clone();
        stored.sort();
        assert_eq!(stored, vec!["a", "b", "c", "d"]);
        // [a b c d] → 409; [a b] → 409, [c d] ok; [a] ok, [b] → 409 (own id).
        let sent: Vec<Vec<String>> = p.requests().into_iter().map(|(_, ids)| ids).collect();
        assert_eq!(sent.len(), 5, "{sent:?}");
        assert_eq!(sent[0], vec!["a", "b", "c", "d"]);
    }

    #[tokio::test]
    async fn a_duplicate_id_that_is_not_the_items_own_is_not_acknowledged() {
        let stored = Arc::new(Mutex::new(vec!["someone-elses".to_string()]));
        let (d, _) = platform(jobs_platform(stored)).await;
        let foreign = item(
            "row1",
            OutboxItemType::DispatchJob,
            serde_json::json!({"id": "someone-elses"}),
        );
        let out = d.send_batch(&[foreign]).await;
        assert_eq!(out[0].status, OutboxStatus::InternalError);
        assert!(out[0].message.contains("DUPLICATE_ID"));
    }

    #[tokio::test]
    async fn batch_too_large_is_split_until_it_fits() {
        let (d, p) = platform(|body| {
            if body["items"].as_array().unwrap().len() > 2 {
                (
                    StatusCode::BAD_REQUEST,
                    serde_json::json!({"code": "BATCH_TOO_LARGE", "message": "max 2 items per batch"}),
                )
            } else {
                (StatusCode::CREATED, all_success(body))
            }
        })
        .await;
        let items: Vec<OutboxItem> = (0..5).map(|i| job(&format!("j{i}"))).collect();
        let out = d.send_batch(&items).await;
        assert!(out.iter().all(DispatchOutcome::is_success), "{out:?}");
        let accepted: Vec<String> = p
            .requests()
            .into_iter()
            .filter(|(_, ids)| ids.len() <= 2)
            .flat_map(|(_, ids)| ids)
            .collect();
        assert_eq!(accepted, vec!["j0", "j1", "j2", "j3", "j4"]);
    }

    #[tokio::test]
    async fn a_plain_400_is_not_split() {
        let (d, p) = platform(|_| {
            (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"code": "INVALID_ID", "message": "bad id"}),
            )
        })
        .await;
        let out = d.send_batch(&[job("a"), job("b")]).await;
        assert!(out.iter().all(|o| o.status == OutboxStatus::BadRequest));
        assert_eq!(p.requests().len(), 1);
    }

    #[tokio::test]
    async fn requests_never_exceed_the_platform_limit() {
        let (d, p) = platform(|body| (StatusCode::CREATED, all_success(body))).await;
        let items: Vec<OutboxItem> = (0..MAX_PLATFORM_BATCH + 5)
            .map(|i| {
                item(
                    &format!("e{i}"),
                    OutboxItemType::Event,
                    serde_json::json!({}),
                )
            })
            .collect();
        let out = d.send_batch(&items).await;
        assert_eq!(out.len(), MAX_PLATFORM_BATCH + 5);
        assert!(out.iter().all(DispatchOutcome::is_success));
        let sizes: Vec<usize> = p.requests().iter().map(|(_, ids)| ids.len()).collect();
        assert_eq!(sizes, vec![MAX_PLATFORM_BATCH, 5]);
    }

    #[tokio::test]
    async fn each_type_goes_to_its_endpoint() {
        let (d, p) = platform(|body| (StatusCode::CREATED, all_success(body))).await;
        for t in OutboxItemType::ALL {
            d.send_batch(&[item("x", t, serde_json::json!({}))]).await;
        }
        let paths: Vec<String> = p.requests().into_iter().map(|(path, _)| path).collect();
        assert_eq!(
            paths,
            vec![
                "/api/events/batch",
                "/api/dispatch-jobs/batch",
                "/api/audit-logs/batch"
            ]
        );
    }
}
