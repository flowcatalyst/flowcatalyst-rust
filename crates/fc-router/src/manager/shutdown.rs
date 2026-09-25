//! Manager start/stop: `start()` (spawns every consumer poll task plus the
//! in-pipeline reaper) and `shutdown()` (graceful drain, bounded by a
//! timeout). See `docs/developers/router-concurrency-audit.md` for the
//! `CancellationToken`-based shutdown-signalling convention every spawned
//! task here follows.

use std::sync::Arc;
use std::time::Duration;

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

    /// Idle bound for an `in_pipeline` entry before this reaper considers
    /// it stuck — see [`Self::reap_stale_entries`] for the full rule (it
    /// ages on last-seen, with an absolute ceiling of 8× this).
    const IN_PIPELINE_TTL: Duration = Duration::from_secs(15 * 60);
    const IN_PIPELINE_REAPER_INTERVAL: Duration = Duration::from_secs(60);

    /// Defence-in-depth reaper for stuck `in_pipeline` entries, alongside
    /// the lifecycle manager's 5-minute sweep: same rule, finer cadence.
    /// Exits when the manager's shutdown token is cancelled.
    fn spawn_in_pipeline_reaper(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let token = self.shutdown.child_token();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Self::IN_PIPELINE_REAPER_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Skip the immediate first tick so we don't reap during startup.
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        self.reap_in_pipeline(Self::IN_PIPELINE_TTL);
                    }
                    _ = token.cancelled() => {
                        info!("In-pipeline reaper shutting down");
                        break;
                    }
                }
            }
        })
    }

    /// Graceful shutdown, in Go's order (`Server.Run`'s shutdown sequence):
    ///
    /// 1. **Stop polling** ([`Self::stop_polling`]): intake ends, but every
    ///    consumer stays alive, so work already routed can still be acked.
    /// 2. **Drain**: pools stop admitting, each group's *buffered* remainder
    ///    is released back to the broker (R-49 — shutdown never works
    ///    through a backlog against a slow target), and the in-hand
    ///    deliveries are awaited, bounded by the drain budget. When anything
    ///    was released, the stopped poll loops' last receives are awaited
    ///    too (same budget), so a released message caught by one is handed
    ///    back rather than left with a receive nobody reads.
    /// 3. **Tear down**: only now are the consumers stopped and the registry
    ///    emptied, and the pools shut down.
    ///
    /// Consumers used to be stopped first. For NATS that discarded the
    /// pending-ack map, so the acks of deliveries that then completed during
    /// the drain failed and the work was redelivered after `ack_wait` — long
    /// after the router's duplicate guard had expired.
    ///
    /// Uses the default 60s drain budget ([`Self::DEFAULT_DRAIN_TIMEOUT`]).
    /// Callers that want the budget to be operator/env-tunable (`bin/fc-router`
    /// honours `FC_DRAIN_TIMEOUT_SECONDS`) should call
    /// [`Self::shutdown_with_timeout`] directly instead.
    pub async fn shutdown(&self) {
        self.shutdown_with_timeout(Self::DEFAULT_DRAIN_TIMEOUT)
            .await
    }

    /// Default drain budget for [`Self::shutdown`] — 60s, matching Go's
    /// `DrainTimeout` default. `bin/fc-router`'s `FC_DRAIN_TIMEOUT_SECONDS`
    /// also defaults to this.
    pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);

    /// End every consumer's poll loop while leaving work already in the
    /// pipeline running and ackable (Go: `StopPolling`). Latched: the
    /// stalled-consumer watchdog will not respawn what this stopped, and a
    /// config reload will not start new consumers. Idempotent.
    pub fn stop_polling(&self) {
        self.polling_stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        for rc in self
            .consumers
            .active()
            .into_iter()
            .chain(self.consumers.detaching())
        {
            rc.stop_poll.cancel();
        }
        // Wake loops parked on the capacity gate so they see the cancel.
        self.capacity_notify().notify_waiters();
    }

    /// Whether [`Self::stop_polling`] has run.
    pub fn polling_stopped(&self) -> bool {
        self.polling_stopped
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Wait until every started poll loop (active or detaching) has exited,
    /// or `deadline`. Returns whether they all did.
    pub(crate) async fn await_poll_loops(&self, deadline: tokio::time::Instant) -> bool {
        let loops: Vec<_> = self
            .consumers
            .active()
            .into_iter()
            .chain(self.consumers.detaching())
            .filter(|rc| {
                rc.poll_task_started
                    .load(std::sync::atomic::Ordering::SeqCst)
            })
            .map(|rc| rc.poll_exited.clone())
            .collect();
        let all = future::join_all(loops.iter().map(|t| t.cancelled()));
        if tokio::time::timeout_at(deadline, all).await.is_ok() {
            return true;
        }
        warn!("Shutdown: a stopped poll loop was still receiving at the drain deadline");
        false
    }

    /// Same as [`Self::shutdown`], but with an explicit drain budget instead
    /// of the [`Self::DEFAULT_DRAIN_TIMEOUT`] default — the bounded wait for
    /// every pool's tracked (in-hand) tasks to finish before shutdown gives
    /// up and lets the process exit anyway.
    pub async fn shutdown_with_timeout(&self, drain_timeout: Duration) {
        info!(
            drain_timeout_secs = drain_timeout.as_secs(),
            in_flight = self.in_pipeline.len(),
            "QueueManager shutting down..."
        );

        // 1. Stop polling; consumers stay alive for the drain's acks.
        self.stop_polling();

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

        // 2. Drain all pools (non-blocking: flips `running`, closes the
        //    tracker).
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
        let deadline = tokio::time::Instant::now() + drain_timeout;
        let drained = tokio::time::timeout_at(
            deadline,
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

        // What was released is visible on the broker again, and a poll loop
        // stopped in step 1 may still have a receive in flight (see
        // `bounded_poll`): wait, within what is left of the budget, for
        // those receives to finish and hand back whatever they caught, so no
        // released message is left with a receive whose caller has exited.
        // Nothing released, nothing to wait for: an idle long poll is left
        // to the process exit, as before.
        if released_total > 0 {
            self.await_poll_loops(deadline).await;
        }

        // 3. Tear down. From here nothing more is routed or reconfigured.
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.shutdown.cancel();

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
