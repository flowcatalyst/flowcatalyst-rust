//! Health/liveness/readiness probes, Prometheus metrics, consumer health,
//! and stream-processor health.

use super::AppState;
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Duration as ChronoDuration, Utc};
use fc_common::HealthStatus;
use serde::Serialize;
use utoipa::ToSchema;

/// Simple health response for basic health check
#[derive(Serialize, ToSchema)]
pub struct SimpleHealthResponse {
    /// Health status: UP, DEGRADED
    pub status: String,
    /// Application version
    pub version: String,
}

/// Kubernetes probe response
#[derive(Serialize, ToSchema)]
pub struct ProbeResponse {
    /// Probe status: LIVE, READY, NOT_READY
    pub status: String,
}

/// Health check endpoint
///
/// Item 5 (bench rig finding 2026-09-07): answers 503 until
/// [`crate::manager::QueueManager::consumers_started`] flips — the HTTP
/// listener and `QueueManager::start()` are independent tasks with no
/// ordering between them (`bin/fc-router/src/main.rs`), so before this fix
/// `/health` could (and under the bench rig's poll cadence, did) answer 200
/// before a single consumer poll task existed. A caller that treats "health
/// returns 200" as "the router is consuming" — the bench rig's drain clock,
/// an operator's own health probe — needs that to be true, not just "the
/// HTTP listener is bound".
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Health status", body = SimpleHealthResponse),
        (status = 503, description = "Consumers not started yet", body = SimpleHealthResponse)
    )
)]
pub(crate) async fn health_handler(State(state): State<AppState>) -> Response {
    if !state.queue_manager.consumers_started() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SimpleHealthResponse {
                status: "STARTING".to_string(),
                version: fc_common::BUILD_VERSION.to_string(),
            }),
        )
            .into_response();
    }

    let pool_stats = state.queue_manager.get_pool_stats();
    let report = state.health_service.get_health_report(&pool_stats);

    let status = match report.status {
        HealthStatus::Healthy => "UP",
        HealthStatus::Warning => "UP",
        HealthStatus::Degraded => "DEGRADED",
    };

    Json(SimpleHealthResponse {
        status: status.to_string(),
        version: fc_common::BUILD_VERSION.to_string(),
    })
    .into_response()
}

/// Simple health handler (no state dependency)
pub(crate) async fn simple_health_handler() -> Json<SimpleHealthResponse> {
    Json(SimpleHealthResponse {
        status: "UP".to_string(),
        version: fc_common::BUILD_VERSION.to_string(),
    })
}

/// Kubernetes liveness probe - returns 200 if the application is running
#[utoipa::path(
    get,
    path = "/health/live",
    tag = "health",
    responses(
        (status = 200, description = "Application is live", body = ProbeResponse)
    )
)]
pub(crate) async fn liveness_probe() -> Json<ProbeResponse> {
    Json(ProbeResponse {
        status: "LIVE".to_string(),
    })
}

/// Kubernetes readiness probe - returns 200 if ready to accept traffic
#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "health",
    responses(
        (status = 200, description = "Application is ready", body = ProbeResponse),
        (status = 503, description = "Application is not ready", body = ProbeResponse)
    )
)]
pub(crate) async fn readiness_probe(State(state): State<AppState>) -> Response {
    // Java: check broker connectivity via consumer is_healthy() before health report
    crate::router_metrics::record_broker_connection_attempt();
    let broker_healthy = state.queue_manager.check_broker_connectivity().await;
    crate::router_metrics::set_broker_available(broker_healthy);
    if broker_healthy {
        crate::router_metrics::record_broker_connection_success();
    } else {
        crate::router_metrics::record_broker_connection_failure();
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ProbeResponse {
                status: "NOT_READY".to_string(),
            }),
        )
            .into_response();
    }

    let pool_stats = state.queue_manager.get_pool_stats();
    let report = state.health_service.get_health_report(&pool_stats);

    match report.status {
        HealthStatus::Healthy | HealthStatus::Warning => (
            StatusCode::OK,
            Json(ProbeResponse {
                status: "READY".to_string(),
            }),
        )
            .into_response(),
        HealthStatus::Degraded => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ProbeResponse {
                status: "NOT_READY".to_string(),
            }),
        )
            .into_response(),
    }
}

/// Prometheus metrics endpoint
#[utoipa::path(
    get,
    path = "/metrics",
    tag = "monitoring",
    responses(
        (status = 200, description = "Prometheus metrics", content_type = "text/plain")
    )
)]
pub(crate) async fn metrics_handler(State(state): State<AppState>) -> Response {
    let output = match &state.metrics_handle {
        Some(handle) => handle.render(),
        None => {
            // Fallback when no Prometheus recorder is installed
            "# No Prometheus recorder configured\n".to_string()
        }
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        output,
    )
        .into_response()
}

/// Consumer health endpoint (`/monitoring/consumer-health`)
pub(crate) async fn consumer_health_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let now = Utc::now();
    let now_ms = now.timestamp_millis();

    // Get all consumer IDs from the health service
    let pool_stats = state.queue_manager.get_pool_stats();
    let _report = state.health_service.get_health_report(&pool_stats);

    // Build consumer health map from queue manager's consumer list
    let consumer_ids = state.queue_manager.consumer_ids().await;
    let mut consumers = serde_json::Map::new();

    for consumer_id in &consumer_ids {
        let health = state.health_service.get_consumer_health(consumer_id);
        let last_poll_time_ms = health.last_poll_time_ms.unwrap_or(0);
        let time_since_last_poll_ms = health.time_since_last_poll_ms.unwrap_or(-1);

        let last_poll_time_str = if last_poll_time_ms > 0 {
            // Convert elapsed ms back to an approximate absolute time
            let poll_time = now - ChronoDuration::milliseconds(time_since_last_poll_ms);
            poll_time.to_rfc3339()
        } else {
            "never".to_string()
        };

        let time_since_last_poll_seconds = if time_since_last_poll_ms > 0 {
            time_since_last_poll_ms / 1000
        } else {
            -1
        };

        let details = serde_json::json!({
            "mapKey": consumer_id,
            "queueIdentifier": consumer_id,
            "consumerQueueIdentifier": consumer_id,
            "instanceId": state.instance_id,
            "isHealthy": health.is_healthy,
            "lastPollTimeMs": last_poll_time_ms,
            "lastPollTime": last_poll_time_str,
            "timeSinceLastPollMs": time_since_last_poll_ms,
            "timeSinceLastPollSeconds": time_since_last_poll_seconds,
            "isRunning": health.is_running,
        });
        consumers.insert(consumer_id.clone(), details);
    }

    Json(serde_json::json!({
        "currentTimeMs": now_ms,
        "currentTime": now.to_rfc3339(),
        "consumers": consumers,
    }))
}

// ============================================================================
// Stream Health Endpoints
// ============================================================================

/// Stream processor health response
#[derive(Serialize, ToSchema)]
pub(crate) struct StreamHealthResponse {
    /// Overall status: UP, DEGRADED, DOWN
    status: String,
    /// Whether live probe passes
    live: bool,
    /// Whether ready probe passes
    ready: bool,
    /// Individual stream health details
    streams: Vec<StreamHealthDetail>,
    /// Error messages if any
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
}

/// Health detail for a single stream
#[derive(Serialize, ToSchema)]
struct StreamHealthDetail {
    name: String,
    status: String,
    #[serde(rename = "batchSequence")]
    batch_sequence: u64,
    #[serde(rename = "inFlightCount")]
    in_flight_count: u32,
    #[serde(rename = "pendingCount")]
    pending_count: u32,
    #[serde(rename = "errorCount")]
    error_count: u64,
    #[serde(rename = "lastCheckpointAt")]
    last_checkpoint_at: Option<String>,
}

/// Get stream processor health status
pub(crate) async fn stream_health_handler(
    State(state): State<AppState>,
) -> Json<StreamHealthResponse> {
    match &state.stream_health_service {
        Some(service) => {
            let health = service.get_aggregated_health();
            let streams: Vec<StreamHealthDetail> = service
                .get_all_stream_health()
                .iter()
                .map(|h| {
                    let status_snapshot = h.status();
                    StreamHealthDetail {
                        name: h.name().to_string(),
                        status: format!("{:?}", status_snapshot.status).to_uppercase(),
                        batch_sequence: status_snapshot.batch_sequence,
                        in_flight_count: status_snapshot.in_flight_count,
                        pending_count: status_snapshot.pending_count,
                        error_count: status_snapshot.error_count,
                        last_checkpoint_at: status_snapshot
                            .last_checkpoint_at
                            .map(|dt| dt.to_rfc3339()),
                    }
                })
                .collect();

            let status = if health.is_live() && health.is_ready() {
                "UP"
            } else if health.is_live() {
                "DEGRADED"
            } else {
                "DOWN"
            };

            Json(StreamHealthResponse {
                status: status.to_string(),
                live: health.is_live(),
                ready: health.is_ready(),
                streams,
                errors: health.errors,
            })
        }
        None => {
            // No stream health service configured
            Json(StreamHealthResponse {
                status: "DISABLED".to_string(),
                live: true,
                ready: true,
                streams: vec![],
                errors: vec![],
            })
        }
    }
}

/// Stream liveness probe - checks if streams are alive
pub(crate) async fn stream_liveness_handler(State(state): State<AppState>) -> Response {
    match &state.stream_health_service {
        Some(service) => {
            let health = service.get_aggregated_health();
            if health.is_live() {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({ "status": "LIVE" })),
                )
                    .into_response()
            } else {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "status": "NOT_LIVE",
                        "errors": health.errors
                    })),
                )
                    .into_response()
            }
        }
        None => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "LIVE" })),
        )
            .into_response(),
    }
}

/// Stream readiness probe - checks if streams are ready to process
pub(crate) async fn stream_readiness_handler(State(state): State<AppState>) -> Response {
    match &state.stream_health_service {
        Some(service) => {
            let health = service.get_aggregated_health();
            if health.is_ready() {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({ "status": "READY" })),
                )
                    .into_response()
            } else {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "status": "NOT_READY",
                        "errors": health.errors
                    })),
                )
                    .into_response()
            }
        }
        None => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "READY" })),
        )
            .into_response(),
    }
}
