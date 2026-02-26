//! Monitoring API
//!
//! REST endpoints for platform monitoring and observability.

use axum::{
    extract::State,
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::ToSchema;
use serde::Serialize;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::{
    DispatchJobRepository, EventTypeRepository,
    SubscriptionRepository, DispatchPoolRepository, ClientRepository,
    PrincipalRepository, ApplicationRepository,
};
use crate::DispatchStatus;

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

/// Dashboard metrics response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardMetrics {
    /// Total events received
    pub total_events: u64,
    /// Events in last hour
    pub events_last_hour: u64,
    /// Total dispatch jobs
    pub total_jobs: u64,
    /// Jobs by status
    pub jobs_by_status: HashMap<String, u64>,
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
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlatformStats {
    /// Total number of clients
    pub total_clients: u64,
    /// Active clients
    pub active_clients: u64,
    /// Total principals (users + service accounts)
    pub total_principals: u64,
    /// Total applications
    pub total_applications: u64,
    /// Active applications
    pub active_applications: u64,
    /// Total event types
    pub total_event_types: u64,
    /// Active event types
    pub active_event_types: u64,
    /// Total subscriptions
    pub total_subscriptions: u64,
    /// Active subscriptions
    pub active_subscriptions: u64,
    /// Total dispatch pools
    pub total_dispatch_pools: u64,
    /// Total events received (all time)
    pub total_events: u64,
    /// Total dispatch jobs (all time)
    pub total_dispatch_jobs: u64,
    /// Dispatch jobs by status
    pub jobs_by_status: HashMap<String, u64>,
}

/// Stats state (subset of MonitoringState for the stats endpoint)
#[derive(Clone)]
pub struct StatsState {
    pub client_repo: Arc<ClientRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub application_repo: Arc<ApplicationRepository>,
    pub event_type_repo: Arc<EventTypeRepository>,
    pub subscription_repo: Arc<SubscriptionRepository>,
    pub dispatch_pool_repo: Arc<DispatchPoolRepository>,
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
}

/// Monitoring service state
#[derive(Clone)]
pub struct MonitoringState {
    pub leader_state: LeaderState,
    pub circuit_breakers: CircuitBreakerRegistry,
    pub in_flight: InFlightTracker,
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
    pub start_time: std::time::Instant,
}

/// Get standby status
#[utoipa::path(
    get,
    path = "/standby-status",
    tag = "monitoring",
    operation_id = "getApiAdminMonitoringStandbyStatus",
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
        role: if is_leader { "LEADER".to_string() } else { "STANDBY".to_string() },
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
    operation_id = "getApiAdminMonitoringDashboard",
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

    // Get job counts by status
    let pending = state.dispatch_job_repo.count_by_status(DispatchStatus::Pending).await.unwrap_or(0);
    let queued = state.dispatch_job_repo.count_by_status(DispatchStatus::Queued).await.unwrap_or(0);
    let in_progress = state.dispatch_job_repo.count_by_status(DispatchStatus::InProgress).await.unwrap_or(0);
    let completed = state.dispatch_job_repo.count_by_status(DispatchStatus::Completed).await.unwrap_or(0);
    let failed = state.dispatch_job_repo.count_by_status(DispatchStatus::Failed).await.unwrap_or(0);

    let mut jobs_by_status = HashMap::new();
    jobs_by_status.insert("PENDING".to_string(), pending);
    jobs_by_status.insert("QUEUED".to_string(), queued);
    jobs_by_status.insert("IN_PROGRESS".to_string(), in_progress);
    jobs_by_status.insert("COMPLETED".to_string(), completed);
    jobs_by_status.insert("FAILED".to_string(), failed);

    let total_jobs = pending + queued + in_progress + completed + failed;

    Ok(Json(DashboardMetrics {
        total_events: 0, // Would need event repo
        events_last_hour: 0,
        total_jobs,
        jobs_by_status,
        active_subscriptions: 0, // Would need subscription repo
        active_pools: 0, // Would need pool repo
        health: SystemHealth {
            status: "UP".to_string(),
            uptime_seconds: state.start_time.elapsed().as_secs(),
            memory_used_mb: 0, // Could use sysinfo crate
            cpu_usage_percent: 0.0,
        },
    }))
}

/// Get circuit breaker states
#[utoipa::path(
    get,
    path = "/circuit-breakers",
    tag = "monitoring",
    operation_id = "getApiAdminMonitoringCircuitBreakers",
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
    operation_id = "getApiAdminMonitoringInFlightMessages",
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

/// Get platform statistics
#[utoipa::path(
    get,
    path = "",
    tag = "stats",
    operation_id = "getApiAdminStats",
    responses(
        (status = 200, description = "Platform statistics", body = PlatformStats)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_platform_stats(
    State(state): State<StatsState>,
    auth: Authenticated,
) -> Result<Json<PlatformStats>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    // Get client counts (find_active exists)
    let active_clients = state.client_repo.find_active().await?.len() as u64;
    // Total clients estimated as active (no find_all method)
    let total_clients = active_clients;

    // Get principal counts (find_active exists)
    let active_principals = state.principal_repo.find_active().await?;
    let total_principals = active_principals.len() as u64;

    // Get application counts (find_active exists)
    let active_applications = state.application_repo.find_active().await?.len() as u64;
    let total_applications = active_applications;

    // Get event type counts (find_active exists)
    let active_event_types = state.event_type_repo.find_active().await?.len() as u64;
    let total_event_types = active_event_types;

    // Get subscription counts (find_active exists)
    let active_subscriptions = state.subscription_repo.find_active().await?.len() as u64;
    let total_subscriptions = active_subscriptions;

    // Get dispatch pool count (find_active exists)
    let active_pools = state.dispatch_pool_repo.find_active().await?.len() as u64;
    let total_dispatch_pools = active_pools;

    // Event count not available without full scan
    let total_events = 0u64;

    // Get dispatch job counts (count methods exist)
    let total_dispatch_jobs = state.dispatch_job_repo.count_all().await.unwrap_or(0);
    let pending = state.dispatch_job_repo.count_by_status(DispatchStatus::Pending).await.unwrap_or(0);
    let queued = state.dispatch_job_repo.count_by_status(DispatchStatus::Queued).await.unwrap_or(0);
    let in_progress = state.dispatch_job_repo.count_by_status(DispatchStatus::InProgress).await.unwrap_or(0);
    let completed = state.dispatch_job_repo.count_by_status(DispatchStatus::Completed).await.unwrap_or(0);
    let failed = state.dispatch_job_repo.count_by_status(DispatchStatus::Failed).await.unwrap_or(0);

    let mut jobs_by_status = HashMap::new();
    jobs_by_status.insert("PENDING".to_string(), pending);
    jobs_by_status.insert("QUEUED".to_string(), queued);
    jobs_by_status.insert("IN_PROGRESS".to_string(), in_progress);
    jobs_by_status.insert("COMPLETED".to_string(), completed);
    jobs_by_status.insert("FAILED".to_string(), failed);

    Ok(Json(PlatformStats {
        total_clients,
        active_clients,
        total_principals,
        total_applications,
        active_applications,
        total_event_types,
        active_event_types,
        total_subscriptions,
        active_subscriptions,
        total_dispatch_pools,
        total_events,
        total_dispatch_jobs,
        jobs_by_status,
    }))
}

/// Create stats router
pub fn stats_router(state: StatsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(get_platform_stats))
        .with_state(state)
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
    operation_id = "getApiAdminMonitoringPoolStats",
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
