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
//!
//! Endpoint handlers are split by area — mirrors Go's `internal/router/api`
//! package layout (and the Javalin `RouterApi` split): [`health`] (probes,
//! `/metrics`, consumer/stream health), [`monitoring`] (dashboard read-side:
//! pool/queue/breaker stats, in-flight views, the mediating view, in-flight
//! detail), [`group_monitoring`] (blocked groups + group-flush
//! suppressions, ledger R-04/R-52/R-53 — Go's `handlers_group_flush.go`),
//! [`warnings`], [`mutations`] (pool config, broker-stats refresh, breaker
//! reset, in-flight force-ACK), [`config`] (reload, local config,
//! standby/traffic status), [`dashboard`] (embedded HTML), [`messages`]
//! (publish/seed), [`test_endpoints`] (`/api/test/*` mocks).
//! This file stays the assembly point: shared [`AppState`], the cached
//! broker-stats helper, the router builders, and the OpenAPI doc.

use crate::{CircuitBreakerRegistry, HealthService, QueueManager, WarningService};
use axum::{
    routing::{delete, get, post, put},
    Router,
};
use fc_queue::{QueueMetrics as FcQueueMetrics, QueuePublisher};
use fc_stream::StreamHealthService;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub mod auth;
pub(crate) mod config;
pub(crate) mod dashboard;
pub(crate) mod group_monitoring;
pub(crate) mod health;
pub(crate) mod messages;
pub mod model;
pub(crate) mod monitoring;
pub(crate) mod mutations;
#[cfg(feature = "oidc-flow")]
pub mod oidc_flow;
pub(crate) mod test_endpoints;
pub(crate) mod warnings;

pub use auth::{
    auth_middleware, create_auth_state, is_public_path, AuthConfig, AuthMode, AuthState,
    OidcValidator, TokenClaims,
};

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

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    info(
        title = "FlowCatalyst Message Router API",
        version = "0.1.0",
        description = "HTTP API for message routing, health monitoring, and pool management"
    ),
    paths(
        health::health_handler,
        health::liveness_probe,
        health::readiness_probe,
        health::metrics_handler,
        monitoring::monitoring_handler,
        monitoring::pool_stats_handler,
        monitoring::queue_metrics_handler,
        mutations::update_pool_config,
        config::reload_config,
        warnings::list_warnings,
        warnings::acknowledge_warning,
        warnings::acknowledge_all_warnings,
        warnings::get_critical_warnings,
        warnings::get_unacknowledged_warnings,
        warnings::get_warnings_by_severity,
        warnings::clear_all_warnings,
        warnings::clear_old_warnings,
        monitoring::dashboard_health_handler,
        monitoring::dashboard_queue_stats_handler,
        monitoring::dashboard_pool_stats_handler,
        warnings::dashboard_warnings_handler,
        monitoring::dashboard_circuit_breakers_handler,
        monitoring::dashboard_in_flight_messages_handler,
        monitoring::in_flight_message_check_handler,
        monitoring::in_flight_message_check_batch_handler,
        monitoring::dashboard_mediating_handler,
        monitoring::in_flight_message_detail_handler,
        warnings::monitoring_acknowledge_warning,
        monitoring::get_circuit_breaker_state,
        mutations::reset_circuit_breaker,
        mutations::reset_all_circuit_breakers,
        mutations::in_flight_force_ack,
        group_monitoring::blocked_groups_handler,
        group_monitoring::group_flushes_handler,
        group_monitoring::clear_group_flush_handler,
        config::get_standby_status,
        config::get_traffic_status,
        messages::seed_messages,
        config::get_local_config,
        test_endpoints::test_fast,
        test_endpoints::test_slow,
        test_endpoints::test_faulty,
        test_endpoints::test_fail,
        test_endpoints::test_success,
        test_endpoints::test_pending,
        test_endpoints::test_client_error,
        test_endpoints::test_server_error,
        test_endpoints::test_stats,
        test_endpoints::reset_test_stats,
        messages::publish_message,
    ),
    components(schemas(
        health::SimpleHealthResponse,
        health::ProbeResponse,
        monitoring::MonitoringResponse,
        warnings::WarningsQuery,
        mutations::PoolConfigUpdateRequest,
        config::ConfigReloadRequest,
        config::PoolConfigRequest,
        config::ConfigReloadResponse,
        monitoring::QueueMetricsResponse,
        model::PublishMessageRequest,
        model::PublishMessageResponse,
        model::PoolStatusResponse,
        monitoring::DashboardHealthResponse,
        monitoring::DashboardHealthDetails,
        monitoring::DashboardQueueStats,
        monitoring::DashboardPoolStats,
        warnings::DashboardWarning,
        monitoring::DashboardCircuitBreakerStats,
        monitoring::InFlightMessagesQuery,
        config::StandbyStatusResponse,
        config::TrafficStatusResponse,
        messages::SeedMessageRequest,
        messages::SeedMessageResponse,
        warnings::ClearWarningsQuery,
        monitoring::CircuitBreakerStateResponse,
        monitoring::MediatingInfo,
        monitoring::InFlightMessageDetail,
        mutations::ForceAckResponse,
        group_monitoring::BlockedGroupInfo,
        group_monitoring::GroupFlushPoolInfo,
        group_monitoring::GroupFlushEntry,
        group_monitoring::ClearGroupFlushResponse,
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
///
/// `router_http_prefix`, when `Some`, additionally nests the *entire* route
/// tree — public and protected alike — under that path prefix, so it answers
/// at both root (today's default behaviour) and `<prefix>/...`. This mirrors
/// Go's `internal/server.MountRouterHTTP`, which mounts the router HTTP
/// surface under `FC_ROUTER_HTTP_PREFIX` (default `/router`) inside the
/// unified `fc-server` binary; a Go-dialect ECS task definition health-checks
/// and operates against `<prefix>/...` URLs, while this crate's own
/// `bin/fc-router` deployments and tests keep hitting root paths unchanged.
/// Auth is unaffected by nesting: the public/protected split above already
/// happened before nesting runs, so the nested public routes (health,
/// metrics, swagger, …) stay auth-free under the prefix too — nesting only
/// adds path-prefix matching, it never re-applies (or removes) a layer.
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
    router_http_prefix: Option<String>,
) -> Router {
    let cached_broker_stats = Arc::new(CachedBrokerStats::new(queue_manager.clone()));

    // Background refresh of cached broker stats. The task holds only a `Weak`
    // reference, so it exits on its own when the router (and the `AppState` that
    // owns the `Arc<CachedBrokerStats>`) is dropped — the same self-terminating
    // pattern as the mediator host-pool sweep task (`mediator/inner.rs`). This
    // avoids the shutdown-channel plumbing whose omission previously leaked this
    // task forever.
    {
        let weak: Weak<CachedBrokerStats> = Arc::downgrade(&cached_broker_stats);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                // First tick fires immediately, giving the initial fetch.
                ticker.tick().await;
                let Some(cached) = weak.upgrade() else { break };
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
        .route("/health", get(health::health_handler))
        .route("/q/health", get(health::health_handler))
        // Kubernetes probes
        .route("/health/live", get(health::liveness_probe))
        .route("/health/ready", get(health::readiness_probe))
        .route("/health/startup", get(health::readiness_probe))
        .route("/q/health/live", get(health::liveness_probe))
        .route("/q/health/ready", get(health::readiness_probe))
        // Prometheus metrics
        .route("/metrics", get(health::metrics_handler))
        .route("/q/metrics", get(health::metrics_handler))
        .with_state(state.clone());

    // Protected routes — auth middleware applied when configured
    let protected_routes = Router::new()
        // Detailed monitoring
        .route("/monitoring", get(monitoring::monitoring_handler))
        .route("/monitoring/health", get(monitoring::dashboard_health_handler))
        .route("/monitoring/pools", get(monitoring::pool_stats_handler))
        .route("/monitoring/pools/{poolCode}", put(mutations::update_pool_config))
        .route("/monitoring/queues", get(monitoring::queue_metrics_handler))
        .route(
            "/monitoring/broker-stats/refresh",
            post(mutations::broker_stats_refresh_handler),
        )
        // Dashboard-compatible endpoints
        .route(
            "/monitoring/queue-stats",
            get(monitoring::dashboard_queue_stats_handler),
        )
        .route(
            "/monitoring/pool-stats",
            get(monitoring::dashboard_pool_stats_handler),
        )
        .route("/monitoring/warnings", get(warnings::dashboard_warnings_handler))
        .route(
            "/monitoring/warnings/{id}/acknowledge",
            post(warnings::monitoring_acknowledge_warning),
        )
        .route(
            "/monitoring/warnings/unacknowledged",
            get(warnings::get_unacknowledged_warnings),
        )
        .route(
            "/monitoring/warnings/severity/{severity}",
            get(warnings::get_warnings_by_severity),
        )
        .route(
            "/monitoring/circuit-breakers",
            get(monitoring::dashboard_circuit_breakers_handler),
        )
        .route(
            "/monitoring/circuit-breakers/{name}/state",
            get(monitoring::get_circuit_breaker_state),
        )
        .route(
            "/monitoring/circuit-breakers/{name}/reset",
            post(mutations::reset_circuit_breaker),
        )
        .route(
            "/monitoring/circuit-breakers/reset-all",
            post(mutations::reset_all_circuit_breakers),
        )
        .route(
            "/monitoring/in-flight-messages",
            get(monitoring::dashboard_in_flight_messages_handler),
        )
        .route(
            "/monitoring/in-flight-messages/check",
            get(monitoring::in_flight_message_check_handler),
        )
        .route(
            "/monitoring/in-flight-messages/check-batch",
            post(monitoring::in_flight_message_check_batch_handler),
        )
        .route(
            "/monitoring/in-flight-messages/detail",
            get(monitoring::in_flight_message_detail_handler),
        )
        .route(
            "/monitoring/in-flight-messages/{messageId}/ack",
            post(mutations::in_flight_force_ack),
        )
        .route("/monitoring/mediating", get(monitoring::dashboard_mediating_handler))
        .route(
            "/monitoring/blocked-groups",
            get(group_monitoring::blocked_groups_handler),
        )
        .route(
            "/monitoring/group-flushes",
            get(group_monitoring::group_flushes_handler),
        )
        .route(
            "/monitoring/group-flushes/{pool}/{group}/clear",
            post(group_monitoring::clear_group_flush_handler),
        )
        .route("/monitoring/dashboard", get(dashboard::dashboard_html_handler))
        .route("/monitoring/consumer-health", get(health::consumer_health_handler))
        .route("/monitoring/standby-status", get(config::get_standby_status))
        .route("/monitoring/traffic-status", get(config::get_traffic_status))
        // Java-compatible dashboard path alias
        .route("/dashboard.html", get(dashboard::dashboard_html_handler))
        // Stream processor health endpoints
        .route("/monitoring/stream-health", get(health::stream_health_handler))
        .route(
            "/monitoring/stream-health/live",
            get(health::stream_liveness_handler),
        )
        .route(
            "/monitoring/stream-health/ready",
            get(health::stream_readiness_handler),
        )
        // Configuration management
        .route("/config/reload", post(config::reload_config))
        .route("/api/config", get(config::get_local_config))
        // Warnings management
        .route(
            "/warnings",
            get(warnings::list_warnings).delete(warnings::clear_all_warnings),
        )
        .route("/warnings/{id}/acknowledge", post(warnings::acknowledge_warning))
        .route(
            "/warnings/acknowledge-all",
            post(warnings::acknowledge_all_warnings),
        )
        .route("/warnings/critical", get(warnings::get_critical_warnings))
        .route(
            "/warnings/unacknowledged",
            get(warnings::get_unacknowledged_warnings),
        )
        .route("/warnings/old", delete(warnings::clear_old_warnings))
        // Message seeding (test)
        .route("/api/seed/messages", post(messages::seed_messages))
        // Test response endpoints (development)
        .route("/api/test/fast", post(test_endpoints::test_fast))
        .route("/api/test/slow", post(test_endpoints::test_slow))
        .route("/api/test/faulty", post(test_endpoints::test_faulty))
        .route("/api/test/fail", post(test_endpoints::test_fail))
        .route("/api/test/success", post(test_endpoints::test_success))
        .route("/api/test/pending", post(test_endpoints::test_pending))
        .route("/api/test/client-error", post(test_endpoints::test_client_error))
        .route("/api/test/server-error", post(test_endpoints::test_server_error))
        .route(
            "/api/test/stats",
            get(test_endpoints::test_stats).post(test_endpoints::reset_test_stats),
        )
        .route("/api/test/stats/reset", post(test_endpoints::reset_test_stats))
        // Java-compatible benchmark endpoints (aliases for test endpoints)
        .route("/api/benchmark/process", post(test_endpoints::test_fast))
        .route("/api/benchmark/process-slow", post(test_endpoints::test_slow))
        .route("/api/benchmark/stats", get(test_endpoints::test_stats))
        .route("/api/benchmark/reset", post(test_endpoints::reset_test_stats))
        // Message publishing
        .route("/messages", post(messages::publish_message))
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

    // FC_ROUTER_HTTP_PREFIX (R-XX / drop-in compat with Go's MountRouterHTTP):
    // when set, serve the whole route tree BOTH at root and nested under the
    // prefix. `router` at this point already has auth applied only to the
    // protected half (see above), so `nest`-ing it changes nothing about
    // which paths require credentials — the nested public routes stay public.
    if let Some(prefix) = normalize_router_http_prefix(router_http_prefix.as_deref()) {
        info!(prefix = %prefix, "Router HTTP surface additionally nested under prefix");
        router = Router::new().merge(router.clone()).nest(&prefix, router);
    }

    router
}

/// Normalizes `FC_ROUTER_HTTP_PREFIX` into a `Router::nest`-able path:
/// `None`/empty/whitespace-only/`"/"` all mean "no nesting" (today's
/// root-only behaviour, and axum's `nest` rejects an empty or root path
/// outright); otherwise the value is coerced to a leading `/` and no
/// trailing `/`.
fn normalize_router_http_prefix(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return None;
    }
    let mut prefix = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    while prefix.len() > 1 && prefix.ends_with('/') {
        prefix.pop();
    }
    if prefix == "/" {
        None
    } else {
        Some(prefix)
    }
}

#[cfg(test)]
mod prefix_tests {
    use super::normalize_router_http_prefix;

    #[test]
    fn none_and_blank_and_root_disable_nesting() {
        assert_eq!(normalize_router_http_prefix(None), None);
        assert_eq!(normalize_router_http_prefix(Some("")), None);
        assert_eq!(normalize_router_http_prefix(Some("   ")), None);
        assert_eq!(normalize_router_http_prefix(Some("/")), None);
    }

    #[test]
    fn adds_leading_slash_and_strips_trailing_slash() {
        assert_eq!(
            normalize_router_http_prefix(Some("router")),
            Some("/router".to_string())
        );
        assert_eq!(
            normalize_router_http_prefix(Some("/router/")),
            Some("/router".to_string())
        );
        assert_eq!(
            normalize_router_http_prefix(Some("/router")),
            Some("/router".to_string())
        );
    }
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
        .route("/health", get(health::simple_health_handler))
        .route("/messages", post(messages::simple_publish_message))
        .with_state(state)
}
