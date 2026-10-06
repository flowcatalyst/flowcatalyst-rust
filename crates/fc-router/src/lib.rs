//! FlowCatalyst Message Router
//!
//! This crate provides the core message routing functionality with:
//! - QueueManager: Central orchestrator for message routing
//! - ProcessPool: Worker pools with concurrency control, rate limiting, and FIFO ordering
//! - HttpMediator: HTTP-based message delivery with circuit breaker and retry
//! - WarningService: In-memory warning storage with categories and severity
//! - HealthService: System health monitoring with rolling windows
//! - Lifecycle: Background tasks for visibility extension, health checks, etc.
//! - PoolMetricsCollector: Enhanced metrics with sliding windows and percentiles
//! - CircuitBreakerRegistry: Per-endpoint circuit breaker tracking for monitoring
//! - ConfigSync: Dynamic configuration sync from central service
//! - Standby: Active/standby high availability with Redis leader election
//! - API: HTTP API endpoints for monitoring, health, and message publishing

pub mod api;
pub mod bootstrap;
pub mod circuit_breaker_registry;
pub mod config_sync;
pub mod error;
pub mod event_counters;
pub mod flight_recorder;
pub mod group_flush;
pub mod health;
pub mod http_pool;
pub mod lifecycle;
pub mod manager;
pub mod mediator;
pub mod metrics;
pub mod notification;
pub mod platform_token;
pub mod pool;
pub mod queue_health_monitor;
pub mod router_metrics;
pub mod settled;
pub mod standby;
pub mod traffic;
pub mod warning;

#[cfg(feature = "oidc-flow")]
pub use api::oidc_flow::{
    oidc_flow_routes, OidcFlowConfig, OidcFlowState, PendingOidcStateStore, SessionStore,
};
pub use circuit_breaker_registry::{
    breaker_key, CircuitBreakerConfig, CircuitBreakerRegistry, CircuitBreakerState,
    CircuitBreakerStats,
};
pub use config_sync::{ConfigSyncConfig, ConfigSyncError, ConfigSyncService};
pub use error::RouterError;
pub use group_flush::{
    GroupFlushRegistry, GroupFlushStats, GroupSuppression, DEFAULT_FLUSH_TTL, MAX_FLUSH_TTL,
};
pub use health::{HealthService, HealthServiceConfig};
pub use http_pool::{HostConnectionPool, HostKey, HostKeyError, HostPoolRegistry, HostPoolSizing};
pub use lifecycle::{LifecycleConfig, LifecycleManager};
pub use manager::{
    stall_config_for_mediation_timeout, ConsumerFactory, InFlightMessageInfo, QueueManager,
};
pub use mediator::{HttpMediator, HttpMediatorConfig, HttpVersion, Mediator, RetryPolicy};
pub use metrics::{MetricsConfig, PoolMetricsCollector};
pub use notification::{
    create_notification_service, create_notification_service_with_scheduler,
    BatchingNotificationService, NotificationConfig, NotificationService,
    NotificationServiceWithScheduler, TeamsWebhookNotificationService,
};
#[cfg(feature = "email")]
pub use notification::{EmailConfig, EmailNotificationService};
pub use platform_token::{origin_of, PlatformTokenSource, TokenError};
pub use pool::{
    deferred_delay, disposition_of, retry_delay, BrokerAction, Disposition, DispositionMetric,
    GroupEffect, GroupInfo, MediatingEntry, PoolConfigUpdate, ProcessPool,
    MAX_IN_PIPELINE_ATTEMPTS,
};
pub use queue_health_monitor::{spawn_queue_health_monitor, QueueHealthConfig, QueueHealthMonitor};
pub use settled::{HttpSettledReporter, SettledJob, SettledReport, SettledReporter, SETTLED_PATH};
pub use standby::{
    spawn_leadership_monitor, LeadershipStatus, StandbyAwareProcessor, StandbyRouterConfig,
};
pub use traffic::{spawn_traffic_watcher, TrafficError, TrafficStrategy};
#[cfg(feature = "alb")]
pub use traffic::{AlbTrafficConfig, AwsAlbTrafficStrategy};
pub use warning::{WarningService, WarningServiceConfig};

// Re-export QueueMetrics for API
pub use api::CachedBrokerStats;
pub use fc_queue::QueueMetrics;
use std::result;
use std::sync::OnceLock;

pub type Result<T> = result::Result<T, RouterError>;

/// Install the process's Prometheus metrics recorder (once) and return a
/// handle for rendering it.
///
/// Call early in main() before any metrics are recorded. Later calls return
/// the same handle, so every listener that serves `/metrics` (the router
/// API, fc-server's metrics port) renders one registry: the router's, the
/// scheduler's and the stream processor's series alike.
#[expect(
    clippy::expect_used,
    reason = "start-up: the recorder is installed once; failing means another global recorder exists"
)]
pub fn init_prometheus_recorder() -> metrics_exporter_prometheus::PrometheusHandle {
    static HANDLE: OnceLock<metrics_exporter_prometheus::PrometheusHandle> = OnceLock::new();
    HANDLE
        .get_or_init(|| {
            metrics_exporter_prometheus::PrometheusBuilder::new()
                .install_recorder()
                .expect("Failed to install Prometheus metrics recorder")
        })
        .clone()
}
