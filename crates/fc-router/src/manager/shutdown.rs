//! Manager start/stop: `start()` (spawns every consumer poll task plus the
//! in-pipeline reaper) and `shutdown()` (graceful drain, bounded by a
//! timeout). See `docs/developers/router-concurrency-audit.md` for the
//! `CancellationToken`-based shutdown-signalling convention every spawned
//! task here follows.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future;
use tracing::{debug, info, warn};

use super::QueueManager;
use crate::pool::ProcessPool;
use crate::Result;

impl QueueManager {
    /// Start the queue manager and all consumers.
    ///
    /// **Why `self: Arc<Self>`** (owned, not borrowed): the body fans out
    /// to `spawn_consumer_poll_task(&self)` and
    /// `self.clone().spawn_in_pipeline_reaper()`, each of which moves an
    /// Arc clone into a spawned task that outlives this function. Taking
    /// owned `Arc<Self>` means the caller's last reference is consumed
    /// at the call site; the spawned tasks become the new owners.
    pub async fn start(self: Arc<Self>) -> Result<()> {
        let consumers = self.consumers.active();
        info!(consumers = consumers.len(), "Starting QueueManager");

        let mut handles = Vec::new();
        for rc in consumers {
            // A consumer created by config sync (`reload_config`) already
            // has its poll loop; only consumers registered directly
            // (`add_consumer`) are started here. `spawn_consumer_poll_task`
            // is a no-op for an instance whose loop is running, so this
            // check only keeps the start-up path quiet.
            if rc
                .poll_task_started
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                debug!(
                    consumer = %rc.identifier(),
                    "start(): consumer already has a poll task running (started via config sync)"
                );
                continue;
            }
            handles.push(self.spawn_consumer_poll_task(rc));
        }

        // Item 5: every configured consumer now has a poll task spawned —
        // flip the readiness gate `api::health::health_handler` reads.
        self.consumers_started
            .store(true, std::sync::atomic::Ordering::SeqCst);

        // Defence-in-depth: reaper for stuck `in_pipeline` entries.
        handles.push(self.clone().spawn_in_pipeline_reaper());

        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

    /// TTL of an `in_pipeline` entry before the reaper considers it stuck.
    /// Production processing should never take this long; legitimate
    /// long-running work should have its visibility timeout extended.
    const IN_PIPELINE_TTL: Duration = Duration::from_secs(15 * 60);
    const IN_PIPELINE_REAPER_INTERVAL: Duration = Duration::from_secs(60);

    /// Spawn a periodic task that scans `in_pipeline` and removes any entry
    /// older than `IN_PIPELINE_TTL`. This is a safety net for cases where a
    /// callback is dropped without firing AND its `Drop` impl somehow
    /// doesn't run (e.g. forgotten ownership in a future map). Without this,
    /// SQS would keep redelivering and `filter_duplicates` would silently
    /// swallow each redelivery as a duplicate, leaving thousands of
    /// messages stuck on the queue.
    /// **Why `self: Arc<Self>`** (owned): the spawned reaper task closes
    /// over `in_pipeline` and `app_index` (Arc clones extracted from
    /// `self`) and lives until shutdown — the receiver's Arc is consumed
    /// by the call site and the task becomes the new owner of the
    /// captured references.
    /// **Shutdown signalling.** `token` is a child of `self.shutdown`
    /// (`CancellationToken`), level-triggered: cancellation is observed
    /// immediately by `token.cancelled()` even if this task were somehow
    /// spawned after `shutdown()` had already run — there is no
    /// subscribe-before-signal race like the old `broadcast` channel had.
    fn spawn_in_pipeline_reaper(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let token = self.shutdown.child_token();
        let in_pipeline = self.in_pipeline.clone();
        let app_index = self.app_message_to_pipeline_key.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Self::IN_PIPELINE_REAPER_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Skip the immediate first tick so we don't reap during startup.
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let now = Instant::now();

                        // Snapshot candidates first (don't mutate while iterating).
                        // Each candidate captures the full context we need to log
                        // — once we yank the entry from the map, this is gone.
                        struct Candidate {
                            pipeline_key: String,
                            app_message_id: String,
                            broker_message_id: Option<String>,
                            queue_identifier: String,
                            pool_code: String,
                            message_group_id: Option<String>,
                            age_secs: u64,
                        }
                        let mut candidates: Vec<Candidate> = Vec::new();
                        for entry in in_pipeline.iter() {
                            let age = now.duration_since(entry.value().started_at);
                            if age > Self::IN_PIPELINE_TTL {
                                candidates.push(Candidate {
                                    pipeline_key: entry.key().clone(),
                                    app_message_id: entry.value().message_id.clone(),
                                    broker_message_id: entry.value().broker_message_id.clone(),
                                    queue_identifier: entry.value().queue_identifier.clone(),
                                    pool_code: entry.value().pool_code.clone(),
                                    message_group_id: entry.value().message_group_id.clone(),
                                    age_secs: age.as_secs(),
                                });
                            }
                        }

                        for c in &candidates {
                            in_pipeline.remove(&c.pipeline_key);
                            app_index.remove(&c.app_message_id);
                            warn!(
                                pipeline_key = %c.pipeline_key,
                                app_message_id = %c.app_message_id,
                                broker_message_id = ?c.broker_message_id,
                                queue = %c.queue_identifier,
                                pool_code = %c.pool_code,
                                message_group_id = ?c.message_group_id,
                                age_secs = c.age_secs,
                                ttl_secs = Self::IN_PIPELINE_TTL.as_secs(),
                                "Reaped stuck in_pipeline entry — SQS redelivery will retry"
                            );
                        }

                        if !candidates.is_empty() {
                            warn!(
                                count = candidates.len(),
                                ttl_secs = Self::IN_PIPELINE_TTL.as_secs(),
                                "in_pipeline reaper cycle: {} entries expired",
                                candidates.len()
                            );
                        }
                    }
                    _ = token.cancelled() => {
                        info!("In-pipeline reaper shutting down");
                        break;
                    }
                }
            }
        })
    }

    /// Graceful shutdown.
    ///
    /// Cancels [`Self::shutdown_token`]'s parent (level-triggered — every
    /// consumer poll task and background watcher observes it immediately,
    /// even one spawned after this call started), stops consumers, drains
    /// every pool (active and already-draining), and waits — bounded by a
    /// 60s drain budget — for tracked pool work to finish via
    /// [`ProcessPool::wait_drained`], instead of polling a "drained?" flag
    /// on a fixed sleep interval.
    ///
    /// **R-49 (ruled 2026-09-02):** the intended semantic is narrower than
    /// what this currently does. A worker should finish only the message
    /// it's in the middle of (its in-hand delivery), then immediately
    /// release the rest of its group's *buffered* backlog back to the
    /// broker (NACK, undelivered) rather than continuing to drain it —
    /// draining the whole backlog against a slow target could take
    /// arbitrarily long, past any drain budget, right up to the point the
    /// orchestrator's SIGKILL severs everything mid-flight anyway.
    ///
    /// R-49 (ledger): shutdown finishes the message currently in the air
    /// and RELEASES each group's buffered remainder back to the broker —
    /// it never drains a whole backlog against a slow target, and never
    /// abandons buffered work to visibility-timeout limbo. The sequencing:
    /// `pool.drain()` stops admission, then `pool.release_remainder()`
    /// empties every group buffer with explicit NACKs, so a drain task
    /// mid-loop finds its queue empty after the in-hand task and exits.
    /// The bounded `wait_drained` below therefore only ever waits on
    /// in-hand deliveries, not backlogs.
    ///
    /// Uses the default 60s drain budget ([`Self::DEFAULT_DRAIN_TIMEOUT`]).
    /// Callers that want the budget to be operator/env-tunable (`bin/fc-router`
    /// honours `FC_DRAIN_TIMEOUT_SECONDS`) should call
    /// [`Self::shutdown_with_timeout`] directly instead.
    pub async fn shutdown(&self) {
        self.shutdown_with_timeout(Self::DEFAULT_DRAIN_TIMEOUT)
            .await
    }

    /// Default drain budget for [`Self::shutdown`] — 60s, matching the
    /// value this crate has always used. `bin/fc-router`'s
    /// `FC_DRAIN_TIMEOUT_SECONDS` also defaults to this.
    pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);

    /// Same as [`Self::shutdown`], but with an explicit drain budget instead
    /// of the [`Self::DEFAULT_DRAIN_TIMEOUT`] default — the bounded wait for
    /// every pool's tracked (in-hand) tasks to finish before shutdown gives
    /// up and lets the process exit anyway.
    pub async fn shutdown_with_timeout(&self, drain_timeout: Duration) {
        info!(
            drain_timeout_secs = drain_timeout.as_secs(),
            "QueueManager shutting down..."
        );
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);

        // Signal all consumer loops / background watchers to stop.
        self.shutdown.cancel();

        // Stop all consumers. Clone the Arcs and drop the read guard before
        // awaiting `stop()` on each — never hold `consumers` across an
        // `.await` (see the field's doc comment / item 4 of the manager
        // shutdown convention).
        // Go's Shutdown: every consumer — active or still detaching — is
        // stopped and the registry emptied.
        for rc in self
            .consumers
            .drain_active()
            .into_iter()
            .chain(self.consumers.drain_detaching())
        {
            rc.stop_poll.cancel();
            rc.consumer.stop().await;
        }

        // Collect every pool — active, already-draining, AND any
        // predecessor orphaned into `orphaned_draining` by a later Active
        // insert under the same code — before awaiting anything. DashMap
        // `Ref`s must never be held across an `.await`; collecting the
        // `Arc<ProcessPool>` clones into a `Vec` first and dropping the
        // iterator does that. Including `orphaned_draining` here is what
        // makes the router specification's shutdown MUST (release every
        // pool's buffered remainder — `docs/router-specification.md` §5.3)
        // hold for a displaced predecessor too, not just the pools still
        // reachable through `pools`.
        let mut pools: Vec<Arc<ProcessPool>> =
            self.pools.iter().map(|e| e.value().pool.clone()).collect();
        pools.extend(self.orphaned_draining.lock().iter().cloned());

        // Drain all pools (non-blocking: flips `running`, closes the tracker).
        for pool in &pools {
            pool.drain().await;
        }

        // R-49: release each group's buffered remainder back to the broker
        // (explicit NACKs). After this, the only work left is the in-hand
        // message inside each live drain/immediate task — which is what the
        // bounded wait below is for.
        let mut released_total = 0usize;
        for pool in &pools {
            released_total += pool.release_remainder().await;
        }
        if released_total > 0 {
            info!(
                released = released_total,
                "Shutdown released buffered messages back to the broker"
            );
        }

        // Wait for every pool's tracked tasks to finish, bounded by a timeout.
        let drained = tokio::time::timeout(
            drain_timeout,
            future::join_all(pools.iter().map(|p| p.wait_drained())),
        )
        .await;

        if drained.is_err() {
            let still_busy = pools.iter().filter(|p| p.tracked_tasks() > 0).count();
            warn!(
                still_busy_pools = still_busy,
                total_pools = pools.len(),
                timeout_secs = drain_timeout.as_secs(),
                "Shutdown drain timed out — some pools still had in-flight work"
            );
        }

        // Log any remaining in-flight messages (they'll be NACKed when tasks are dropped)
        let remaining = self.in_pipeline.len();
        if remaining > 0 {
            warn!(
                remaining = remaining,
                "Remaining in-flight messages will be NACKed"
            );
            self.in_pipeline.clear();
            self.app_message_to_pipeline_key.clear();
        }

        // Shutdown pools (idempotent alongside the `drain()` above — same
        // non-blocking flip-and-close semantics; kept for call-site clarity).
        for pool in &pools {
            pool.shutdown().await;
        }

        // R-59: clear synth-pool idle tracking alongside the pools
        // themselves — every entry it could point to is now gone.
        self.synth_pools.clear();

        // Every orphaned predecessor was just drained/released/shut down
        // above along with everything else in `pools` — nothing left to
        // track.
        self.orphaned_draining.lock().clear();

        info!("QueueManager shutdown complete");
    }
}

#[cfg(test)]
mod start_double_spawn_tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;
    use async_trait::async_trait;
    use fc_common::{PoolConfig, RouterConfig};
    use fc_queue::QueueConsumer;
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

    /// Blocks `poll()` forever (until `stop()`) on an un-fired `Notify`,
    /// counting how many times it's actually called — the real poll loop
    /// this spawns settles into `tokio::select!` awaiting that `poll()`
    /// (or cancellation) on its very first iteration, so `poll_calls()`
    /// reads exactly 1 for as long as the loop stays parked there.
    struct BlockingPollConsumer {
        id: &'static str,
        poll_calls: AtomicU32,
        block: tokio::sync::Notify,
    }

    impl BlockingPollConsumer {
        fn new(id: &'static str) -> Self {
            Self {
                id,
                poll_calls: AtomicU32::new(0),
                block: tokio::sync::Notify::new(),
            }
        }

        fn poll_calls(&self) -> u32 {
            self.poll_calls.load(AtomicOrdering::SeqCst)
        }
    }

    #[async_trait]
    impl QueueConsumer for BlockingPollConsumer {
        fn identifier(&self) -> &str {
            self.id
        }
        async fn poll(&self, _: u32) -> fc_queue::Result<Vec<fc_common::QueuedMessage>> {
            self.poll_calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.block.notified().await;
            Err(fc_queue::QueueError::Stopped)
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
        async fn stop(&self) {
            self.block.notify_waiters();
        }
    }

    /// Item 4 (router bench rig, 2026-09-07): `start()` must not start a
    /// second poll loop for a consumer whose loop is already running (the
    /// config-sync path spawns loops before `start()` runs). The consumer's
    /// first loop is parked inside `poll()`, so a second loop would show up
    /// as a second `poll()` call.
    #[tokio::test]
    async fn start_does_not_attempt_a_second_spawn_for_an_already_running_consumer() {
        let manager = Arc::new(QueueManager::builder(HttpMediatorConfig::dev()).build());
        manager
            .apply_config(RouterConfig {
                processing_pools: vec![PoolConfig {
                    code: "DEFAULT-POOL".to_string(),
                    concurrency: 5,
                    rate_limit_per_minute: None,
                }],
                queues: vec![],
            })
            .await
            .unwrap();

        let counter = Arc::new(BlockingPollConsumer::new("dup"));
        let consumer: Arc<dyn QueueConsumer> = counter.clone();
        manager.add_consumer(consumer).await;
        let rc = manager.consumers.get("dup").expect("registered");
        let poll_task = manager.spawn_consumer_poll_task(rc);

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(counter.poll_calls(), 1);

        let start_task = tokio::spawn(manager.clone().start());
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(
            counter.poll_calls(),
            1,
            "start() must not start a second poll loop for a consumer whose loop is running"
        );

        manager.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), poll_task).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), start_task).await;
    }

    /// Regression guard: a consumer that was only ever registered (never
    /// spawned — the dev-mode `add_consumer` path) must still get a real
    /// poll task from `start()`.
    #[tokio::test]
    async fn start_still_spawns_a_real_poll_task_for_a_never_started_consumer() {
        let manager = Arc::new(QueueManager::builder(HttpMediatorConfig::dev()).build());
        manager
            .apply_config(RouterConfig {
                processing_pools: vec![PoolConfig {
                    code: "DEFAULT-POOL".to_string(),
                    concurrency: 5,
                    rate_limit_per_minute: None,
                }],
                queues: vec![],
            })
            .await
            .unwrap();

        let counter = Arc::new(BlockingPollConsumer::new("never-started"));
        let consumer: Arc<dyn QueueConsumer> = counter.clone();
        manager.add_consumer(consumer).await;

        let start_task = tokio::spawn(manager.clone().start());
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(
            counter.poll_calls() > 0,
            "the spawned poll task must actually call poll() on the consumer"
        );

        manager.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), start_task).await;
    }
}
