//! Operator group views: blocked groups (ledger R-04) and group-flush
//! suppressions (ledger R-52/R-53) — mirrors Go's `handlers_group_flush.go`.
//!
//! Both traverse every pool the manager is tracking, including one still
//! finishing an asynchronous removal-drain (see `QueueManager::all_pools`'s
//! doc), so a pool mid-removal doesn't disappear from either view before
//! its buffered work / live suppressions actually clear.

use super::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ── Blocked groups (ledger R-04) ─────────────────────────────────────────

/// Query params for the blocked-groups endpoint.
#[derive(Deserialize, Default, ToSchema)]
pub(crate) struct BlockedGroupsQuery {
    #[serde(rename = "poolCode")]
    pool_code: Option<String>,
}

/// One live message group a pool is currently holding — the operator
/// "blocked groups" view. Mirrors Go's `BlockedGroupInfo` field for
/// field: `parkedAt`/`suppressedUntil` are omitted entirely (not sent as
/// an explicit `null`) when absent, so the dashboard's `r.parkedAt`/
/// `r.suppressedUntil` truthiness checks work without a separate null
/// check — same rationale as Go's own dto.go doc comment for this type.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlockedGroupInfo {
    group: String,
    pool_code: String,
    buffered: usize,
    working: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    parked_at: Option<DateTime<Utc>>,
    suppressed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    suppressed_until: Option<DateTime<Utc>>,
}

/// Every live message group across every pool this router is tracking —
/// buffered awaiting a drainer, being drained, or parked with none
/// running — including a pool still finishing an asynchronous
/// removal-drain.
#[utoipa::path(
    get,
    path = "/monitoring/blocked-groups",
    tag = "monitoring",
    params(
        ("poolCode" = Option<String>, Query, description = "Filter by pool code (exact match, case-insensitive)")
    ),
    responses(
        (status = 200, description = "Live message groups", body = Vec<BlockedGroupInfo>)
    )
)]
pub(crate) async fn blocked_groups_handler(
    State(state): State<AppState>,
    Query(query): Query<BlockedGroupsQuery>,
) -> Json<Vec<BlockedGroupInfo>> {
    let pool_filter = query.pool_code.as_deref();
    let out: Vec<BlockedGroupInfo> = state
        .queue_manager
        .blocked_groups()
        .into_iter()
        .filter(|g| pool_filter.is_none_or(|f| g.pool_code.eq_ignore_ascii_case(f)))
        .map(|g| BlockedGroupInfo {
            group: g.group,
            pool_code: g.pool_code,
            buffered: g.buffered,
            working: g.working,
            parked_at: g.parked_at,
            suppressed: g.suppressed,
            suppressed_until: g.suppressed_until,
        })
        .collect();
    Json(out)
}

// ── Group-flush suppression (ledger R-52/R-53) ───────────────────────────

/// One active suppression: a message group and when it lapses on its own
/// (TTL-bounded). Mirrors Go's `GroupFlushEntry`.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GroupFlushEntry {
    group: String,
    suppressed_until: DateTime<Utc>,
}

/// One pool's group-flush suppression snapshot: every group currently
/// suppressed on it, plus its lifetime flush/suppressed counters. Mirrors
/// Go's `GroupFlushPoolInfo`.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GroupFlushPoolInfo {
    pool_code: String,
    active_count: usize,
    total_flushes: u64,
    total_suppressed: u64,
    groups: Vec<GroupFlushEntry>,
}

/// Convert a monotonic `Instant` (the registry's internal expiry clock) to
/// an approximate wall-clock `DateTime<Utc>` — same technique as
/// `ProcessPool::group_snapshot`'s `to_wall_clock` (that one already runs
/// inside `pool.rs` for `parked_at`/its own suppression lookup;
/// `GroupSuppression::until` stays `Instant`-typed at the domain layer, so
/// this is where its sibling conversion happens for the wire).
fn suppression_to_wall_clock(until: std::time::Instant) -> DateTime<Utc> {
    let now_instant = std::time::Instant::now();
    let now_utc = Utc::now();
    if until >= now_instant {
        let delta = until - now_instant;
        now_utc + chrono::Duration::from_std(delta).unwrap_or_default()
    } else {
        let delta = now_instant - until;
        now_utc - chrono::Duration::from_std(delta).unwrap_or_default()
    }
}

/// Active group-flush suppressions across every pool: a snapshot per pool
/// of every message group currently suppressed (a target asked the router
/// to stop sending it for a bounded window) plus that pool's lifetime
/// flush/suppressed counters.
#[utoipa::path(
    get,
    path = "/monitoring/group-flushes",
    tag = "monitoring",
    responses(
        (status = 200, description = "Group-flush suppressions by pool", body = Vec<GroupFlushPoolInfo>)
    )
)]
pub(crate) async fn group_flushes_handler(
    State(state): State<AppState>,
) -> Json<Vec<GroupFlushPoolInfo>> {
    let out: Vec<GroupFlushPoolInfo> = state
        .queue_manager
        .group_flush_snapshots()
        .into_iter()
        .map(|snap| GroupFlushPoolInfo {
            pool_code: snap.pool_code,
            active_count: snap.active_count,
            total_flushes: snap.total_flushes,
            total_suppressed: snap.total_suppressed,
            groups: snap
                .groups
                .into_iter()
                .map(|g| GroupFlushEntry {
                    group: g.group,
                    suppressed_until: suppression_to_wall_clock(g.until),
                })
                .collect(),
        })
        .collect();
    Json(out)
}

/// Response for lifting an active group-flush suppression early. Mirrors
/// Go's `ClearGroupFlushResponse`.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClearGroupFlushResponse {
    cleared: bool,
    pool_code: String,
    group: String,
}

/// Lift an active group-flush suppression early (operator override).
#[utoipa::path(
    post,
    path = "/monitoring/group-flushes/{pool}/{group}/clear",
    tag = "monitoring",
    params(
        ("pool" = String, Path, description = "Pool code"),
        ("group" = String, Path, description = "Message group id")
    ),
    responses(
        (status = 200, description = "Suppression cleared", body = ClearGroupFlushResponse),
        (status = 404, description = "No active suppression for that pool/group")
    )
)]
pub(crate) async fn clear_group_flush_handler(
    State(state): State<AppState>,
    Path((pool, group)): Path<(String, String)>,
) -> Response {
    if state.queue_manager.clear_group_flush(&pool, &group) {
        (
            StatusCode::OK,
            Json(ClearGroupFlushResponse {
                cleared: true,
                pool_code: pool,
                group,
            }),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("no active suppression for pool {pool} group {group}")
            })),
        )
            .into_response()
    }
}
