//! Monitoring / dashboard read-side: overview, pool/queue stats, dashboard
//! health, circuit-breaker snapshots, and in-flight message views.

use super::AppState;
use crate::{CircuitBreakerState, InFlightMessageInfo, QueueMetrics};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use fc_common::{HealthReport, HealthStatus, PoolStats};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use utoipa::ToSchema;

/// The upper-case wire name of a [`HealthStatus`] in monitoring responses.
/// (`HealthStatus`'s own serde form is `Healthy`/`Warning`/`Degraded`, which
/// `HealthReport` still uses, so this can't be a serde rename.)
fn health_status_str(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Healthy => "HEALTHY",
        HealthStatus::Warning => "WARNING",
        HealthStatus::Degraded => "DEGRADED",
    }
}

/// Detailed monitoring response
#[derive(Serialize, ToSchema)]
pub struct MonitoringResponse {
    /// Overall status: HEALTHY, WARNING, DEGRADED
    pub status: String,
    /// Application version
    pub version: String,
    /// Detailed health report
    pub health_report: HealthReport,
    /// Pool statistics
    pub pool_stats: Vec<PoolStats>,
    /// Number of active (unacknowledged) warnings
    pub active_warnings: u32,
    /// Number of critical warnings
    pub critical_warnings: u32,
}

/// Response for queue metrics endpoint
#[derive(Serialize, ToSchema)]
pub struct QueueMetricsResponse {
    /// Queue identifier
    pub queue_identifier: String,
    /// Number of messages waiting in the queue
    pub pending_messages: u64,
    /// Number of messages currently being processed
    pub in_flight_messages: u64,
}

impl From<QueueMetrics> for QueueMetricsResponse {
    fn from(m: QueueMetrics) -> Self {
        QueueMetricsResponse {
            queue_identifier: m.queue_identifier,
            pending_messages: m.pending_messages,
            in_flight_messages: m.in_flight_messages,
        }
    }
}

/// Detailed monitoring information
#[utoipa::path(
    get,
    path = "/monitoring",
    tag = "monitoring",
    responses(
        (status = 200, description = "Monitoring data", body = MonitoringResponse)
    )
)]
pub(crate) async fn monitoring_handler(State(state): State<AppState>) -> Json<MonitoringResponse> {
    let pool_stats = state.queue_manager.get_pool_stats();
    let health_report = state.health_service.get_health_report(&pool_stats);
    let active_warnings = state.warning_service.unacknowledged_count() as u32;
    let critical_warnings = state.warning_service.critical_count() as u32;

    let status = health_status_str(health_report.status);

    Json(MonitoringResponse {
        status: status.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        health_report,
        pool_stats,
        active_warnings,
        critical_warnings,
    })
}

/// Pool statistics
#[utoipa::path(
    get,
    path = "/monitoring/pools",
    tag = "monitoring",
    responses(
        (status = 200, description = "Pool statistics", body = Vec<PoolStats>)
    )
)]
pub(crate) async fn pool_stats_handler(State(state): State<AppState>) -> Json<Vec<PoolStats>> {
    Json(state.queue_manager.get_pool_stats())
}

/// Queue metrics
#[utoipa::path(
    get,
    path = "/monitoring/queues",
    tag = "monitoring",
    responses(
        (status = 200, description = "Queue metrics", body = Vec<QueueMetricsResponse>)
    )
)]
pub(crate) async fn queue_metrics_handler(
    State(state): State<AppState>,
) -> Json<Vec<QueueMetricsResponse>> {
    let metrics = state.queue_manager.get_queue_metrics().await;
    Json(
        metrics
            .into_iter()
            .map(QueueMetricsResponse::from)
            .collect(),
    )
}

/// Dashboard health response
#[derive(Serialize, ToSchema)]
pub(crate) struct DashboardHealthResponse {
    status: String,
    timestamp: String,
    #[serde(rename = "uptimeMillis")]
    uptime_millis: u64,
    details: Option<DashboardHealthDetails>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct DashboardHealthDetails {
    #[serde(rename = "totalQueues")]
    total_queues: u32,
    #[serde(rename = "healthyQueues")]
    healthy_queues: u32,
    #[serde(rename = "totalPools")]
    total_pools: u32,
    #[serde(rename = "healthyPools")]
    healthy_pools: u32,
    #[serde(rename = "activeWarnings")]
    active_warnings: u32,
    #[serde(rename = "criticalWarnings")]
    critical_warnings: u32,
    #[serde(rename = "circuitBreakersOpen")]
    circuit_breakers_open: u32,
    #[serde(rename = "degradationReason")]
    degradation_reason: Option<String>,
}

static START_TIME: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn get_uptime_millis() -> u64 {
    START_TIME
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

/// Health endpoint for dashboard
#[utoipa::path(
    get,
    path = "/monitoring/health",
    tag = "monitoring",
    responses(
        (status = 200, description = "Dashboard health", body = DashboardHealthResponse)
    )
)]
pub(crate) async fn dashboard_health_handler(
    State(state): State<AppState>,
) -> Json<DashboardHealthResponse> {
    let pool_stats = state.queue_manager.get_pool_stats();
    let health_report = state.health_service.get_health_report(&pool_stats);

    let status = health_status_str(health_report.status);

    let degradation_reason = if !health_report.issues.is_empty() {
        Some(health_report.issues.join("; "))
    } else {
        None
    };

    // Count open circuit breakers from the registry
    let circuit_breakers_open = state
        .circuit_breaker_registry
        .get_all_stats()
        .values()
        .filter(|s| s.state == CircuitBreakerState::Open)
        .count() as u32;

    Json(DashboardHealthResponse {
        status: status.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        uptime_millis: get_uptime_millis(),
        details: Some(DashboardHealthDetails {
            total_queues: (health_report.consumers_healthy + health_report.consumers_unhealthy),
            healthy_queues: health_report.consumers_healthy,
            total_pools: (health_report.pools_healthy + health_report.pools_unhealthy),
            healthy_pools: health_report.pools_healthy,
            active_warnings: health_report.active_warnings,
            critical_warnings: health_report.critical_warnings,
            circuit_breakers_open,
            degradation_reason,
        }),
    })
}

/// Query parameters accepted by the dashboard stats endpoints.
#[derive(Deserialize, Default)]
pub(crate) struct DashboardStatsQuery {
    /// "5min" | "30min" | "all" | "all-time" (default: all-time).
    #[serde(default)]
    time_window: Option<String>,
    /// "true" forces a live SQS fetch before serving queue stats.
    #[serde(default)]
    refresh: Option<String>,
}

/// Parse the dashboard `time_window` query value. `None` means "all time".
fn parse_time_window(raw: Option<&str>) -> Option<Duration> {
    match raw.unwrap_or("").trim() {
        "5min" | "5m" => Some(Duration::from_secs(300)),
        "30min" | "30m" => Some(Duration::from_secs(1800)),
        // "all" | "all-time" | "" | unknown -> all-time
        _ => None,
    }
}

/// Queue stats for dashboard. Counts (`totalMessages`, `totalConsumed`,
/// `totalFailed`, `totalDeferred`, `successRate`) are scoped to the requested
/// time window. Live-state fields (`pendingMessages`, `messagesNotVisible`,
/// `currentSize`) always reflect the current queue state.
#[derive(Serialize, ToSchema)]
pub(crate) struct DashboardQueueStats {
    name: String,
    #[serde(rename = "totalMessages")]
    total_messages: u64,
    #[serde(rename = "totalConsumed")]
    total_consumed: u64,
    #[serde(rename = "totalFailed")]
    total_failed: u64,
    #[serde(rename = "totalDeferred")]
    total_deferred: u64,
    #[serde(rename = "successRate")]
    success_rate: f64,
    #[serde(rename = "currentSize")]
    current_size: u64,
    throughput: f64,
    #[serde(rename = "pendingMessages")]
    pending_messages: u64,
    #[serde(rename = "messagesNotVisible")]
    messages_not_visible: u64,
}

/// Queue stats endpoint for dashboard
#[utoipa::path(
    get,
    path = "/monitoring/queue-stats",
    tag = "monitoring",
    responses(
        (status = 200, description = "Queue stats for dashboard")
    )
)]
pub(crate) async fn dashboard_queue_stats_handler(
    State(state): State<AppState>,
    Query(params): Query<DashboardStatsQuery>,
) -> Json<HashMap<String, DashboardQueueStats>> {
    if params.refresh.as_deref() == Some("true") {
        state.cached_broker_stats.refresh().await;
    }
    let window = parse_time_window(params.time_window.as_deref());
    let metrics = state.cached_broker_stats.get_windowed(window).await;
    let mut result = HashMap::new();

    for m in metrics {
        // pending_messages = messages waiting in queue
        // in_flight_messages = messages currently being processed
        let current_size = m.pending_messages + m.in_flight_messages;

        // Success rate from acked vs (acked + nacked); deferred (rate limit /
        // capacity) is not counted as a failure.
        let total_processed = m.total_acked + m.total_nacked;
        let success_rate = if total_processed > 0 {
            m.total_acked as f64 / total_processed as f64
        } else {
            1.0
        };

        let stats = DashboardQueueStats {
            name: m.queue_identifier.clone(),
            total_messages: m.total_polled,
            total_consumed: m.total_acked,
            total_failed: m.total_nacked,
            total_deferred: m.total_deferred,
            success_rate,
            current_size,
            throughput: 0.0,
            pending_messages: m.pending_messages,
            messages_not_visible: m.in_flight_messages,
        };
        result.insert(m.queue_identifier, stats);
    }

    Json(result)
}

/// Pool stats for dashboard. Throughput-style fields (`totalProcessed`,
/// `totalSucceeded`, `totalFailed`, `totalRateLimited`, `successRate`,
/// `averageProcessingTimeMs`) are scoped to the requested time window.
/// Live-state fields (`activeWorkers`, `queueSize`, etc.) always reflect
/// the current pool state.
#[derive(Serialize, ToSchema)]
pub(crate) struct DashboardPoolStats {
    #[serde(rename = "poolCode")]
    pool_code: String,
    #[serde(rename = "totalProcessed")]
    total_processed: u64,
    #[serde(rename = "totalSucceeded")]
    total_succeeded: u64,
    #[serde(rename = "totalFailed")]
    total_failed: u64,
    #[serde(rename = "totalRateLimited")]
    total_rate_limited: u64,
    #[serde(rename = "successRate")]
    success_rate: f64,
    #[serde(rename = "activeWorkers")]
    active_workers: u32,
    #[serde(rename = "availablePermits")]
    available_permits: u32,
    #[serde(rename = "maxConcurrency")]
    max_concurrency: u32,
    #[serde(rename = "queueSize")]
    queue_size: u32,
    #[serde(rename = "maxQueueCapacity")]
    max_queue_capacity: u32,
    #[serde(rename = "averageProcessingTimeMs")]
    average_processing_time_ms: f64,
}

/// Pool stats endpoint for dashboard
#[utoipa::path(
    get,
    path = "/monitoring/pool-stats",
    tag = "monitoring",
    responses(
        (status = 200, description = "Pool stats for dashboard")
    )
)]
pub(crate) async fn dashboard_pool_stats_handler(
    State(state): State<AppState>,
    Query(params): Query<DashboardStatsQuery>,
) -> Json<HashMap<String, DashboardPoolStats>> {
    let window = parse_time_window(params.time_window.as_deref());
    let pool_stats = state.queue_manager.get_pool_stats();
    let mut result = HashMap::new();

    const FIVE_MIN: Duration = Duration::from_secs(300);
    const THIRTY_MIN: Duration = Duration::from_secs(1800);

    for s in pool_stats {
        let (succeeded, failed, success_rate, avg_ms, rate_limited) = match (&s.metrics, window) {
            (Some(m), Some(w)) if w == FIVE_MIN => (
                m.last_5_min.success_count,
                m.last_5_min.failure_count,
                m.last_5_min.success_rate,
                m.last_5_min.processing_time.avg_ms,
                m.last_5_min.rate_limited_count,
            ),
            (Some(m), Some(w)) if w == THIRTY_MIN => (
                m.last_30_min.success_count,
                m.last_30_min.failure_count,
                m.last_30_min.success_rate,
                m.last_30_min.processing_time.avg_ms,
                m.last_30_min.rate_limited_count,
            ),
            (Some(m), _) => (
                m.total_success,
                m.total_failure,
                m.success_rate,
                m.processing_time.avg_ms,
                m.total_rate_limited,
            ),
            (None, _) => (0, 0, 1.0, 0.0, 0),
        };

        let stats = DashboardPoolStats {
            pool_code: s.pool_code.clone(),
            total_processed: succeeded + failed,
            total_succeeded: succeeded,
            total_failed: failed,
            total_rate_limited: rate_limited,
            success_rate,
            active_workers: s.active_workers,
            available_permits: s.concurrency.saturating_sub(s.active_workers),
            max_concurrency: s.concurrency,
            queue_size: s.queue_size,
            max_queue_capacity: s.queue_capacity,
            average_processing_time_ms: avg_ms,
        };
        result.insert(s.pool_code, stats);
    }

    Json(result)
}

/// Circuit breaker stats for dashboard
#[derive(Serialize, ToSchema)]
pub(crate) struct DashboardCircuitBreakerStats {
    name: String,
    state: String,
    #[serde(rename = "successfulCalls")]
    successful_calls: u64,
    #[serde(rename = "failedCalls")]
    failed_calls: u64,
    #[serde(rename = "rejectedCalls")]
    rejected_calls: u64,
    #[serde(rename = "failureRate")]
    failure_rate: f64,
    #[serde(rename = "bufferedCalls")]
    buffered_calls: u32,
    #[serde(rename = "bufferSize")]
    buffer_size: u32,
}

/// Circuit breakers endpoint for dashboard
#[utoipa::path(
    get,
    path = "/monitoring/circuit-breakers",
    tag = "monitoring",
    responses(
        (status = 200, description = "Circuit breakers for dashboard")
    )
)]
pub(crate) async fn dashboard_circuit_breakers_handler(
    State(state): State<AppState>,
) -> Json<HashMap<String, DashboardCircuitBreakerStats>> {
    let stats = state.circuit_breaker_registry.get_all_stats();
    let result: HashMap<String, DashboardCircuitBreakerStats> = stats
        .into_iter()
        .map(|(name, s)| {
            (
                name,
                DashboardCircuitBreakerStats {
                    name: s.name,
                    state: format!("{:?}", s.state).to_uppercase(),
                    successful_calls: s.successful_calls,
                    failed_calls: s.failed_calls,
                    rejected_calls: s.rejected_calls,
                    failure_rate: s.failure_rate,
                    buffered_calls: s.buffered_calls,
                    buffer_size: s.buffer_size,
                },
            )
        })
        .collect();
    Json(result)
}

/// Query params for in-flight messages
#[derive(Deserialize, Default, ToSchema)]
pub(crate) struct InFlightMessagesQuery {
    limit: Option<usize>,
    #[serde(rename = "messageId")]
    message_id: Option<String>,
    #[serde(rename = "poolCode")]
    pool_code: Option<String>,
}

/// In-flight messages endpoint for dashboard
#[utoipa::path(
    get,
    path = "/monitoring/in-flight-messages",
    tag = "monitoring",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum number of messages to return"),
        ("messageId" = Option<String>, Query, description = "Filter by message ID (substring, case-insensitive)"),
        ("poolCode" = Option<String>, Query, description = "Filter by pool code (exact match, case-insensitive)")
    ),
    responses(
        (status = 200, description = "In-flight messages", body = Vec<InFlightMessageInfo>)
    )
)]
pub(crate) async fn dashboard_in_flight_messages_handler(
    State(state): State<AppState>,
    Query(query): Query<InFlightMessagesQuery>,
) -> Json<Vec<InFlightMessageInfo>> {
    let limit = query.limit.unwrap_or(100);
    let messages = state.queue_manager.get_in_flight_messages(
        limit,
        query.message_id.as_deref(),
        query.pool_code.as_deref(),
    );
    Json(messages)
}

/// Query params for the in-flight check endpoint.
#[derive(Deserialize, Default, ToSchema)]
pub(crate) struct InFlightCheckQuery {
    /// The application message ID to check (e.g. `evt_…` or `djb_…`).
    #[serde(rename = "messageId")]
    message_id: String,
}

/// Result of checking whether a message is currently held in the router.
///
/// Designed for an external recovery system to ask "is this stuck-looking
/// message actually still being processed by the router?" before
/// re-enqueueing. Always returns 200; `inPipeline=false` means the router
/// does NOT have it (safe to resend).
#[derive(serde::Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InFlightCheckResponse {
    /// Echo of the queried `messageId`.
    message_id: String,
    /// True when the router currently holds the message in its in-pipeline
    /// map. False when it does not — safe for the caller to resend.
    in_pipeline: bool,
    /// Populated only when `inPipeline=true`. Lets the caller decide whether
    /// to skip / wait / force-resend based on age and pool.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<InFlightMessageInfo>,
}

/// Check whether a single application message ID is currently held in the
/// router's in-pipeline map.
///
/// O(1) lookup. Use this from an external system that maintains its own
/// view of "messages that should be retried" to avoid double-enqueueing
/// while the router is still actively processing.
#[utoipa::path(
    get,
    path = "/monitoring/in-flight-messages/check",
    tag = "monitoring",
    params(
        ("messageId" = String, Query, description = "Application message ID to look up (e.g. evt_… or djb_…)")
    ),
    responses(
        (status = 200, description = "Lookup result", body = InFlightCheckResponse)
    )
)]
pub(crate) async fn in_flight_message_check_handler(
    State(state): State<AppState>,
    Query(query): Query<InFlightCheckQuery>,
) -> Json<InFlightCheckResponse> {
    let detail = state
        .queue_manager
        .lookup_in_flight_by_app_id(&query.message_id);
    Json(InFlightCheckResponse {
        message_id: query.message_id,
        in_pipeline: detail.is_some(),
        detail,
    })
}

/// Cap on the number of message IDs accepted in one batch check. Beyond
/// this, the caller should split the request — the per-id check is O(1)
/// but very large arrays bloat request/response framing.
const IN_FLIGHT_CHECK_BATCH_LIMIT: usize = 5000;

/// Body for the batch in-flight check.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InFlightCheckBatchRequest {
    /// Application message IDs to look up. Capped at
    /// `IN_FLIGHT_CHECK_BATCH_LIMIT`; longer lists return 400.
    message_ids: Vec<String>,
}

/// Batch check whether each of the given application message IDs is
/// currently held in the router's in-pipeline map.
///
/// Returns a flat object keyed by message ID with a boolean value:
/// `true` = router has it (caller should NOT resend); `false` = router does
/// not have it (safe to resend). Each lookup is O(1); response framing is
/// the only meaningful cost beyond that.
#[utoipa::path(
    post,
    path = "/monitoring/in-flight-messages/check-batch",
    tag = "monitoring",
    request_body = InFlightCheckBatchRequest,
    responses(
        (status = 200, description = "Map of messageId → inPipeline boolean", body = std::collections::HashMap<String, bool>),
        (status = 400, description = "Too many IDs in one request")
    )
)]
pub(crate) async fn in_flight_message_check_batch_handler(
    State(state): State<AppState>,
    Json(body): Json<InFlightCheckBatchRequest>,
) -> Result<Json<std::collections::HashMap<String, bool>>, (axum::http::StatusCode, String)> {
    if body.message_ids.len() > IN_FLIGHT_CHECK_BATCH_LIMIT {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            format!(
                "messageIds exceeds limit of {} (got {}). Split the request.",
                IN_FLIGHT_CHECK_BATCH_LIMIT,
                body.message_ids.len()
            ),
        ));
    }

    let mut result = std::collections::HashMap::with_capacity(body.message_ids.len());
    for id in body.message_ids {
        let present = state.queue_manager.is_in_flight_by_app_id(&id);
        result.insert(id, present);
    }
    Ok(Json(result))
}

/// Circuit breaker state response
#[derive(Serialize, ToSchema)]
pub(crate) struct CircuitBreakerStateResponse {
    name: String,
    state: String,
}

/// Get circuit breaker state
#[utoipa::path(
    get,
    path = "/monitoring/circuit-breakers/{name}/state",
    tag = "circuit-breakers",
    params(
        ("name" = String, Path, description = "Circuit breaker name (URL-encoded)")
    ),
    responses(
        (status = 200, description = "Circuit breaker state", body = CircuitBreakerStateResponse),
        (status = 404, description = "Circuit breaker not found")
    )
)]
pub(crate) async fn get_circuit_breaker_state(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Response {
    // URL decode the name
    let decoded_name = urlencoding::decode(&name).unwrap_or(std::borrow::Cow::Borrowed(&name));

    match state.circuit_breaker_registry.get_state(&decoded_name) {
        Some(breaker_state) => {
            let state_str = match breaker_state {
                CircuitBreakerState::Closed => "CLOSED",
                CircuitBreakerState::Open => "OPEN",
                CircuitBreakerState::HalfOpen => "HALF_OPEN",
            };
            (
                StatusCode::OK,
                Json(CircuitBreakerStateResponse {
                    name: decoded_name.to_string(),
                    state: state_str.to_string(),
                }),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Circuit breaker not found" })),
        )
            .into_response(),
    }
}

// ── Mediating (operator-surface parity with Go's dashboardMediating) ────

/// Query params for the mediating endpoint.
#[derive(Deserialize, Default, ToSchema)]
pub(crate) struct MediatingQuery {
    limit: Option<usize>,
    #[serde(rename = "poolCode")]
    pool_code: Option<String>,
}

/// One message currently inside a pool worker — the live, never-reaped
/// "mediating right now" view (its count matches the pools' active
/// workers). `elapsedTimeMs` is how long it has been in the worker this
/// attempt. Mirrors Go's `MediatingInfo` field for field.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediatingInfo {
    message_id: String,
    pool_code: String,
    group: String,
    queue: String,
    target: String,
    /// **Always 0 in this port** — see `fc_router::MediatingEntry::attempts`'s
    /// doc for why (no in-pipeline retry-with-front-reinsertion concept
    /// exists here; `HttpMediator`'s bounded retry burst is invisible at
    /// this layer).
    attempts: u32,
    elapsed_time_ms: u64,
}

/// List messages currently being mediated (live, never reaped).
///
/// Distinct from `/monitoring/in-flight-messages`, which is the reaped
/// dedup tracker: this set only ever contains messages a worker is
/// actively holding right now (rate-limiter wait or in-flight HTTP call),
/// so a long-running delivery stays listed for its whole duration. Sorted
/// by elapsed DESC — the longest-running / stuck deliveries surface first,
/// matching the Go dashboard.
#[utoipa::path(
    get,
    path = "/monitoring/mediating",
    tag = "monitoring",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum number of rows to return (default 200)"),
        ("poolCode" = Option<String>, Query, description = "Filter by pool code (exact match, case-insensitive)")
    ),
    responses(
        (status = 200, description = "Messages currently being mediated", body = Vec<MediatingInfo>)
    )
)]
pub(crate) async fn dashboard_mediating_handler(
    State(state): State<AppState>,
    Query(query): Query<MediatingQuery>,
) -> Json<Vec<MediatingInfo>> {
    let limit = query.limit.unwrap_or(200);
    let pool_filter = query.pool_code.as_deref();
    let now = std::time::Instant::now();

    let mut out: Vec<MediatingInfo> = state
        .queue_manager
        .mediating_snapshot()
        .into_iter()
        .filter(|e| pool_filter.is_none_or(|f| e.pool_code.eq_ignore_ascii_case(f)))
        .map(|e| MediatingInfo {
            message_id: e.message_id,
            pool_code: e.pool_code,
            group: e.group,
            queue: e.queue,
            target: e.target,
            attempts: e.attempts,
            elapsed_time_ms: now.saturating_duration_since(e.mediated_at).as_millis() as u64,
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.elapsed_time_ms));
    out.truncate(limit);
    Json(out)
}

// ── In-flight detail (joins the tracker with the mediating set) ─────────

/// Query params for the in-flight detail endpoint.
#[derive(Deserialize, ToSchema)]
pub(crate) struct InFlightDetailQuery {
    #[serde(rename = "messageId")]
    message_id: String,
}

/// Full operator view of one tracked message — joins the tracker entry
/// (exact message-id match) with the live mediating set. Mirrors Go's
/// `InFlightMessageDetail`; see its own doc comment for the `status`
/// vocabulary (`MEDIATING`/`RETRY_BACKOFF`/`TRACKED_IDLE`). A miss returns
/// `inPipeline: false` rather than 404 — "not in the pipeline" is the
/// answer, not an error.
///
/// **Known gap versus Go, not fabricated here:** Go's `lastSeenAt`/
/// `lastSeenElapsedMs` (refreshed on every broker redelivery — the
/// "phantom entry" signal) and a genuine `RETRY_BACKOFF` status have no
/// Rust equivalent: this port's `InFlightMessage` tracks no
/// last-redelivery timestamp, and `attempts` is always 0 (see
/// `MediatingInfo::attempts`'s doc) so `status` here is only ever
/// `MEDIATING` or `TRACKED_IDLE`. Both fields are simply omitted (`null`)
/// rather than fabricated equal to `addedToInPipelineAt` — the
/// dashboard's `d.lastSeenAt ? … : '—'` check already treats that the
/// same as Go omitting the key entirely.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InFlightMessageDetail {
    message_id: String,
    in_pipeline: bool,
    status: Option<String>,
    broker_message_id: Option<String>,
    queue_id: String,
    pool_code: String,
    message_group: String,
    attempts: u32,
    elapsed_time_ms: u64,
    added_to_in_pipeline_at: Option<chrono::DateTime<Utc>>,
    mediation_target: Option<String>,
    mediating_elapsed_ms: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/monitoring/in-flight-messages/detail",
    tag = "monitoring",
    params(
        ("messageId" = String, Query, description = "Application message ID to look up")
    ),
    responses(
        (status = 200, description = "Full detail for one in-flight message", body = InFlightMessageDetail)
    )
)]
pub(crate) async fn in_flight_message_detail_handler(
    State(state): State<AppState>,
    Query(query): Query<InFlightDetailQuery>,
) -> Json<InFlightMessageDetail> {
    let message_id = query.message_id;
    let Some(info) = state.queue_manager.lookup_in_flight_by_app_id(&message_id) else {
        return Json(InFlightMessageDetail {
            message_id,
            in_pipeline: false,
            status: None,
            broker_message_id: None,
            queue_id: String::new(),
            pool_code: String::new(),
            message_group: String::new(),
            attempts: 0,
            elapsed_time_ms: 0,
            added_to_in_pipeline_at: None,
            mediation_target: None,
            mediating_elapsed_ms: None,
        });
    };

    let mut out = InFlightMessageDetail {
        message_id: message_id.clone(),
        in_pipeline: true,
        status: Some(
            if info.attempts > 0 {
                "RETRY_BACKOFF"
            } else {
                "TRACKED_IDLE"
            }
            .to_string(),
        ),
        broker_message_id: info.broker_message_id,
        queue_id: info.queue_id,
        pool_code: info.pool_code,
        message_group: info.message_group,
        attempts: info.attempts,
        elapsed_time_ms: info.elapsed_time_ms,
        added_to_in_pipeline_at: Some(info.added_to_in_pipeline_at),
        mediation_target: None,
        mediating_elapsed_ms: None,
    };

    let now = std::time::Instant::now();
    if let Some(m) = state
        .queue_manager
        .mediating_snapshot()
        .into_iter()
        .find(|e| e.message_id == message_id)
    {
        out.status = Some("MEDIATING".to_string());
        out.mediation_target = Some(m.target);
        out.mediating_elapsed_ms =
            Some(now.saturating_duration_since(m.mediated_at).as_millis() as u64);
    }

    Json(out)
}
