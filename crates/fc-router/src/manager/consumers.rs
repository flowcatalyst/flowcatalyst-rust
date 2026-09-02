//! Consumer poll loop plus consumer-facing queries: liveness/health,
//! broker connectivity, queue metrics, and restart-by-replacement.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use fc_common::{WarningCategory, WarningSeverity};
use fc_queue::{QueueConsumer, QueueMetrics};

use super::QueueManager;

/// Sleep for `d`, but race it against `token`. Returns `true` if the token
/// was cancelled before `d` elapsed (caller should stop looping), `false`
/// if the sleep completed normally. Used for the pacing sleeps in the
/// consumer poll loop (backpressure, empty-poll, partial-batch, and error
/// pauses) so a shutdown that lands mid-pause exits promptly instead of
/// waiting out the rest of the sleep — across many consumers those pauses
/// would otherwise add real seconds to shutdown latency.
async fn sleep_or_cancel(token: &CancellationToken, d: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => false,
        _ = token.cancelled() => true,
    }
}

impl QueueManager {
    /// Spawn a poll task for a single consumer. Returns the JoinHandle.
    /// Called from both `start()` (initial consumers) and `sync_queue_consumers`
    /// (hot-added consumers).
    ///
    /// **Why `self: &Arc<Self>`**: the spawned task captures
    /// `manager = self.clone()` so it can call back into the manager for
    /// the lifetime of the consumer. That clone needs the receiver to be
    /// an `Arc`, not `&Self`.
    ///
    /// **Shutdown signalling.** `token` is a child of `self.shutdown`
    /// (`CancellationToken`), level-triggered: `QueueManager::shutdown()`
    /// cancelling the parent marks this child cancelled immediately, even
    /// if the token was created (i.e. this task was hot-added via
    /// `sync_queue_consumers`) *after* shutdown had already begun — unlike
    /// the old `broadcast` channel, there is no "subscribed too late to see
    /// the signal" window. Every pacing sleep in the loop below
    /// (backpressure, empty-poll, partial-batch, error) races the token via
    /// [`sleep_or_cancel`] so a shutdown mid-pause exits promptly instead of
    /// waiting out the full sleep. `route_batch` itself is deliberately
    /// **not** raced against cancellation — once a batch is accepted for
    /// processing it must run to completion so messages are acked/nacked
    /// rather than abandoned mid-poll.
    pub(super) fn spawn_consumer_poll_task(
        self: &Arc<Self>,
        consumer: Arc<dyn QueueConsumer + Send + Sync>,
    ) -> tokio::task::JoinHandle<()> {
        let manager = self.clone();
        let token = self.shutdown.child_token();

        tokio::spawn(async move {
            // R-36: this task is the SOURCE of consumer liveness — the
            // health service only ever reads a snapshot of what's recorded
            // here (`record_consumer_poll` below stamps last-seen; this
            // flag says "meant to be polling"), so there is exactly one
            // heartbeat, not a second copy that can drift from it. Flip on
            // before the loop starts, and back off once it exits for any
            // reason (shutdown, `QueueError::Stopped`, or a replacement
            // spawned by `restart_consumer`) so a dead poll task is never
            // reported as still running.
            if let Some(ref health_service) = manager.health_service {
                health_service.set_consumer_running(consumer.identifier(), true);
            }

            let mut last_poll_end = Instant::now();
            const STARVATION_THRESHOLD: Duration = Duration::from_secs(30);

            loop {
                // Detect thread/task starvation: warn if >30s between poll loops (Java: 30s)
                let loop_gap = last_poll_end.elapsed();
                if loop_gap > STARVATION_THRESHOLD {
                    warn!(
                        consumer = %consumer.identifier(),
                        gap_seconds = loop_gap.as_secs(),
                        "Task starvation detected: {}s between poll loops (threshold: {}s)",
                        loop_gap.as_secs(),
                        STARVATION_THRESHOLD.as_secs()
                    );
                }

                // R-26/R-34: not the leader (standby losing/regaining
                // leadership) — pause polling. In-flight deliveries and
                // buffered group work are untouched; this only stops *new*
                // messages from being pulled off the broker. Resumes as soon
                // as `manager.is_leader()` flips back via
                // `spawn_leadership_monitor`, no consumer rebuild needed.
                if !manager.is_leader() {
                    debug!(consumer = %consumer.identifier(), "Not leader — pausing poll");

                    if let Some(ref health_service) = manager.health_service {
                        health_service.record_consumer_poll(consumer.identifier());
                    }

                    if sleep_or_cancel(&token, Duration::from_secs(2)).await {
                        info!(consumer = %consumer.identifier(), "Consumer shutting down");
                        break;
                    }
                    continue;
                }

                // Backpressure: if all pools are full, wait instead of polling.
                // Prevents hot poll-defer loop that wastes SQS API calls.
                if !manager.has_pool_capacity() {
                    debug!(consumer = %consumer.identifier(), "All pools at capacity — pausing poll");

                    // A capacity wait is a deliberate pause, not a stall — record
                    // liveness before pausing so the lifecycle health monitor's
                    // "no poll recorded in 60s" check never misreads a run of
                    // full pools as a dead consumer and kills a perfectly good
                    // one (see `restart_consumer`'s doc comment for the history
                    // here). `record_consumer_poll` only stamps a last-seen
                    // `Instant` — it doesn't feed any poll-count metric — so
                    // calling it on a non-poll iteration doesn't inflate
                    // anything downstream.
                    if let Some(ref health_service) = manager.health_service {
                        health_service.record_consumer_poll(consumer.identifier());
                    }

                    if sleep_or_cancel(&token, Duration::from_secs(2)).await {
                        info!(consumer = %consumer.identifier(), "Consumer shutting down");
                        break;
                    }
                    continue;
                }

                tokio::select! {
                    _ = token.cancelled() => {
                        info!(consumer = %consumer.identifier(), "Consumer shutting down");
                        break;
                    }
                    result = consumer.poll(10) => {
                        last_poll_end = Instant::now();

                        // Record consumer poll with health service
                        if let Some(ref health_service) = manager.health_service {
                            health_service.record_consumer_poll(consumer.identifier());
                        }

                        match result {
                            Ok(messages) if messages.is_empty() => {
                                // No messages — SQS long poll already waited up to 20s.
                                // Brief pause before re-polling.
                                if sleep_or_cancel(&token, Duration::from_secs(1)).await {
                                    info!(consumer = %consumer.identifier(), "Consumer shutting down");
                                    break;
                                }
                            }
                            Ok(messages) => {
                                let count = messages.len();
                                if let Err(e) = manager.route_batch(messages, consumer.clone()).await {
                                    error!(error = %e, "Error routing batch");
                                }
                                // Full batch (10) — re-poll immediately, more messages likely waiting.
                                // Partial batch (< 10) — brief pause, queue is draining.
                                if count < 10
                                    && sleep_or_cancel(&token, Duration::from_millis(500)).await
                                {
                                    info!(consumer = %consumer.identifier(), "Consumer shutting down");
                                    break;
                                }
                            }
                            Err(fc_queue::QueueError::Stopped) => {
                                // The consumer was stopped (directly, or as
                                // part of `restart_consumer` swapping in a
                                // replacement) — `poll()` will keep returning
                                // `Stopped` forever, so looping on it would
                                // spin at 1s intervals reporting a dead
                                // consumer as "just erroring". Exit instead;
                                // whoever stopped this consumer is
                                // responsible for spawning any replacement.
                                info!(consumer = %consumer.identifier(), "consumer stopped — poll task exiting");
                                break;
                            }
                            Err(e) => {
                                error!(error = %e, consumer = %consumer.identifier(), "Error polling");
                                if sleep_or_cancel(&token, Duration::from_secs(1)).await {
                                    info!(consumer = %consumer.identifier(), "Consumer shutting down");
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // R-36: every exit path above falls out of the loop here — flip
            // the liveness flag off so a stopped/replaced consumer stops
            // reading as "meant to be polling" (see the doc comment above
            // the `true` set at task start).
            if let Some(ref health_service) = manager.health_service {
                health_service.set_consumer_running(consumer.identifier(), false);
            }
        })
    }

    /// Get list of all consumer identifiers
    pub async fn consumer_ids(&self) -> Vec<String> {
        self.consumers.read().await.keys().cloned().collect()
    }

    /// Check broker connectivity by verifying all consumers report healthy.
    /// Java: BrokerHealthService.checkBrokerConnectivity() pings the broker (SQS listQueues,
    /// NATS connection state, ActiveMQ test connection). Returns false if any consumer
    /// reports unhealthy, indicating the broker is unreachable.
    pub async fn check_broker_connectivity(&self) -> bool {
        // Clone the Arcs and drop the read guard before iterating — keeps
        // this consistent with every other consumers-read site in the file
        // (see item 4 of the manager shutdown/lock convention), even though
        // `is_healthy()` itself is synchronous today.
        let consumers: Vec<Arc<dyn QueueConsumer + Send + Sync>> = {
            let guard = self.consumers.read().await;
            if guard.is_empty() {
                return true; // No consumers configured — nothing to check
            }
            guard.values().cloned().collect()
        };
        for consumer in consumers {
            if !consumer.is_healthy() {
                warn!(
                    consumer = %consumer.identifier(),
                    "Broker connectivity check failed: consumer unhealthy"
                );
                return false;
            }
        }
        true
    }

    /// Restart a specific consumer by ID — actually replaces it.
    ///
    /// Stops the existing consumer, asks the configured [`super::ConsumerFactory`]
    /// to build a fresh one from the queue's last-known `QueueConfig`, swaps
    /// the replacement into `consumers`, and spawns a new poll task for it
    /// (see [`Self::spawn_consumer_poll_task`], which now exits promptly on
    /// `QueueError::Stopped` rather than looping on it forever). Returns
    /// `true` only if a live replacement ends up running.
    ///
    /// **Why `self: &Arc<Self>`**: it calls `spawn_consumer_poll_task`, which
    /// needs an `Arc` clone to hand to the spawned task.
    ///
    /// **No factory / no stored config → no-op, not a stop.** Building a
    /// replacement requires both a [`super::ConsumerFactory`] and a `QueueConfig`
    /// for this id. If either is missing, this deliberately does **not**
    /// stop the existing consumer — stopping it with nothing to replace it
    /// is exactly the bug this method used to have (the old body called
    /// `consumer.stop()` and returned `true` with a comment saying "a new
    /// poll loop will need to be started externally", which nothing ever
    /// did — the consumer just died in place). Instead it logs a warning,
    /// records a `ConsumerHealth` warning, and returns `false`.
    ///
    /// **Factory failure → self-healing via the next reload.** If
    /// `create_consumer` errors, the dead entry is removed from `consumers`
    /// but its `queue_configs` entry is deliberately left in place. The next
    /// `reload_config` → `sync_queue_consumers` pass computes "new" queues
    /// as config entries not already present in `consumers` (see that
    /// method's step (c)) — since this id is now missing from `consumers`
    /// but still present in the caller's config, it gets recreated through
    /// the ordinary hot-add path instead of being permanently stranded by a
    /// single transient factory failure.
    pub async fn restart_consumer(self: &Arc<Self>, consumer_id: &str) -> bool {
        // Serialise against `apply_config` / `reload_config`, which hold
        // `pool_configs.write()` for their whole duration (see that field's
        // doc comment — it doubles as the reload lock). Held across the
        // awaits below on purpose: without it, a health-triggered restart
        // racing a reload that removes this very queue could stop the old
        // consumer, then swap a fresh one into `consumers` *after* the
        // reload removed it — resurrecting a queue the config just dropped.
        // Nothing on the hot path takes this lock, so the only thing this
        // can wait on is an in-flight reload.
        let _reload_guard = self.pool_configs.read().await;

        // Brief read lock — clone the Arc and drop the guard before any
        // `.await` (same discipline as `sync_queue_consumers`).
        let old = {
            let guard = self.consumers.read().await;
            guard.get(consumer_id).cloned()
        };
        let Some(old) = old else {
            warn!(consumer_id = %consumer_id, "Consumer not found for restart");
            return false;
        };

        // Brief read lock — clone the stored QueueConfig, if any.
        let queue_config = {
            let guard = self.queue_configs.read().await;
            guard.get(consumer_id).cloned()
        };

        let (factory, queue_config) = match (self.consumer_factory.as_ref(), queue_config) {
            (Some(factory), Some(cfg)) => (factory, cfg),
            _ => {
                warn!(
                    consumer_id = %consumer_id,
                    "Cannot restart consumer: no consumer factory and/or stored queue \
                     config available to build a replacement — restart is unsupported \
                     without both, leaving the existing consumer running"
                );
                self.warning_service.add_warning(
                    WarningCategory::ConsumerHealth,
                    WarningSeverity::Warn,
                    format!(
                        "Restart requested for consumer [{}] but no consumer factory/config \
                         is available to build a replacement — restart unsupported here",
                        consumer_id
                    ),
                    "QueueManager".to_string(),
                );
                return false;
            }
        };

        info!(consumer_id = %consumer_id, "Restarting consumer: stopping old instance");
        // Stopping this makes its poll task observe `QueueError::Stopped` on
        // its next poll and exit on its own (see (b) in spawn_consumer_poll_task).
        old.stop().await;

        match factory.create_consumer(&queue_config).await {
            Ok(new_consumer) => {
                // Brief write lock — swap in the replacement.
                {
                    let mut guard = self.consumers.write().await;
                    guard.insert(consumer_id.to_string(), new_consumer.clone());
                }
                self.spawn_consumer_poll_task(new_consumer);
                info!(consumer_id = %consumer_id, "Consumer restarted with a fresh instance");
                true
            }
            Err(e) => {
                error!(
                    consumer_id = %consumer_id,
                    error = %e,
                    "Failed to create replacement consumer during restart"
                );
                self.warning_service.add_warning(
                    WarningCategory::ConsumerHealth,
                    WarningSeverity::Critical,
                    format!(
                        "Failed to create replacement consumer for [{}] during restart: {}",
                        consumer_id, e
                    ),
                    "QueueManager".to_string(),
                );
                // Remove the dead entry from `consumers` but leave
                // `queue_configs` alone — see the self-healing note above.
                let mut guard = self.consumers.write().await;
                guard.remove(consumer_id);
                false
            }
        }
    }

    /// Check if a consumer is healthy
    pub async fn is_consumer_healthy(&self, consumer_id: &str) -> bool {
        let consumers = self.consumers.read().await;
        consumers
            .get(consumer_id)
            .map(|c| c.is_healthy())
            .unwrap_or(false)
    }

    /// Get queue metrics from all consumers
    pub async fn get_queue_metrics(&self) -> Vec<QueueMetrics> {
        // Snapshot before awaiting `get_metrics()` per consumer — this can
        // be an SQS API call, and holding the read lock across it would
        // stall reloads / other readers for however long the whole sweep
        // takes.
        let consumers: Vec<(String, Arc<dyn QueueConsumer + Send + Sync>)> = {
            let guard = self.consumers.read().await;
            guard
                .iter()
                .map(|(id, c)| (id.clone(), c.clone()))
                .collect()
        };
        let mut metrics = Vec::with_capacity(consumers.len());

        for (id, consumer) in consumers {
            match consumer.get_metrics().await {
                Ok(Some(m)) => metrics.push(m),
                Ok(None) => {
                    debug!(consumer_id = %id, "Consumer does not support metrics");
                }
                Err(e) => {
                    warn!(consumer_id = %id, error = %e, "Failed to get queue metrics");
                }
            }
        }

        metrics
    }

    /// Get counter metrics only (no SQS API call — instant atomic reads)
    pub async fn get_queue_metrics_counters_only(&self) -> Vec<QueueMetrics> {
        let consumers = self.consumers.read().await;
        let mut metrics = Vec::with_capacity(consumers.len());

        for consumer in consumers.values() {
            if let Some(m) = consumer.get_counters() {
                metrics.push(m);
            }
        }

        metrics
    }
}

#[cfg(test)]
mod consumer_liveness_tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;
    use crate::warning::WarningService;
    use async_trait::async_trait;
    use fc_common::QueuedMessage;
    use fc_queue::Result as QueueResult;

    /// Never returns messages; `poll` just proves the task is alive.
    struct IdleConsumer {
        id: &'static str,
    }

    #[async_trait]
    impl QueueConsumer for IdleConsumer {
        fn identifier(&self) -> &str {
            self.id
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
        async fn stop(&self) {}
    }

    fn manager_with_health() -> (Arc<QueueManager>, Arc<crate::health::HealthService>) {
        let health_service = Arc::new(crate::health::HealthService::new(
            crate::health::HealthServiceConfig {
                consumer_stall_threshold_secs: 60,
                ..crate::health::HealthServiceConfig::default()
            },
            Arc::new(WarningService::default()),
        ));
        let manager = Arc::new(
            QueueManager::builder(HttpMediatorConfig::dev())
                .health_service(health_service.clone())
                .build(),
        );
        (manager, health_service)
    }

    /// A leadership-paused consumer must never read as stalled: the poll
    /// loop's "not leader" branch keeps calling `record_consumer_poll`
    /// (manager.rs, `spawn_consumer_poll_task`) specifically so this holds.
    #[tokio::test]
    async fn leadership_paused_consumer_is_not_reported_as_stalled() {
        let (manager, health_service) = manager_with_health();
        manager.set_leader(false);

        let consumer = Arc::new(IdleConsumer { id: "paused" });
        let handle = manager.spawn_consumer_poll_task(consumer);

        // Give the task a couple of "not leader" iterations to run.
        tokio::time::sleep(Duration::from_millis(150)).await;

        assert!(
            health_service.is_consumer_healthy("paused"),
            "a leadership-paused consumer is deliberately idle, not stalled"
        );
        assert!(
            !health_service.get_stalled_consumers().contains(&"paused".to_string()),
            "a leadership-paused consumer must not show up as stalled"
        );

        manager.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(
            !health_service.is_consumer_healthy("paused"),
            "set_consumer_running(false) must run once the poll task exits"
        );
    }

    /// The full lifecycle: `set_consumer_running` flips true when the poll
    /// task starts and false once it exits, bracketing the task exactly —
    /// this is what lets `is_consumer_healthy`/`get_stalled_consumers`
    /// reflect a real consumer instead of an empty map (R-36's "zero
    /// production call sites" gap).
    #[tokio::test]
    async fn consumer_running_flag_brackets_the_poll_tasks_lifetime() {
        let (manager, health_service) = manager_with_health();

        let consumer = Arc::new(IdleConsumer { id: "lifecycle" });
        let handle = manager.spawn_consumer_poll_task(consumer);

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(health_service.is_consumer_healthy("lifecycle"));

        manager.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;

        assert!(
            !health_service.is_consumer_healthy("lifecycle"),
            "an exited poll task must no longer read as running"
        );
    }
}
