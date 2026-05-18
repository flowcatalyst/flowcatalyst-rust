//! Monitoring API
//!
//! REST endpoints for platform monitoring and observability.

use axum::{extract::State, Json};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::DispatchJobRepository;

/// Standby status response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StandbyStatus {
    /// Whether this instance is the leader
    pub is_leader: bool,
    /// Instance ID
    pub instance_id: String,
    /// Current role (LEADER or STANDBY)
    pub role: String,
    /// Leader instance ID (if known)
    pub leader_id: Option<String>,
    /// Last heartbeat time
    pub last_heartbeat: Option<String>,
    /// Cluster members
    pub cluster_members: Vec<ClusterMember>,
}

/// Cluster member info
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClusterMember {
    pub instance_id: String,
    pub role: String,
    pub last_seen: String,
    pub healthy: bool,
}

/// Dashboard metrics response.
///
/// `total_events` and `total_jobs` are **approximate** — read from
/// `pg_class.reltuples` (the planner's row estimate maintained by autovacuum/
/// ANALYZE). Within a few % of accurate; sub-millisecond regardless of row
/// count, where exact counts on `msg_events` / `msg_dispatch_jobs` would be a
/// non-starter at production scale.
///
/// `jobs_by_status` was removed: migration 015 dropped the status index on
/// `msg_dispatch_jobs` (it's a write-optimized table; the read projection
/// has the index). Per-status counts are now full table scans, so they're
/// not surfaced here. If you need them, query the read projection directly.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardMetrics {
    /// Approximate total events received (planner estimate).
    pub total_events: u64,
    /// Events in last hour. Currently always 0 — needs a time-windowed
    /// counter on the projection or an external metrics store.
    pub events_last_hour: u64,
    /// Approximate total dispatch jobs (planner estimate).
    pub total_jobs: u64,
    /// Active subscriptions
    pub active_subscriptions: u64,
    /// Active dispatch pools
    pub active_pools: u64,
    /// System health
    pub health: SystemHealth,
}

/// System health info
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SystemHealth {
    pub status: String,
    pub uptime_seconds: u64,
    pub memory_used_mb: u64,
    pub cpu_usage_percent: f32,
}

/// Circuit breaker state
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CircuitBreakerState {
    /// Target identifier
    pub target: String,
    /// Current state (CLOSED, OPEN, HALF_OPEN)
    pub state: String,
    /// Failure count
    pub failure_count: u32,
    /// Success count since last failure
    pub success_count: u32,
    /// Last failure time
    pub last_failure: Option<String>,
    /// Time until reset (if open)
    pub reset_at: Option<String>,
}

/// Circuit breakers response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CircuitBreakersResponse {
    pub breakers: Vec<CircuitBreakerState>,
    pub total_open: usize,
    pub total_half_open: usize,
    pub total_closed: usize,
}

/// In-flight message info
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InFlightMessage {
    pub job_id: String,
    pub event_id: Option<String>,
    pub target_url: String,
    pub started_at: String,
    pub elapsed_ms: u64,
    pub attempt: u32,
    pub pool_id: Option<String>,
    pub message_group: Option<String>,
}

/// In-flight messages response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InFlightMessagesResponse {
    pub messages: Vec<InFlightMessage>,
    pub total_in_flight: usize,
    pub by_pool: HashMap<String, usize>,
    pub by_message_group: HashMap<String, usize>,
}

/// Leader election state (shared across handlers)
#[derive(Clone)]
pub struct LeaderState {
    pub is_leader: Arc<RwLock<bool>>,
    pub instance_id: String,
    pub leader_id: Arc<RwLock<Option<String>>>,
    pub cluster_members: Arc<RwLock<Vec<ClusterMember>>>,
}

impl LeaderState {
    pub fn new(instance_id: String) -> Self {
        Self {
            is_leader: Arc::new(RwLock::new(false)),
            instance_id,
            leader_id: Arc::new(RwLock::new(None)),
            cluster_members: Arc::new(RwLock::new(vec![])),
        }
    }

    pub async fn set_leader(&self, is_leader: bool) {
        let mut guard = self.is_leader.write().await;
        *guard = is_leader;
        if is_leader {
            let mut leader = self.leader_id.write().await;
            *leader = Some(self.instance_id.clone());
        }
    }
}

/// Circuit breaker registry
#[derive(Clone, Default)]
pub struct CircuitBreakerRegistry {
    pub breakers: Arc<RwLock<HashMap<String, CircuitBreakerState>>>,
}

impl CircuitBreakerRegistry {
    pub fn new() -> Self {
        Self {
            breakers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_all(&self) -> Vec<CircuitBreakerState> {
        let guard = self.breakers.read().await;
        guard.values().cloned().collect()
    }

    pub async fn update(&self, target: &str, state: CircuitBreakerState) {
        let mut guard = self.breakers.write().await;
        guard.insert(target.to_string(), state);
    }
}

/// In-flight message tracker
#[derive(Clone, Default)]
pub struct InFlightTracker {
    pub messages: Arc<RwLock<HashMap<String, InFlightMessage>>>,
}

impl InFlightTracker {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add(&self, job_id: &str, msg: InFlightMessage) {
        let mut guard = self.messages.write().await;
        guard.insert(job_id.to_string(), msg);
    }

    pub async fn remove(&self, job_id: &str) {
        let mut guard = self.messages.write().await;
        guard.remove(job_id);
    }

    pub async fn get_all(&self) -> Vec<InFlightMessage> {
        let guard = self.messages.read().await;
        guard.values().cloned().collect()
    }
}

/// Platform statistics response
/// Monitoring service state
#[derive(Clone)]
pub struct MonitoringState {
    pub leader_state: LeaderState,
    pub circuit_breakers: CircuitBreakerRegistry,
    pub in_flight: InFlightTracker,
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
    /// Used by `get_dashboard` for `pg_class.reltuples` lookups —
    /// `msg_dispatch_jobs` / `msg_events` can be billions of rows where
    /// `COUNT(*)` is a non-starter.
    pub pool: sqlx::PgPool,
    pub start_time: std::time::Instant,
}

/// Get standby status
#[utoipa::path(
    get,
    path = "/standby-status",
    tag = "monitoring",
    operation_id = "getApiMonitoringStandbyStatus",
    responses(
        (status = 200, description = "Standby status", body = StandbyStatus)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_standby_status(
    State(state): State<MonitoringState>,
    auth: Authenticated,
) -> Result<Json<StandbyStatus>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let is_leader = *state.leader_state.is_leader.read().await;
    let leader_id = state.leader_state.leader_id.read().await.clone();
    let cluster_members = state.leader_state.cluster_members.read().await.clone();

    Ok(Json(StandbyStatus {
        is_leader,
        instance_id: state.leader_state.instance_id.clone(),
        role: if is_leader {
            "LEADER".to_string()
        } else {
            "STANDBY".to_string()
        },
        leader_id,
        last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
        cluster_members,
    }))
}

/// Get dashboard metrics
#[utoipa::path(
    get,
    path = "/dashboard",
    tag = "monitoring",
    operation_id = "getApiMonitoringDashboard",
    responses(
        (status = 200, description = "Dashboard metrics", body = DashboardMetrics)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dashboard(
    State(state): State<MonitoringState>,
    auth: Authenticated,
) -> Result<Json<DashboardMetrics>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    // Approximate row counts via pg_class.reltuples. One round-trip for
    // both message tables; sub-millisecond regardless of row count.
    let row: Option<(f32, f32)> = sqlx::query_as(
        "SELECT \
            COALESCE(MAX(CASE WHEN relname = 'msg_dispatch_jobs' THEN reltuples END), 0)::float4, \
            COALESCE(MAX(CASE WHEN relname = 'msg_events' THEN reltuples END), 0)::float4 \
         FROM pg_class \
         WHERE relname IN ('msg_dispatch_jobs', 'msg_events') AND relkind = 'r'",
    )
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let (total_jobs, total_events) = row
        .map(|(j, e)| (j.max(0.0) as u64, e.max(0.0) as u64))
        .unwrap_or((0, 0));

    Ok(Json(DashboardMetrics {
        total_events,
        events_last_hour: 0, // Time-windowed counter not yet wired.
        total_jobs,
        active_subscriptions: 0, // Would need subscription repo.
        active_pools: 0,         // Would need pool repo.
        health: SystemHealth {
            status: "UP".to_string(),
            uptime_seconds: state.start_time.elapsed().as_secs(),
            memory_used_mb: 0, // Could use sysinfo crate.
            cpu_usage_percent: 0.0,
        },
    }))
}

/// Get circuit breaker states
#[utoipa::path(
    get,
    path = "/circuit-breakers",
    tag = "monitoring",
    operation_id = "getApiMonitoringCircuitBreakers",
    responses(
        (status = 200, description = "Circuit breaker states", body = CircuitBreakersResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_circuit_breakers(
    State(state): State<MonitoringState>,
    auth: Authenticated,
) -> Result<Json<CircuitBreakersResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let breakers = state.circuit_breakers.get_all().await;

    let total_open = breakers.iter().filter(|b| b.state == "OPEN").count();
    let total_half_open = breakers.iter().filter(|b| b.state == "HALF_OPEN").count();
    let total_closed = breakers.iter().filter(|b| b.state == "CLOSED").count();

    Ok(Json(CircuitBreakersResponse {
        breakers,
        total_open,
        total_half_open,
        total_closed,
    }))
}

/// Get in-flight messages
#[utoipa::path(
    get,
    path = "/in-flight-messages",
    tag = "monitoring",
    operation_id = "getApiMonitoringInFlightMessages",
    responses(
        (status = 200, description = "In-flight messages", body = InFlightMessagesResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_in_flight_messages(
    State(state): State<MonitoringState>,
    auth: Authenticated,
) -> Result<Json<InFlightMessagesResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let messages = state.in_flight.get_all().await;
    let total_in_flight = messages.len();

    // Group by pool
    let mut by_pool: HashMap<String, usize> = HashMap::new();
    for msg in &messages {
        if let Some(ref pool_id) = msg.pool_id {
            *by_pool.entry(pool_id.clone()).or_insert(0) += 1;
        }
    }

    // Group by message group
    let mut by_message_group: HashMap<String, usize> = HashMap::new();
    for msg in &messages {
        if let Some(ref group) = msg.message_group {
            *by_message_group.entry(group.clone()).or_insert(0) += 1;
        }
    }

    Ok(Json(InFlightMessagesResponse {
        messages,
        total_in_flight,
        by_pool,
        by_message_group,
    }))
}

/// Pool statistics response (with enhanced metrics)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PoolStatsResponse {
    pub pools: Vec<fc_common::PoolStats>,
    pub total_pools: usize,
    pub total_active_workers: u32,
    pub total_queue_size: u32,
    /// Aggregate success rate across all pools
    pub aggregate_success_rate: f64,
    /// Aggregate throughput (messages/sec) across all pools
    pub aggregate_throughput_per_sec: f64,
}

/// Get pool statistics with enhanced metrics
#[utoipa::path(
    get,
    path = "/pool-stats",
    tag = "monitoring",
    operation_id = "getApiMonitoringPoolStats",
    responses(
        (status = 200, description = "Pool statistics with enhanced metrics", body = PoolStatsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_pool_stats(
    State(_state): State<MonitoringState>,
    auth: Authenticated,
) -> Result<Json<PoolStatsResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    // Note: In a full implementation, the router's QueueManager would be
    // passed to the monitoring state to get real pool stats.
    // For now, return empty stats as the router runs in a separate process.
    let pools: Vec<fc_common::PoolStats> = Vec::new();

    let total_active_workers: u32 = pools.iter().map(|p| p.active_workers).sum();
    let total_queue_size: u32 = pools.iter().map(|p| p.queue_size).sum();

    // Calculate aggregate metrics from enhanced metrics if available
    let mut total_success = 0u64;
    let mut total_failure = 0u64;
    let mut total_throughput = 0.0f64;

    for pool in &pools {
        if let Some(ref metrics) = pool.metrics {
            total_success += metrics.total_success;
            total_failure += metrics.total_failure;
            total_throughput += metrics.last_5_min.throughput_per_sec;
        }
    }

    let aggregate_success_rate = if total_success + total_failure > 0 {
        total_success as f64 / (total_success + total_failure) as f64
    } else {
        1.0
    };

    Ok(Json(PoolStatsResponse {
        total_pools: pools.len(),
        pools,
        total_active_workers,
        total_queue_size,
        aggregate_success_rate,
        aggregate_throughput_per_sec: total_throughput,
    }))
}

/// Create monitoring router
pub fn monitoring_router(state: MonitoringState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(get_standby_status))
        .routes(routes!(get_dashboard))
        .routes(routes!(get_circuit_breakers))
        .routes(routes!(get_in_flight_messages))
        .routes(routes!(get_pool_stats))
        .with_state(state)
}
