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
use crate::standby::{spawn_leadership_monitor, StandbyAwareProcessor};
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
    /// Pause between consecutive consumer rebuilds in one watchdog sweep
    /// (Go: `consumerRestartDelay`, 5s).
    pub consumer_restart_delay: Duration,
    /// How long a consumer's poll loop may go without a heartbeat before the
    /// watchdog rebuilds it (Go: `ConsumerStallThreshold`, 60s).
    pub consumer_stall_threshold: Duration,
    /// Interval for reaping stale in-pipeline entries and idle circuit breakers
    pub reaper_interval: Duration,
    /// Max age for in-pipeline entries before they are reaped
    pub in_pipeline_max_age: Duration,
    /// Max age for pending-delete broker IDs before they are reaped
    pub pending_delete_max_age: Duration,
    /// Max idle time for circuit breakers before eviction
    pub circuit_breaker_max_idle: Duration,
    /// R-59: idle TTL for synthesised per-client fallback pools
    /// (`{identifier}-DEFAULT-POOL`, see `QueueManager::ensure_fallback_pool`),
    /// swept on the same reaper tick as `in_pipeline_max_age` /
    /// `circuit_breaker_max_idle`. `Duration::ZERO` disables the sweep — see
    /// `QueueManager::evict_idle_synth_pools` and the
    /// `FC_ROUTER_SYNTH_POOL_IDLE_SECS` wiring in `bin/fc-router/src/main.rs`.
    /// Mirrors Go's `ServerConfig.SynthPoolIdleAge` default (1 hour).
    pub synth_pool_idle_ttl: Duration,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            memory_health_interval: Duration::from_secs(60),
            consumer_health_interval: Duration::from_secs(30),
            warning_cleanup_interval: Duration::from_secs(300), // 5 minutes
            health_report_interval: Duration::from_secs(60),
            consumer_restart_delay: Duration::from_secs(5),
            consumer_stall_threshold: Duration::from_secs(60),
            reaper_interval: Duration::from_secs(300), // 5 minutes
            in_pipeline_max_age: Duration::from_secs(900), // 15 minutes
            pending_delete_max_age: Duration::from_secs(60), // 1 minute — short so deliberate resends are reprocessed
            circuit_breaker_max_idle: Duration::from_secs(3600), // 1 hour
            synth_pool_idle_ttl: Duration::from_secs(3600),  // 1 hour, matches Go's default
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
    standby: Option<Arc<StandbyAwareProcessor>>,
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

        // Consumer health monitor with auto-restart (Go:
        // `consumerHealthLoop` → `Manager.RestartStalledConsumers`). The
        // watchdog judges consumers by the manager's own heartbeat, builds a
        // replacement before retiring a stalled consumer, leaves consumers
        // paused for capacity or leadership alone, and escalates to CRITICAL
        // after 10 failed attempts.
        {
            let manager = manager.clone();
            let health_service = health_service.clone();
            let token = shutdown.child_token();
            let interval = config.consumer_health_interval;
            let restart_delay = config.consumer_restart_delay;
            let threshold = config.consumer_stall_threshold;

            tasks.push(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                // The first tick fires immediately; nothing can be stalled yet.
                ticker.tick().await;

                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            health_service.cleanup();
                            let n = manager
                                .restart_stalled_consumers(threshold, restart_delay, &token)
                                .await;
                            if n > 0 {
                                warn!(count = n, "Restarted stalled consumers");
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
            let synth_pool_idle_ttl = config.synth_pool_idle_ttl;

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

                            // X-11 / R-26/R-49: retire detached consumers
                            // nothing in the pipeline references any more.
                            let retired = manager.retire_detached_consumers();
                            if retired > 0 {
                                info!(retired, "Retired detached consumers");
                            }

                            // R-59: evict synthesised per-client fallback pools
                            // idle past their TTL (drains via the same path as
                            // a config-removed pool; see
                            // QueueManager::evict_idle_synth_pools).
                            let evicted_synth_pools =
                                manager.evict_idle_synth_pools(synth_pool_idle_ttl).await;
                            if evicted_synth_pools > 0 {
                                info!(
                                    evicted = evicted_synth_pools,
                                    "Evicted idle synthesised fallback pools"
                                );
                            }

                            // Drop pool counters for pools that no longer
                            // exist. Consumer liveness is read live from the
                            // manager, so it needs no pruning (Go:
                            // RemoveStaleEntries) — this used to prune it by
                            // config name while it was keyed by identifier,
                            // blinding the watchdog to NATS consumers.
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
        }
    }

    /// Start lifecycle tasks with optional config sync and standby support
    pub fn start_with_features(
        manager: Arc<QueueManager>,
        warning_service: Arc<WarningService>,
        health_service: Arc<HealthService>,
        config: LifecycleConfig,
        config_sync: Option<Arc<ConfigSyncService>>,
        standby: Option<Arc<StandbyAwareProcessor>>,
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
            info!("Starting leadership monitor background task");
            let handle = spawn_leadership_monitor(
                standby_proc.clone(),
                manager_for_leadership,
                lifecycle.shutdown.child_token(),
            );
            lifecycle.tasks.push(handle);
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
    pub fn standby(&self) -> Option<&Arc<StandbyAwareProcessor>> {
        self.standby.as_ref()
    }

    /// Check if this instance should process messages (respects standby
    /// mode; without standby it always processes)
    pub fn should_process(&self) -> bool {
        self.standby.as_ref().is_none_or(|s| s.should_process())
    }

    /// Check if this instance is the leader (always, without standby)
    pub fn is_leader(&self) -> bool {
        self.standby.as_ref().is_none_or(|s| s.is_leader())
    }

    /// Signal shutdown to all lifecycle tasks, bounded-join them, then
    /// release leadership. Call this AFTER the queue manager's shutdown
    /// (which stops polling and drains), as Go's `Server.Run` does.
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

        // Leadership is released last (Go: election.Stop after the manager's
        // shutdown): releasing it while this instance was still draining let
        // a standby start polling the same queues underneath it.
        if let Some(ref standby) = self.standby {
            standby.shutdown().await;
        }
    }

    /// Get a child cancellation token for spawning additional tasks that
    /// should stop when this lifecycle manager shuts down.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.child_token()
    }

    /// Spawn a background task that evicts breakers idle for longer than
    /// `max_idle` from `registry`, on the warning cleanup cadence. The task
    /// is joined by [`Self::shutdown`].
    pub fn spawn_circuit_breaker_eviction(
        &mut self,
        registry: Arc<CircuitBreakerRegistry>,
        max_idle: Duration,
    ) {
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

    /// Spawn a background task that purges expired OIDC sessions and
    /// pending states every 60s. The task is joined by [`Self::shutdown`].
    #[cfg(feature = "oidc-flow")]
    pub fn spawn_oidc_store_cleanup(
        &mut self,
        session_store: Arc<SessionStore>,
        pending_states: Arc<PendingOidcStateStore>,
    ) {
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
            consumer_stall_threshold: long,
            reaper_interval: long,
            in_pipeline_max_age: long,
            pending_delete_max_age: long,
            circuit_breaker_max_idle: long,
            synth_pool_idle_ttl: long,
        };

        let mut lifecycle =
            LifecycleManager::start(manager, warning_service, health_service, config);

        lifecycle.spawn_circuit_breaker_eviction(
            Arc::new(CircuitBreakerRegistry::new(CircuitBreakerConfig::default())),
            long,
        );

        tokio::time::timeout(Duration::from_secs(2), lifecycle.shutdown())
            .await
            .expect("shutdown should complete promptly via CancellationToken");

        assert!(lifecycle.tasks.is_empty());
    }
}
