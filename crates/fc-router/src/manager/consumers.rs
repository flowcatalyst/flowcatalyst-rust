//! Consumer poll loop plus consumer-facing queries: liveness/health,
//! broker connectivity, queue metrics, the stalled-consumer watchdog
//! (rebuild before retire), and the retirement of detached consumers.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use fc_common::{WarningCategory, WarningSeverity};
use fc_queue::QueueMetrics;

use super::registry::is_stale;
use super::{QueueManager, RestartRecord, RunningConsumer};
use crate::health::ConsumerStatsProvider;

/// Sleep for `d`, but race it against `token`. Returns `true` if the token
/// was cancelled before `d` elapsed (caller should stop looping), `false`
/// if the sleep completed normally.
async fn sleep_or_cancel(token: &CancellationToken, d: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => false,
        _ = token.cancelled() => true,
    }
}

/// Park untimed on the manager's capacity-freed gate (G12) until consumer
/// `rc` has capacity again (Go: `awaitCapacity`) or `token` is cancelled.
/// Returns `true` if cancelled first.
///
/// Wakes on the gate (a pool crossing back under capacity, a reconfigure, a
/// new pool) and on the earliest of this consumer's deferrals coming due —
/// nothing signals the gate when budget frees up that way.
///
/// **Race-free by construction.** The `Notified` future is created (which
/// captures the gate's current `notify_waiters()` call count) *before* the
/// capacity check, and `tokio::sync::Notify` guarantees a `notify_waiters()`
/// call is observed by a `Notified` created before it, whether or not it has
/// been polled yet.
async fn wait_for_capacity_or_cancel(
    manager: &QueueManager,
    rc: &RunningConsumer,
    token: &CancellationToken,
) -> bool {
    loop {
        let notified = manager.capacity_notify().notified();
        if manager.has_capacity_for(rc) {
            return false;
        }
        let due = rc.earliest_deferral();
        tokio::select! {
            _ = notified => {}
            _ = async {
                match due {
                    Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
                    None => std::future::pending::<()>().await,
                }
            } => {}
            _ = token.cancelled() => return true,
        }
    }
}

/// What one bounded poll produced.
enum PollOutcome {
    Messages(Vec<fc_common::QueuedMessage>),
    /// Nothing arrived. `waited` is true when the poll itself already spent
    /// the whole poll timeout waiting (a blocking backend), so re-polling at
    /// once is not a hot loop.
    Empty {
        waited: bool,
    },
    Stopped,
    Error(String),
    Cancelled,
}

impl QueueManager {
    /// One `poll()`, bounded by the poll timeout (Go: `consumerPollTimeout`)
    /// and raced against the consumer's poll token.
    ///
    /// A poll that runs out the timeout is, for a backend whose `poll()`
    /// blocks by contract (NATS's standing subscription), simply an empty
    /// poll — provided the backend still vouches for its broker link
    /// (`last_broker_activity`); for every other backend, or a NATS
    /// subscription that has died, it is a poll error (Go G13/G14).
    async fn bounded_poll(&self, rc: &RunningConsumer) -> PollOutcome {
        rc.polls_started.fetch_add(1, Ordering::SeqCst);
        let outcome = tokio::select! {
            _ = rc.stop_poll.cancelled() => PollOutcome::Cancelled,
            res = tokio::time::timeout(self.poll_timeout, rc.consumer.poll(10)) => match res {
                Ok(Ok(messages)) if messages.is_empty() => PollOutcome::Empty { waited: false },
                Ok(Ok(messages)) => PollOutcome::Messages(messages),
                Ok(Err(fc_queue::QueueError::Stopped)) => PollOutcome::Stopped,
                Ok(Err(e)) => PollOutcome::Error(e.to_string()),
                Err(_) => {
                    let vouched = rc
                        .consumer
                        .last_broker_activity()
                        .is_some_and(|t| t.elapsed() < self.poll_timeout);
                    if vouched {
                        PollOutcome::Empty { waited: true }
                    } else {
                        PollOutcome::Error(format!(
                            "poll did not return within {:?}",
                            self.poll_timeout
                        ))
                    }
                }
            },
        };
        rc.polls_returned.fetch_add(1, Ordering::SeqCst);
        outcome
    }

    /// Spawn the poll loop for `rc` (Go: `runConsumer`). A second call for
    /// the same instance is a no-op. The loop runs until `rc.stop_poll` is
    /// cancelled (detach, manager shutdown) or the consumer reports
    /// `Stopped`.
    ///
    /// The heartbeat (`rc.beat()`) is stamped on a SUCCESSFUL poll (empty or
    /// not), while paused for capacity, and while paused for leadership —
    /// never on a poll error, so a consumer that errors on every poll goes
    /// stale and the watchdog rebuilds it (Go stamps only on success). A
    /// consumer whose poll returns `Stopped` exits the loop and is left in
    /// the registry with a stale heartbeat for the watchdog to rebuild.
    ///
    /// `route_batch` is not raced against cancellation: once a batch is
    /// accepted it runs to completion so every message is acked or nacked.
    pub(super) fn spawn_consumer_poll_task(
        self: &Arc<Self>,
        rc: Arc<RunningConsumer>,
    ) -> tokio::task::JoinHandle<()> {
        if rc.poll_task_started.swap(true, Ordering::SeqCst) {
            debug!(consumer = %rc.identifier(), "Poll task already running for this consumer instance");
            return tokio::spawn(async {});
        }
        self.wire_health_provider();
        rc.beat();

        let manager = self.clone();
        tokio::spawn(async move {
            let token = rc.stop_poll.clone();
            let id = rc.identifier().to_string();
            let mut capacity_paused = false;

            loop {
                if token.is_cancelled() {
                    break;
                }

                // R-26/R-34 (owner ruling: losing leadership only pauses
                // polling): in-flight work is untouched; only new intake
                // stops. The heartbeat stays fresh — a paused consumer is
                // not a stalled one.
                if !manager.is_leader() {
                    rc.beat();
                    if sleep_or_cancel(&token, Duration::from_secs(2)).await {
                        break;
                    }
                    continue;
                }

                // Backpressure (G12): park untimed on the capacity gate.
                // A consumer paused for capacity is doing its job, so its
                // heartbeat stays fresh (Go: the watchdog must not rebuild
                // it — a rebuild used to strand the very buffers it waited
                // on).
                if !manager.has_capacity_for(&rc) {
                    if !capacity_paused {
                        capacity_paused = true;
                        warn!(consumer = %id, "Destination pools at capacity and deferral budget spent — pausing poll");
                        manager.warning_service.add_warning(
                            WarningCategory::PoolHealth,
                            WarningSeverity::Warn,
                            format!(
                                "Consumer [{}] paused — its destination pools are at capacity and {} deferrals are outstanding (budget {})",
                                id,
                                rc.deferrals_outstanding(Instant::now()),
                                manager.deferral_budget
                            ),
                            "ConsumerLoop".to_string(),
                        );
                    }
                    rc.beat();
                    if wait_for_capacity_or_cancel(&manager, &rc, &token).await {
                        break;
                    }
                    continue;
                } else if capacity_paused {
                    capacity_paused = false;
                    info!(consumer = %id, "Capacity returned; resuming poll");
                }

                let polled = manager.bounded_poll(&rc).await;
                manager.report_rejected(&rc);
                match polled {
                    PollOutcome::Cancelled => break,
                    PollOutcome::Messages(messages) => {
                        rc.beat();
                        if let Err(e) = manager.route_batch_from(messages, &rc).await {
                            error!(error = %e, consumer = %id, "Error routing batch");
                        }
                        // G12: re-poll immediately after any batch.
                    }
                    PollOutcome::Empty { waited } => {
                        rc.beat();
                        if !waited && sleep_or_cancel(&token, Duration::from_secs(1)).await {
                            break;
                        }
                    }
                    PollOutcome::Stopped => {
                        // The consumer will never poll again. Exit; the
                        // entry stays registered with a heartbeat that now
                        // goes stale, so the watchdog rebuilds it (Go).
                        warn!(consumer = %id, "Consumer stopped; poll loop exiting for rebuild");
                        break;
                    }
                    PollOutcome::Error(e) => {
                        warn!(consumer = %id, error = %e, "Consumer poll error");
                        if sleep_or_cancel(&token, Duration::from_secs(1)).await {
                            break;
                        }
                    }
                }
            }
            debug!(consumer = %id, generation = rc.generation, "Poll loop exited");
        })
    }

    /// Raise a CONFIGURATION/ERROR warning for every message the consumer
    /// removed at its parse boundary since the last poll (SQS deleted it,
    /// NATS terminated it, Postgres quarantined it). Such a message is
    /// never delivered, and the cause — a producer sending something this
    /// router cannot read, such as an unsupported mediation type — is one
    /// an operator can fix; Go warns on it (corpus case
    /// `unsupported-mediation-type`). It used to be a log line only.
    fn report_rejected(&self, rc: &RunningConsumer) {
        for rejected in rc.consumer.take_rejected() {
            error!(
                queue = %rc.identifier(),
                broker_message_id = ?rejected.broker_message_id,
                reason = %rejected.reason,
                "Malformed message removed from the queue without delivery"
            );
            self.warning_service.add_warning(
                WarningCategory::Configuration,
                WarningSeverity::Error,
                format!(
                    "Malformed message {} on queue {} removed without delivery: {}",
                    rejected.broker_message_id.as_deref().unwrap_or("(no id)"),
                    rc.identifier(),
                    rejected.reason
                ),
                "QueueManager".to_string(),
            );
        }
    }

    /// Point the health service at this manager for consumer liveness (Go:
    /// `Health.SetConsumerStats(Manager)`). Idempotent; held weakly.
    fn wire_health_provider(self: &Arc<Self>) {
        if let Some(ref hs) = self.health_service {
            let weak: Weak<Self> = Arc::downgrade(self);
            let provider: Weak<dyn ConsumerStatsProvider> = weak;
            hs.set_consumer_stats_provider(provider);
        }
    }

    /// Registry keys (config queue names) of every active consumer.
    pub async fn consumer_ids(&self) -> Vec<String> {
        self.consumers.names()
    }

    /// Check broker connectivity by verifying all consumers report healthy.
    pub async fn check_broker_connectivity(&self) -> bool {
        let consumers = self.consumers.active();
        if consumers.is_empty() {
            return true;
        }
        for rc in consumers {
            if !rc.consumer.is_healthy() {
                warn!(
                    consumer = %rc.identifier(),
                    "Broker connectivity check failed: consumer unhealthy"
                );
                return false;
            }
        }
        true
    }

    /// Build a replacement for `old` from its queue config, bounded by the
    /// rebuild timeout. `None` (with the reason logged) when it can't be
    /// built.
    async fn build_replacement(
        &self,
        old: &RunningConsumer,
    ) -> Result<Arc<RunningConsumer>, String> {
        let (Some(factory), Some(cfg)) = (self.consumer_factory.as_ref(), old.queue_config.clone())
        else {
            return Err(
                "no consumer factory and/or queue config to build a replacement".to_string(),
            );
        };
        match tokio::time::timeout(self.rebuild_timeout, factory.create_consumer(&cfg)).await {
            Ok(Ok(consumer)) => {
                Ok(self.new_running_consumer(consumer, old.name.clone(), Some(cfg)))
            }
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err(format!(
                "build did not finish within {:?}",
                self.rebuild_timeout
            )),
        }
    }

    /// Swap `new` in for `old` and detach `old` (Go: build-then-swap in
    /// `RestartStalledConsumers`). Returns false — and discards `new` —
    /// when `old` is no longer the registered instance.
    async fn swap_in(
        self: &Arc<Self>,
        old: &Arc<RunningConsumer>,
        new: Arc<RunningConsumer>,
    ) -> bool {
        if !self.consumers.replace_if_current(old, new.clone()) {
            new.stop_poll.cancel();
            new.consumer.stop().await;
            return false;
        }
        self.detach_consumer(old.clone()).await;
        self.spawn_consumer_poll_task(new);
        true
    }

    /// Stop `rc` polling and move it to the detaching list: its in-flight
    /// messages still resolve it for ack/nack until
    /// [`Self::retire_detached_consumers`] finds nothing left that it
    /// polled (X-11 / R-26/R-49). Every backend keeps ack/nack working
    /// after `stop()`, which here only ends intake.
    pub(super) async fn detach_consumer(&self, rc: Arc<RunningConsumer>) {
        rc.stop_poll.cancel();
        rc.consumer.stop().await;
        self.consumers.detach(rc);
    }

    /// Rebuild one consumer now (operator/test entry point), resolved by
    /// identifier or registry name. Builds the replacement FIRST: a build
    /// that fails leaves the existing consumer running untouched and returns
    /// `false` (Go: "a rebuild that fails or hangs can never leave the queue
    /// with no consumer at all").
    pub async fn restart_consumer(self: &Arc<Self>, consumer_id: &str) -> bool {
        let _reload_guard = self.pool_configs.read().await;
        let Some(old) = self
            .consumers
            .get_by_id(consumer_id)
            .or_else(|| self.consumers.get(consumer_id))
        else {
            warn!(consumer_id = %consumer_id, "Consumer not found for restart");
            return false;
        };
        match self.build_replacement(&old).await {
            Ok(new) => {
                let swapped = self.swap_in(&old, new).await;
                if swapped {
                    info!(consumer_id = %consumer_id, "Consumer restarted with a fresh instance");
                }
                swapped
            }
            Err(e) => {
                warn!(
                    consumer_id = %consumer_id,
                    error = %e,
                    "Could not build a replacement consumer; leaving the existing one in place"
                );
                self.warning_service.add_warning(
                    WarningCategory::ConsumerHealth,
                    WarningSeverity::Warn,
                    format!(
                        "Restart of consumer [{}] failed; the existing consumer is left in place: {}",
                        consumer_id, e
                    ),
                    "QueueManager".to_string(),
                );
                false
            }
        }
    }

    /// Rebuild every consumer whose heartbeat is older than `threshold`
    /// (Go: `RestartStalledConsumers`). Returns how many were replaced.
    ///
    /// - Nothing happens once polling has been stopped for shutdown.
    /// - The replacement is built first (bounded by the rebuild timeout)
    ///   and swapped in only if the stalled instance is still the
    ///   registered one AND still stalled; only then is the old one
    ///   detached. A failed build leaves the existing entry in place, and
    ///   its failures count toward CRITICAL escalation (after 10).
    /// - Consumers paused for capacity or leadership keep their heartbeat
    ///   fresh and are never candidates.
    /// - `restart_delay` separates consecutive rebuilds in one sweep.
    /// - A consumer's attempt count is forgotten only once it has been quiet
    ///   for three thresholds, so one that flaps between restarts still
    ///   escalates.
    pub async fn restart_stalled_consumers(
        self: &Arc<Self>,
        threshold: Duration,
        restart_delay: Duration,
        cancel: &CancellationToken,
    ) -> usize {
        if threshold.is_zero() || self.polling_stopped.load(Ordering::SeqCst) {
            return 0;
        }
        let _reload_guard = self.pool_configs.read().await;

        let stalled: Vec<Arc<RunningConsumer>> = self
            .consumers
            .active()
            .into_iter()
            .filter(|rc| {
                rc.poll_task_started.load(Ordering::SeqCst) && is_stale(rc.last_poll(), threshold)
            })
            .collect();

        {
            let recovery_window = threshold * 3;
            let stalled_names: std::collections::HashSet<&str> =
                stalled.iter().map(|rc| rc.name.as_str()).collect();
            self.restart_attempts.lock().retain(|name, rec| {
                stalled_names.contains(name.as_str()) || rec.last.elapsed() <= recovery_window
            });
        }

        let mut restarted = 0usize;
        for (i, old) in stalled.iter().enumerate() {
            let attempts = self
                .restart_attempts
                .lock()
                .get(&old.name)
                .map(|r| r.attempts)
                .unwrap_or(0);
            let severity = if attempts >= Self::CONSUMER_RESTART_CRITICAL_AFTER {
                WarningSeverity::Critical
            } else {
                WarningSeverity::Warn
            };
            let cause = old.poll_state();
            self.warning_service.add_warning(
                WarningCategory::ConsumerHealth,
                severity,
                format!(
                    "Consumer {} is stalled ({}), restart attempt {}",
                    old.name,
                    cause,
                    attempts + 1
                ),
                "QueueManager".to_string(),
            );
            warn!(
                queue = %old.name,
                attempt = attempts + 1,
                cause,
                polls_started = old.polls_started.load(Ordering::SeqCst),
                polls_returned = old.polls_returned.load(Ordering::SeqCst),
                "Stalled consumer detected, attempting restart"
            );

            if i > 0 && sleep_or_cancel(cancel, restart_delay).await {
                return restarted;
            }

            let new = match self.build_replacement(old).await {
                Ok(new) => new,
                Err(e) => {
                    let n = self.bump_restart(&old.name);
                    error!(
                        queue = %old.name,
                        attempt = n,
                        error = %e,
                        "Failed to rebuild stalled consumer; leaving the existing entry in place"
                    );
                    continue;
                }
            };

            // Still stalled? A consumer that completed a poll while we were
            // building has recovered; replacing it would only cancel its
            // work.
            if !is_stale(old.last_poll(), threshold) {
                new.stop_poll.cancel();
                new.consumer.stop().await;
                info!(queue = %old.name, "Stalled consumer recovered before its restart; leaving it alone");
                continue;
            }
            if self.swap_in(old, new).await {
                self.bump_restart(&old.name);
                restarted += 1;
            }
        }
        restarted
    }

    fn bump_restart(&self, name: &str) -> u32 {
        let mut attempts = self.restart_attempts.lock();
        let rec = attempts.entry(name.to_string()).or_insert(RestartRecord {
            attempts: 0,
            last: Instant::now(),
        });
        rec.attempts += 1;
        rec.last = Instant::now();
        rec.attempts
    }

    /// Finish the teardown of detached consumers that nothing in the
    /// pipeline still references (Go: `retireDetachedConsumers`): no
    /// in-flight entry from their queue that started before they were
    /// detached. The replacement's own traffic, which shares the queue
    /// identifier, never holds a detached consumer up. Returns how many
    /// were retired.
    pub fn retire_detached_consumers(&self) -> usize {
        let retired = self.consumers.take_retirable(|rc| {
            let Some(detached_at) = rc.detached_at() else {
                return true;
            };
            let id = rc.identifier();
            !self
                .in_pipeline
                .iter()
                .any(|e| e.value().queue_identifier == id && e.value().started_at < detached_at)
        });
        for rc in &retired {
            info!(
                queue = %rc.identifier(),
                "Retired detached consumer; nothing that pre-dates its detach still references its queue"
            );
        }
        retired.len()
    }

    /// Number of detached consumers not yet retired.
    pub fn detaching_consumer_count(&self) -> usize {
        self.consumers.detaching().len()
    }

    /// Whether a consumer (by registry name or identifier) reports its
    /// broker connection healthy.
    pub async fn is_consumer_healthy(&self, consumer_id: &str) -> bool {
        self.consumers
            .get(consumer_id)
            .or_else(|| self.consumers.get_by_id(consumer_id))
            .is_some_and(|rc| rc.consumer.is_healthy())
    }

    /// Get queue metrics from all consumers
    pub async fn get_queue_metrics(&self) -> Vec<QueueMetrics> {
        let consumers = self.consumers.active();
        let mut metrics = Vec::with_capacity(consumers.len());
        for rc in consumers {
            match rc.consumer.get_metrics().await {
                Ok(Some(m)) => metrics.push(m),
                Ok(None) => {
                    debug!(consumer_id = %rc.name, "Consumer does not support metrics");
                }
                Err(e) => {
                    warn!(consumer_id = %rc.name, error = %e, "Failed to get queue metrics");
                }
            }
        }
        metrics
    }

    /// Get counter metrics only (no broker round trip — instant atomic reads)
    pub async fn get_queue_metrics_counters_only(&self) -> Vec<QueueMetrics> {
        self.consumers
            .active()
            .iter()
            .filter_map(|rc| rc.consumer.get_counters())
            .collect()
    }

    /// Queue configs of the active consumers, by registry name.
    pub fn queue_configs(&self) -> HashMap<String, fc_common::QueueConfig> {
        self.consumers
            .active()
            .into_iter()
            .filter_map(|rc| rc.queue_config.clone().map(|c| (rc.name.clone(), c)))
            .collect()
    }
}

impl ConsumerStatsProvider for QueueManager {
    fn consumer_stats(&self) -> Vec<super::ConsumerStat> {
        self.consumers.stats()
    }
}

#[cfg(test)]
mod consumer_liveness_tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;
    use crate::warning::WarningService;
    use crate::ConsumerFactory;
    use async_trait::async_trait;
    use fc_common::QueuedMessage;
    use fc_queue::{QueueConsumer, Result as QueueResult};
    use std::sync::atomic::{AtomicBool, AtomicU32};

    /// Never returns messages; counts polls; can be told to fail every poll
    /// or to report `Stopped`.
    struct TestConsumer {
        id: &'static str,
        polls: AtomicU32,
        fail: AtomicBool,
        stopped: AtomicBool,
    }

    impl TestConsumer {
        fn new(id: &'static str) -> Arc<Self> {
            Arc::new(Self {
                id,
                polls: AtomicU32::new(0),
                fail: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
            })
        }
        fn polls(&self) -> u32 {
            self.polls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl QueueConsumer for TestConsumer {
        fn identifier(&self) -> &str {
            self.id
        }
        async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            if self.stopped.load(Ordering::SeqCst) {
                return Err(fc_queue::QueueError::Stopped);
            }
            if self.fail.load(Ordering::SeqCst) {
                return Err(fc_queue::QueueError::NotConnected);
            }
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
        async fn stop(&self) {
            self.stopped.store(true, Ordering::SeqCst);
        }
    }

    /// Hands out pre-built consumers, or fails while `fail` is set.
    struct QueueFactory {
        next: parking_lot::Mutex<Vec<Arc<TestConsumer>>>,
        fail: AtomicBool,
        builds: AtomicU32,
    }

    #[async_trait]
    impl ConsumerFactory for QueueFactory {
        async fn create_consumer(
            &self,
            _config: &fc_common::QueueConfig,
        ) -> crate::Result<Arc<dyn QueueConsumer>> {
            self.builds.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(crate::RouterError::ConsumerBuild("broker down".into()));
            }
            let c = self.next.lock().pop().expect("no consumer queued");
            Ok(c as Arc<dyn QueueConsumer>)
        }
    }

    fn queue_config(name: &str) -> fc_common::QueueConfig {
        fc_common::QueueConfig {
            name: name.to_string(),
            uri: format!("test://{name}"),
            connections: 1,
            visibility_timeout: 30,
        }
    }

    fn manager_with(
        factory: Option<Arc<QueueFactory>>,
    ) -> (Arc<QueueManager>, Arc<crate::health::HealthService>) {
        let health_service = Arc::new(crate::health::HealthService::new(
            crate::health::HealthServiceConfig {
                consumer_stall_threshold_secs: 60,
                ..crate::health::HealthServiceConfig::default()
            },
            Arc::new(WarningService::default()),
        ));
        let mut b = QueueManager::builder(HttpMediatorConfig::dev())
            .health_service(health_service.clone())
            .rebuild_timeout(Duration::from_secs(2));
        if let Some(f) = factory {
            b = b.consumer_factory(f);
        }
        (Arc::new(b.build()), health_service)
    }

    async fn register_and_start(
        manager: &Arc<QueueManager>,
        c: Arc<TestConsumer>,
    ) -> Arc<RunningConsumer> {
        manager.add_consumer(c.clone()).await;
        let rc = manager.consumers.get(c.id).unwrap();
        manager.spawn_consumer_poll_task(rc.clone());
        rc
    }

    /// A message the backend removed at its parse boundary raises a
    /// CONFIGURATION/ERROR warning (Go warns; corpus
    /// `unsupported-mediation-type`).
    #[tokio::test]
    async fn malformed_messages_removed_by_the_backend_raise_a_config_error() {
        struct Rejecting {
            log: fc_queue::RejectedLog,
        }
        #[async_trait]
        impl QueueConsumer for Rejecting {
            fn identifier(&self) -> &str {
                "rejecting"
            }
            async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
                self.log.record(
                    Some("m-1".to_string()),
                    "unknown variant `SMTP`, expected `HTTP`",
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
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
            fn take_rejected(&self) -> Vec<fc_queue::RejectedMessage> {
                self.log.take()
            }
            async fn stop(&self) {}
        }
        let (manager, _hs) = manager_with(None);
        let c = Arc::new(Rejecting {
            log: fc_queue::RejectedLog::default(),
        });
        manager.add_consumer(c).await;
        let rc = manager.consumers.get("rejecting").unwrap();
        manager.spawn_consumer_poll_task(rc);
        tokio::time::sleep(Duration::from_millis(60)).await;
        let warnings = manager
            .warning_service
            .get_warnings_by_category(WarningCategory::Configuration);
        assert!(!warnings.is_empty());
        assert_eq!(warnings[0].severity, WarningSeverity::Error);
        assert!(warnings[0].message.contains("SMTP"));
        manager.shutdown().await;
    }

    /// A leadership-paused consumer must never read as stalled: the poll
    /// loop's "not leader" branch keeps the heartbeat fresh.
    #[tokio::test]
    async fn leadership_paused_consumer_is_not_reported_as_stalled() {
        let (manager, health_service) = manager_with(None);
        manager.set_leader(false);
        let c = TestConsumer::new("paused");
        let rc = register_and_start(&manager, c.clone()).await;
        rc.set_last_poll(Instant::now() - Duration::from_secs(120));

        tokio::time::sleep(Duration::from_millis(150)).await;

        assert_eq!(c.polls(), 0, "a non-leader must not poll");
        assert!(health_service.is_consumer_healthy("paused"));
        assert!(health_service.get_stalled_consumers().is_empty());

        manager.shutdown().await;
        assert!(
            !health_service.is_consumer_healthy("paused"),
            "a shut-down manager has no running consumers"
        );
    }

    /// Health reads liveness from the manager (Go: `ConsumerStats`): a
    /// registered, polling consumer is healthy; once the manager drops it,
    /// health says so — there is no second copy that a stale poll task's
    /// exit could erase or keep alive.
    #[tokio::test]
    async fn health_reads_consumer_liveness_from_the_manager() {
        let (manager, health_service) = manager_with(None);
        let c = TestConsumer::new("lifecycle");
        register_and_start(&manager, c.clone()).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(health_service.is_consumer_healthy("lifecycle"));
        assert!(manager.is_consumer_healthy("lifecycle").await);

        manager.shutdown().await;
        assert!(!health_service.is_consumer_healthy("lifecycle"));
    }

    /// Go stamps the heartbeat only on a SUCCESSFUL poll: a consumer that
    /// errors on every poll must go stale (and so be rebuilt), not look
    /// alive because each error still stamped it.
    #[tokio::test]
    async fn poll_errors_do_not_stamp_the_heartbeat() {
        let (manager, health_service) = manager_with(None);
        let c = TestConsumer::new("erroring");
        c.fail.store(true, Ordering::SeqCst);
        let rc = register_and_start(&manager, c.clone()).await;
        let stale = Instant::now() - Duration::from_secs(120);
        rc.set_last_poll(stale);

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(c.polls() > 0, "the loop must keep polling");
        assert!(
            rc.last_poll() <= stale + Duration::from_millis(1),
            "an errored poll must not refresh the heartbeat"
        );
        assert_eq!(
            health_service.get_stalled_consumers(),
            vec!["erroring".to_string()]
        );

        // Recovery: the next successful poll stamps it.
        c.fail.store(false, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(health_service.is_consumer_healthy("erroring"));
        manager.shutdown().await;
    }

    /// Adding a second instance under the same name replaces the first: the
    /// first stops being polled (it is detached, still resolvable for acks),
    /// so two instances never poll one queue.
    #[tokio::test]
    async fn second_instance_for_a_name_replaces_the_first() {
        let (manager, _hs) = manager_with(None);
        let a = TestConsumer::new("dup-queue");
        let b = TestConsumer::new("dup-queue");
        register_and_start(&manager, a.clone()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        register_and_start(&manager, b.clone()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        let a_polls = a.polls();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            a.polls() <= a_polls + 1,
            "the replaced instance must stop polling"
        );
        assert!(b.polls() > 0, "the new instance must poll");
        assert_eq!(manager.detaching_consumer_count(), 1);
        // Spawning the same instance twice is a no-op.
        let rc = manager.consumers.get("dup-queue").unwrap();
        let h = manager.spawn_consumer_poll_task(rc);
        tokio::time::timeout(Duration::from_millis(200), h)
            .await
            .expect("second spawn for a running instance is a no-op")
            .unwrap();
        manager.shutdown().await;
    }

    /// H9/H10 (Go `RestartStalledConsumers`): a consumer whose poll loop
    /// exited on `Stopped` is left registered with a stale heartbeat and
    /// rebuilt by the watchdog — build first, swap, then detach the old one.
    #[tokio::test]
    async fn watchdog_rebuilds_a_consumer_whose_loop_exited() {
        let replacement = TestConsumer::new("q1");
        let factory = Arc::new(QueueFactory {
            next: parking_lot::Mutex::new(vec![replacement.clone()]),
            fail: AtomicBool::new(false),
            builds: AtomicU32::new(0),
        });
        let (manager, _hs) = manager_with(Some(factory.clone()));
        let original = TestConsumer::new("q1");
        let rc =
            manager.new_running_consumer(original.clone(), "q1".into(), Some(queue_config("q1")));
        manager.consumers.insert(rc.clone());
        manager.spawn_consumer_poll_task(rc.clone());

        // The broker side of the consumer dies: poll returns Stopped.
        original.stopped.store(true, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let polls_after_exit = original.polls();
        rc.set_last_poll(Instant::now() - Duration::from_secs(120));

        let token = CancellationToken::new();
        let n = manager
            .restart_stalled_consumers(Duration::from_secs(60), Duration::ZERO, &token)
            .await;
        assert_eq!(n, 1);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(replacement.polls() > 0, "the replacement must be polling");
        assert_eq!(original.polls(), polls_after_exit);
        let current = manager.consumers.get("q1").unwrap();
        assert_ne!(current.generation, rc.generation);
        assert_eq!(manager.detaching_consumer_count(), 1);
        // Nothing in flight from the old one: the reaper retires it.
        assert_eq!(manager.retire_detached_consumers(), 1);
        manager.shutdown().await;
    }

    /// A failed rebuild leaves the existing entry in place (Go: build
    /// before retire), and the attempts escalate to CRITICAL after 10.
    #[tokio::test]
    async fn failed_rebuild_keeps_the_existing_consumer_and_escalates() {
        let factory = Arc::new(QueueFactory {
            next: parking_lot::Mutex::new(vec![]),
            fail: AtomicBool::new(true),
            builds: AtomicU32::new(0),
        });
        let (manager, _hs) = manager_with(Some(factory.clone()));
        let original = TestConsumer::new("q2");
        let rc =
            manager.new_running_consumer(original.clone(), "q2".into(), Some(queue_config("q2")));
        manager.consumers.insert(rc.clone());
        rc.poll_task_started.store(true, Ordering::SeqCst);

        let token = CancellationToken::new();
        for _ in 0..11 {
            rc.set_last_poll(Instant::now() - Duration::from_secs(120));
            let n = manager
                .restart_stalled_consumers(Duration::from_secs(60), Duration::ZERO, &token)
                .await;
            assert_eq!(n, 0);
        }
        assert_eq!(factory.builds.load(Ordering::SeqCst), 11);
        assert_eq!(
            manager.consumers.get("q2").unwrap().generation,
            rc.generation,
            "a failed rebuild must leave the existing consumer registered"
        );
        assert!(
            manager.warning_service.critical_count() > 0,
            "a consumer that cannot be rebuilt escalates to CRITICAL"
        );
        // restart_consumer (operator) has the same contract.
        assert!(!manager.restart_consumer("q2").await);
        assert_eq!(
            manager.consumers.get("q2").unwrap().generation,
            rc.generation
        );
    }

    /// H10: a consumer paused for capacity keeps a fresh heartbeat, so the
    /// watchdog never "restarts" it (a restart used to strand the buffers it
    /// was waiting on, and a new PgPool leaked on every restart).
    #[tokio::test]
    async fn watchdog_skips_consumers_with_a_fresh_heartbeat() {
        let factory = Arc::new(QueueFactory {
            next: parking_lot::Mutex::new(vec![]),
            fail: AtomicBool::new(false),
            builds: AtomicU32::new(0),
        });
        let (manager, _hs) = manager_with(Some(factory.clone()));
        let c = TestConsumer::new("q3");
        let rc = manager.new_running_consumer(c, "q3".into(), Some(queue_config("q3")));
        manager.consumers.insert(rc.clone());
        rc.poll_task_started.store(true, Ordering::SeqCst);
        rc.beat();
        let token = CancellationToken::new();
        let n = manager
            .restart_stalled_consumers(Duration::from_secs(60), Duration::ZERO, &token)
            .await;
        assert_eq!(n, 0);
        assert_eq!(factory.builds.load(Ordering::SeqCst), 0);
    }
}

#[cfg(test)]
mod g12_capacity_gate_tests {
    use super::*;
    use crate::mediator::Mediator;
    use async_trait::async_trait;
    use fc_common::{
        BatchMessage, MediationOutcome, MediationType, Message, MessageCallback, PoolConfig,
    };
    use fc_queue::QueueConsumer;

    /// Resolves every mediation instantly with a bare 200 — these tests
    /// care about queue-capacity crossing timing, never about mediation
    /// outcome.
    struct InstantSuccessMediator;

    #[async_trait]
    impl Mediator for InstantSuccessMediator {
        async fn mediate(&self, _message: &Message) -> MediationOutcome {
            MediationOutcome::success(200)
        }
    }

    struct NoOpCallback;

    #[async_trait]
    impl MessageCallback for NoOpCallback {
        async fn ack(&self) {}
        async fn nack(&self, _delay_seconds: Option<u32>) {}
    }

    fn dummy_batch_message(id: &str) -> BatchMessage {
        BatchMessage {
            message: Message {
                id: id.to_string(),
                pool_code: "TEST".to_string(),
                auth_token: None,
                signing_secret: None,
                mediation_type: MediationType::HTTP,
                mediation_target: "http://localhost/x".to_string(),
                message_group_id: None,
                high_priority: false,
                dispatch_mode: fc_common::DispatchMode::Immediate,
                dispatch_mode_specified: true,
            },
            receipt_handle: format!("rh-{id}"),
            broker_message_id: Some(format!("bh-{id}")),
            queue_identifier: "q".to_string(),
            batch_id: None,
            callback: Box::new(NoOpCallback),
        }
    }

    /// Build a manager with one real pool saturated to exactly its
    /// capacity (`max(concurrency * 20, 50)` — concurrency 1 → 50), so
    /// `has_pool_capacity()` reads false. The 50 `submit()` calls are
    /// awaited back-to-back with no other `.await` in between; on this
    /// current-thread test runtime nothing spawned by `submit()` gets a
    /// chance to run until the caller's task itself yields, so every
    /// admission lands before any worker starts — the saturation is exact
    /// and race-free.
    async fn manager_with_saturated_pool() -> Arc<QueueManager> {
        let manager = Arc::new(
            QueueManager::builder_with_shared_mediator(Arc::new(InstantSuccessMediator))
                .deferral_budget(1)
                .build(),
        );
        let pool_config = PoolConfig {
            code: "TEST".to_string(),
            concurrency: 1,
            rate_limit_per_minute: None,
        };
        let pool = manager
            .get_or_create_pool("TEST", Some(pool_config))
            .await
            .expect("pool creation must succeed");
        for i in 0..50 {
            pool.submit(dummy_batch_message(&format!("m{i}")))
                .await
                .expect("submit must succeed while under capacity");
        }
        assert!(
            !manager.has_pool_capacity(),
            "pool must read as full immediately after saturating it"
        );
        manager
    }

    /// A consumer whose last batch fed the saturated "TEST" pool and whose
    /// deferral budget (1) is spent — so it must park.
    fn parked_consumer(manager: &QueueManager) -> Arc<RunningConsumer> {
        struct Nop;
        #[async_trait]
        impl QueueConsumer for Nop {
            fn identifier(&self) -> &str {
                "parked"
            }
            async fn poll(&self, _: u32) -> fc_queue::Result<Vec<fc_common::QueuedMessage>> {
                Ok(vec![])
            }
            async fn ack(&self, _: &str) -> fc_queue::Result<()> {
                Ok(())
            }
            async fn nack(&self, _: &str, _: Option<u32>) -> fc_queue::Result<()> {
                Ok(())
            }
            async fn extend_visibility(&self, _: &str, _: u32) -> fc_queue::Result<()> {
                Ok(())
            }
            fn is_healthy(&self) -> bool {
                true
            }
            async fn stop(&self) {}
        }
        let rc = manager.new_running_consumer(Arc::new(Nop), "parked".into(), None);
        rc.set_dest_pools(vec!["TEST".to_string()]);
        rc.note_deferral(Instant::now() + Duration::from_secs(60));
        assert!(!manager.has_capacity_for(&rc));
        rc
    }

    /// Go's hasCapacityFor: with its destination pools full, a consumer
    /// keeps polling (deferring what doesn't fit) while its deferral budget
    /// lasts, parks once it is spent, and wakes when a deferral comes due.
    #[tokio::test]
    async fn capacity_gate_uses_destination_pools_and_the_deferral_budget() {
        let manager = manager_with_saturated_pool().await;
        let rc = manager.new_running_consumer(
            Arc::new(PartialThenEmptyConsumer {
                call_times: parking_lot::Mutex::new(vec![]),
                second_call: Arc::new(tokio::sync::Notify::new()),
            }),
            "q".into(),
            None,
        );
        rc.set_dest_pools(vec!["TEST".to_string()]);
        assert!(
            manager.has_capacity_for(&rc),
            "budget not spent: keep polling"
        );
        rc.note_deferral(Instant::now() + Duration::from_millis(150));
        assert!(
            !manager.has_capacity_for(&rc),
            "full and budget spent: park"
        );

        // Another pool with room elsewhere does not unpark it (Go judges by
        // this queue's destinations, not "any pool has room").
        manager.get_or_create_pool("OTHER", None).await.unwrap();
        assert!(!manager.has_capacity_for(&rc));

        // The deferral coming due frees budget and wakes the wait.
        let token = CancellationToken::new();
        let start = Instant::now();
        assert!(!wait_for_capacity_or_cancel(&manager, &rc, &token).await);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    /// G12: `wait_for_capacity_or_cancel` must resolve within 100ms of the
    /// pool's capacity-freed signal actually firing — not the fixed 2s
    /// sleep it replaced.
    ///
    /// The moment this function's first `.await` point is reached (inside
    /// the `tokio::select!`), the runtime is free to run the 50 previously
    /// -spawned worker tasks for the first time: the first one acquires
    /// the pool's single concurrency permit and decrements `queue_size`
    /// from 50 (== capacity) to 49 — the exact full→not-full crossing that
    /// fires `capacity_notify.notify_waiters()` via `QueueSlotReleaser`.
    /// That crossing is what resolves the wait — it happens on the very
    /// first scheduling opportunity after we start waiting, so a real
    /// (not simulated) capacity-freed signal resolves this well inside the
    /// 100ms bound.
    ///
    /// Mutant check: reverting `wait_for_capacity_or_cancel` to
    /// `sleep_or_cancel(&token, Duration::from_secs(2))` — confirmed by
    /// hand while implementing this fix — fails this bound at ~2s actual.
    #[tokio::test]
    async fn resolves_within_100ms_of_capacity_actually_returning() {
        let manager = manager_with_saturated_pool().await;
        let token = CancellationToken::new();

        let rc = parked_consumer(&manager);
        let start = Instant::now();
        let cancelled = wait_for_capacity_or_cancel(&manager, &rc, &token).await;
        let elapsed = start.elapsed();

        assert!(
            !cancelled,
            "must not report cancelled — the token was never cancelled"
        );
        assert!(
            elapsed < Duration::from_millis(100),
            "wait_for_capacity_or_cancel took {:?} — expected within 100ms \
             of the pool's capacity-freed signal",
            elapsed
        );
    }

    /// G12: an already-cancelled token must win immediately over an
    /// unresolved capacity wait — a shutdown landing while a consumer is
    /// parked on this gate must not add a multi-second stall (mirrors the
    /// Java pin `stopsPromptlyWhileParkedForCapacity`).
    #[tokio::test]
    async fn stops_promptly_when_cancelled_while_parked() {
        let manager = manager_with_saturated_pool().await;
        let token = CancellationToken::new();
        token.cancel();
        let rc = parked_consumer(&manager);

        let start = Instant::now();
        let cancelled = wait_for_capacity_or_cancel(&manager, &rc, &token).await;
        let elapsed = start.elapsed();

        assert!(
            cancelled,
            "a pre-cancelled token must be reported as cancelled"
        );
        assert!(
            elapsed < Duration::from_millis(100),
            "cancellation took {:?} to be observed — expected well under 100ms",
            elapsed
        );
    }

    /// Returns one partial batch (3 messages, well under the poll's
    /// request size of 10) on its first call, then empty batches — and
    /// records the wall-clock time of every call, so a test can measure
    /// the gap between the first and second poll.
    struct PartialThenEmptyConsumer {
        call_times: parking_lot::Mutex<Vec<Instant>>,
        second_call: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl QueueConsumer for PartialThenEmptyConsumer {
        fn identifier(&self) -> &str {
            "partial-then-empty"
        }
        async fn poll(&self, _: u32) -> fc_queue::Result<Vec<fc_common::QueuedMessage>> {
            let call_index = {
                let mut times = self.call_times.lock();
                times.push(Instant::now());
                times.len()
            };
            if call_index == 2 {
                self.second_call.notify_waiters();
            }
            if call_index == 1 {
                let batch: Vec<fc_common::QueuedMessage> = (0..3)
                    .map(|i| fc_common::QueuedMessage {
                        message: Message {
                            id: format!("partial-{i}"),
                            pool_code: String::new(),
                            auth_token: None,
                            signing_secret: None,
                            mediation_type: MediationType::HTTP,
                            mediation_target: "http://localhost/x".to_string(),
                            message_group_id: None,
                            high_priority: false,
                            dispatch_mode: fc_common::DispatchMode::Immediate,
                            dispatch_mode_specified: true,
                        },
                        receipt_handle: format!("rh-partial-{i}"),
                        broker_message_id: Some(format!("bh-partial-{i}")),
                        queue_identifier: "partial-then-empty".to_string(),
                    })
                    .collect();
                Ok(batch)
            } else {
                Ok(vec![])
            }
        }
        async fn ack(&self, _: &str) -> fc_queue::Result<()> {
            Ok(())
        }
        async fn nack(&self, _: &str, _: Option<u32>) -> fc_queue::Result<()> {
            Ok(())
        }
        async fn extend_visibility(&self, _: &str, _: u32) -> fc_queue::Result<()> {
            Ok(())
        }
        fn is_healthy(&self) -> bool {
            true
        }
        async fn stop(&self) {}
    }

    /// G12 (`batchesRepollImmediately` in the Java pin): a partial batch
    /// (fewer than the requested 10) must trigger an immediate re-poll,
    /// not the old fixed 500ms pause — a partial batch never meant "the
    /// queue is draining, slow down" and the pause held the loop back from
    /// work regardless of whether the broker already had the next batch
    /// ready.
    ///
    /// Every pool the 3 routed messages land in (the synthesised
    /// DEFAULT-POOL, capacity 400) stays far under capacity throughout, so
    /// this measures the partial-batch pacing in isolation from the
    /// capacity gate.
    ///
    /// Mutant check: reinstating `sleep_or_cancel(&token,
    /// Duration::from_millis(500))` after a partial batch — confirmed by
    /// hand while implementing this fix — pushes the gap past 500ms
    /// against this test's 100ms bound.
    #[tokio::test]
    async fn partial_batch_repolls_within_100ms() {
        let manager = Arc::new(
            QueueManager::builder_with_shared_mediator(Arc::new(InstantSuccessMediator)).build(),
        );
        let call_times = parking_lot::Mutex::new(Vec::new());
        let second_call = Arc::new(tokio::sync::Notify::new());
        let consumer = Arc::new(PartialThenEmptyConsumer {
            call_times,
            second_call: second_call.clone(),
        });

        let waiting = second_call.notified();
        manager.add_consumer(consumer.clone()).await;
        let rc = manager.consumers.get("partial-then-empty").unwrap();
        let handle = manager.spawn_consumer_poll_task(rc);

        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .expect("second poll() call must happen");

        let gap = {
            let times = consumer.call_times.lock();
            assert!(times.len() >= 2, "expected at least two poll() calls");
            times[1].duration_since(times[0])
        };

        assert!(
            gap < Duration::from_millis(100),
            "gap between poll() calls was {:?} — a partial batch must \
             re-poll immediately, not pause",
            gap
        );

        manager.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }
}
