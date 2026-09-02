//! R-59: per-client synthesised fallback pool tests.
//!
//! A message whose `pool_code` ends `-DEFAULT-POOL` (and isn't exactly the
//! global `DEFAULT-POOL`) is a per-client fallback: if no such pool exists,
//! `QueueManager` synthesises one on demand with default settings (no
//! ROUTING warning — unlike a genuinely unknown code), tracks its last-routed
//! time, and evicts it once idle past a TTL on the lifecycle reaper's
//! `evict_idle_synth_pools` sweep, draining buffered work rather than
//! dropping it (R-26/R-49). A config reload that later defines the same code
//! takes ownership: the pool updates in place and leaves synthesis tracking.
//! Mirrors Go's `internal/router/manager_synth_pool_evict_test.go`
//! (`ensureFallbackPool`/`trackSynthPool`/`touchSynthPool`/
//! `forgetSynthPool`/`EvictIdleSynthPools`).

use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fc_common::{
    MediationOutcome, MediationType, Message, PoolConfig, QueuedMessage, RouterConfig,
    WarningCategory,
};
use fc_queue::{QueueConsumer, QueueError};
use fc_router::{Mediator, QueueManager};

/// Mock mediator: records every delivered message id, always succeeds.
struct MockMediator {
    call_count: AtomicU32,
    processed_ids: parking_lot::Mutex<Vec<String>>,
}

impl MockMediator {
    fn new() -> Self {
        Self {
            call_count: AtomicU32::new(0),
            processed_ids: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }

    fn processed_ids(&self) -> Vec<String> {
        self.processed_ids.lock().clone()
    }
}

#[async_trait]
impl Mediator for MockMediator {
    async fn mediate(&self, message: &Message) -> MediationOutcome {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.processed_ids.lock().push(message.id.clone());
        tokio::time::sleep(Duration::from_millis(5)).await;
        MediationOutcome::success(200)
    }
}

/// Mediator that blocks the FIRST call on a `Notify` (after signalling
/// `started`) so a test can deterministically observe "one message in
/// flight, the rest of its ordered group still buffered" before releasing
/// it — used by the drain-preserves-buffered-work test.
struct GatedMediator {
    call_count: AtomicU32,
    processed_ids: parking_lot::Mutex<Vec<String>>,
    hold_first: AtomicBool,
    started: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl GatedMediator {
    fn new(started: Arc<tokio::sync::Notify>, release: Arc<tokio::sync::Notify>) -> Self {
        Self {
            call_count: AtomicU32::new(0),
            processed_ids: parking_lot::Mutex::new(Vec::new()),
            hold_first: AtomicBool::new(true),
            started,
            release,
        }
    }

    fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }

    fn processed_ids(&self) -> Vec<String> {
        self.processed_ids.lock().clone()
    }
}

#[async_trait]
impl Mediator for GatedMediator {
    async fn mediate(&self, message: &Message) -> MediationOutcome {
        if self.hold_first.swap(false, Ordering::SeqCst) {
            self.started.notify_one();
            self.release.notified().await;
        }
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.processed_ids.lock().push(message.id.clone());
        MediationOutcome::success(200)
    }
}

/// Mock queue consumer: serves a fixed message list, records acks/nacks.
struct MockQueueConsumer {
    identifier: String,
    messages: parking_lot::Mutex<Vec<QueuedMessage>>,
    acked: parking_lot::Mutex<Vec<String>>,
    nacked: parking_lot::Mutex<Vec<(String, Option<u32>)>>,
    running: AtomicBool,
}

impl MockQueueConsumer {
    fn with_messages(identifier: &str, messages: Vec<QueuedMessage>) -> Self {
        Self {
            identifier: identifier.to_string(),
            messages: parking_lot::Mutex::new(messages),
            acked: parking_lot::Mutex::new(Vec::new()),
            nacked: parking_lot::Mutex::new(Vec::new()),
            running: AtomicBool::new(true),
        }
    }

    fn acked(&self) -> Vec<String> {
        self.acked.lock().clone()
    }
}

#[async_trait]
impl QueueConsumer for MockQueueConsumer {
    fn identifier(&self) -> &str {
        &self.identifier
    }

    async fn poll(&self, max_messages: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(QueueError::Stopped);
        }
        let mut messages = self.messages.lock();
        let count = std::cmp::min(max_messages as usize, messages.len());
        Ok(messages.drain(0..count).collect())
    }

    async fn ack(&self, receipt_handle: &str) -> fc_queue::Result<()> {
        self.acked.lock().push(receipt_handle.to_string());
        Ok(())
    }

    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> fc_queue::Result<()> {
        self.nacked
            .lock()
            .push((receipt_handle.to_string(), delay_seconds));
        Ok(())
    }

    async fn extend_visibility(
        &self,
        _receipt_handle: &str,
        _seconds: u32,
    ) -> fc_queue::Result<()> {
        Ok(())
    }

    fn is_healthy(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn message(id: &str, pool_code: &str, group: Option<&str>) -> Message {
    Message {
        id: id.to_string(),
        pool_code: pool_code.to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: "http://localhost:8080/test".to_string(),
        message_group_id: group.map(|s| s.to_string()),
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(), // NextOnError — requires ordering
        dispatch_mode_specified: true,
    }
}

fn queued(id: &str, pool_code: &str, queue_id: &str) -> QueuedMessage {
    QueuedMessage {
        message: message(id, pool_code, None),
        receipt_handle: format!("receipt-{id}"),
        broker_message_id: Some(format!("broker-{id}")),
        queue_identifier: queue_id.to_string(),
    }
}

fn queued_in_group(id: &str, pool_code: &str, queue_id: &str, group: &str) -> QueuedMessage {
    QueuedMessage {
        message: message(id, pool_code, Some(group)),
        receipt_handle: format!("receipt-{id}"),
        broker_message_id: Some(format!("broker-{id}")),
        queue_identifier: queue_id.to_string(),
    }
}

/// Routes `messages` through `manager` as a single poll batch from a fresh
/// mock consumer, and returns that consumer (so callers can inspect
/// acks/nacks).
async fn route(manager: &Arc<QueueManager>, messages: Vec<QueuedMessage>) -> Arc<MockQueueConsumer> {
    let consumer = Arc::new(MockQueueConsumer::with_messages("q", messages));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();
    consumer
}

/// Polls `cond` until it's true or `timeout` elapses.
async fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + timeout;
    while !cond() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A message naming a per-client fallback pool that doesn't exist yet is
/// synthesised on demand (default settings) and delivered — and, unlike an
/// unknown non-fallback code, raises no ROUTING warning: this is the
/// expected shape for a short-lived client's traffic, not a producer bug.
#[tokio::test]
async fn synthesises_and_delivers_on_demand_without_a_warning() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    route(&manager, vec![queued("m1", "acme-DEFAULT-POOL", "q")]).await;

    wait_until(Duration::from_millis(500), || mediator.call_count() >= 1).await;
    assert_eq!(mediator.call_count(), 1);
    assert_eq!(mediator.processed_ids(), vec!["m1".to_string()]);

    let pool = manager
        .get_pool("acme-DEFAULT-POOL")
        .expect("per-client fallback pool must be synthesised on demand");
    assert_eq!(pool.concurrency(), 20, "default synthesis settings");

    assert!(
        manager
            .warning_service()
            .get_warnings_by_category(WarningCategory::Routing)
            .is_empty(),
        "a fallback-shaped code must not raise the unknown-pool-code ROUTING warning"
    );
}

/// An unknown, non-fallback-shaped pool code is unaffected by R-59: it still
/// warns and still falls back to the shared DEFAULT-POOL (which itself gets
/// synthesised on demand, same as before this feature existed).
#[tokio::test]
async fn unknown_non_fallback_pool_code_still_warns_and_uses_default_pool() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    route(&manager, vec![queued("m1", "NOPE", "q")]).await;

    wait_until(Duration::from_millis(500), || mediator.call_count() >= 1).await;
    assert_eq!(mediator.call_count(), 1);
    assert_eq!(
        manager
            .warning_service()
            .get_warnings_by_category(WarningCategory::Routing)
            .len(),
        1,
        "an unknown non-fallback code must still warn, exactly as before R-59"
    );
    assert!(manager.get_pool("NOPE").is_none());
    assert!(manager.get_pool("DEFAULT-POOL").is_some());
}

/// A synthesised pool idle past its TTL is stopped and removed.
#[tokio::test]
async fn idle_past_ttl_is_evicted() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    route(&manager, vec![queued("m1", "acme-DEFAULT-POOL", "q")]).await;
    assert!(manager.get_pool("acme-DEFAULT-POOL").is_some());

    // A 1ns TTL is already exceeded by the time evict_idle_synth_pools runs.
    tokio::time::sleep(Duration::from_millis(2)).await;
    let evicted = manager.evict_idle_synth_pools(Duration::from_nanos(1)).await;

    assert_eq!(evicted, 1);
    assert!(
        manager.get_pool("acme-DEFAULT-POOL").is_none(),
        "the idle synthesised pool must be removed"
    );
}

/// After eviction, the next message naming the code gets a fresh pool — same
/// shape as ensure_fallback_pool's normal on-demand synthesis — and still
/// delivers.
#[tokio::test]
async fn evicted_pool_is_resynthesised_fresh_on_demand() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    route(&manager, vec![queued("m1", "acme-DEFAULT-POOL", "q1")]).await;
    let first = manager.get_pool("acme-DEFAULT-POOL").expect("synthesised");

    tokio::time::sleep(Duration::from_millis(2)).await;
    assert_eq!(
        manager.evict_idle_synth_pools(Duration::from_nanos(1)).await,
        1
    );
    assert!(manager.get_pool("acme-DEFAULT-POOL").is_none());

    route(&manager, vec![queued("m2", "acme-DEFAULT-POOL", "q2")]).await;
    let second = manager
        .get_pool("acme-DEFAULT-POOL")
        .expect("re-synthesised on demand");
    assert!(
        !Arc::ptr_eq(&first, &second),
        "eviction must produce a genuinely fresh pool"
    );

    wait_until(Duration::from_millis(500), || mediator.call_count() >= 2).await;
    assert_eq!(mediator.call_count(), 2, "both pools must have delivered");
}

/// A pool routed to within the TTL must not be evicted, even if another
/// synthesised pool alongside it is idle.
#[tokio::test]
async fn recent_traffic_is_spared_while_idle_sibling_is_evicted() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    route(&manager, vec![queued("m1", "idle-DEFAULT-POOL", "q1")]).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    route(&manager, vec![queued("m2", "busy-DEFAULT-POOL", "q2")]).await; // routed just now

    let evicted = manager
        .evict_idle_synth_pools(Duration::from_millis(15))
        .await;

    assert_eq!(evicted, 1);
    assert!(
        manager.get_pool("idle-DEFAULT-POOL").is_none(),
        "idle past the TTL must be evicted"
    );
    assert!(
        manager.get_pool("busy-DEFAULT-POOL").is_some(),
        "recently routed must survive"
    );
}

/// A second message routed to an already-synthesised pool must reset its
/// idle clock — the eviction sweep must not judge solely by creation time.
#[tokio::test]
async fn touch_on_hit_path_resets_idle_clock() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    route(&manager, vec![queued("m1", "acme-DEFAULT-POOL", "q1")]).await; // creates it
    tokio::time::sleep(Duration::from_millis(30)).await;
    route(&manager, vec![queued("m2", "acme-DEFAULT-POOL", "q2")]).await; // hit path; must touch

    let evicted = manager
        .evict_idle_synth_pools(Duration::from_millis(15))
        .await;

    assert_eq!(
        evicted, 0,
        "a pool touched inside the TTL must not be evicted"
    );
    assert!(manager.get_pool("acme-DEFAULT-POOL").is_some());
}

/// The global DEFAULT-POOL and a pool config explicitly defines (even one
/// whose code matches the fallback suffix) are never evicted, regardless of
/// idle time.
#[tokio::test]
async fn global_default_pool_and_configured_pools_are_never_evicted() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    manager
        .apply_config(RouterConfig {
            processing_pools: vec![
                PoolConfig {
                    code: "DEFAULT-POOL".to_string(),
                    concurrency: 20,
                    rate_limit_per_minute: None,
                },
                PoolConfig {
                    code: "acme-DEFAULT-POOL".to_string(),
                    concurrency: 3,
                    rate_limit_per_minute: None,
                },
            ],
            queues: vec![],
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(2)).await;
    let evicted = manager.evict_idle_synth_pools(Duration::from_nanos(1)).await;

    assert_eq!(evicted, 0, "config-owned pools must never be evicted");
    assert!(manager.get_pool("DEFAULT-POOL").is_some());
    assert!(manager.get_pool("acme-DEFAULT-POOL").is_some());
}

/// A pool this manager synthesised on demand, then later defined by config
/// with the SAME code, must stop being eviction-eligible — config always
/// wins, and the existing pool is updated in place, never replaced.
#[tokio::test]
async fn config_defining_a_synthesised_code_takes_ownership_without_pool_replacement() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    route(&manager, vec![queued("m1", "acme-DEFAULT-POOL", "q")]).await;
    let synthesised = manager
        .get_pool("acme-DEFAULT-POOL")
        .expect("synthesised on demand");
    assert_eq!(synthesised.concurrency(), 20);

    manager
        .reload_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "acme-DEFAULT-POOL".to_string(),
                concurrency: 7,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let after_reload = manager
        .get_pool("acme-DEFAULT-POOL")
        .expect("must still exist after config takes ownership");
    assert!(
        Arc::ptr_eq(&synthesised, &after_reload),
        "reload_config updates the existing pool in place, it doesn't replace it"
    );
    assert_eq!(
        after_reload.concurrency(),
        7,
        "the configured settings must actually apply"
    );

    tokio::time::sleep(Duration::from_millis(2)).await;
    let evicted = manager.evict_idle_synth_pools(Duration::from_nanos(1)).await;
    assert_eq!(evicted, 0, "a code config now owns must survive eviction");
    assert!(manager.get_pool("acme-DEFAULT-POOL").is_some());
}

/// A non-positive TTL disables the sweep rather than evicting everything
/// immediately. `Duration` has no negative representation — see
/// `bin/fc-router/src/main.rs` for how a negative
/// `FC_ROUTER_SYNTH_POOL_IDLE_SECS` maps onto `Duration::ZERO` here.
#[tokio::test]
async fn ttl_disabled_sweep_is_a_no_op() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    route(&manager, vec![queued("m1", "acme-DEFAULT-POOL", "q")]).await;
    tokio::time::sleep(Duration::from_millis(2)).await;

    assert_eq!(manager.evict_idle_synth_pools(Duration::ZERO).await, 0);
    assert!(manager.get_pool("acme-DEFAULT-POOL").is_some());
}

/// `shutdown()` clears synth-pool idle tracking alongside the pools it
/// drains — a stale entry must never point at a pool eviction can no longer
/// reach. Unlike Go's `Manager.Shutdown` (which *is* re-enterable: standby
/// mode calls it on leadership loss and later `Reconfigure`s the same
/// `Manager` on regain, so it also empties `m.pools`/`m.synthPools` back to
/// fresh maps), Rust's `QueueManager::shutdown()` is a one-time terminal
/// call from `main.rs` with no regain path — it drains and shuts down every
/// pool but does not clear `pools`/`draining_pools` themselves (harmless
/// when the process exits right after). So this test asserts what R-59
/// actually owns here — `synth_pools` — via `synth_pool_count()`, rather
/// than asserting `get_pool` returns `None` the way the Go test does.
#[tokio::test]
async fn shutdown_clears_synth_pool_tracking() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    route(&manager, vec![queued("m1", "acme-DEFAULT-POOL", "q")]).await;
    assert_eq!(manager.synth_pool_count(), 1);

    manager.shutdown().await;
    assert_eq!(
        manager.synth_pool_count(),
        0,
        "shutdown must clear synth-pool idle tracking"
    );
}

/// R-26/R-49: evicting an idle synthesised pool drains its buffered work —
/// it does not drop it. Two ordered-group messages are routed together; the
/// mediator blocks the first mid-delivery so the second is still sitting in
/// the pool's buffer (not yet started) at the moment eviction runs. The pool
/// must finish delivering *both* messages, in order, via the drain path
/// (`begin_pool_drain`) before the drain watcher finally cleans it up.
#[tokio::test]
async fn evicted_synth_pool_drains_buffered_group_work_instead_of_dropping_it() {
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let mediator = Arc::new(GatedMediator::new(started.clone(), release.clone()));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    let consumer = route(
        &manager,
        vec![
            queued_in_group("m1", "acme-DEFAULT-POOL", "q", "g1"),
            queued_in_group("m2", "acme-DEFAULT-POOL", "q", "g1"),
        ],
    )
    .await;

    // m1 is now mediating (blocked on `release`); m2 is buffered behind it
    // in the same ordered group, not yet started.
    started.notified().await;
    assert_eq!(mediator.call_count(), 0, "m1 hasn't returned from mediate() yet");

    tokio::time::sleep(Duration::from_millis(2)).await;
    let evicted = manager.evict_idle_synth_pools(Duration::from_nanos(1)).await;
    assert_eq!(evicted, 1, "the pool must be evicted while m1 is in flight");
    assert!(
        manager.get_pool("acme-DEFAULT-POOL").is_none(),
        "evicted pools leave active routing immediately"
    );

    // Let m1 finish. The evicted pool is only DRAINING (begin_pool_drain
    // calls pool.drain(), never release_remainder()), so its own group-drain
    // task must pick m2 up next rather than the buffer being force-nacked.
    release.notify_one();

    wait_until(Duration::from_secs(2), || mediator.call_count() >= 2).await;
    assert_eq!(
        mediator.call_count(),
        2,
        "both messages must be delivered — buffered work must be drained, not dropped"
    );
    assert_eq!(
        mediator.processed_ids(),
        vec!["m1".to_string(), "m2".to_string()],
        "FIFO ordering within the group must be preserved through the drain"
    );

    wait_until(Duration::from_secs(2), || manager.draining_pool_count() == 0).await;
    assert_eq!(
        manager.draining_pool_count(),
        0,
        "the drain watcher must finish and remove the pool once its buffer is empty"
    );
    assert_eq!(
        consumer.acked().len(),
        2,
        "both messages must have been ACKed, not returned to the broker"
    );
}
