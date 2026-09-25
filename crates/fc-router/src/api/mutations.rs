//! Operator mutations: pool config hot-update, broker-stats refresh,
//! circuit-breaker reset / reset-all, in-flight force-ACK.

use super::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use utoipa::ToSchema;

/// Request to update pool configuration. Omitting a field leaves that knob
/// unchanged (Go: `PoolConfigUpdateRequest`).
#[derive(Debug, Deserialize, ToSchema)]
pub struct PoolConfigUpdateRequest {
    /// New concurrency limit
    pub concurrency: Option<u32>,
    /// New rate limit (messages per minute)
    pub rate_limit_per_minute: Option<u32>,
}

/// Update a pool's concurrency and/or rate limit in place (Go:
/// `Manager.UpdatePool`). An unknown pool is a 404 — it is not created.
#[utoipa::path(
    put,
    path = "/monitoring/pools/{poolCode}",
    tag = "monitoring",
    params(
        ("poolCode" = String, Path, description = "Pool code to update")
    ),
    request_body = PoolConfigUpdateRequest,
    responses(
        (status = 200, description = "Pool updated"),
        (status = 404, description = "Pool not found or update rejected")
    )
)]
pub(crate) async fn update_pool_config(
    State(state): State<AppState>,
    Path(pool_code): Path<String>,
    Json(req): Json<PoolConfigUpdateRequest>,
) -> Response {
    if !state
        .queue_manager
        .update_pool(&pool_code, req.concurrency, req.rate_limit_per_minute)
        .await
    {
        warn!(pool_code = %pool_code, "Pool update rejected (unknown pool or invalid concurrency)");
        return (
            StatusCode::NOT_FOUND,
            [(axum::http::header::CONTENT_TYPE, "application/problem+json")],
            Json(serde_json::json!({
                "title": "Not Found",
                "status": 404,
                "detail": format!("pool not found or update rejected: {pool_code}"),
            })),
        )
            .into_response();
    }
    info!(
        pool_code = %pool_code,
        concurrency = ?req.concurrency,
        rate_limit = ?req.rate_limit_per_minute,
        "Pool configuration updated via API"
    );
    let mut new_config = serde_json::Map::new();
    if let Some(c) = req.concurrency {
        new_config.insert("concurrency".into(), c.into());
    }
    if let Some(r) = req.rate_limit_per_minute {
        new_config.insert("rate_limit_per_minute".into(), r.into());
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "pool_code": pool_code,
            "new_config": new_config,
        })),
    )
        .into_response()
}

/// Refresh broker stats on demand (called when user clicks refresh in dashboard)
#[utoipa::path(
    post,
    path = "/monitoring/broker-stats/refresh",
    tag = "monitoring",
    responses(
        (status = 200, description = "Broker stats refreshed")
    )
)]
pub(crate) async fn broker_stats_refresh_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    state.cached_broker_stats.refresh().await;
    let age = state.cached_broker_stats.age_seconds().await;
    Json(serde_json::json!({
        "refreshed": true,
        "ageSeconds": age.unwrap_or(0)
    }))
}

/// Reset a circuit breaker
#[utoipa::path(
    post,
    path = "/monitoring/circuit-breakers/{name}/reset",
    tag = "circuit-breakers",
    params(
        ("name" = String, Path, description = "Circuit breaker name (URL-encoded)")
    ),
    responses(
        (status = 200, description = "Circuit breaker reset"),
        (status = 500, description = "Failed to reset")
    )
)]
pub(crate) async fn reset_circuit_breaker(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let decoded_name = urlencoding::decode(&name).unwrap_or(std::borrow::Cow::Borrowed(&name));

    if state.circuit_breaker_registry.reset(&decoded_name) {
        info!(name = %decoded_name, "Circuit breaker reset");
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success" })),
        )
            .into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to reset circuit breaker" })),
        )
            .into_response()
    }
}

/// Reset all circuit breakers
#[utoipa::path(
    post,
    path = "/monitoring/circuit-breakers/reset-all",
    tag = "circuit-breakers",
    responses(
        (status = 200, description = "All circuit breakers reset")
    )
)]
pub(crate) async fn reset_all_circuit_breakers(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    state.circuit_breaker_registry.reset_all();
    info!("All circuit breakers reset");
    Json(serde_json::json!({ "status": "success" }))
}

/// Reports what the force-ACK mutation did. `removed: true` means the
/// tracker entry is gone (requeues will now be processed); `brokerAcked`
/// reports the best-effort broker delete separately. Mirrors Go's
/// `ForceAckResponse` field for field.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForceAckResponse {
    message_id: String,
    removed: bool,
    broker_acked: bool,
    broker_ack_error: Option<String>,
    queue_id: String,
    pool_code: String,
    elapsed_time_ms: u64,
    /// Warns that a delivery attempt was still running inside a worker
    /// when the entry was cleared; that attempt finishes on its own —
    /// force-ACK does not abort it.
    was_mediating: bool,
}

/// Force-ACK an in-flight message (operator override).
///
/// Deletes the broker copy (freshest receipt handle) and releases the
/// tracker entry so future redeliveries/requeues are processed instead of
/// being ACK-dropped as duplicates. Does not abort a delivery attempt
/// already running in a worker.
#[utoipa::path(
    post,
    path = "/monitoring/in-flight-messages/{messageId}/ack",
    tag = "monitoring",
    params(
        ("messageId" = String, Path, description = "Application message ID to force-ACK")
    ),
    responses(
        (status = 200, description = "Tracker entry cleared", body = ForceAckResponse),
        (status = 404, description = "Message not currently tracked")
    )
)]
pub(crate) async fn in_flight_force_ack(
    State(state): State<AppState>,
    Path(message_id): Path<String>,
) -> Response {
    // Capture the mediating state BEFORE clearing the entry, so the
    // response can warn that a live delivery attempt will still run to
    // completion (mirrors Go's `inFlightForceAck` handler).
    let was_mediating = state
        .queue_manager
        .mediating_snapshot()
        .iter()
        .any(|e| e.message_id == message_id);

    match state.queue_manager.force_ack_in_flight(&message_id).await {
        Some(res) => {
            warn!(
                message_id = %message_id,
                queue = %res.queue_id,
                pool = %res.pool_code,
                broker_acked = res.broker_acked,
                was_mediating,
                "in-flight message force-acked via operator API"
            );
            (
                StatusCode::OK,
                Json(ForceAckResponse {
                    message_id: res.message_id,
                    removed: true,
                    broker_acked: res.broker_acked,
                    broker_ack_error: res.broker_ack_error,
                    queue_id: res.queue_id,
                    pool_code: res.pool_code,
                    elapsed_time_ms: res.elapsed_ms,
                    was_mediating,
                }),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("message not in pipeline: {message_id}")
            })),
        )
            .into_response(),
    }
}
