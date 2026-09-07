//! Health Service - System health monitoring with rolling windows
//!
//! Provides:
//! - Overall health status determination
//! - 30-minute rolling window for success rates
//! - Pool and consumer health tracking
//! - Integration with warning service

use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::warning::WarningService;
use fc_common::{ConsumerHealth, HealthReport, HealthStatus, PoolStats};
use fc_queue::QueueConsumer;

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
    /// Max active warnings before status degrades from Healthy to Warning (Java: 5)
    pub max_warnings_healthy: u32,
    /// Max active warnings before status degrades from Warning to Degraded (Java: 20)
    pub max_warnings_warning: u32,
}

impl Default for HealthServiceConfig {
    fn default() -> Self {
        Self {
            healthy_threshold: 0.90,                      // 90% success rate
            warning_threshold: 0.70,                      // 70% success rate
            rolling_window: Duration::from_secs(30 * 60), // 30 minutes
            warning_age_minutes: 30,
            consumer_stall_threshold_secs: 60,
            max_warnings_healthy: 5,  // Java: maxWarningsForHealthy = 5
            max_warnings_warning: 20, // Java: maxWarningsForWarning = 20
        }
    }
}

/// Rolling window counter for success/failure rates.
/// Uses a VecDeque so expired events can be popped from the front in O(1)
/// instead of a full O(n) retain() scan on every record.
#[derive(Debug)]
struct RollingCounter {
    window: Duration,
    events: RwLock<VecDeque<(Instant, bool)>>,
}

impl RollingCounter {
    fn new(window: Duration) -> Self {
        Self {
            window,
            events: RwLock::new(VecDeque::new()),
        }
    }

    fn record(&self, success: bool) {
        let mut events = self.events.write();
        let cutoff = Instant::now() - self.window;

        // Pop expired events from front (they're ordered by time)
        while let Some(&(t, _)) = events.front() {
            if t <= cutoff {
                events.pop_front();
            } else {
                break;
            }
        }

        events.push_back((Instant::now(), success));
    }

    fn success_rate(&self) -> Option<f64> {
        let events = self.events.read();
        let cutoff = Instant::now() - self.window;

        let mut total = 0usize;
        let mut successes = 0usize;
        for &(t, s) in events.iter() {
            if t > cutoff {
                total += 1;
                if s {
                    successes += 1;
                }
            }
        }

        if total == 0 {
            None
        } else {
            Some(successes as f64 / total as f64)
        }
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

    /// G13 (`docs/go-mirror/2026-09-06-go-fix-list.md`): a handle to the
    /// live consumer, registered for as long as its poll task is running
    /// (`register_consumer`/`unregister_consumer`, called in lockstep with
    /// `set_consumer_running`), so `last_alive` can consult
    /// `QueueConsumer::last_broker_activity()` as a second liveness signal
    /// while a poll is in flight. Empty for a backend that never overrides
    /// the trait default — this adds nothing for those.
    consumer_broker: RwLock<HashMap<String, Arc<dyn QueueConsumer + Send + Sync>>>,
}

impl HealthService {
    pub fn new(config: HealthServiceConfig, warning_service: Arc<WarningService>) -> Self {
        Self {
            config,
            warning_service,
            pool_counters: RwLock::new(HashMap::new()),
            consumer_last_poll: RwLock::new(HashMap::new()),
            consumer_running: RwLock::new(HashMap::new()),
            consumer_broker: RwLock::new(HashMap::new()),
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

    /// Set consumer running state.
    ///
    /// Item 1 (router bench rig, 2026-09-07): the `true` transition (poll
    /// task start — the only place this is ever called with `true`, see
    /// `spawn_consumer_poll_task`) also seeds `consumer_last_poll` with
    /// "now". Before this, a freshly spawned consumer had NO entry in
    /// `consumer_last_poll` until its first `poll()` actually returned,
    /// and `get_stalled_consumers`'s old `unwrap_or(true)` treated that
    /// absence as "already stalled" — combined with `tokio::time::interval`
    /// firing its FIRST tick immediately (t≈0), every consumer still on
    /// its first poll at that instant was flagged "Stalled consumer
    /// detected" during an otherwise perfectly healthy drain (reproduced
    /// on the bench rig against Postgres and SQS — both bound `poll()` in
    /// well under the 60s default threshold, so this startup race, not a
    /// genuinely slow poll, was the only way to trigger it). Seeding here
    /// makes a fresh consumer "alive since just now" and age exactly like
    /// a real heartbeat, closing that window without weakening the check
    /// for a consumer that never completes a first poll at all — it still
    /// ages past the threshold and gets flagged, correctly.
    pub fn set_consumer_running(&self, consumer_id: &str, running: bool) {
        self.consumer_running
            .write()
            .insert(consumer_id.to_string(), running);
        if running {
            self.consumer_last_poll
                .write()
                .insert(consumer_id.to_string(), Instant::now());
        }
    }

    /// Register the live consumer handle for `consumer_id` so `last_alive`
    /// can consult [`QueueConsumer::last_broker_activity`] while its poll
    /// task is running. Call in lockstep with `set_consumer_running(id,
    /// true)`; pair with [`Self::unregister_consumer`] on exit.
    pub fn register_consumer(
        &self,
        consumer_id: &str,
        consumer: Arc<dyn QueueConsumer + Send + Sync>,
    ) {
        self.consumer_broker
            .write()
            .insert(consumer_id.to_string(), consumer);
    }

    /// Drop the registered consumer handle for `consumer_id`. Call in
    /// lockstep with `set_consumer_running(id, false)`.
    pub fn unregister_consumer(&self, consumer_id: &str) {
        self.consumer_broker.write().remove(consumer_id);
    }

    /// G13: the most recent evidence `consumer_id` is alive — the later of
    /// its last recorded `poll()`-return heartbeat and (while a poll task
    /// is actually registered) its own [`QueueConsumer::last_broker_activity`].
    /// `None` only when there is no heartbeat AND no broker-activity signal
    /// at all — i.e. a consumer id nothing has ever recorded liveness for.
    /// This can only ever push the returned instant LATER than the plain
    /// `last_poll` value (rescuing a consumer the old check would call
    /// stale); it never makes it earlier, so a genuinely wedged consumer
    /// (broker override itself stale, or none registered) is never hidden.
    fn last_alive(&self, consumer_id: &str, last_poll: &HashMap<String, Instant>) -> Option<Instant> {
        let by_poll = last_poll.get(consumer_id).copied();
        let by_broker = self
            .consumer_broker
            .read()
            .get(consumer_id)
            .and_then(|c| c.last_broker_activity());
        match (by_poll, by_broker) {
            (Some(p), Some(b)) => Some(if b > p { b } else { p }),
            (Some(p), None) => Some(p),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    /// Check if a consumer is healthy (alive within the stall threshold —
    /// see `last_alive`)
    pub fn is_consumer_healthy(&self, consumer_id: &str) -> bool {
        let threshold = Duration::from_secs(self.config.consumer_stall_threshold_secs);

        let is_running = self
            .consumer_running
            .read()
            .get(consumer_id)
            .copied()
            .unwrap_or(false);

        if !is_running {
            return false;
        }

        let last_poll = self.consumer_last_poll.read();
        self.last_alive(consumer_id, &last_poll)
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
            && self
                .last_alive(consumer_id, &last_poll)
                .map(|t| {
                    t.elapsed() < Duration::from_secs(self.config.consumer_stall_threshold_secs)
                })
                .unwrap_or(false);

        ConsumerHealth {
            queue_identifier: consumer_id.to_string(),
            is_healthy,
            last_poll_time_ms,
            time_since_last_poll_ms,
            is_running,
        }
    }

    /// Get stalled consumer IDs.
    ///
    /// Item 1: a consumer with NO liveness evidence at all (`last_alive`
    /// returns `None` — never polled, never broker-registered) is NOT
    /// reported stalled here; `set_consumer_running(id, true)` always
    /// seeds a heartbeat the instant the poll task starts (see its doc
    /// comment), so `None` should only ever occur for an id this service
    /// was never told is running in the first place — `is_running` already
    /// filters those out below regardless.
    pub fn get_stalled_consumers(&self) -> Vec<String> {
        let threshold = Duration::from_secs(self.config.consumer_stall_threshold_secs);
        let last_poll = self.consumer_last_poll.read();
        let running = self.consumer_running.read();

        running
            .iter()
            .filter(|(id, &is_running)| {
                is_running
                    && self
                        .last_alive(id, &last_poll)
                        .map(|t| t.elapsed() >= threshold)
                        .unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Calculate overall health status
    ///
    /// R-36 (2026-09-02): pool success rate is **out of readiness** — a
    /// failing target is not a failing router. `pools_healthy` /
    /// `pools_unhealthy` and their `issues` entries are still computed and
    /// returned below (warning + metric only, for `/monitoring` and the
    /// dashboard), but deliberately do **not** feed into `status`. Only
    /// consumer liveness and warning volume/severity can move `status`.
    pub fn get_health_report(&self, pool_stats: &[PoolStats]) -> HealthReport {
        let mut issues = Vec::new();

        // Check pool health based on success rates. Metric/warning surface
        // only (R-36) — see the doc comment above; these counts never
        // factor into `status`.
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
        let active_warnings = self
            .warning_service
            .get_active_warnings(self.config.warning_age_minutes);
        let active_warnings_count = active_warnings.len() as u32;
        let critical_warnings = self.warning_service.critical_count() as u32;

        if critical_warnings > 0 {
            issues.push(format!("{} critical warnings", critical_warnings));
        }

        // Determine overall status (matches Java warning-count thresholds).
        // R-36: pool success rate is deliberately excluded — see this
        // method's doc comment. Consumer liveness and warning volume are
        // the only readiness inputs besides critical warnings.
        let status = if critical_warnings > 0
            || (consumers_unhealthy > 0 && consumers_healthy == 0)
            || active_warnings_count > self.config.max_warnings_warning
        {
            HealthStatus::Degraded
        } else if consumers_unhealthy > 0
            || active_warnings_count > self.config.max_warnings_healthy
        {
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

    /// Remove tracking entries for pools and consumers that no longer exist.
    /// Call this after config reload to prevent stale entries from accumulating.
    pub fn remove_stale_entries(
        &self,
        active_pool_codes: &[String],
        active_consumer_ids: &[String],
    ) {
        // Remove pool counters for pools that no longer exist
        {
            let counters = self.pool_counters.read();
            // Only take write lock if there's something to remove
            if counters
                .keys()
                .any(|code| !active_pool_codes.contains(code))
            {
                drop(counters);
                let mut counters = self.pool_counters.write();
                let before = counters.len();
                counters.retain(|code, _| active_pool_codes.contains(code));
                let removed = before - counters.len();
                if removed > 0 {
                    debug!(removed = removed, "Removed stale pool counter entries");
                }
            }
        }

        // Remove consumer tracking for consumers that no longer exist
        {
            let last_poll = self.consumer_last_poll.read();
            if last_poll.keys().any(|id| !active_consumer_ids.contains(id)) {
                drop(last_poll);
                self.consumer_last_poll
                    .write()
                    .retain(|id, _| active_consumer_ids.contains(id));
            }
        }
        {
            let running = self.consumer_running.read();
            if running.keys().any(|id| !active_consumer_ids.contains(id)) {
                drop(running);
                self.consumer_running
                    .write()
                    .retain(|id, _| active_consumer_ids.contains(id));
            }
        }
        {
            let broker = self.consumer_broker.read();
            if broker.keys().any(|id| !active_consumer_ids.contains(id)) {
                drop(broker);
                self.consumer_broker
                    .write()
                    .retain(|id, _| active_consumer_ids.contains(id));
            }
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

    #[test]
    fn test_warning_count_thresholds() {
        use fc_common::{WarningCategory, WarningSeverity};

        let warning_service = Arc::new(WarningService::default());
        let service = HealthService::new(HealthServiceConfig::default(), warning_service.clone());

        service.set_consumer_running("consumer-1", true);
        service.record_consumer_poll("consumer-1");

        let stats: Vec<PoolStats> = vec![];

        // 0 warnings → Healthy
        let report = service.get_health_report(&stats);
        assert_eq!(report.status, HealthStatus::Healthy);

        // 6 warnings (> 5 threshold) → Warning
        for i in 0..6 {
            warning_service.add_warning(
                WarningCategory::Processing,
                WarningSeverity::Warn,
                format!("test warning {}", i),
                "test".to_string(),
            );
        }
        let report = service.get_health_report(&stats);
        assert_eq!(report.status, HealthStatus::Warning);

        // 21 warnings (> 20 threshold) → Degraded
        for i in 6..21 {
            warning_service.add_warning(
                WarningCategory::Processing,
                WarningSeverity::Warn,
                format!("test warning {}", i),
                "test".to_string(),
            );
        }
        let report = service.get_health_report(&stats);
        assert_eq!(report.status, HealthStatus::Degraded);
    }

    /// R-36: a failing target is not a failing router. Every pool below the
    /// healthy threshold (including 0% success) must still report
    /// `Healthy` overall — pool success rate is metric/warning-surface only
    /// and must never move `status`, which is what `/health/ready` reads.
    #[test]
    fn unhealthy_pools_never_degrade_status() {
        let service = create_test_service();
        service.set_consumer_running("consumer-1", true);
        service.record_consumer_poll("consumer-1");

        // 100% failure on every recorded pool.
        for _ in 0..10 {
            service.record_pool_result("POOL-A", false);
            service.record_pool_result("POOL-B", false);
        }

        let stats = vec![
            PoolStats {
                pool_code: "POOL-A".to_string(),
                concurrency: 10,
                active_workers: 0,
                queue_size: 0,
                queue_capacity: 100,
                message_group_count: 0,
                rate_limit_per_minute: None,
                is_rate_limited: false,
                metrics: None,
            },
            PoolStats {
                pool_code: "POOL-B".to_string(),
                concurrency: 10,
                active_workers: 0,
                queue_size: 0,
                queue_capacity: 100,
                message_group_count: 0,
                rate_limit_per_minute: None,
                is_rate_limited: false,
                metrics: None,
            },
        ];

        let report = service.get_health_report(&stats);
        assert_eq!(
            report.status,
            HealthStatus::Healthy,
            "an unhealthy pool must not move status off Healthy"
        );
        // Still surfaced as metric/issue, just not as a status input.
        assert_eq!(report.pools_unhealthy, 2);
        assert_eq!(report.pools_healthy, 0);
        assert!(report.issues.iter().any(|i| i.contains("POOL-A")));
    }

    /// R-36: the flip side — a consumer that stops polling (stalled, not
    /// paused) must degrade status, since `/health/ready` reads it and a
    /// non-polling router genuinely cannot work.
    #[test]
    fn stalled_consumer_degrades_status() {
        let cfg = HealthServiceConfig {
            consumer_stall_threshold_secs: 0, // instantly "stale" once any time passes
            ..HealthServiceConfig::default()
        };
        let service = HealthService::new(cfg, Arc::new(WarningService::default()));

        service.set_consumer_running("consumer-1", true);
        service.record_consumer_poll("consumer-1");
        // With a 0s threshold, `elapsed() < threshold` is false as soon as
        // any time passes — force it deterministically either way.
        std::thread::sleep(Duration::from_millis(5));

        assert!(!service.is_consumer_healthy("consumer-1"));
        assert!(service
            .get_stalled_consumers()
            .contains(&"consumer-1".to_string()));

        let report = service.get_health_report(&[]);
        assert_eq!(
            report.status,
            HealthStatus::Degraded,
            "the only consumer stalling must degrade status (all consumers unhealthy)"
        );
    }

    // --- Item 1 (router bench rig, 2026-09-07): liveness must be
    // `last_poll` OR (while a poll task is registered) broker-activity
    // evidence, and a freshly started consumer must never read as
    // already-stalled. ---

    use async_trait::async_trait;
    use fc_common::QueuedMessage;
    use fc_queue::Result as QueueResult;
    use std::sync::Mutex as StdMutex;

    /// A consumer whose `last_broker_activity()` is entirely test-driven —
    /// `poll()` itself is never actually called in these tests, which are
    /// about the health-service side of the liveness check only.
    struct FakeConsumer {
        broker_activity: StdMutex<Option<Instant>>,
    }

    impl FakeConsumer {
        fn with_activity(activity: Option<Instant>) -> Arc<Self> {
            Arc::new(Self {
                broker_activity: StdMutex::new(activity),
            })
        }
    }

    #[async_trait]
    impl QueueConsumer for FakeConsumer {
        fn identifier(&self) -> &str {
            "fake"
        }
        async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
            Ok(vec![])
        }
        async fn ack(&self, _: &str) -> QueueResult<()> {
            Ok(())
        }
        async fn nack(&self, _: &str, _: Option<u32>) -> QueueResult<()> {
            Ok(())
        }
        async fn extend_visibility(&self, _: &str, _: u32) -> QueueResult<()> {
            Ok(())
        }
        fn is_healthy(&self) -> bool {
            true
        }
        fn last_broker_activity(&self) -> Option<Instant> {
            *self.broker_activity.lock().unwrap()
        }
        async fn stop(&self) {}
    }

    /// Pins the exact startup bug this unit fixes: the instant a poll task
    /// starts (`set_consumer_running(id, true)` — the only place this is
    /// ever called with `true`), the consumer must NOT read as stalled,
    /// even though no `poll()` has returned yet. Before this fix,
    /// `consumer_last_poll` had no entry until the first real poll
    /// returned, and `get_stalled_consumers`'s `unwrap_or(true)` treated
    /// that absence as "already stalled" — flagged on the health monitor's
    /// very first (immediate) tick, against a perfectly healthy consumer.
    ///
    /// Mutant check: reverting `set_consumer_running` to skip the
    /// `consumer_last_poll` seed on `running == true` reproduces the old
    /// behaviour and fails this assertion (confirmed by hand: with the
    /// seed removed, `get_stalled_consumers` immediately contains
    /// "consumer-1").
    #[test]
    fn freshly_started_consumer_is_not_stalled_before_its_first_poll_returns() {
        let cfg = HealthServiceConfig {
            consumer_stall_threshold_secs: 60,
            ..HealthServiceConfig::default()
        };
        let service = HealthService::new(cfg, Arc::new(WarningService::default()));

        // Exactly what `spawn_consumer_poll_task` does at task start —
        // no `record_consumer_poll` call has happened yet.
        service.set_consumer_running("consumer-1", true);

        assert!(
            service.is_consumer_healthy("consumer-1"),
            "a consumer that just started, mid its first poll, must read healthy"
        );
        assert!(
            !service.get_stalled_consumers().contains(&"consumer-1".to_string()),
            "a consumer that just started must not be reported stalled before \
             it has ever had a chance to complete a poll"
        );
    }

    /// The other half of the same guarantee: a consumer that genuinely
    /// never completes a first poll (deadlocked from the start, no
    /// broker-activity override registered) must still eventually be
    /// flagged once the threshold it was SEEDED at (task start) elapses —
    /// the seed must age normally, not grant permanent immunity.
    ///
    /// Mutant check: seeding `consumer_last_poll` with a value that never
    /// ages (e.g. re-stamping "now" on every `get_stalled_consumers` call
    /// instead of once at start) would fail this — confirmed by hand.
    #[test]
    fn a_consumer_that_never_completes_a_poll_still_goes_stale_eventually() {
        let cfg = HealthServiceConfig {
            consumer_stall_threshold_secs: 0, // instantly "stale" once any time passes
            ..HealthServiceConfig::default()
        };
        let service = HealthService::new(cfg, Arc::new(WarningService::default()));

        service.set_consumer_running("consumer-1", true);
        std::thread::sleep(Duration::from_millis(5));

        assert!(
            service.get_stalled_consumers().contains(&"consumer-1".to_string()),
            "a consumer whose seeded start-time heartbeat has aged past the \
             threshold, with no poll ever completing and no broker-activity \
             override, must be reported stalled"
        );
    }

    /// G13's rescue case: `last_poll` is stale (long past the threshold —
    /// exactly what an idle NATS standing-subscription `poll()` blocked
    /// well past the stall threshold looks like from the outside), but the
    /// registered consumer's `last_broker_activity()` reports activity
    /// inside the threshold — the consumer must NOT be reported stalled.
    ///
    /// Mutant check: dropping the broker-activity branch from `last_alive`
    /// (i.e. reverting `get_stalled_consumers` to consult only
    /// `consumer_last_poll`) fails this — confirmed by hand while
    /// implementing the fix.
    #[test]
    fn stale_last_poll_is_rescued_by_recent_broker_activity() {
        let cfg = HealthServiceConfig {
            consumer_stall_threshold_secs: 1,
            ..HealthServiceConfig::default()
        };
        let service = HealthService::new(cfg, Arc::new(WarningService::default()));

        service.set_consumer_running("consumer-1", true);
        // Force `consumer_last_poll` far into the past — simulates a poll
        // that has been in flight far longer than the threshold.
        service
            .consumer_last_poll
            .write()
            .insert("consumer-1".to_string(), Instant::now() - Duration::from_secs(60));

        let consumer = FakeConsumer::with_activity(Some(Instant::now()));
        service.register_consumer("consumer-1", consumer);

        assert!(
            service.is_consumer_healthy("consumer-1"),
            "recent broker activity must rescue a consumer whose poll-return \
             heartbeat alone would read as stale"
        );
        assert!(
            !service.get_stalled_consumers().contains(&"consumer-1".to_string())
        );
    }

    /// G13's other half — "never hides a real hang": stale `last_poll` AND
    /// stale (or absent) broker activity must still be flagged. A
    /// registered consumer whose own broker-activity signal has itself
    /// gone stale (connection dropped, nothing delivered in a long time)
    /// gets no benefit from being registered at all.
    ///
    /// Mutant check: making `last_alive` ignore the broker-activity
    /// instant's own age (e.g. treating ANY `Some(_)` as "alive now")
    /// fails this — confirmed by hand.
    #[test]
    fn stale_broker_activity_does_not_rescue_a_genuinely_hung_consumer() {
        let cfg = HealthServiceConfig {
            consumer_stall_threshold_secs: 1,
            ..HealthServiceConfig::default()
        };
        let service = HealthService::new(cfg, Arc::new(WarningService::default()));

        service.set_consumer_running("consumer-1", true);
        service
            .consumer_last_poll
            .write()
            .insert("consumer-1".to_string(), Instant::now() - Duration::from_secs(60));

        let consumer = FakeConsumer::with_activity(Some(Instant::now() - Duration::from_secs(60)));
        service.register_consumer("consumer-1", consumer);

        assert!(
            !service.is_consumer_healthy("consumer-1"),
            "a consumer with stale broker activity too must still be flagged — \
             the broker signal must never permanently mask a real hang"
        );
        assert!(service.get_stalled_consumers().contains(&"consumer-1".to_string()));
    }

    /// `unregister_consumer` must actually drop the registration — after
    /// it, a stale `last_poll` is judged on its own again (no leftover
    /// broker-activity rescue from a since-exited poll task).
    #[test]
    fn unregister_consumer_removes_the_broker_activity_rescue() {
        let cfg = HealthServiceConfig {
            consumer_stall_threshold_secs: 1,
            ..HealthServiceConfig::default()
        };
        let service = HealthService::new(cfg, Arc::new(WarningService::default()));

        service.set_consumer_running("consumer-1", true);
        service
            .consumer_last_poll
            .write()
            .insert("consumer-1".to_string(), Instant::now() - Duration::from_secs(60));

        let consumer = FakeConsumer::with_activity(Some(Instant::now()));
        service.register_consumer("consumer-1", consumer);
        assert!(service.is_consumer_healthy("consumer-1"));

        service.unregister_consumer("consumer-1");
        assert!(
            !service.is_consumer_healthy("consumer-1"),
            "once unregistered, a stale last_poll must go back to being stale"
        );
    }
}
