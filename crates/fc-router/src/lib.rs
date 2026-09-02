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
pub mod circuit_breaker_registry;
pub mod config_sync;
pub mod error;
pub mod health;
pub mod http_pool;
pub mod lifecycle;
pub mod manager;
pub mod mediator;
pub mod metrics;
pub mod notification;
pub mod pool;
pub mod queue_health_monitor;
pub mod router_metrics;
pub mod standby;
pub mod traffic;
pub mod warning;

#[cfg(feature = "oidc-flow")]
pub use api::oidc_flow::{
    oidc_flow_routes, OidcFlowConfig, OidcFlowState, PendingOidcStateStore, SessionStore,
};
pub use circuit_breaker_registry::{
    CircuitBreakerConfig, CircuitBreakerRegistry, CircuitBreakerState, CircuitBreakerStats,
};
pub use config_sync::{
    spawn_config_sync_task, ConfigSyncConfig, ConfigSyncError, ConfigSyncResult, ConfigSyncService,
};
pub use error::RouterError;
pub use health::{HealthService, HealthServiceConfig};
pub use http_pool::{HostConnectionPool, HostKey, HostKeyError, HostPoolRegistry, HostPoolSizing};
pub use lifecycle::{LifecycleConfig, LifecycleManager};
pub use manager::{ConsumerFactory, InFlightMessageInfo, QueueManager};
pub use mediator::{HttpMediator, HttpMediatorConfig, HttpVersion, Mediator, RetryPolicy};
pub use metrics::{MetricsConfig, PoolMetricsCollector};
pub use notification::{
    create_notification_service, create_notification_service_with_scheduler,
    BatchingNotificationService, NoOpNotificationService, NotificationConfig, NotificationService,
    NotificationServiceWithScheduler, TeamsWebhookNotificationService,
};
#[cfg(feature = "email")]
pub use notification::{EmailConfig, EmailNotificationService};
pub use pool::{PoolConfigUpdate, ProcessPool};
pub use queue_health_monitor::{spawn_queue_health_monitor, QueueHealthConfig, QueueHealthMonitor};
pub use standby::{
    spawn_leadership_monitor, LeadershipStatus, StandbyAwareProcessor, StandbyProcessor,
    StandbyRouterConfig,
};
pub use traffic::{spawn_traffic_watcher, NoopTrafficStrategy, TrafficError, TrafficStrategy};
#[cfg(feature = "alb")]
pub use traffic::{AlbTrafficConfig, AwsAlbTrafficStrategy};
pub use warning::{WarningService, WarningServiceConfig};

// Re-export QueueMetrics for API
pub use api::CachedBrokerStats;
pub use fc_queue::QueueMetrics;

pub type Result<T> = std::result::Result<T, RouterError>;

/// Initialize the Prometheus metrics recorder and return a handle for rendering.
///
/// Must be called once early in main() before any metrics are recorded.
/// The handle is passed to the API router so `/metrics` and `/q/metrics`
/// serve real Prometheus-format output.
pub fn init_prometheus_recorder() -> metrics_exporter_prometheus::PrometheusHandle {
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("Failed to install Prometheus metrics recorder")
}
