//! Lifecycle Manager - Background tasks for the message router
//!
//! Handles:
//! - Visibility timeout extension for long-running messages
//! - Memory health monitoring
//! - Consumer health monitoring
//! - Warning service cleanup
//! - Graceful shutdown coordination
//! - Configuration sync (when enabled)
//! - Standby/HA coordination (when enabled)

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn, debug, error};

use fc_common::{WarningCategory, WarningSeverity};
use crate::manager::QueueManager;
use crate::health::HealthService;
use crate::warning::WarningService;
use crate::config_sync::{ConfigSyncService, spawn_config_sync_task};
use crate::standby::{StandbyProcessor, spawn_leadership_monitor};

/// Configuration for the lifecycle manager
#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    /// Interval for visibility extension checks
    pub visibility_extension_interval: Duration,
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
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            visibility_extension_interval: Duration::from_secs(55),
            memory_health_interval: Duration::from_secs(60),
            consumer_health_interval: Duration::from_secs(30),
            warning_cleanup_interval: Duration::from_secs(300),  // 5 minutes
            health_report_interval: Duration::from_secs(60),
            consumer_restart_delay: Duration::from_secs(5),
        }
    }
}

/// Manages lifecycle tasks for the message router
pub struct LifecycleManager {
    shutdown_tx: broadcast::Sender<()>,
    warning_service: Arc<WarningService>,
    health_service: Arc<HealthService>,
    /// Optional config sync service
    config_sync: Option<Arc<ConfigSyncService>>,
    /// Optional standby processor
    standby: Option<Arc<StandbyProcessor>>,
}

impl LifecycleManager {
    /// Create a new lifecycle manager without starting tasks
    pub fn new(
        warning_service: Arc<WarningService>,
        health_service: Arc<HealthService>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            shutdown_tx,
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
        let (shutdown_tx, _) = broadcast::channel(1);

        // Visibility timeout extender
        {
            let manager = manager.clone();
            let _warning_service = warning_service.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            let interval = config.visibility_extension_interval;

            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);

                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            debug!("Running visibility extension check");
                            manager.extend_visibility_for_long_running().await;
                        }
                        _ = shutdown_rx.recv() => {
                            info!("Visibility extender shutting down");
                            break;
                        }
                    }
                }
            });
        }

        // Memory health monitor
        {
            let manager = manager.clone();
            let warning_service = warning_service.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            let interval = config.memory_health_interval;

            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);

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
                        _ = shutdown_rx.recv() => {
                            info!("Memory health monitor shutting down");
                            break;
                        }
                    }
                }
            });
        }

        // Consumer health monitor with auto-restart
        {
            let manager = manager.clone();
            let health_service = health_service.clone();
            let warning_service = warning_service.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            let interval = config.consumer_health_interval;
            let restart_delay = config.consumer_restart_delay;

            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                let mut restart_attempts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
                const MAX_RESTART_ATTEMPTS: u32 = 3;

                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            let stalled = health_service.get_stalled_consumers();
                            for consumer_id in stalled {
                                let attempts = restart_attempts.entry(consumer_id.clone()).or_insert(0);

                                if *attempts < MAX_RESTART_ATTEMPTS {
                                    warn!(
                                        consumer_id = %consumer_id,
                                        attempt = *attempts + 1,
                                        max_attempts = MAX_RESTART_ATTEMPTS,
                                        "Stalled consumer detected, attempting restart"
                                    );

                                    warning_service.add_warning(
                                        WarningCategory::ConsumerHealth,
                                        WarningSeverity::Warn,
                                        format!("Consumer {} is stalled, restart attempt {}", consumer_id, *attempts + 1),
                                        "LifecycleManager".to_string(),
                                    );

                                    // Wait before restart
                                    tokio::time::sleep(restart_delay).await;

                                    // Attempt restart
                                    if manager.restart_consumer(&consumer_id).await {
                                        *attempts += 1;
                                        info!(consumer_id = %consumer_id, "Consumer restart initiated");
                                    }
                                } else {
                                    // Max attempts reached - critical warning
                                    error!(
                                        consumer_id = %consumer_id,
                                        attempts = *attempts,
                                        "Consumer restart attempts exhausted"
                                    );

                                    warning_service.add_warning(
                                        WarningCategory::ConsumerHealth,
                                        WarningSeverity::Critical,
                                        format!("Consumer {} restart failed after {} attempts", consumer_id, *attempts),
                                        "LifecycleManager".to_string(),
                                    );
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
                        _ = shutdown_rx.recv() => {
                            info!("Consumer health monitor shutting down");
                            break;
                        }
                    }
                }
            });
        }

        // Warning service cleanup
        {
            let warning_service = warning_service.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            let interval = config.warning_cleanup_interval;

            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);

                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            debug!("Running warning service cleanup");
                            warning_service.cleanup();
                        }
                        _ = shutdown_rx.recv() => {
                            info!("Warning cleanup task shutting down");
                            break;
                        }
                    }
                }
            });
        }

        // Health report logger
        {
            let manager = manager.clone();
            let health_service = health_service.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            let interval = config.health_report_interval;

            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);

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
                        _ = shutdown_rx.recv() => {
                            info!("Health report logger shutting down");
                            break;
                        }
                    }
                }
            });
        }

        info!("Lifecycle manager started with all background tasks");

        Self {
            shutdown_tx,
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
        standby: Option<Arc<StandbyProcessor>>,
    ) -> Self {
        // Start the base lifecycle manager
        let mut lifecycle = Self::start(manager, warning_service, health_service, config);

        // Start config sync task if provided and enabled
        if let Some(ref sync_service) = config_sync {
            if sync_service.is_enabled() {
                info!("Starting configuration sync background task");
                spawn_config_sync_task(sync_service.clone(), lifecycle.shutdown_tx.clone());
            }
        }

        // Start leadership monitor if standby is enabled
        if let Some(ref standby_proc) = standby {
            if standby_proc.is_standby_enabled() {
                info!("Starting leadership monitor background task");
                spawn_leadership_monitor(standby_proc.clone(), lifecycle.shutdown_tx.clone());
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

    /// Signal shutdown to all lifecycle tasks
    pub async fn shutdown(&self) {
        info!("Lifecycle manager shutting down...");

        // Shutdown standby processor first
        if let Some(ref standby) = self.standby {
            standby.shutdown().await;
        }

        // Signal all tasks to stop
        let _ = self.shutdown_tx.send(());
    }

    /// Get the shutdown sender for spawning additional tasks
    pub fn shutdown_sender(&self) -> broadcast::Sender<()> {
        self.shutdown_tx.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LifecycleConfig::default();
        assert_eq!(config.visibility_extension_interval, Duration::from_secs(55));
        assert_eq!(config.memory_health_interval, Duration::from_secs(60));
    }
}
