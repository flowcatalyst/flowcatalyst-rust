//! /bff/dashboard — admin dashboard summary stats
//!
//! Returns a single payload combining:
//! - **Exact counts** for control-plane tables (clients, users, roles).
//!   These are bounded — typically thousands at most — so `COUNT(*)` is
//!   sub-millisecond.
//! - **Approximate counts** for high-volume message-plane tables
//!   (`msg_events`, `msg_dispatch_jobs`, `aud_logs`, `iam_login_attempts`).
//!   These can grow to billions, where exact counts would be a non-starter.
//!   We read `pg_class.reltuples` — the planner's row estimate maintained by
//!   autovacuum/ANALYZE. Within a few percent of accurate after the table
//!   has been ANALYZEd, and constant-time regardless of row count.
//!
//! Frontend renders the message-plane numbers with a `~` prefix to make the
//! approximation explicit.

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;

use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// Response shape for `GET /bff/dashboard/stats`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStatsResponse {
    /// Exact count of clients.
    pub total_clients: u64,
    /// Exact count of active USER-type principals.
    pub active_users: u64,
    /// Exact count of roles (CODE + DATABASE + SDK).
    pub roles_defined: u64,
    /// Approximate row count for `msg_events` (planner estimate).
    pub events_approx: u64,
    /// Approximate row count for `msg_dispatch_jobs` (planner estimate).
    pub dispatch_jobs_approx: u64,
    /// Approximate row count for `aud_logs` (planner estimate).
    pub audit_logs_approx: u64,
    /// Approximate row count for `iam_login_attempts` (planner estimate).
    pub login_attempts_approx: u64,
}

#[derive(Clone)]
pub struct BffDashboardState {
    pub pool: PgPool,
}

/// Read `pg_class.reltuples` for a list of tables in a single round-trip.
/// Returns 0 for tables that haven't been analyzed yet.
async fn fetch_reltuples(pool: &PgPool, names: &[&str]) -> Result<Vec<(String, u64)>, sqlx::Error> {
    let owned: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    let rows: Vec<(String, f32)> = sqlx::query_as(
        "SELECT relname, reltuples::float4 FROM pg_class \
         WHERE relname = ANY($1) AND relkind = 'r'",
    )
    .bind(&owned)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(name, n)| (name, n.max(0.0) as u64))
        .collect())
}

/// `GET /bff/dashboard/stats`
#[utoipa::path(
    get,
    path = "/stats",
    tag = "bff-dashboard",
    operation_id = "getBffDashboardStats",
    responses(
        (status = 200, description = "Dashboard stats", body = DashboardStatsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dashboard_stats(
    State(state): State<BffDashboardState>,
    auth: Authenticated,
) -> Result<Json<DashboardStatsResponse>, PlatformError> {
    crate::checks::can_view_dashboard_stats(&auth.0)?;

    // Control plane: exact counts. These tables are bounded (thousands at
    // most) so COUNT(*) is sub-millisecond — keeping these in one place
    // here rather than scattering helpers across each repository.
    let (total_clients,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tnt_clients")
        .fetch_one(&state.pool)
        .await
        .map_err(|e| PlatformError::internal(format!("count clients: {}", e)))?;
    let (active_users,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM iam_principals WHERE type = 'USER' AND active = TRUE")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| PlatformError::internal(format!("count users: {}", e)))?;
    let (roles_defined,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM iam_roles")
        .fetch_one(&state.pool)
        .await
        .map_err(|e| PlatformError::internal(format!("count roles: {}", e)))?;
    let total_clients = total_clients.max(0) as u64;
    let active_users = active_users.max(0) as u64;
    let roles_defined = roles_defined.max(0) as u64;

    // Message plane: planner estimates from pg_class. Tables are batched
    // into one query.
    let mut by_name: std::collections::HashMap<String, u64> = fetch_reltuples(
        &state.pool,
        &[
            "msg_events",
            "msg_dispatch_jobs",
            "aud_logs",
            "iam_login_attempts",
        ],
    )
    .await
    .map_err(|e| PlatformError::internal(format!("Failed to read pg_class.reltuples: {}", e)))?
    .into_iter()
    .collect();

    Ok(Json(DashboardStatsResponse {
        total_clients,
        active_users,
        roles_defined,
        events_approx: by_name.remove("msg_events").unwrap_or(0),
        dispatch_jobs_approx: by_name.remove("msg_dispatch_jobs").unwrap_or(0),
        audit_logs_approx: by_name.remove("aud_logs").unwrap_or(0),
        login_attempts_approx: by_name.remove("iam_login_attempts").unwrap_or(0),
    }))
}

pub fn bff_dashboard_router(state: BffDashboardState) -> Router {
    Router::new()
        .route("/stats", get(get_dashboard_stats))
        .with_state(state)
}
