//! Health Service - System health monitoring with rolling windows
//!
//! Provides:
//! - Overall health status determination
//! - 30-minute rolling window for success rates
//! - Pool and consumer health tracking
//! - Integration with warning service

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tracing::{debug, warn};

use fc_common::{HealthStatus, HealthReport, PoolStats, ConsumerHealth};
use crate::warning::WarningService;

/// Configuration for health service
#[derive(Debug, Clone)]
pub struct HealthServiceConfig {
    /// Success rate threshold for healthy status (0.0 - 1.0)
    pub healthy_threshold: f64,
    /// Success rate threshold for warning status (0.0 - 1.0)
    pub warning_threshold: f64,
    /// Rolling window duration for rate calculations
    pub rolling_window: Duration,
    /// Maximum age of warnings to consider (minutes)
    pub warning_age_minutes: i64,
    /// Consumer stall threshold (seconds since last poll)
    pub consumer_stall_threshold_secs: u64,
}

impl Default for HealthServiceConfig {
    fn default() -> Self {
        Self {
            healthy_threshold: 0.90,  // 90% success rate
            warning_threshold: 0.70,  // 70% success rate
            rolling_window: Duration::from_secs(30 * 60),  // 30 minutes
            warning_age_minutes: 30,
            consumer_stall_threshold_secs: 60,
        }
    }
}

/// Rolling window counter for success/failure rates
#[derive(Debug)]
struct RollingCounter {
    window: Duration,
    events: RwLock<Vec<(Instant, bool)>>,  // (timestamp, success)
}

impl RollingCounter {
    fn new(window: Duration) -> Self {
        Self {
            window,
            events: RwLock::new(Vec::new()),
        }
    }

    fn record(&self, success: bool) {
        let mut events = self.events.write();
        events.push((Instant::now(), success));

        // Cleanup old events
        let cutoff = Instant::now() - self.window;
        events.retain(|(t, _)| *t > cutoff);
    }

    fn success_rate(&self) -> Option<f64> {
        let events = self.events.read();
        let cutoff = Instant::now() - self.window;

        let recent: Vec<_> = events.iter().filter(|(t, _)| *t > cutoff).collect();
        if recent.is_empty() {
            return None;
        }

        let successes = recent.iter().filter(|(_, s)| *s).count();
        Some(successes as f64 / recent.len() as f64)
    }

    #[allow(dead_code)]
    fn total_count(&self) -> usize {
        let events = self.events.read();
        let cutoff = Instant::now() - self.window;
        events.iter().filter(|(t, _)| *t > cutoff).count()
    }
}

/// Health service with rolling window calculations
pub struct HealthService {
    config: HealthServiceConfig,
    warning_service: Arc<WarningService>,

    /// Pool success rate counters
    pool_counters: RwLock<HashMap<String, RollingCounter>>,

    /// Consumer health tracking
    consumer_last_poll: RwLock<HashMap<String, Instant>>,

    /// Consumer running state
    consumer_running: RwLock<HashMap<String, bool>>,
}

impl HealthService {
    pub fn new(config: HealthServiceConfig, warning_service: Arc<WarningService>) -> Self {
        Self {
            config,
            warning_service,
            pool_counters: RwLock::new(HashMap::new()),
            consumer_last_poll: RwLock::new(HashMap::new()),
            consumer_running: RwLock::new(HashMap::new()),
        }
    }

    /// Record a pool processing result
    pub fn record_pool_result(&self, pool_code: &str, success: bool) {
        let mut counters = self.pool_counters.write();
        let counter = counters
            .entry(pool_code.to_string())
            .or_insert_with(|| RollingCounter::new(self.config.rolling_window));
        counter.record(success);
    }

    /// Get success rate for a pool
    pub fn get_pool_success_rate(&self, pool_code: &str) -> Option<f64> {
        self.pool_counters
            .read()
            .get(pool_code)
            .and_then(|c| c.success_rate())
    }

    /// Record consumer poll
    pub fn record_consumer_poll(&self, consumer_id: &str) {
        self.consumer_last_poll
            .write()
            .insert(consumer_id.to_string(), Instant::now());
    }

    /// Set consumer running state
    pub fn set_consumer_running(&self, consumer_id: &str, running: bool) {
        self.consumer_running
            .write()
            .insert(consumer_id.to_string(), running);
    }

    /// Check if a consumer is healthy (polled recently)
    pub fn is_consumer_healthy(&self, consumer_id: &str) -> bool {
        let threshold = Duration::from_secs(self.config.consumer_stall_threshold_secs);

        let is_running = self.consumer_running
            .read()
            .get(consumer_id)
            .copied()
            .unwrap_or(false);

        if !is_running {
            return false;
        }

        self.consumer_last_poll
            .read()
            .get(consumer_id)
            .map(|t| t.elapsed() < threshold)
            .unwrap_or(false)
    }

    /// Get consumer health details
    pub fn get_consumer_health(&self, consumer_id: &str) -> ConsumerHealth {
        let last_poll = self.consumer_last_poll.read();
        let running = self.consumer_running.read();

        let is_running = running.get(consumer_id).copied().unwrap_or(false);
        let last_poll_time = last_poll.get(consumer_id);

        let (last_poll_time_ms, time_since_last_poll_ms) = match last_poll_time {
            Some(t) => {
                let elapsed = t.elapsed().as_millis() as i64;
                (Some(elapsed), Some(elapsed))
            }
            None => (None, None),
        };

        let is_healthy = is_running
            && last_poll_time
                .map(|t| t.elapsed() < Duration::from_secs(self.config.consumer_stall_threshold_secs))
                .unwrap_or(false);

        ConsumerHealth {
            queue_identifier: consumer_id.to_string(),
            is_healthy,
            last_poll_time_ms,
            time_since_last_poll_ms,
            is_running,
        }
    }

    /// Get stalled consumer IDs
    pub fn get_stalled_consumers(&self) -> Vec<String> {
        let threshold = Duration::from_secs(self.config.consumer_stall_threshold_secs);
        let last_poll = self.consumer_last_poll.read();
        let running = self.consumer_running.read();

        running
            .iter()
            .filter(|(id, &is_running)| {
                is_running && last_poll
                    .get(*id)
                    .map(|t| t.elapsed() >= threshold)
                    .unwrap_or(true)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Calculate overall health status
    pub fn get_health_report(&self, pool_stats: &[PoolStats]) -> HealthReport {
        let mut issues = Vec::new();

        // Check pool health based on success rates
        let mut pools_healthy = 0u32;
        let mut pools_unhealthy = 0u32;

        for stat in pool_stats {
            if let Some(rate) = self.get_pool_success_rate(&stat.pool_code) {
                if rate >= self.config.healthy_threshold {
                    pools_healthy += 1;
                } else {
                    pools_unhealthy += 1;
                    issues.push(format!(
                        "Pool {} success rate: {:.1}%",
                        stat.pool_code,
                        rate * 100.0
                    ));
                }
            } else {
                // No data yet - consider healthy
                pools_healthy += 1;
            }
        }

        // Check consumer health
        let running = self.consumer_running.read();
        let consumers_total = running.len() as u32;
        let stalled = self.get_stalled_consumers();
        let consumers_unhealthy = stalled.len() as u32;
        let consumers_healthy = consumers_total.saturating_sub(consumers_unhealthy);

        for consumer_id in &stalled {
            issues.push(format!("Consumer {} is stalled", consumer_id));
        }

        // Check warnings
        let active_warnings = self.warning_service
            .get_active_warnings(self.config.warning_age_minutes);
        let active_warnings_count = active_warnings.len() as u32;
        let critical_warnings = self.warning_service.critical_count() as u32;

        if critical_warnings > 0 {
            issues.push(format!("{} critical warnings", critical_warnings));
        }

        // Determine overall status
        let status = if critical_warnings > 0
            || (pools_unhealthy > 0 && pools_healthy == 0)
            || (consumers_unhealthy > 0 && consumers_healthy == 0)
        {
            HealthStatus::Degraded
        } else if pools_unhealthy > 0 || consumers_unhealthy > 0 || active_warnings_count > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        };

        if status != HealthStatus::Healthy {
            debug!(
                status = ?status,
                pools_healthy,
                pools_unhealthy,
                consumers_healthy,
                consumers_unhealthy,
                active_warnings = active_warnings_count,
                "Health report generated"
            );
        }

        HealthReport {
            status,
            pools_healthy,
            pools_unhealthy,
            consumers_healthy,
            consumers_unhealthy,
            active_warnings: active_warnings_count,
            critical_warnings,
            issues,
        }
    }

    /// Check if overall system is healthy
    pub fn is_healthy(&self, pool_stats: &[PoolStats]) -> bool {
        self.get_health_report(pool_stats).status == HealthStatus::Healthy
    }

    /// Periodic cleanup and maintenance
    pub fn cleanup(&self) {
        // Cleanup warning service
        self.warning_service.cleanup();

        // Log any stalled consumers
        let stalled = self.get_stalled_consumers();
        if !stalled.is_empty() {
            warn!(
                count = stalled.len(),
                consumers = ?stalled,
                "Detected stalled consumers"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_service() -> HealthService {
        let warning_service = Arc::new(WarningService::default());
        HealthService::new(HealthServiceConfig::default(), warning_service)
    }

    #[test]
    fn test_record_pool_result() {
        let service = create_test_service();

        // Record some successes
        for _ in 0..10 {
            service.record_pool_result("TEST", true);
        }

        let rate = service.get_pool_success_rate("TEST");
        assert_eq!(rate, Some(1.0));
    }

    #[test]
    fn test_consumer_health() {
        let service = create_test_service();

        service.set_consumer_running("consumer-1", true);
        service.record_consumer_poll("consumer-1");

        assert!(service.is_consumer_healthy("consumer-1"));
    }

    #[test]
    fn test_health_report() {
        let service = create_test_service();

        // Setup healthy state
        service.set_consumer_running("consumer-1", true);
        service.record_consumer_poll("consumer-1");
        service.record_pool_result("DEFAULT", true);

        let stats = vec![PoolStats {
            pool_code: "DEFAULT".to_string(),
            concurrency: 10,
            active_workers: 5,
            queue_size: 0,
            queue_capacity: 100,
            message_group_count: 0,
            rate_limit_per_minute: None,
            is_rate_limited: false,
            metrics: None,
        }];

        let report = service.get_health_report(&stats);
        assert_eq!(report.status, HealthStatus::Healthy);
    }
}
