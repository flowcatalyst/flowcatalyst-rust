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

/// Park untimed on the manager's capacity-freed gate (G12,
/// `docs/go-mirror/2026-09-06-go-fix-list.md`) until either it fires or
/// `token` is cancelled. Returns `true` if cancelled first (caller should
/// stop looping).
///
/// **Race-free by construction.** The `Notified` future is created (which
/// captures the gate's current `notify_waiters()` call count) *before*
/// `has_pool_capacity` is checked, and `tokio::sync::Notify` guarantees a
/// `notify_waiters()` call is observed by a `Notified` as long as it
/// happens after that `Notified` was created — whether or not it has been
/// polled yet. So a pool that frees capacity between our last check and
/// this call (however small that window) cannot be missed: either the
/// re-check below already sees it, or the wait resolves immediately
/// because the notification landed after `notified()` was created but
/// before `.await` started. This is what closes the lost-wakeup race a
/// bare re-poll-on-a-timer can't.
async fn wait_for_capacity_or_cancel(manager: &QueueManager, token: &CancellationToken) -> bool {
    let notified = manager.capacity_notify().notified();
    if manager.has_pool_capacity() {
        // Freed between the caller's check and here — don't wait at all.
        return false;
    }
    tokio::select! {
        _ = notified => false,
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
        // Item 1 (router bench rig, 2026-09-07): refuse to spawn a second
        // poll task for a queue that already has one running — see the
        // `polling_consumer_ids` field doc for the full mechanism this
        // guards against (production mode's `initial_sync()` and
        // `QueueManager::start()` could each spawn one for the same
        // queue) and for why the map is generation-tagged rather than a
        // bare set (ABA safety against `restart_consumer`'s deliberate
        // preemption). `Entry::and_modify`/`or_insert` is a single atomic
        // check-and-set on this key's shard — no separate
        // contains()-then-insert() race window. The no-op path still
        // returns a `JoinHandle` (an already-finished trivial task) so
        // every call site keeps working with the same return type.
        let generation = self
            .next_poll_task_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let mut already_running = false;
        self.polling_consumer_ids
            .entry(consumer.identifier().to_string())
            .and_modify(|_| already_running = true)
            .or_insert(generation);
        if already_running {
            warn!(
                consumer = %consumer.identifier(),
                "Refusing to spawn a second poll task for this queue — one is already running"
            );
            return tokio::spawn(async {});
        }

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
            // G12: gates the capacity pause's warning/resume log to once
            // per transition rather than once per loop iteration.
            let mut capacity_paused = false;

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

                // Backpressure (G12): if all pools are full, park untimed on
                // the manager's capacity-freed gate instead of polling —
                // never a fixed sleep, which starves the workers on a fast
                // broker once the pools drain faster than the sleep lets
                // the loop notice (docs/go-mirror/2026-09-06-go-fix-list.md
                // G12). `capacity_paused` gates the warning/resume log to
                // once per transition, not once per loop iteration.
                if !manager.has_pool_capacity() {
                    if !capacity_paused {
                        capacity_paused = true;
                        warn!(consumer = %consumer.identifier(), "All pools at capacity — pausing poll");
                        manager.warning_service.add_warning(
                            WarningCategory::QueueHealth,
                            WarningSeverity::Warn,
                            format!(
                                "Consumer [{}] paused — all pools at capacity",
                                consumer.identifier()
                            ),
                            "ConsumerLoop".to_string(),
                        );
                    } else {
                        debug!(consumer = %consumer.identifier(), "All pools at capacity — still paused");
                    }

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

                    if wait_for_capacity_or_cancel(&manager, &token).await {
                        info!(consumer = %consumer.identifier(), "Consumer shutting down");
                        break;
                    }
                    continue;
                } else if capacity_paused {
                    capacity_paused = false;
                    info!(consumer = %consumer.identifier(), "Capacity returned; resuming poll");
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
                                if let Err(e) = manager.route_batch(messages, consumer.clone()).await {
                                    error!(error = %e, "Error routing batch");
                                }
                                // G12: a partial batch re-polls immediately,
                                // exactly like a full one — it never means
                                // "the queue is draining, slow down"; the
                                // broker may already have the next batch
                                // ready, and a 500ms pause here holds the
                                // loop back from work regardless of whether
                                // that's true (owner ruling 2026-09-07, same
                                // as the capacity-wait fix above).
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
            // Mirror image of the guard at spawn time — release this id so
            // a legitimate later spawn (e.g. `restart_consumer`'s
            // replacement, after `old.stop()`) is not itself refused as a
            // false-positive duplicate. Generation-checked (`remove_if`,
            // not a bare `remove`): if `restart_consumer` already
            // preempted this id (removed it and let a replacement spawn
            // under a fresh generation) before this task noticed
            // `Stopped` and got here, the current entry's generation
            // no longer matches ours — leave the replacement's
            // registration alone.
            manager
                .polling_consumer_ids
                .remove_if(consumer.identifier(), |_, g| *g == generation);
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
        let old_identifier = old.identifier().to_string();

        // Item 1: deliberate preemption of `spawn_consumer_poll_task`'s
        // duplicate guard. `old.stop()` only flips a flag — the old poll
        // task notices `Stopped` and exits (removing its own guard entry)
        // on its *own* next loop iteration, which for some backends can
        // be seconds away, not synchronously here. Without this explicit
        // removal, spawning the replacement below could be refused as a
        // false-positive "already running" duplicate of the very consumer
        // this call just stopped. Safe against the old task's own delayed
        // cleanup clobbering the replacement's registration: that cleanup
        // is generation-checked (`remove_if` in `spawn_consumer_poll_task`)
        // and the replacement always spawns under a fresh generation.
        self.polling_consumer_ids.remove(&old_identifier);

        match factory.create_consumer(&queue_config).await {
            Ok(new_consumer) => {
                // Brief write lock — swap in the replacement. Also keeps
                // the identifier-keyed resolution index (G10) in lockstep:
                // the replacement's own `identifier()` may equal the old
                // one (same broker-native identity, e.g. NATS's
                // `<stream>/<consumer>` reprovisioned unchanged) or differ,
                // so the old identifier key is dropped explicitly rather
                // than relying on the new insert to overwrite it.
                {
                    let mut guard = self.consumers.write().await;
                    guard.insert(consumer_id.to_string(), new_consumer.clone());
                    let mut by_id = self.consumers_by_id.write().await;
                    by_id.remove(&old_identifier);
                    by_id.insert(new_consumer.identifier().to_string(), new_consumer.clone());
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
                // Remove the dead entry from `consumers` (and its
                // identifier-keyed mirror — the old consumer is stopped,
                // it must not stay resolvable) but leave `queue_configs`
                // alone — see the self-healing note above.
                let mut guard = self.consumers.write().await;
                guard.remove(consumer_id);
                self.consumers_by_id.write().await.remove(&old_identifier);
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

    /// Never returns messages; counts every `poll()` call so a test can
    /// tell whether this specific consumer instance was ever actually
    /// polled — as opposed to merely being registered somewhere.
    struct CountingIdleConsumer {
        id: &'static str,
        poll_calls: Arc<std::sync::atomic::AtomicU32>,
    }

    #[async_trait]
    impl QueueConsumer for CountingIdleConsumer {
        fn identifier(&self) -> &str {
            self.id
        }
        async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
            self.poll_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // A real broker poll never returns instantly forever — avoid
            // spinning this test's runtime hot while still polling
            // repeatedly enough to prove liveness within the test's own
            // short window.
            tokio::time::sleep(Duration::from_millis(10)).await;
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

    /// Item 1 (router bench rig, 2026-09-07): reproduces the exact shape
    /// of the bug at the unit level — two *different* `QueueConsumer`
    /// instances that happen to share an `identifier()` (exactly what
    /// happened when `main.rs` built a second, independent `PostgresQueue`
    /// for a queue `sync_queue_consumers` had already spawned a poller
    /// for) must never both end up polling. The second
    /// `spawn_consumer_poll_task` call for that id must be a no-op.
    ///
    /// Pins: (1) the second call's `JoinHandle` finishes promptly (it's a
    /// trivial already-done task, not a second live poller) instead of
    /// running indefinitely like a real poll loop would; (2) the second
    /// consumer's `poll()` is never called, ever — only the first
    /// consumer's counter moves.
    ///
    /// Mutant check (removed the `polling_consumer_ids.insert(...)` guard
    /// — i.e. `spawn_consumer_poll_task` unconditionally spawns, the
    /// pre-fix behaviour — confirmed by hand while implementing this fix,
    /// then restored): assertion (2) fails immediately — `counter_b`
    /// climbs above 0 just like `counter_a`, proving a second poller
    /// really did run.
    #[tokio::test]
    async fn spawning_a_second_poll_task_for_the_same_id_is_a_no_op() {
        let (manager, _health_service) = manager_with_health();

        let counter_a = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counter_b = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let consumer_a = Arc::new(CountingIdleConsumer {
            id: "dup-queue",
            poll_calls: counter_a.clone(),
        });
        let consumer_b = Arc::new(CountingIdleConsumer {
            id: "dup-queue",
            poll_calls: counter_b.clone(),
        });

        let handle_a = manager.spawn_consumer_poll_task(consumer_a);
        let handle_b = manager.spawn_consumer_poll_task(consumer_b);

        // The second, duplicate spawn must be a no-op task that finishes
        // essentially immediately — a real poll loop never returns on its
        // own.
        tokio::time::timeout(Duration::from_millis(200), handle_b)
            .await
            .expect("the second spawn_consumer_poll_task call for an id \
                     already being polled must return promptly, not run \
                     forever like a real poller")
            .expect("the no-op task must not panic");

        // Give consumer A's real poll loop several iterations.
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(
            counter_a.load(std::sync::atomic::Ordering::SeqCst) > 0,
            "the first (legitimate) spawn's consumer must actually be polling"
        );
        assert_eq!(
            counter_b.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the second, duplicate spawn's consumer must never be polled — \
             exactly the router-bench-rig race (two independent pollers \
             for the same queue_name)"
        );

        manager.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), handle_a).await;
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
            QueueManager::builder_with_shared_mediator(Arc::new(InstantSuccessMediator)).build(),
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

        let start = Instant::now();
        let cancelled = wait_for_capacity_or_cancel(&manager, &token).await;
        let elapsed = start.elapsed();

        assert!(!cancelled, "must not report cancelled — the token was never cancelled");
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

        let start = Instant::now();
        let cancelled = wait_for_capacity_or_cancel(&manager, &token).await;
        let elapsed = start.elapsed();

        assert!(cancelled, "a pre-cancelled token must be reported as cancelled");
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
        let handle = manager.spawn_consumer_poll_task(consumer.clone());

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
