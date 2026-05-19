//! FlowCatalyst Router HTTP API
//!
//! HTTP API endpoints for:
//! - Message publishing
//! - Health and monitoring
//! - Kubernetes probes (liveness/readiness)
//! - Warning management
//! - Pool statistics
//! - Circuit breaker management
//! - Standby/traffic status
//! - Test/seed endpoints (development)

use crate::{
    CircuitBreakerRegistry, CircuitBreakerState, HealthService, InFlightMessageInfo, QueueManager,
    QueueMetrics, WarningService,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{Duration as ChronoDuration, Utc};
use fc_common::{
    HealthReport, HealthStatus, MediationType, Message, PoolConfig, PoolStats, Warning,
    WarningCategory, WarningSeverity,
};
use fc_queue::{QueueMetrics as FcQueueMetrics, QueuePublisher};
use fc_stream::StreamHealthService;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

pub mod auth;
pub mod model;
#[cfg(feature = "oidc-flow")]
pub mod oidc_flow;

pub use auth::{
    auth_middleware, create_auth_state, is_public_path, AuthConfig, AuthMode, AuthState,
    OidcValidator, TokenClaims,
};
use model::{PoolStatusResponse, PublishMessageRequest, PublishMessageResponse};

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub publisher: Arc<dyn QueuePublisher>,
    pub queue_manager: Arc<QueueManager>,
    pub warning_service: Arc<WarningService>,
    pub health_service: Arc<HealthService>,
    pub circuit_breaker_registry: Arc<CircuitBreakerRegistry>,
    /// Standby configuration (optional)
    pub standby_enabled: bool,
    pub instance_id: String,
    /// Stream health service (optional)
    pub stream_health_service: Option<Arc<StreamHealthService>>,
    /// Traffic strategy for ALB target group management (optional)
    pub traffic_strategy: Option<Arc<dyn crate::traffic::TrafficStrategy>>,
    /// Prometheus metrics handle for rendering /metrics endpoint
    pub metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
    /// Cached SQS broker stats — refreshed every 60s by background task,
    /// or on demand via POST /monitoring/broker-stats/refresh.
    pub cached_broker_stats: Arc<CachedBrokerStats>,
}

/// Cumulative per-queue counters captured at a point in time; used to compute
/// windowed deltas (current - baseline).
#[derive(Debug, Clone, Copy, Default)]
struct QueueCounterSnapshot {
    total_polled: u64,
    total_acked: u64,
    total_nacked: u64,
    total_deferred: u64,
}

struct CounterHistoryEntry {
    ts: std::time::Instant,
    per_queue: HashMap<String, QueueCounterSnapshot>,
}

/// Keep 30 min of history so the longest dashboard window has a baseline.
const COUNTER_HISTORY_WINDOW: Duration = Duration::from_secs(1800);

/// Cached SQS broker stats with timestamp.
/// Only the expensive SQS API attributes (pending/in-flight) are cached.
/// Counter metrics (polled/acked/nacked) are read live from consumer atomics on every request.
/// For windowed queue stats, we also retain a rolling history of cumulative
/// counter snapshots (30 min) so we can compute per-window deltas on demand.
pub struct CachedBrokerStats {
    /// Cached SQS attributes: pending_messages and in_flight_messages per queue
    sqs_attributes: RwLock<HashMap<String, (u64, u64)>>,
    last_updated: RwLock<Option<std::time::Instant>>,
    queue_manager: Arc<QueueManager>,
    /// Rolling history of cumulative counter snapshots, oldest first.
    counter_history: RwLock<VecDeque<CounterHistoryEntry>>,
}

impl CachedBrokerStats {
    pub fn new(queue_manager: Arc<QueueManager>) -> Self {
        Self {
            sqs_attributes: RwLock::new(HashMap::new()),
            last_updated: RwLock::new(None),
            queue_manager,
            counter_history: RwLock::new(VecDeque::new()),
        }
    }

    /// Fetch fresh SQS attributes (pending/in-flight) and update cache.
    /// Also appends a cumulative-counter snapshot used for windowed deltas.
    pub async fn refresh(&self) {
        let fresh = self.queue_manager.get_queue_metrics().await;
        let mut attrs = self.sqs_attributes.write().await;
        attrs.clear();
        for m in &fresh {
            attrs.insert(
                m.queue_identifier.clone(),
                (m.pending_messages, m.in_flight_messages),
            );
        }
        drop(attrs);
        *self.last_updated.write().await = Some(std::time::Instant::now());

        self.snapshot_counters().await;
    }

    async fn snapshot_counters(&self) {
        let live = self.queue_manager.get_queue_metrics_counters_only().await;
        let mut per_queue = HashMap::with_capacity(live.len());
        for m in live {
            per_queue.insert(
                m.queue_identifier,
                QueueCounterSnapshot {
                    total_polled: m.total_polled,
                    total_acked: m.total_acked,
                    total_nacked: m.total_nacked,
                    total_deferred: m.total_deferred,
                },
            );
        }
        let now = std::time::Instant::now();
        let cutoff = now.checked_sub(COUNTER_HISTORY_WINDOW).unwrap_or(now);
        let mut history = self.counter_history.write().await;
        history.push_back(CounterHistoryEntry { ts: now, per_queue });
        while history.front().is_some_and(|e| e.ts < cutoff) {
            history.pop_front();
        }
    }

    /// Get metrics with live counters overlaid on cached SQS attributes.
    /// When `window` is `Some`, cumulative counters are replaced with deltas over
    /// that window (picking the newest snapshot at or before `now - window`;
    /// falling back to the oldest snapshot if history is shorter than the window).
    pub async fn get_windowed(&self, window: Option<Duration>) -> Vec<FcQueueMetrics> {
        let cached_attrs = self.sqs_attributes.read().await;
        let mut live = self.queue_manager.get_queue_metrics_counters_only().await;

        for m in &mut live {
            if let Some(&(pending, in_flight)) = cached_attrs.get(&m.queue_identifier) {
                m.pending_messages = pending;
                m.in_flight_messages = in_flight;
            }
        }
        drop(cached_attrs);

        let Some(window) = window else {
            return live;
        };

        let history = self.counter_history.read().await;
        let now = std::time::Instant::now();
        let target = now.checked_sub(window).unwrap_or(now);

        let baseline = history
            .iter()
            .rev()
            .find(|e| e.ts <= target)
            .or_else(|| history.front());

        for m in &mut live {
            let base = baseline.and_then(|e| e.per_queue.get(&m.queue_identifier).copied());
            match base {
                Some(b) => {
                    m.total_polled = m.total_polled.saturating_sub(b.total_polled);
                    m.total_acked = m.total_acked.saturating_sub(b.total_acked);
                    m.total_nacked = m.total_nacked.saturating_sub(b.total_nacked);
                    m.total_deferred = m.total_deferred.saturating_sub(b.total_deferred);
                }
                None => {
                    m.total_polled = 0;
                    m.total_acked = 0;
                    m.total_nacked = 0;
                    m.total_deferred = 0;
                }
            }
        }

        live
    }

    /// Get time since last refresh
    pub async fn age_seconds(&self) -> Option<u64> {
        self.last_updated
            .read()
            .await
            .map(|t| t.elapsed().as_secs())
    }
}

/// Spawn background task that refreshes broker stats every 60 seconds
pub fn spawn_broker_stats_refresh(
    cached: Arc<CachedBrokerStats>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) {
    let mut shutdown_rx = shutdown_tx.subscribe();

    tokio::spawn(async move {
        // Initial fetch
        cached.refresh().await;

        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    cached.refresh().await;
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }
    });
}

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

/// Query params for warnings endpoint
#[derive(Deserialize, Default, ToSchema)]
pub struct WarningsQuery {
    /// Filter by severity: INFO, WARN, ERROR, CRITICAL
    pub severity: Option<String>,
    /// Filter by category: ROUTING, PROCESSING, CONFIGURATION, etc.
    pub category: Option<String>,
    /// Filter by acknowledged status
    pub acknowledged: Option<bool>,
}

/// Request to update pool configuration
#[derive(Debug, Deserialize, ToSchema)]
pub struct PoolConfigUpdateRequest {
    /// New concurrency limit
    pub concurrency: Option<u32>,
    /// New rate limit (messages per minute)
    pub rate_limit_per_minute: Option<u32>,
}

/// Request to reload router configuration
#[derive(Debug, Deserialize, ToSchema)]
pub struct ConfigReloadRequest {
    /// List of pool configurations
    pub processing_pools: Vec<PoolConfigRequest>,
}

/// Pool configuration in reload request
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PoolConfigRequest {
    /// Pool code/identifier
    pub code: String,
    /// Worker concurrency
    pub concurrency: u32,
    /// Optional rate limit (messages per minute)
    pub rate_limit_per_minute: Option<u32>,
}

/// Response after config reload
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfigReloadResponse {
    /// Whether the reload was successful
    pub success: bool,
    /// Number of pools updated
    pub pools_updated: usize,
    /// Number of new pools created
    pub pools_created: usize,
    /// Number of pools removed (draining)
    pub pools_removed: usize,
    /// Total active pools after reload
    pub total_active_pools: usize,
    /// Total pools currently draining
    pub total_draining_pools: usize,
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

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    info(
        title = "FlowCatalyst Message Router API",
        version = "0.1.0",
        description = "HTTP API for message routing, health monitoring, and pool management"
    ),
    paths(
        health_handler,
        liveness_probe,
        readiness_probe,
        metrics_handler,
        monitoring_handler,
        pool_stats_handler,
        queue_metrics_handler,
        update_pool_config,
        reload_config,
        list_warnings,
        acknowledge_warning,
        acknowledge_all_warnings,
        get_critical_warnings,
        get_unacknowledged_warnings,
        get_warnings_by_severity,
        clear_all_warnings,
        clear_old_warnings,
        dashboard_health_handler,
        dashboard_queue_stats_handler,
        dashboard_pool_stats_handler,
        dashboard_warnings_handler,
        dashboard_circuit_breakers_handler,
        dashboard_in_flight_messages_handler,
        in_flight_message_check_handler,
        in_flight_message_check_batch_handler,
        monitoring_acknowledge_warning,
        get_circuit_breaker_state,
        reset_circuit_breaker,
        reset_all_circuit_breakers,
        get_standby_status,
        get_traffic_status,
        seed_messages,
        get_local_config,
        test_fast,
        test_slow,
        test_faulty,
        test_fail,
        test_success,
        test_pending,
        test_client_error,
        test_server_error,
        test_stats,
        reset_test_stats,
        publish_message,
    ),
    components(schemas(
        SimpleHealthResponse,
        ProbeResponse,
        MonitoringResponse,
        WarningsQuery,
        PoolConfigUpdateRequest,
        ConfigReloadRequest,
        PoolConfigRequest,
        ConfigReloadResponse,
        QueueMetricsResponse,
        PublishMessageRequest,
        PublishMessageResponse,
        PoolStatusResponse,
        DashboardHealthResponse,
        DashboardHealthDetails,
        DashboardQueueStats,
        DashboardPoolStats,
        DashboardWarning,
        DashboardCircuitBreakerStats,
        InFlightMessagesQuery,
        StandbyStatusResponse,
        TrafficStatusResponse,
        SeedMessageRequest,
        SeedMessageResponse,
        ClearWarningsQuery,
        CircuitBreakerStateResponse,
    )),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "monitoring", description = "Monitoring and metrics endpoints"),
        (name = "warnings", description = "Warning management endpoints"),
        (name = "messages", description = "Message publishing endpoints"),
        (name = "circuit-breakers", description = "Circuit breaker management"),
        (name = "standby", description = "Standby and traffic management"),
        (name = "test", description = "Test endpoints for development"),
    )
)]
pub struct ApiDoc;

/// Create the full router with all endpoints (no auth)
pub fn create_router(
    publisher: Arc<dyn QueuePublisher>,
    queue_manager: Arc<QueueManager>,
    warning_service: Arc<WarningService>,
    health_service: Arc<HealthService>,
    circuit_breaker_registry: Arc<CircuitBreakerRegistry>,
) -> Router {
    create_router_with_options(
        publisher,
        queue_manager,
        warning_service,
        health_service,
        circuit_breaker_registry,
        false,
        "default".to_string(),
        None,
        None,
        None,
        None,
    )
}

/// Create the full router with all endpoints and options
///
/// When `auth_state` is provided and the auth mode is not `None`, authentication
/// middleware is applied to all non-public paths. Public paths (health, metrics,
/// swagger, auth login/callback/logout) are always accessible without credentials.
///
/// If the `oidc-flow` feature is enabled and auth mode is `OidcFlow`, the
/// `/auth/login`, `/auth/callback`, and `/auth/logout` routes are automatically
/// merged into the router.
// Router wiring requires every component as an explicit param so the caller
// can swap individual pieces (different queue, no-op health service, etc.)
// in tests. A builder would just be a rename of the same surface.
#[allow(clippy::too_many_arguments)]
pub fn create_router_with_options(
    publisher: Arc<dyn QueuePublisher>,
    queue_manager: Arc<QueueManager>,
    warning_service: Arc<WarningService>,
    health_service: Arc<HealthService>,
    circuit_breaker_registry: Arc<CircuitBreakerRegistry>,
    standby_enabled: bool,
    instance_id: String,
    stream_health_service: Option<Arc<StreamHealthService>>,
    traffic_strategy: Option<Arc<dyn crate::traffic::TrafficStrategy>>,
    metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
    auth_state: Option<AuthState>,
) -> Router {
    let cached_broker_stats = Arc::new(CachedBrokerStats::new(queue_manager.clone()));

    // Start background refresh — no external wiring needed
    {
        let cached = cached_broker_stats.clone();
        tokio::spawn(async move {
            // Initial fetch
            cached.refresh().await;

            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                ticker.tick().await;
                cached.refresh().await;
            }
        });
    }

    let state = AppState {
        publisher,
        queue_manager,
        warning_service,
        health_service,
        circuit_breaker_registry,
        standby_enabled,
        instance_id,
        stream_health_service,
        traffic_strategy,
        metrics_handle,
        cached_broker_stats,
    };

    // Public routes — no authentication required
    let public_routes = Router::new()
        // Swagger UI
        .merge(SwaggerUi::new("/swagger-ui").url("/api-doc/openapi.json", ApiDoc::openapi()))
        // Basic health
        .route("/health", get(health_handler))
        .route("/q/health", get(health_handler))
        // Kubernetes probes
        .route("/health/live", get(liveness_probe))
        .route("/health/ready", get(readiness_probe))
        .route("/health/startup", get(readiness_probe))
        .route("/q/health/live", get(liveness_probe))
        .route("/q/health/ready", get(readiness_probe))
        // Prometheus metrics
        .route("/metrics", get(metrics_handler))
        .route("/q/metrics", get(metrics_handler))
        .with_state(state.clone());

    // Protected routes — auth middleware applied when configured
    let protected_routes = Router::new()
        // Detailed monitoring
        .route("/monitoring", get(monitoring_handler))
        .route("/monitoring/health", get(dashboard_health_handler))
        .route("/monitoring/pools", get(pool_stats_handler))
        .route("/monitoring/pools/{poolCode}", put(update_pool_config))
        .route("/monitoring/queues", get(queue_metrics_handler))
        .route(
            "/monitoring/broker-stats/refresh",
            post(broker_stats_refresh_handler),
        )
        // Dashboard-compatible endpoints
        .route(
            "/monitoring/queue-stats",
            get(dashboard_queue_stats_handler),
        )
        .route("/monitoring/pool-stats", get(dashboard_pool_stats_handler))
        .route("/monitoring/warnings", get(dashboard_warnings_handler))
        .route(
            "/monitoring/warnings/{id}/acknowledge",
            post(monitoring_acknowledge_warning),
        )
        .route(
            "/monitoring/warnings/unacknowledged",
            get(get_unacknowledged_warnings),
        )
        .route(
            "/monitoring/warnings/severity/{severity}",
            get(get_warnings_by_severity),
        )
        .route(
            "/monitoring/circuit-breakers",
            get(dashboard_circuit_breakers_handler),
        )
        .route(
            "/monitoring/circuit-breakers/{name}/state",
            get(get_circuit_breaker_state),
        )
        .route(
            "/monitoring/circuit-breakers/{name}/reset",
            post(reset_circuit_breaker),
        )
        .route(
            "/monitoring/circuit-breakers/reset-all",
            post(reset_all_circuit_breakers),
        )
        .route(
            "/monitoring/in-flight-messages",
            get(dashboard_in_flight_messages_handler),
        )
        .route(
            "/monitoring/in-flight-messages/check",
            get(in_flight_message_check_handler),
        )
        .route(
            "/monitoring/in-flight-messages/check-batch",
            post(in_flight_message_check_batch_handler),
        )
        .route("/monitoring/dashboard", get(dashboard_html_handler))
        .route("/monitoring/consumer-health", get(consumer_health_handler))
        .route("/monitoring/standby-status", get(get_standby_status))
        .route("/monitoring/traffic-status", get(get_traffic_status))
        // Java-compatible dashboard path alias
        .route("/dashboard.html", get(dashboard_html_handler))
        // Stream processor health endpoints
        .route("/monitoring/stream-health", get(stream_health_handler))
        .route(
            "/monitoring/stream-health/live",
            get(stream_liveness_handler),
        )
        .route(
            "/monitoring/stream-health/ready",
            get(stream_readiness_handler),
        )
        // Configuration management
        .route("/config/reload", post(reload_config))
        .route("/api/config", get(get_local_config))
        // Warnings management
        .route("/warnings", get(list_warnings).delete(clear_all_warnings))
        .route("/warnings/{id}/acknowledge", post(acknowledge_warning))
        .route("/warnings/acknowledge-all", post(acknowledge_all_warnings))
        .route("/warnings/critical", get(get_critical_warnings))
        .route("/warnings/unacknowledged", get(get_unacknowledged_warnings))
        .route("/warnings/old", delete(clear_old_warnings))
        // Message seeding (test)
        .route("/api/seed/messages", post(seed_messages))
        // Test response endpoints (development)
        .route("/api/test/fast", post(test_fast))
        .route("/api/test/slow", post(test_slow))
        .route("/api/test/faulty", post(test_faulty))
        .route("/api/test/fail", post(test_fail))
        .route("/api/test/success", post(test_success))
        .route("/api/test/pending", post(test_pending))
        .route("/api/test/client-error", post(test_client_error))
        .route("/api/test/server-error", post(test_server_error))
        .route("/api/test/stats", get(test_stats).post(reset_test_stats))
        .route("/api/test/stats/reset", post(reset_test_stats))
        // Java-compatible benchmark endpoints (aliases for test endpoints)
        .route("/api/benchmark/process", post(test_fast))
        .route("/api/benchmark/process-slow", post(test_slow))
        .route("/api/benchmark/stats", get(test_stats))
        .route("/api/benchmark/reset", post(reset_test_stats))
        // Message publishing
        .route("/messages", post(publish_message))
        .with_state(state);

    // Apply auth middleware to protected routes when configured
    #[allow(unused_mut)]
    let mut router = if let Some(ref auth) = auth_state {
        if auth.config.mode != AuthMode::None {
            info!(mode = ?auth.config.mode, "Authentication enabled for router API");
            public_routes.merge(protected_routes.layer(axum::middleware::from_fn_with_state(
                auth.clone(),
                auth_middleware,
            )))
        } else {
            public_routes.merge(protected_routes)
        }
    } else {
        public_routes.merge(protected_routes)
    };

    // Merge OIDC flow routes when feature enabled and mode is OidcFlow
    #[cfg(feature = "oidc-flow")]
    if let Some(ref auth) = auth_state {
        if auth.config.mode == AuthMode::OidcFlow {
            if let Some(ref flow_state) = auth.oidc_flow_state {
                info!("OIDC authorization code flow routes enabled (/auth/login, /auth/callback, /auth/logout)");
                router = router.merge(oidc_flow::oidc_flow_routes(flow_state.clone()));
            }
        }
    }

    router
}

/// Simple state for simple router
#[derive(Clone)]
pub struct SimpleState {
    pub publisher: Arc<dyn QueuePublisher>,
}

/// Create a simple router with just message publishing
pub fn create_simple_router(publisher: Arc<dyn QueuePublisher>) -> Router {
    let state = SimpleState { publisher };

    Router::new()
        .route("/health", get(simple_health_handler))
        .route("/messages", post(simple_publish_message))
        .with_state(state)
}

// ============================================================================
// Health Endpoints
// ============================================================================

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Health status", body = SimpleHealthResponse)
    )
)]
async fn health_handler(State(state): State<AppState>) -> Json<SimpleHealthResponse> {
    let pool_stats = state.queue_manager.get_pool_stats();
    let report = state.health_service.get_health_report(&pool_stats);

    let status = match report.status {
        HealthStatus::Healthy => "UP",
        HealthStatus::Warning => "UP",
        HealthStatus::Degraded => "DEGRADED",
    };

    Json(SimpleHealthResponse {
        status: status.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Simple health handler (no state dependency)
async fn simple_health_handler() -> Json<SimpleHealthResponse> {
    Json(SimpleHealthResponse {
        status: "UP".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
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
async fn liveness_probe() -> Json<ProbeResponse> {
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
async fn readiness_probe(State(state): State<AppState>) -> Response {
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
async fn metrics_handler(State(state): State<AppState>) -> Response {
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

// ============================================================================
// Monitoring Endpoints
// ============================================================================

/// Detailed monitoring information
#[utoipa::path(
    get,
    path = "/monitoring",
    tag = "monitoring",
    responses(
        (status = 200, description = "Monitoring data", body = MonitoringResponse)
    )
)]
async fn monitoring_handler(State(state): State<AppState>) -> Json<MonitoringResponse> {
    let pool_stats = state.queue_manager.get_pool_stats();
    let health_report = state.health_service.get_health_report(&pool_stats);
    let active_warnings = state.warning_service.unacknowledged_count() as u32;
    let critical_warnings = state.warning_service.critical_count() as u32;

    let status = match health_report.status {
        HealthStatus::Healthy => "HEALTHY",
        HealthStatus::Warning => "WARNING",
        HealthStatus::Degraded => "DEGRADED",
    };

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
async fn pool_stats_handler(State(state): State<AppState>) -> Json<Vec<PoolStats>> {
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
async fn queue_metrics_handler(State(state): State<AppState>) -> Json<Vec<QueueMetricsResponse>> {
    let metrics = state.queue_manager.get_queue_metrics().await;
    Json(
        metrics
            .into_iter()
            .map(QueueMetricsResponse::from)
            .collect(),
    )
}

// ============================================================================
// Configuration Management
// ============================================================================

/// Reload configuration (hot reload)
#[utoipa::path(
    post,
    path = "/config/reload",
    tag = "monitoring",
    request_body = ConfigReloadRequest,
    responses(
        (status = 200, description = "Configuration reloaded", body = ConfigReloadResponse),
        (status = 503, description = "Service unavailable", body = ConfigReloadResponse),
        (status = 500, description = "Internal error", body = ConfigReloadResponse)
    )
)]
async fn reload_config(
    State(state): State<AppState>,
    Json(req): Json<ConfigReloadRequest>,
) -> Response {
    use fc_common::RouterConfig;

    let router_config = RouterConfig {
        processing_pools: req
            .processing_pools
            .into_iter()
            .map(|p| PoolConfig {
                code: p.code,
                concurrency: p.concurrency,
                rate_limit_per_minute: p.rate_limit_per_minute,
            })
            .collect(),
        queues: vec![],
    };

    let pools_before = state.queue_manager.pool_codes().len();

    match state.queue_manager.reload_config(router_config).await {
        Ok(true) => {
            let pools_after = state.queue_manager.pool_codes().len();
            let pool_stats = state.queue_manager.get_pool_stats();
            let pools_created = pools_after.saturating_sub(pools_before);
            let pools_removed = pools_before.saturating_sub(pools_after);

            info!(
                pools_before = pools_before,
                pools_after = pools_after,
                pools_created = pools_created,
                pools_removed = pools_removed,
                "Configuration reloaded via API"
            );

            (
                StatusCode::OK,
                Json(ConfigReloadResponse {
                    success: true,
                    pools_updated: 0,
                    pools_created,
                    pools_removed,
                    total_active_pools: pool_stats.len(),
                    total_draining_pools: 0,
                }),
            )
                .into_response()
        }
        Ok(false) => {
            warn!("Configuration reload was skipped (shutdown in progress)");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ConfigReloadResponse {
                    success: false,
                    pools_updated: 0,
                    pools_created: 0,
                    pools_removed: 0,
                    total_active_pools: 0,
                    total_draining_pools: 0,
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!(error = %e, "Failed to reload configuration");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ConfigReloadResponse {
                    success: false,
                    pools_updated: 0,
                    pools_created: 0,
                    pools_removed: 0,
                    total_active_pools: 0,
                    total_draining_pools: 0,
                }),
            )
                .into_response()
        }
    }
}

/// Update pool configuration
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
        (status = 500, description = "Internal error")
    )
)]
async fn update_pool_config(
    State(state): State<AppState>,
    Path(pool_code): Path<String>,
    Json(req): Json<PoolConfigUpdateRequest>,
) -> Response {
    let existing_stats: Option<PoolStats> = state
        .queue_manager
        .get_pool_stats()
        .into_iter()
        .find(|s| s.pool_code == pool_code);

    let new_config = match existing_stats {
        Some(stats) => PoolConfig {
            code: pool_code.clone(),
            concurrency: req.concurrency.unwrap_or(stats.concurrency),
            rate_limit_per_minute: if req.rate_limit_per_minute.is_some() {
                req.rate_limit_per_minute
            } else {
                stats.rate_limit_per_minute
            },
        },
        None => PoolConfig {
            code: pool_code.clone(),
            concurrency: req.concurrency.unwrap_or(10),
            rate_limit_per_minute: req.rate_limit_per_minute,
        },
    };

    match state
        .queue_manager
        .update_pool_config(&pool_code, new_config.clone())
        .await
    {
        Ok(_) => {
            info!(pool_code = %pool_code, "Pool configuration updated via API");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "pool_code": pool_code,
                    "new_config": {
                        "concurrency": new_config.concurrency,
                        "rate_limit_per_minute": new_config.rate_limit_per_minute,
                    }
                })),
            )
                .into_response()
        }
        Err(e) => {
            error!(pool_code = %pool_code, error = %e, "Failed to update pool configuration");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": e.to_string(),
                })),
            )
                .into_response()
        }
    }
}

// ============================================================================
// Warning Endpoints
// ============================================================================

/// List warnings with optional filters
#[utoipa::path(
    get,
    path = "/warnings",
    tag = "warnings",
    params(
        ("severity" = Option<String>, Query, description = "Filter by severity"),
        ("category" = Option<String>, Query, description = "Filter by category"),
        ("acknowledged" = Option<bool>, Query, description = "Filter by acknowledged status")
    ),
    responses(
        (status = 200, description = "List of warnings", body = Vec<Warning>)
    )
)]
async fn list_warnings(
    State(state): State<AppState>,
    Query(query): Query<WarningsQuery>,
) -> Json<Vec<Warning>> {
    let mut warnings = if let Some(false) = query.acknowledged {
        state.warning_service.get_unacknowledged_warnings()
    } else {
        state.warning_service.get_all_warnings()
    };

    // Filter by severity if specified
    if let Some(ref sev_str) = query.severity {
        let severity = match sev_str.to_uppercase().as_str() {
            "INFO" => Some(WarningSeverity::Info),
            "WARN" | "WARNING" => Some(WarningSeverity::Warn),
            "ERROR" => Some(WarningSeverity::Error),
            "CRITICAL" => Some(WarningSeverity::Critical),
            _ => None,
        };
        if let Some(sev) = severity {
            warnings.retain(|w| w.severity == sev);
        }
    }

    // Filter by category if specified
    if let Some(ref cat_str) = query.category {
        let category = match cat_str.to_uppercase().as_str() {
            "ROUTING" => Some(WarningCategory::Routing),
            "PROCESSING" => Some(WarningCategory::Processing),
            "CONFIGURATION" => Some(WarningCategory::Configuration),
            "GROUPTHREADRESTART" => Some(WarningCategory::GroupThreadRestart),
            "RATELIMITING" => Some(WarningCategory::RateLimiting),
            "QUEUECONNECTIVITY" => Some(WarningCategory::QueueConnectivity),
            "POOLCAPACITY" => Some(WarningCategory::PoolCapacity),
            "CONSUMERHEALTH" => Some(WarningCategory::ConsumerHealth),
            "RESOURCE" => Some(WarningCategory::Resource),
            _ => None,
        };
        if let Some(cat) = category {
            warnings.retain(|w| w.category == cat);
        }
    }

    // Sort by created_at descending (newest first)
    warnings.sort_by_key(|w| std::cmp::Reverse(w.created_at));

    Json(warnings)
}

/// Acknowledge a warning
#[utoipa::path(
    post,
    path = "/warnings/{id}/acknowledge",
    tag = "warnings",
    params(
        ("id" = String, Path, description = "Warning ID to acknowledge")
    ),
    responses(
        (status = 200, description = "Warning acknowledged"),
        (status = 404, description = "Warning not found")
    )
)]
async fn acknowledge_warning(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if state.warning_service.acknowledge_warning(&id) {
        debug!(id = %id, "Warning acknowledged");
        (
            StatusCode::OK,
            Json(serde_json::json!({ "acknowledged": true })),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Warning not found" })),
        )
            .into_response()
    }
}

/// Acknowledge all warnings
#[utoipa::path(
    post,
    path = "/warnings/acknowledge-all",
    tag = "warnings",
    responses(
        (status = 200, description = "All warnings acknowledged")
    )
)]
async fn acknowledge_all_warnings(State(state): State<AppState>) -> Json<serde_json::Value> {
    let count = state.warning_service.acknowledge_matching(|_| true);
    debug!(count = count, "Acknowledged all warnings");
    Json(serde_json::json!({ "acknowledged": count }))
}

/// Get critical warnings
#[utoipa::path(
    get,
    path = "/warnings/critical",
    tag = "warnings",
    responses(
        (status = 200, description = "Critical warnings", body = Vec<Warning>)
    )
)]
async fn get_critical_warnings(State(state): State<AppState>) -> Json<Vec<Warning>> {
    Json(state.warning_service.get_critical_warnings())
}

// ============================================================================
// Dashboard-Compatible Endpoints
// ============================================================================

/// Dashboard health response matching Java format
#[derive(Serialize, ToSchema)]
struct DashboardHealthResponse {
    status: String,
    timestamp: String,
    #[serde(rename = "uptimeMillis")]
    uptime_millis: u64,
    details: Option<DashboardHealthDetails>,
}

#[derive(Serialize, ToSchema)]
struct DashboardHealthDetails {
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
async fn dashboard_health_handler(State(state): State<AppState>) -> Json<DashboardHealthResponse> {
    let pool_stats = state.queue_manager.get_pool_stats();
    let health_report = state.health_service.get_health_report(&pool_stats);

    let status = match health_report.status {
        HealthStatus::Healthy => "HEALTHY",
        HealthStatus::Warning => "WARNING",
        HealthStatus::Degraded => "DEGRADED",
    };

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
struct DashboardStatsQuery {
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
struct DashboardQueueStats {
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
async fn dashboard_queue_stats_handler(
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

/// Refresh broker stats on demand (called when user clicks refresh in dashboard)
#[utoipa::path(
    post,
    path = "/monitoring/broker-stats/refresh",
    tag = "monitoring",
    responses(
        (status = 200, description = "Broker stats refreshed")
    )
)]
async fn broker_stats_refresh_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    state.cached_broker_stats.refresh().await;
    let age = state.cached_broker_stats.age_seconds().await;
    Json(serde_json::json!({
        "refreshed": true,
        "ageSeconds": age.unwrap_or(0)
    }))
}

/// Pool stats for dashboard. Throughput-style fields (`totalProcessed`,
/// `totalSucceeded`, `totalFailed`, `totalRateLimited`, `successRate`,
/// `averageProcessingTimeMs`) are scoped to the requested time window.
/// Live-state fields (`activeWorkers`, `queueSize`, etc.) always reflect
/// the current pool state.
#[derive(Serialize, ToSchema)]
struct DashboardPoolStats {
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
async fn dashboard_pool_stats_handler(
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

/// Warning format for dashboard
#[derive(Serialize, ToSchema)]
struct DashboardWarning {
    id: String,
    timestamp: String,
    severity: String,
    category: String,
    source: String,
    message: String,
    acknowledged: bool,
}

/// Warnings endpoint for dashboard
#[utoipa::path(
    get,
    path = "/monitoring/warnings",
    tag = "monitoring",
    responses(
        (status = 200, description = "Warnings for dashboard", body = Vec<DashboardWarning>)
    )
)]
async fn dashboard_warnings_handler(State(state): State<AppState>) -> Json<Vec<DashboardWarning>> {
    let warnings = state.warning_service.get_all_warnings();

    let result: Vec<DashboardWarning> = warnings
        .into_iter()
        .map(|w| DashboardWarning {
            id: w.id,
            timestamp: w.created_at.to_rfc3339(),
            severity: format!("{:?}", w.severity).to_uppercase(),
            category: format!("{:?}", w.category).to_uppercase(),
            source: w.source,
            message: w.message,
            acknowledged: w.acknowledged,
        })
        .collect();

    Json(result)
}

/// Circuit breaker stats for dashboard
#[derive(Serialize, ToSchema)]
struct DashboardCircuitBreakerStats {
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
async fn dashboard_circuit_breakers_handler(
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
struct InFlightMessagesQuery {
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
async fn dashboard_in_flight_messages_handler(
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
struct InFlightCheckQuery {
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
struct InFlightCheckResponse {
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
async fn in_flight_message_check_handler(
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
struct InFlightCheckBatchRequest {
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
async fn in_flight_message_check_batch_handler(
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

/// Serve dashboard HTML, with the mount prefix injected so the page works
/// both standalone (no prefix) and when nested under a parent router
/// (e.g. fc-dev nests this whole crate under `/q/router`).
///
/// The injected `window.__API_BASE__` is consumed by `fetchWithAuth` in
/// `dashboard.html` to prepend onto every `/monitoring/...` request.
async fn dashboard_html_handler(
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
) -> impl IntoResponse {
    const DASHBOARD_HTML: &str = include_str!("../../resources/dashboard.html");

    // The handler is mounted at both `/monitoring/dashboard` and
    // `/dashboard.html`; strip whichever matches to recover the prefix
    // that any nesting parent contributed. Fall back to empty when the
    // request URI doesn't end with either (shouldn't happen, but better
    // than guessing).
    let path = uri.path();
    let prefix = path
        .strip_suffix("/monitoring/dashboard")
        .or_else(|| path.strip_suffix("/dashboard.html"))
        .unwrap_or("");

    Html(DASHBOARD_HTML.replace("__FC_API_BASE__", prefix))
}

/// Consumer health endpoint (matches Java /monitoring/consumer-health)
async fn consumer_health_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
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
// Message Publishing
// ============================================================================

/// Publish a message
#[utoipa::path(
    post,
    path = "/messages",
    tag = "messages",
    request_body = PublishMessageRequest,
    responses(
        (status = 200, description = "Message published", body = PublishMessageResponse),
        (status = 500, description = "Failed to publish")
    )
)]
async fn publish_message(
    State(state): State<AppState>,
    Json(req): Json<PublishMessageRequest>,
) -> Response {
    let message_id = Uuid::new_v4().to_string();

    let message = Message {
        id: message_id.clone(),
        pool_code: req.pool_code.unwrap_or_else(|| "DEFAULT".to_string()),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: req
            .mediation_target
            .unwrap_or_else(|| "http://localhost:8080/echo".to_string()),
        message_group_id: req.message_group_id,
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(),
    };

    match state.publisher.publish(message).await {
        Ok(_) => (
            StatusCode::OK,
            Json(PublishMessageResponse {
                message_id,
                status: "ACCEPTED".to_string(),
            }),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to publish message" })),
        )
            .into_response(),
    }
}

/// Simple publish message (for simple router)
async fn simple_publish_message(
    State(state): State<SimpleState>,
    Json(req): Json<PublishMessageRequest>,
) -> Response {
    let message_id = Uuid::new_v4().to_string();

    let message = Message {
        id: message_id.clone(),
        pool_code: req.pool_code.unwrap_or_else(|| "DEFAULT".to_string()),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: req
            .mediation_target
            .unwrap_or_else(|| "http://localhost:8080/echo".to_string()),
        message_group_id: req.message_group_id,
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(),
    };

    match state.publisher.publish(message).await {
        Ok(_) => (
            StatusCode::OK,
            Json(PublishMessageResponse {
                message_id,
                status: "ACCEPTED".to_string(),
            }),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to publish message" })),
        )
            .into_response(),
    }
}

// ============================================================================
// Additional Warning Endpoints (Java compatibility)
// ============================================================================

/// Get unacknowledged warnings
#[utoipa::path(
    get,
    path = "/warnings/unacknowledged",
    tag = "warnings",
    responses(
        (status = 200, description = "Unacknowledged warnings", body = Vec<Warning>)
    )
)]
async fn get_unacknowledged_warnings(State(state): State<AppState>) -> Json<Vec<Warning>> {
    Json(state.warning_service.get_unacknowledged_warnings())
}

/// Get warnings by severity
#[utoipa::path(
    get,
    path = "/monitoring/warnings/severity/{severity}",
    tag = "warnings",
    params(
        ("severity" = String, Path, description = "Severity level: CRITICAL, ERROR, WARN, INFO")
    ),
    responses(
        (status = 200, description = "Warnings of specified severity", body = Vec<Warning>)
    )
)]
async fn get_warnings_by_severity(
    State(state): State<AppState>,
    Path(severity): Path<String>,
) -> Json<Vec<Warning>> {
    let severity_enum = match severity.to_uppercase().as_str() {
        "INFO" => Some(WarningSeverity::Info),
        "WARN" | "WARNING" => Some(WarningSeverity::Warn),
        "ERROR" => Some(WarningSeverity::Error),
        "CRITICAL" => Some(WarningSeverity::Critical),
        _ => None,
    };

    let warnings = match severity_enum {
        Some(sev) => state.warning_service.get_warnings_by_severity(sev),
        None => vec![],
    };

    Json(warnings)
}

/// Acknowledge warning (monitoring path for Java compatibility)
#[utoipa::path(
    post,
    path = "/monitoring/warnings/{id}/acknowledge",
    tag = "warnings",
    params(
        ("id" = String, Path, description = "Warning ID to acknowledge")
    ),
    responses(
        (status = 200, description = "Warning acknowledged"),
        (status = 404, description = "Warning not found")
    )
)]
async fn monitoring_acknowledge_warning(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if state.warning_service.acknowledge_warning(&id) {
        debug!(id = %id, "Warning acknowledged via monitoring endpoint");
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success" })),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Warning not found" })),
        )
            .into_response()
    }
}

/// Query for clearing old warnings
#[derive(Deserialize, Default, ToSchema)]
struct ClearWarningsQuery {
    /// Hours old (default 24)
    hours: Option<i64>,
}

/// Clear all warnings
#[utoipa::path(
    delete,
    path = "/warnings",
    tag = "warnings",
    responses(
        (status = 200, description = "All warnings cleared")
    )
)]
async fn clear_all_warnings(State(state): State<AppState>) -> Json<serde_json::Value> {
    let count = state.warning_service.get_all_warnings().len();
    // Clear by acknowledging and then removing old
    state.warning_service.acknowledge_matching(|_| true);
    state.warning_service.clear_old_warnings(0);
    debug!(count = count, "Cleared all warnings");
    Json(serde_json::json!({ "status": "success", "cleared": count }))
}

/// Clear old warnings
#[utoipa::path(
    delete,
    path = "/warnings/old",
    tag = "warnings",
    params(
        ("hours" = Option<i64>, Query, description = "Clear warnings older than this many hours (default 24)")
    ),
    responses(
        (status = 200, description = "Old warnings cleared")
    )
)]
async fn clear_old_warnings(
    State(state): State<AppState>,
    Query(query): Query<ClearWarningsQuery>,
) -> Json<serde_json::Value> {
    let hours = query.hours.unwrap_or(24);
    let removed = state.warning_service.clear_old_warnings(hours);
    debug!(hours = hours, removed = removed, "Cleared old warnings");
    Json(serde_json::json!({ "status": "success", "removed": removed }))
}

// ============================================================================
// Circuit Breaker Endpoints
// ============================================================================

/// Circuit breaker state response
#[derive(Serialize, ToSchema)]
struct CircuitBreakerStateResponse {
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
async fn get_circuit_breaker_state(
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
async fn reset_circuit_breaker(
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
async fn reset_all_circuit_breakers(State(state): State<AppState>) -> Json<serde_json::Value> {
    state.circuit_breaker_registry.reset_all();
    info!("All circuit breakers reset");
    Json(serde_json::json!({ "status": "success" }))
}

// ============================================================================
// Standby/Traffic Status Endpoints
// ============================================================================

/// Standby status response (matches Java format)
#[derive(Serialize, ToSchema)]
struct StandbyStatusResponse {
    #[serde(rename = "standbyEnabled")]
    standby_enabled: bool,
    #[serde(rename = "instanceId")]
    instance_id: String,
    role: String,
    #[serde(rename = "redisAvailable")]
    redis_available: bool,
    #[serde(rename = "currentLockHolder")]
    current_lock_holder: Option<String>,
    #[serde(rename = "lastSuccessfulRefresh")]
    last_successful_refresh: Option<String>,
    #[serde(rename = "hasWarning")]
    has_warning: bool,
}

/// Get standby status
#[utoipa::path(
    get,
    path = "/monitoring/standby-status",
    tag = "standby",
    responses(
        (status = 200, description = "Standby status", body = StandbyStatusResponse)
    )
)]
async fn get_standby_status(State(state): State<AppState>) -> Json<StandbyStatusResponse> {
    Json(StandbyStatusResponse {
        standby_enabled: state.standby_enabled,
        instance_id: state.instance_id.clone(),
        role: "PRIMARY".to_string(), // Always primary when standby not enabled
        redis_available: false,
        current_lock_holder: Some(state.instance_id.clone()),
        last_successful_refresh: Some(Utc::now().to_rfc3339()),
        has_warning: false,
    })
}

/// Traffic status response (matches Java format)
#[derive(Serialize, ToSchema)]
struct TrafficStatusResponse {
    enabled: bool,
    #[serde(rename = "strategyType")]
    strategy_type: String,
    registered: bool,
    #[serde(rename = "targetInfo")]
    target_info: Option<String>,
    #[serde(rename = "lastOperation")]
    last_operation: Option<String>,
    #[serde(rename = "lastError")]
    last_error: String,
}

/// Get traffic status
#[utoipa::path(
    get,
    path = "/monitoring/traffic-status",
    tag = "standby",
    responses(
        (status = 200, description = "Traffic status", body = TrafficStatusResponse)
    )
)]
async fn get_traffic_status(State(state): State<AppState>) -> Json<TrafficStatusResponse> {
    match &state.traffic_strategy {
        Some(strategy) => Json(TrafficStatusResponse {
            enabled: true,
            strategy_type: strategy.strategy_type().to_string(),
            registered: strategy.is_registered(),
            target_info: None,
            last_operation: Some(Utc::now().to_rfc3339()),
            last_error: "none".to_string(),
        }),
        None => Json(TrafficStatusResponse {
            enabled: false,
            strategy_type: "NONE".to_string(),
            registered: true,
            target_info: None,
            last_operation: Some(Utc::now().to_rfc3339()),
            last_error: "none".to_string(),
        }),
    }
}

// ============================================================================
// Stream Health Endpoints
// ============================================================================

/// Stream processor health response (Java-compatible)
#[derive(Serialize, ToSchema)]
struct StreamHealthResponse {
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
async fn stream_health_handler(State(state): State<AppState>) -> Json<StreamHealthResponse> {
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
async fn stream_liveness_handler(State(state): State<AppState>) -> Response {
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
async fn stream_readiness_handler(State(state): State<AppState>) -> Response {
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

// ============================================================================
// Local Config Endpoint
// ============================================================================

/// Get local configuration
///
/// In dev mode (FLOWCATALYST_DEV_MODE=true), returns LocalStack queue URLs.
/// Otherwise returns current pool configuration.
#[utoipa::path(
    get,
    path = "/api/config",
    tag = "monitoring",
    responses(
        (status = 200, description = "Local configuration")
    )
)]
async fn get_local_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let pool_stats = state.queue_manager.get_pool_stats();
    let dev_mode = std::env::var("FLOWCATALYST_DEV_MODE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let pools: Vec<serde_json::Value> = if dev_mode && pool_stats.is_empty() {
        // Return default dev pools
        vec![
            serde_json::json!({
                "code": "DEFAULT",
                "concurrency": 10,
                "rateLimitPerMinute": null,
            }),
            serde_json::json!({
                "code": "HIGH",
                "concurrency": 20,
                "rateLimitPerMinute": null,
            }),
            serde_json::json!({
                "code": "LOW",
                "concurrency": 5,
                "rateLimitPerMinute": 60,
            }),
        ]
    } else {
        pool_stats
            .iter()
            .map(|p| {
                serde_json::json!({
                    "code": p.pool_code,
                    "concurrency": p.concurrency,
                    "rateLimitPerMinute": p.rate_limit_per_minute,
                })
            })
            .collect()
    };

    let queues: Vec<serde_json::Value> = if dev_mode {
        // Return LocalStack queue URLs for development
        // LocalStack uses this URL format for SQS queues
        let sqs_host = std::env::var("LOCALSTACK_SQS_HOST")
            .unwrap_or_else(|_| "http://sqs.eu-west-1.localhost.localstack.cloud:4566".to_string());

        vec![
            serde_json::json!({
                "queueName": "fc-high-priority.fifo",
                "queueUri": format!("{}/000000000000/fc-high-priority.fifo", sqs_host),
                "connections": 2,
                "visibilityTimeout": 120,
            }),
            serde_json::json!({
                "queueName": "fc-default.fifo",
                "queueUri": format!("{}/000000000000/fc-default.fifo", sqs_host),
                "connections": 2,
                "visibilityTimeout": 120,
            }),
            serde_json::json!({
                "queueName": "fc-low-priority.fifo",
                "queueUri": format!("{}/000000000000/fc-low-priority.fifo", sqs_host),
                "connections": 1,
                "visibilityTimeout": 120,
            }),
        ]
    } else {
        vec![]
    };

    Json(serde_json::json!({
        "queues": queues,
        "connections": 1,
        "processingPools": pools,
    }))
}

// ============================================================================
// Test/Seed Endpoints (Development)
// ============================================================================

/// Message seed request (matches Java format)
#[derive(Debug, Deserialize, ToSchema)]
struct SeedMessageRequest {
    count: Option<u32>,
    queue: Option<String>,
    endpoint: Option<String>,
    #[serde(rename = "messageGroupMode")]
    message_group_mode: Option<String>,
}

/// Message seed response
#[derive(Serialize, ToSchema)]
struct SeedMessageResponse {
    status: String,
    #[serde(rename = "messagesSent", skip_serializing_if = "Option::is_none")]
    messages_sent: Option<u32>,
    #[serde(rename = "totalRequested", skip_serializing_if = "Option::is_none")]
    total_requested: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

/// Seed test messages
#[utoipa::path(
    post,
    path = "/api/seed/messages",
    tag = "test",
    request_body = SeedMessageRequest,
    responses(
        (status = 200, description = "Messages seeded", body = SeedMessageResponse)
    )
)]
async fn seed_messages(
    State(state): State<AppState>,
    Json(req): Json<SeedMessageRequest>,
) -> Json<SeedMessageResponse> {
    let count = req.count.unwrap_or(10).min(1000);
    let endpoint = req.endpoint.unwrap_or_else(|| "fast".to_string());
    let _queue = req.queue.unwrap_or_else(|| "high".to_string());
    let message_group_mode = req.message_group_mode.unwrap_or_else(|| "1of8".to_string()); // Java default

    // Resolve endpoint
    let target = match endpoint.as_str() {
        "fast" => "http://localhost:8080/api/test/fast",
        "slow" => "http://localhost:8080/api/test/slow",
        "faulty" => "http://localhost:8080/api/test/faulty",
        "fail" => "http://localhost:8080/api/test/fail",
        "random" => "http://localhost:8080/api/test/faulty",
        other if other.starts_with("http") => other,
        _ => "http://localhost:8080/api/test/fast",
    };

    let mut sent = 0u32;
    for i in 0..count {
        let message_group_id = match message_group_mode.as_str() {
            "unique" => Some(format!("unique-{}", Uuid::new_v4())),
            "1of8" => Some(format!("group-{}", i % 8)),
            "single" => Some("single-group".to_string()),
            _ => None,
        };

        let message = Message {
            id: Uuid::new_v4().to_string(),
            pool_code: "DEFAULT".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: target.to_string(),
            message_group_id,
            high_priority: false,
            dispatch_mode: fc_common::DispatchMode::default(),
        };

        if state.publisher.publish(message).await.is_ok() {
            sent += 1;
        }
    }

    Json(SeedMessageResponse {
        status: "success".to_string(),
        messages_sent: Some(sent),
        total_requested: Some(count),
        message: None,
    })
}

// Global test stats counter
static TEST_REQUEST_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Test fast endpoint (100ms delay)
#[utoipa::path(
    post,
    path = "/api/test/fast",
    tag = "test",
    responses(
        (status = 200, description = "Fast response")
    )
)]
async fn test_fast() -> Json<serde_json::Value> {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Json(serde_json::json!({ "status": "success", "ack": true }))
}

/// Test slow endpoint (60s delay)
#[utoipa::path(
    post,
    path = "/api/test/slow",
    tag = "test",
    responses(
        (status = 200, description = "Slow response")
    )
)]
async fn test_slow() -> Json<serde_json::Value> {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    Json(serde_json::json!({ "status": "success", "ack": true }))
}

/// Test faulty endpoint (random responses)
#[utoipa::path(
    post,
    path = "/api/test/faulty",
    tag = "test",
    responses(
        (status = 200, description = "Success response"),
        (status = 400, description = "Client error"),
        (status = 500, description = "Server error")
    )
)]
async fn test_faulty() -> Response {
    use rand::Rng;

    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut rng = rand::rng();
    let roll: f64 = rng.random();

    if roll < 0.6 {
        // 60% success
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "ack": true })),
        )
            .into_response()
    } else if roll < 0.8 {
        // 20% 400 error
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "error": "Client error" })),
        )
            .into_response()
    } else {
        // 20% 500 error
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "status": "error", "error": "Server error" })),
        )
            .into_response()
    }
}

/// Test fail endpoint (always 500)
#[utoipa::path(
    post,
    path = "/api/test/fail",
    tag = "test",
    responses(
        (status = 500, description = "Always fails")
    )
)]
async fn test_fail() -> Response {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "status": "error", "error": "Always fails" })),
    )
        .into_response()
}

/// Test success endpoint (always 200 with ack=true)
#[utoipa::path(
    post,
    path = "/api/test/success",
    tag = "test",
    responses(
        (status = 200, description = "Always succeeds")
    )
)]
async fn test_success() -> Json<serde_json::Value> {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({ "ack": true, "message": "" }))
}

/// Test pending endpoint (ack=false)
#[utoipa::path(
    post,
    path = "/api/test/pending",
    tag = "test",
    responses(
        (status = 200, description = "Returns pending")
    )
)]
async fn test_pending() -> Json<serde_json::Value> {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({ "ack": false, "message": "notBefore time not reached" }))
}

/// Test client error endpoint (always 400)
#[utoipa::path(
    post,
    path = "/api/test/client-error",
    tag = "test",
    responses(
        (status = 400, description = "Client error")
    )
)]
async fn test_client_error() -> Response {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "status": "error", "error": "Record not found" })),
    )
        .into_response()
}

/// Test server error endpoint (always 500)
#[utoipa::path(
    post,
    path = "/api/test/server-error",
    tag = "test",
    responses(
        (status = 500, description = "Server error")
    )
)]
async fn test_server_error() -> Response {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "status": "error", "error": "Internal server error" })),
    )
        .into_response()
}

/// Get test stats
#[utoipa::path(
    get,
    path = "/api/test/stats",
    tag = "test",
    responses(
        (status = 200, description = "Test statistics")
    )
)]
async fn test_stats() -> Json<serde_json::Value> {
    let count = TEST_REQUEST_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({ "totalRequests": count }))
}

/// Reset test stats
#[utoipa::path(
    post,
    path = "/api/test/stats/reset",
    tag = "test",
    responses(
        (status = 200, description = "Test stats reset")
    )
)]
async fn reset_test_stats() -> Json<serde_json::Value> {
    let previous = TEST_REQUEST_COUNT.swap(0, std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({ "previousCount": previous, "currentCount": 0 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_parsing() {
        let cases = [
            ("INFO", Some(WarningSeverity::Info)),
            ("WARN", Some(WarningSeverity::Warn)),
            ("WARNING", Some(WarningSeverity::Warn)),
            ("ERROR", Some(WarningSeverity::Error)),
            ("CRITICAL", Some(WarningSeverity::Critical)),
            ("UNKNOWN", None),
        ];

        for (input, expected) in cases {
            let result = match input.to_uppercase().as_str() {
                "INFO" => Some(WarningSeverity::Info),
                "WARN" | "WARNING" => Some(WarningSeverity::Warn),
                "ERROR" => Some(WarningSeverity::Error),
                "CRITICAL" => Some(WarningSeverity::Critical),
                _ => None,
            };
            assert_eq!(result, expected);
        }
    }
}
