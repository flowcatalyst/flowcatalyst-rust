//! Lifecycle Manager - Background tasks for the message router
//!
//! Handles:
//! - Memory health monitoring
//! - Consumer health monitoring
//! - Warning service cleanup
//! - Graceful shutdown coordination
//! - Configuration sync (when enabled)
//! - Standby/HA coordination (when enabled)
//!
//! **No visibility-timeout extension.** When SQS visibility expires while a
//! message is still being processed, SQS redelivers and the manager's
//! `filter_duplicates` Phase 1 (Check 1) catches the duplicate, swaps in the
//! new receipt handle, and the original processing continues. When it
//! finishes, ack/nack uses the latest handle. Extending visibility was the
//! source of "Failed to extend visibility … AWS SQS error" log spam — the
//! handle had often expired already by the time the extender fired. Set the
//! queue's SQS visibility timeout (queue-side, AWS console / IaC) to fit
//! your longest realistic mediation if redelivery noise is undesirable.
//!
//! ## Background-task lifecycle (applies to every `tokio::spawn` here)
//!
//! All background tasks in this file follow the same pattern:
//! - **Own:** an interval ticker plus Arc clones of the manager / health
//!   / warning service drawn from the enclosing closure, plus a
//!   [`CancellationToken`] child of `self.shutdown`.
//! - **Exit:** on `token.cancelled()` resolving. `CancellationToken` is
//!   **level-triggered**: `LifecycleManager::shutdown()` calls
//!   `self.shutdown.cancel()`, which immediately marks every child token
//!   (existing or future) cancelled. Unlike a `broadcast` channel, a task
//!   spawned (or a token cloned) *after* `cancel()` still observes the
//!   cancellation instantly — `cancelled()` resolves right away instead of
//!   requiring the caller to have subscribed before the signal fired.
//! - **Joined by:** nobody — these are detached, fire-and-forget tasks.
//!   The cancellation token is the only lifecycle signal.
//!
//! Each `tokio::select!` below selects between two arms: the ticker arm
//! (do the work) and the shutdown arm (log and break). Per-arm intent is
//! obvious from the code; the comment block above each task identifies
//! *what* the task monitors / cleans up.

use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

#[cfg(feature = "oidc-flow")]
use crate::api::oidc_flow::{PendingOidcStateStore, SessionStore};
use crate::circuit_breaker_registry::CircuitBreakerRegistry;
use crate::config_sync::{spawn_config_sync_task, ConfigSyncService};
use crate::health::HealthService;
use crate::manager::QueueManager;
use crate::standby::{spawn_leadership_monitor, StandbyProcessor};
use crate::warning::WarningService;
use fc_common::{WarningCategory, WarningSeverity};

/// Configuration for the lifecycle manager
#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    /// Interval for memory health checks
    pub memory_health_interval: Duration,
    /// Interval for consumer health checks
    pub consumer_health_interval: Duration,
    /// Interval for warning service cleanup
    pub warning_cleanup_interval: Duration,
    /// Interval for health report generation
    pub health_report_interval: Duration,
    /// Consumer restart delay after detecting a stall
    pub consumer_restart_delay: Duration,
    /// Interval for reaping stale in-pipeline entries and idle circuit breakers
    pub reaper_interval: Duration,
    /// Max age for in-pipeline entries before they are reaped
    pub in_pipeline_max_age: Duration,
    /// Max age for pending-delete broker IDs before they are reaped
    pub pending_delete_max_age: Duration,
    /// Max idle time for circuit breakers before eviction
    pub circuit_breaker_max_idle: Duration,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            memory_health_interval: Duration::from_secs(60),
            consumer_health_interval: Duration::from_secs(30),
            warning_cleanup_interval: Duration::from_secs(300), // 5 minutes
            health_report_interval: Duration::from_secs(60),
            consumer_restart_delay: Duration::from_secs(5),
            reaper_interval: Duration::from_secs(300), // 5 minutes
            in_pipeline_max_age: Duration::from_secs(900), // 15 minutes
            pending_delete_max_age: Duration::from_secs(60), // 1 minute — short so deliberate resends are reprocessed
            circuit_breaker_max_idle: Duration::from_secs(3600), // 1 hour
        }
    }
}

/// Manages lifecycle tasks for the message router
pub struct LifecycleManager {
    shutdown: CancellationToken,
    /// Handles for every background task spawned by this manager. `shutdown()`
    /// cancels the token and then bounded-joins these so callers
    /// can observe that background work has actually stopped (previously the
    /// handles were dropped and shutdown only *signalled*, never waited). The
    /// join is time-boxed: a task stuck mid-`.await` is left to be reaped at
    /// process exit rather than blocking shutdown indefinitely.
    tasks: Vec<tokio::task::JoinHandle<()>>,
    warning_service: Arc<WarningService>,
    health_service: Arc<HealthService>,
    /// Optional config sync service
    config_sync: Option<Arc<ConfigSyncService>>,
    /// Optional standby processor
    standby: Option<Arc<StandbyProcessor>>,
    /// Optional circuit breaker registry for idle eviction
    circuit_breaker_registry: Option<Arc<CircuitBreakerRegistry>>,
    /// Optional OIDC session store for periodic cleanup
    #[cfg(feature = "oidc-flow")]
    session_store: Option<Arc<SessionStore>>,
    /// Optional OIDC pending state store for periodic cleanup
    #[cfg(feature = "oidc-flow")]
    pending_oidc_states: Option<Arc<PendingOidcStateStore>>,
}

impl LifecycleManager {
    /// How long `shutdown()` waits for background tasks to finish after
    /// signalling, before leaving any stragglers to process-exit cleanup.
    const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(10);

    /// Create a new lifecycle manager without starting tasks
    pub fn new(warning_service: Arc<WarningService>, health_service: Arc<HealthService>) -> Self {
        Self {
            shutdown: CancellationToken::new(),
            tasks: Vec::new(),
            warning_service,
            health_service,
            config_sync: None,
            standby: None,
            circuit_breaker_registry: None,
            #[cfg(feature = "oidc-flow")]
            session_store: None,
            #[cfg(feature = "oidc-flow")]
            pending_oidc_states: None,
        }
    }

    /// Start all lifecycle tasks
    pub fn start(
        manager: Arc<QueueManager>,
        warning_service: Arc<WarningService>,
        health_service: Arc<HealthService>,
        config: LifecycleConfig,
    ) -> Self {
        let shutdown = CancellationToken::new();
        let mut tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        // Memory health monitor
        {
            let manager = manager.clone();
            let warning_service = warning_service.clone();
            let token = shutdown.child_token();
            let interval = config.memory_health_interval;

            tasks.push(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            if !manager.check_memory_health() {
                                warn!("Memory health check failed - potential leak detected");
                                warning_service.add_warning(
                                    WarningCategory::Resource,
                                    WarningSeverity::Error,
                                    "Potential memory leak detected - in_pipeline map is large".to_string(),
                                    "LifecycleManager".to_string(),
                                );
                            }
                        }
                        _ = token.cancelled() => {
                            info!("Memory health monitor shutting down");
                            break;
                        }
                    }
                }
            }));
        }

        // Consumer health monitor with auto-restart
        {
            let manager = manager.clone();
            let health_service = health_service.clone();
            let warning_service = warning_service.clone();
            let token = shutdown.child_token();
            let interval = config.consumer_health_interval;
            let restart_delay = config.consumer_restart_delay;

            tasks.push(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut restart_attempts: std::collections::HashMap<String, u32> =
                    std::collections::HashMap::new();

                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            let stalled = health_service.get_stalled_consumers();
                            for consumer_id in stalled {
                                let attempts = restart_attempts.entry(consumer_id.clone()).or_insert(0);

                                // Java: retries indefinitely (no max attempts).
                                // Escalate severity after many failed attempts.
                                let severity = if *attempts >= 10 {
                                    WarningSeverity::Critical
                                } else {
                                    WarningSeverity::Warn
                                };

                                warn!(
                                    consumer_id = %consumer_id,
                                    attempt = *attempts + 1,
                                    "Stalled consumer detected, attempting restart"
                                );

                                warning_service.add_warning(
                                    WarningCategory::ConsumerHealth,
                                    severity,
                                    format!("Consumer {} is stalled, restart attempt {}", consumer_id, *attempts + 1),
                                    "LifecycleManager".to_string(),
                                );

                                // Wait before restart — cancel-aware so several stalled
                                // consumers back-to-back can't stack up N × restart_delay
                                // of unresponsiveness to shutdown.
                                tokio::select! {
                                    _ = tokio::time::sleep(restart_delay) => {}
                                    _ = token.cancelled() => {
                                        info!("Consumer health monitor shutting down");
                                        return;
                                    }
                                }

                                // Attempt restart
                                if manager.restart_consumer(&consumer_id).await {
                                    *attempts += 1;
                                    info!(consumer_id = %consumer_id, "Consumer restart initiated");
                                }
                            }

                            // Clear restart attempts for healthy consumers
                            let healthy_consumers: Vec<String> = restart_attempts.keys()
                                .filter(|id| !health_service.get_stalled_consumers().contains(id))
                                .cloned()
                                .collect();
                            for id in healthy_consumers {
                                restart_attempts.remove(&id);
                            }
                        }
                        _ = token.cancelled() => {
                            info!("Consumer health monitor shutting down");
                            break;
                        }
                    }
                }
            }));
        }

        // Warning service cleanup
        {
            let warning_service = warning_service.clone();
            let token = shutdown.child_token();
            let interval = config.warning_cleanup_interval;

            tasks.push(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            debug!("Running warning service cleanup");
                            warning_service.cleanup();
                        }
                        _ = token.cancelled() => {
                            info!("Warning cleanup task shutting down");
                            break;
                        }
                    }
                }
            }));
        }

        // Health report logger
        {
            let manager = manager.clone();
            let health_service = health_service.clone();
            let token = shutdown.child_token();
            let interval = config.health_report_interval;

            tasks.push(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            let pool_stats = manager.get_pool_stats();
                            let report = health_service.get_health_report(&pool_stats);

                            if !report.issues.is_empty() {
                                warn!(
                                    status = ?report.status,
                                    issues = ?report.issues,
                                    "Health report"
                                );
                            } else {
                                debug!(status = ?report.status, "Health report: OK");
                            }
                        }
                        _ = token.cancelled() => {
                            info!("Health report logger shutting down");
                            break;
                        }
                    }
                }
            }));
        }

        // Stale entry reaper (in_pipeline, pending_delete, circuit breakers, health service)
        {
            let manager = manager.clone();
            let health_service = health_service.clone();
            let token = shutdown.child_token();
            let interval = config.reaper_interval;
            let in_pipeline_max_age = config.in_pipeline_max_age;
            let pending_delete_max_age = config.pending_delete_max_age;

            tasks.push(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            debug!("Running stale entry reaper");

                            // Reap stale in_pipeline and pending_delete entries
                            let (reaped_pipeline, reaped_pending) = manager.reap_stale_entries(
                                in_pipeline_max_age,
                                pending_delete_max_age,
                            );

                            // Clean up draining pools that have finished
                            manager.cleanup_draining_pools().await;

                            // Remove stale health service entries for destroyed pools/consumers
                            let pool_codes = manager.pool_codes();
                            let consumer_ids = manager.consumer_ids().await;
                            health_service.remove_stale_entries(&pool_codes, &consumer_ids);

                            if reaped_pipeline > 0 || reaped_pending > 0 {
                                info!(
                                    reaped_pipeline = reaped_pipeline,
                                    reaped_pending = reaped_pending,
                                    "Reaper cycle complete"
                                );
                            }
                        }
                        _ = token.cancelled() => {
                            info!("Stale entry reaper shutting down");
                            break;
                        }
                    }
                }
            }));
        }

        info!("Lifecycle manager started with all background tasks");

        Self {
            shutdown,
            tasks,
            warning_service,
            health_service,
            config_sync: None,
            standby: None,
            circuit_breaker_registry: None,
            #[cfg(feature = "oidc-flow")]
            session_store: None,
            #[cfg(feature = "oidc-flow")]
            pending_oidc_states: None,
        }
    }

    /// Start lifecycle tasks with optional config sync and standby support
    pub fn start_with_features(
        manager: Arc<QueueManager>,
        warning_service: Arc<WarningService>,
        health_service: Arc<HealthService>,
        config: LifecycleConfig,
        config_sync: Option<Arc<ConfigSyncService>>,
        standby: Option<Arc<StandbyProcessor>>,
    ) -> Self {
        // Kept for the leadership monitor below — `Self::start` consumes
        // `manager` (moved into its own background tasks).
        let manager_for_leadership = manager.clone();

        // Start the base lifecycle manager
        let mut lifecycle = Self::start(manager, warning_service, health_service, config);

        // Start config sync task if provided and enabled
        if let Some(ref sync_service) = config_sync {
            if sync_service.is_enabled() {
                info!("Starting configuration sync background task");
                let handle =
                    spawn_config_sync_task(sync_service.clone(), lifecycle.shutdown.child_token());
                lifecycle.tasks.push(handle);
            }
        }

        // Start leadership monitor if standby is enabled. R-26/R-34: this is
        // what actually pauses/resumes consumer polling on leadership
        // loss/regain — `spawn_leadership_monitor` drives
        // `QueueManager::set_leader` every tick.
        if let Some(ref standby_proc) = standby {
            if standby_proc.is_standby_enabled() {
                info!("Starting leadership monitor background task");
                let handle = spawn_leadership_monitor(
                    standby_proc.clone(),
                    manager_for_leadership,
                    lifecycle.shutdown.child_token(),
                );
                lifecycle.tasks.push(handle);
            }
        }

        lifecycle.config_sync = config_sync;
        lifecycle.standby = standby;

        lifecycle
    }

    /// Get warning service reference
    pub fn warning_service(&self) -> &Arc<WarningService> {
        &self.warning_service
    }

    /// Get health service reference
    pub fn health_service(&self) -> &Arc<HealthService> {
        &self.health_service
    }

    /// Get config sync service reference if available
    pub fn config_sync(&self) -> Option<&Arc<ConfigSyncService>> {
        self.config_sync.as_ref()
    }

    /// Get standby processor reference if available
    pub fn standby(&self) -> Option<&Arc<StandbyProcessor>> {
        self.standby.as_ref()
    }

    /// Check if this instance should process messages (respects standby mode)
    pub fn should_process(&self) -> bool {
        match &self.standby {
            Some(standby) => standby.should_process(),
            None => true, // No standby = always process
        }
    }

    /// Check if this instance is the leader
    pub fn is_leader(&self) -> bool {
        match &self.standby {
            Some(standby) => standby.is_leader(),
            None => true, // No standby = always leader
        }
    }

    /// Signal shutdown to all lifecycle tasks, then bounded-join them.
    ///
    /// Cancels the token (every task's `token.cancelled()` resolves immediately,
    /// including tokens cloned after this call — level-triggered, unlike a
    /// broadcast send), then waits up to [`Self::SHUTDOWN_JOIN_TIMEOUT`] for the
    /// spawned tasks to actually finish. A task stuck mid-`.await` past the
    /// timeout is left to be reaped at process exit rather than blocking
    /// shutdown — so this is strictly more graceful than the old fire-and-forget
    /// `send()`, never less.
    pub async fn shutdown(&mut self) {
        info!("Lifecycle manager shutting down...");

        // Shutdown standby processor first
        if let Some(ref standby) = self.standby {
            standby.shutdown().await;
        }

        // Signal all tasks to stop
        self.shutdown.cancel();

        // Bounded-join: wait for the background loops to exit, but don't hang
        // shutdown on a task that's mid-flight past the deadline.
        let handles = std::mem::take(&mut self.tasks);
        if !handles.is_empty() {
            let joined = tokio::time::timeout(
                Self::SHUTDOWN_JOIN_TIMEOUT,
                futures::future::join_all(handles),
            )
            .await;
            match joined {
                Ok(_) => info!("All lifecycle tasks stopped"),
                Err(_) => warn!(
                    timeout_secs = Self::SHUTDOWN_JOIN_TIMEOUT.as_secs(),
                    "Lifecycle tasks did not all stop within timeout — leaving remainder to process exit"
                ),
            }
        }
    }

    /// Get a child cancellation token for spawning additional tasks that
    /// should stop when this lifecycle manager shuts down.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.child_token()
    }

    /// Set the circuit breaker registry for periodic idle eviction.
    /// Starts a background task that evicts idle breakers on the warning cleanup interval.
    pub fn set_circuit_breaker_registry(
        &mut self,
        registry: Arc<CircuitBreakerRegistry>,
        max_idle: Duration,
    ) {
        self.circuit_breaker_registry = Some(registry.clone());

        let token = self.shutdown.child_token();
        // Run at the same cadence as warning cleanup (5 min)
        let interval = Duration::from_secs(300);

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let evicted = registry.evict_idle(max_idle);
                        if evicted > 0 {
                            info!(evicted = evicted, "Evicted idle circuit breakers");
                        }
                    }
                    _ = token.cancelled() => {
                        info!("Circuit breaker eviction task shutting down");
                        break;
                    }
                }
            }
        });
        self.tasks.push(handle);
    }

    /// Set the OIDC stores for periodic expired-entry cleanup.
    /// Starts a background task that cleans up expired sessions and pending states.
    #[cfg(feature = "oidc-flow")]
    pub fn set_oidc_stores(
        &mut self,
        session_store: Arc<SessionStore>,
        pending_states: Arc<PendingOidcStateStore>,
    ) {
        self.session_store = Some(session_store.clone());
        self.pending_oidc_states = Some(pending_states.clone());

        let token = self.shutdown.child_token();
        // Clean up every 60 seconds
        let interval = Duration::from_secs(60);

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        session_store.cleanup();
                        pending_states.cleanup();
                    }
                    _ = token.cancelled() => {
                        info!("OIDC store cleanup task shutting down");
                        break;
                    }
                }
            }
        });
        self.tasks.push(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit_breaker_registry::{CircuitBreakerConfig, CircuitBreakerRegistry};
    use crate::health::{HealthService, HealthServiceConfig};
    use crate::manager::QueueManager;
    use crate::mediator::HttpMediatorConfig;
    use crate::warning::WarningService;

    #[test]
    fn test_default_config() {
        let config = LifecycleConfig::default();
        assert_eq!(config.memory_health_interval, Duration::from_secs(60));
    }

    /// `CancellationToken` is level-triggered: shutdown must complete
    /// promptly and leave `tasks` empty even though every ticker in this
    /// config is set far longer than the test's timeout (so no tick ever
    /// fires) — the only way tasks stop is via `token.cancelled()`.
    #[tokio::test]
    async fn shutdown_cancels_all_tasks_promptly() {
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));
        let warning_service = Arc::new(WarningService::noop());
        let health_service = Arc::new(HealthService::new(
            HealthServiceConfig::default(),
            warning_service.clone(),
        ));

        let long = Duration::from_secs(60);
        let config = LifecycleConfig {
            memory_health_interval: long,
            consumer_health_interval: long,
            warning_cleanup_interval: long,
            health_report_interval: long,
            consumer_restart_delay: long,
            reaper_interval: long,
            in_pipeline_max_age: long,
            pending_delete_max_age: long,
            circuit_breaker_max_idle: long,
        };

        let mut lifecycle =
            LifecycleManager::start(manager, warning_service, health_service, config);

        lifecycle.set_circuit_breaker_registry(
            Arc::new(CircuitBreakerRegistry::new(CircuitBreakerConfig::default())),
            long,
        );

        tokio::time::timeout(Duration::from_secs(2), lifecycle.shutdown())
            .await
            .expect("shutdown should complete promptly via CancellationToken");

        assert!(lifecycle.tasks.is_empty());
    }
}
