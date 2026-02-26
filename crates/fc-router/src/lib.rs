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

pub mod error;
pub mod manager;
pub mod pool;
pub mod mediator;
pub mod lifecycle;
pub mod router_metrics;
pub mod warning;
pub mod health;
pub mod metrics;
pub mod circuit_breaker_registry;
pub mod config_sync;
pub mod standby;
pub mod notification;
pub mod traffic;
pub mod queue_health_monitor;
pub mod api;

pub use error::RouterError;
pub use manager::{QueueManager, InFlightMessageInfo};
pub use pool::{ProcessPool, PoolConfigUpdate};
pub use mediator::{Mediator, HttpMediator, CircuitState, HttpMediatorConfig, HttpVersion};
pub use lifecycle::{LifecycleManager, LifecycleConfig};
pub use warning::{WarningService, WarningServiceConfig};
pub use health::{HealthService, HealthServiceConfig};
pub use metrics::{PoolMetricsCollector, MetricsConfig};
pub use circuit_breaker_registry::{CircuitBreakerRegistry, CircuitBreakerConfig, CircuitBreakerStats, CircuitBreakerState};
pub use config_sync::{ConfigSyncService, ConfigSyncConfig, ConfigSyncResult, spawn_config_sync_task};
pub use standby::{
    StandbyProcessor, StandbyAwareProcessor, StandbyRouterConfig,
    LeadershipStatus, spawn_leadership_monitor,
};
pub use notification::{
    NotificationService, NotificationConfig, TeamsWebhookNotificationService,
    BatchingNotificationService, NoOpNotificationService, create_notification_service,
    create_notification_service_with_scheduler, NotificationServiceWithScheduler,
};
#[cfg(feature = "email")]
pub use notification::{EmailNotificationService, EmailConfig};
pub use traffic::{TrafficStrategy, TrafficError, NoopTrafficStrategy, spawn_traffic_watcher};
#[cfg(feature = "alb")]
pub use traffic::{AwsAlbTrafficStrategy, AlbTrafficConfig};
#[cfg(feature = "oidc-flow")]
pub use api::oidc_flow::{OidcFlowConfig, OidcFlowState, SessionStore, PendingOidcStateStore, oidc_flow_routes};
pub use queue_health_monitor::{
    QueueHealthMonitor, QueueHealthConfig, spawn_queue_health_monitor,
};

// Re-export QueueMetrics for API
pub use fc_queue::QueueMetrics;

pub type Result<T> = std::result::Result<T, RouterError>;
