//! Queue Health Monitor - Monitors queue health and generates warnings
//!
//! Mirrors the Java QueueHealthMonitor with:
//! - QUEUE_BACKLOG: Queue depth exceeds threshold
//! - QUEUE_GROWING: Queue growing for 3+ consecutive check periods

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::manager::QueueManager;
use crate::warning::WarningService;
use fc_common::{WarningCategory, WarningSeverity};
use fc_queue::QueueMetrics;

/// Configuration for queue health monitoring
#[derive(Debug, Clone)]
pub struct QueueHealthConfig {
    /// Enable queue health monitoring
    pub enabled: bool,
    /// Monitoring interval
    pub check_interval: Duration,
    /// Queue depth threshold for backlog warnings
    pub backlog_threshold: u64,
    /// Growth threshold for consecutive growth warnings (messages per period)
    pub growth_threshold: u64,
    /// Number of consecutive growth periods to trigger warning
    pub growth_periods_threshold: u32,
}

impl Default for QueueHealthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval: Duration::from_secs(30),
            backlog_threshold: 1000,
            growth_threshold: 100,
            growth_periods_threshold: 3,
        }
    }
}

/// Queue size history for growth detection
#[derive(Default)]
struct QueueSizeHistory {
    last_size: Option<u64>,
    consecutive_growth_periods: u32,
}

/// Queue Health Monitor
pub struct QueueHealthMonitor {
    config: QueueHealthConfig,
    warning_service: Arc<WarningService>,
    queue_history: parking_lot::Mutex<HashMap<String, QueueSizeHistory>>,
}

impl QueueHealthMonitor {
    pub fn new(config: QueueHealthConfig, warning_service: Arc<WarningService>) -> Self {
        Self {
            config,
            warning_service,
            queue_history: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// Check queue health and generate warnings
    pub fn check_queue_health(&self, metrics: &[QueueMetrics]) {
        if !self.config.enabled {
            return;
        }

        for m in metrics {
            self.check_queue_backlog(&m.queue_identifier, m.pending_messages);
            self.check_queue_growth(&m.queue_identifier, m.pending_messages);
        }
    }

    /// Check if queue depth exceeds backlog threshold
    fn check_queue_backlog(&self, queue_name: &str, current_size: u64) {
        if current_size > self.config.backlog_threshold {
            warn!(
                queue_name = %queue_name,
                current_size = current_size,
                threshold = self.config.backlog_threshold,
                "Queue backlog detected"
            );

            self.warning_service.add_warning(
                WarningCategory::QueueHealth,
                WarningSeverity::Warn,
                format!(
                    "Queue {} depth is {} (threshold: {})",
                    queue_name, current_size, self.config.backlog_threshold
                ),
                "QueueHealthMonitor".to_string(),
            );
        }
    }

    /// Check if queue is growing for 3+ consecutive periods
    fn check_queue_growth(&self, queue_name: &str, current_size: u64) {
        let mut history = self.queue_history.lock();
        let entry = history.entry(queue_name.to_string()).or_default();

        if let Some(previous_size) = entry.last_size {
            if current_size > previous_size {
                let growth = current_size - previous_size;
                if growth >= self.config.growth_threshold {
                    entry.consecutive_growth_periods += 1;

                    if entry.consecutive_growth_periods >= self.config.growth_periods_threshold {
                        warn!(
                            queue_name = %queue_name,
                            periods = entry.consecutive_growth_periods,
                            current_size = current_size,
                            growth = growth,
                            "Queue growth detected"
                        );

                        self.warning_service.add_warning(
                            WarningCategory::QueueHealth,
                            WarningSeverity::Warn,
                            format!("Queue {} growing for {} periods (current depth: {}, growth rate: +{}/{}s)",
                                queue_name, entry.consecutive_growth_periods, current_size, growth,
                                self.config.check_interval.as_secs()),
                            "QueueHealthMonitor".to_string(),
                        );

                        // Cap at 10 to avoid warning spam
                        if entry.consecutive_growth_periods > 10 {
                            entry.consecutive_growth_periods = 10;
                        }
                    }
                } else {
                    // Reset if growth below threshold
                    if entry.consecutive_growth_periods > 0 {
                        debug!(
                            queue_name = %queue_name,
                            periods = entry.consecutive_growth_periods,
                            "Queue stopped growing"
                        );
                    }
                    entry.consecutive_growth_periods = 0;
                }
            } else {
                // Queue not growing (same or smaller)
                if entry.consecutive_growth_periods > 0 {
                    debug!(
                        queue_name = %queue_name,
                        periods = entry.consecutive_growth_periods,
                        "Queue stopped growing"
                    );
                }
                entry.consecutive_growth_periods = 0;
            }
        }

        entry.last_size = Some(current_size);
    }

    /// Get config
    pub fn config(&self) -> &QueueHealthConfig {
        &self.config
    }
}

/// Spawn the queue-health monitoring background task.
///
/// **Owns:** the supplied `Arc<QueueHealthMonitor>` and `Arc<QueueManager>`,
/// plus a [`CancellationToken`] (typically a child of the lifecycle
/// manager's shutdown token).
/// **Exits:** when `shutdown.cancelled()` resolves — level-triggered, so an
/// already-cancelled token still causes an immediate exit for a task
/// spawned after the cancellation.
/// **Joined by:** the caller via the returned `JoinHandle` (lifecycle
/// manager awaits it on graceful shutdown).
pub fn spawn_queue_health_monitor(
    monitor: Arc<QueueHealthMonitor>,
    manager: Arc<QueueManager>,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let interval = monitor.config.check_interval;

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    debug!("Running queue health check");
                    let metrics = manager.get_queue_metrics().await;
                    monitor.check_queue_health(&metrics);
                }
                _ = shutdown.cancelled() => {
                    info!("Queue health monitor shutting down");
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::QueueManager;
    use crate::mediator::HttpMediatorConfig;
    use crate::warning::WarningServiceConfig;

    #[test]
    fn test_default_config() {
        let config = QueueHealthConfig::default();
        assert!(config.enabled);
        assert_eq!(config.backlog_threshold, 1000);
        assert_eq!(config.growth_threshold, 100);
        assert_eq!(config.growth_periods_threshold, 3);
    }

    #[test]
    fn test_backlog_detection() {
        let warning_service = Arc::new(WarningService::new(WarningServiceConfig::default()));
        let monitor = QueueHealthMonitor::new(
            QueueHealthConfig {
                backlog_threshold: 100,
                ..Default::default()
            },
            warning_service.clone(),
        );

        // Should not trigger warning
        monitor.check_queue_backlog("test-queue", 50);
        assert_eq!(warning_service.warning_count(), 0);

        // Should trigger warning
        monitor.check_queue_backlog("test-queue", 150);
        assert_eq!(warning_service.warning_count(), 1);
    }

    #[test]
    fn test_growth_detection() {
        let warning_service = Arc::new(WarningService::new(WarningServiceConfig::default()));
        let monitor = QueueHealthMonitor::new(
            QueueHealthConfig {
                growth_threshold: 50,
                growth_periods_threshold: 3,
                ..Default::default()
            },
            warning_service.clone(),
        );

        // Simulate 3 periods of growth
        monitor.check_queue_growth("test-queue", 100); // First reading, no warning
        monitor.check_queue_growth("test-queue", 200); // Growth 100, period 1
        monitor.check_queue_growth("test-queue", 300); // Growth 100, period 2
        monitor.check_queue_growth("test-queue", 400); // Growth 100, period 3 - should warn

        assert_eq!(warning_service.warning_count(), 1);
    }

    /// A `CancellationToken` is level-triggered: a token cancelled *before*
    /// the task subscribes to it must still cause an immediate exit — the
    /// late-joiner case a `broadcast` channel could not handle.
    #[tokio::test]
    async fn spawn_queue_health_monitor_exits_immediately_on_already_cancelled_token() {
        let warning_service = Arc::new(WarningService::new(WarningServiceConfig::default()));
        let monitor = Arc::new(QueueHealthMonitor::new(
            QueueHealthConfig::default(),
            warning_service,
        ));
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));

        let token = CancellationToken::new();
        token.cancel();

        let handle = spawn_queue_health_monitor(monitor, manager, token);

        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("queue health monitor should exit within 1s of an already-cancelled token")
            .expect("task should not panic");
    }
}
