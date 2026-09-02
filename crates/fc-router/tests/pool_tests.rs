// `for i in 0..N { assert_eq!(processed[i], …) }` reads more naturally
// in assertion-heavy tests than the enumerate() rewrite the lint pushes.
#![allow(clippy::needless_range_loop)]

//! ProcessPool Unit Tests
//!
//! Tests for:
//! - Pool creation and configuration
//! - Concurrent message processing
//! - Rate limiting behavior
//! - Message group ordering (FIFO)
//! - Capacity management
//! - Shutdown behavior

use async_trait::async_trait;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

use fc_common::{
    AckNack, BatchMessage, MediationOutcome, MediationResult, MediationType, Message,
    MessageCallback, PoolConfig,
};

/// Test callback that records ack/nack via a oneshot channel
struct TestCallback {
    tx: parking_lot::Mutex<Option<oneshot::Sender<AckNack>>>,
}

#[async_trait]
impl MessageCallback for TestCallback {
    async fn ack(&self) {
        if let Some(tx) = self.tx.lock().take() {
            let _ = tx.send(AckNack::Ack);
        }
    }
    async fn nack(&self, delay_seconds: Option<u32>) {
        if let Some(tx) = self.tx.lock().take() {
            let _ = tx.send(AckNack::Nack { delay_seconds });
        }
    }
}

/// Mirrors production `QueueMessageCallback`'s `Drop` impl (see
/// `crates/fc-router/src/manager.rs`): if the callback is dropped without
/// `ack()`/`nack()` ever having been called — e.g. an abandoned `PoolTask`
/// falling out of a drain task's queue — fire a best-effort fallback nack.
/// `ack`/`nack` already `take()` the sender before sending, so this is a
/// no-op on the normal resolved path.
impl Drop for TestCallback {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.lock().take() {
            let _ = tx.send(AckNack::Nack {
                delay_seconds: Some(0),
            });
        }
    }
}
use fc_router::{Mediator, ProcessPool};

/// Mock mediator that tracks calls and can simulate delays/failures
struct MockMediator {
    call_count: AtomicU32,
    delay_ms: u64,
    should_fail: bool,
    /// Track message IDs in order they were processed
    processed_ids: parking_lot::Mutex<Vec<String>>,
}

impl MockMediator {
    fn new() -> Self {
        Self {
            call_count: AtomicU32::new(0),
            delay_ms: 0,
            should_fail: false,
            processed_ids: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn with_delay(delay_ms: u64) -> Self {
        Self {
            call_count: AtomicU32::new(0),
            delay_ms,
            should_fail: false,
            processed_ids: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn failing() -> Self {
        Self {
            call_count: AtomicU32::new(0),
            delay_ms: 0,
            should_fail: true,
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

        if self.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        }

        if self.should_fail {
            MediationOutcome {
                result: MediationResult::ErrorProcess,
                delay_seconds: Some(1),
                status_code: Some(500),
                error_message: Some("Mock failure".to_string()),
                flush_group: false,
            }
        } else {
            MediationOutcome::success(200)
        }
    }
}

fn create_test_message(id: &str, group_id: Option<&str>) -> Message {
    Message {
        id: id.to_string(),
        pool_code: "TEST".to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: "http://localhost:8080/test".to_string(),
        message_group_id: group_id.map(|s| s.to_string()),
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(),
        dispatch_mode_specified: true,
    }
}

fn create_batch_message(
    id: &str,
    group_id: Option<&str>,
) -> (BatchMessage, oneshot::Receiver<AckNack>) {
    let (tx, rx) = oneshot::channel();
    let msg = BatchMessage {
        message: create_test_message(id, group_id),
        receipt_handle: format!("receipt-{}", id),
        broker_message_id: Some(format!("broker-{}", id)),
        queue_identifier: "test-queue".to_string(),
        batch_id: Some(std::sync::Arc::from("batch-1")),
        callback: Box::new(TestCallback {
            tx: parking_lot::Mutex::new(Some(tx)),
        }),
    };
    (msg, rx)
}

#[tokio::test]
async fn test_pool_creation() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = ProcessPool::new(config, mediator);

    assert_eq!(pool.code(), "TEST");
    assert_eq!(pool.concurrency(), 5);
    assert_eq!(pool.rate_limit_per_minute(), None);
}

#[tokio::test]
async fn test_pool_with_rate_limit() {
    let config = PoolConfig {
        code: "RATE_LIMITED".to_string(),
        concurrency: 10,
        rate_limit_per_minute: Some(100),
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = ProcessPool::new(config, mediator);

    assert_eq!(pool.rate_limit_per_minute(), Some(100));
}

#[tokio::test]
async fn test_single_message_processing() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = Arc::new(ProcessPool::new(config, mediator.clone()));

    pool.start().await;

    let (batch_msg, rx) = create_batch_message("msg-1", None);
    pool.submit(batch_msg).await.unwrap();

    // Wait for processing
    let result = tokio::time::timeout(Duration::from_secs(5), rx).await;
    assert!(result.is_ok());

    let ack_nack = result.unwrap().unwrap();
    assert!(matches!(ack_nack, AckNack::Ack));
    assert_eq!(mediator.call_count(), 1);
}

#[tokio::test]
async fn test_multiple_messages_concurrent() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 10,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::with_delay(50));
    let pool = Arc::new(ProcessPool::new(config, mediator.clone()));

    pool.start().await;

    // Submit 5 messages concurrently
    let mut receivers = Vec::new();
    for i in 0..5 {
        let (batch_msg, rx) = create_batch_message(&format!("msg-{}", i), None);
        pool.submit(batch_msg).await.unwrap();
        receivers.push(rx);
    }

    // All should complete
    for rx in receivers {
        let result = tokio::time::timeout(Duration::from_secs(5), rx).await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap().unwrap(), AckNack::Ack));
    }

    assert_eq!(mediator.call_count(), 5);
}

#[tokio::test]
async fn test_message_group_fifo_ordering() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 1, // Force sequential processing per group
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::with_delay(10));
    let pool = Arc::new(ProcessPool::new(config, mediator.clone()));

    pool.start().await;

    // Submit messages with same group - should be processed in order
    let mut receivers = Vec::new();
    for i in 0..5 {
        let (batch_msg, rx) = create_batch_message(&format!("msg-{}", i), Some("group-1"));
        pool.submit(batch_msg).await.unwrap();
        receivers.push(rx);
    }

    // Wait for all to complete
    for rx in receivers {
        let result = tokio::time::timeout(Duration::from_secs(10), rx).await;
        assert!(result.is_ok());
    }

    // Check order
    let processed = mediator.processed_ids();
    assert_eq!(processed.len(), 5);
    for i in 0..5 {
        assert_eq!(processed[i], format!("msg-{}", i));
    }
}

#[tokio::test]
async fn test_different_groups_parallel() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 10,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::with_delay(50));
    let pool = Arc::new(ProcessPool::new(config, mediator.clone()));

    pool.start().await;

    // Submit messages to different groups - should process in parallel
    let start = std::time::Instant::now();
    let mut receivers = Vec::new();

    for i in 0..5 {
        let (batch_msg, rx) = create_batch_message(
            &format!("msg-{}", i),
            Some(&format!("group-{}", i)), // Different groups
        );
        pool.submit(batch_msg).await.unwrap();
        receivers.push(rx);
    }

    // Wait for all
    for rx in receivers {
        let _ = tokio::time::timeout(Duration::from_secs(5), rx).await;
    }

    let elapsed = start.elapsed();
    // With 50ms delay per message and parallel processing,
    // should complete much faster than 250ms (5 * 50ms sequential)
    assert!(
        elapsed < Duration::from_millis(200),
        "Expected parallel processing, took {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_failed_message_nack() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::failing());
    let pool = Arc::new(ProcessPool::new(config, mediator.clone()));

    pool.start().await;

    let (batch_msg, rx) = create_batch_message("msg-1", None);
    pool.submit(batch_msg).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), rx).await;
    assert!(result.is_ok());

    let ack_nack = result.unwrap().unwrap();
    assert!(matches!(ack_nack, AckNack::Nack { .. }));
}

#[tokio::test]
async fn test_pool_capacity() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 2,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = Arc::new(ProcessPool::new(config, mediator));

    pool.start().await;

    // Check available capacity
    let initial_capacity = pool.available_capacity();
    assert!(initial_capacity > 0);
}

#[tokio::test]
async fn test_pool_stats() {
    let config = PoolConfig {
        code: "STATS_TEST".to_string(),
        concurrency: 10,
        rate_limit_per_minute: Some(500),
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = Arc::new(ProcessPool::new(config, mediator));

    pool.start().await;

    let stats = pool.get_stats();
    assert_eq!(stats.pool_code, "STATS_TEST");
    assert_eq!(stats.concurrency, 10);
    assert_eq!(stats.rate_limit_per_minute, Some(500));
    assert_eq!(stats.active_workers, 0);
    assert_eq!(stats.queue_size, 0);
}

#[tokio::test]
async fn test_pool_shutdown() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = Arc::new(ProcessPool::new(config, mediator));

    pool.start().await;
    pool.drain().await;

    // After drain, new messages should be rejected
    let (batch_msg, rx) = create_batch_message("msg-1", None);
    pool.submit(batch_msg).await.unwrap();

    let result = tokio::time::timeout(Duration::from_millis(100), rx).await;
    if let Ok(Ok(ack_nack)) = result {
        assert!(matches!(ack_nack, AckNack::Nack { .. }));
    }
}

#[tokio::test]
async fn test_pool_fully_drained() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = Arc::new(ProcessPool::new(config, mediator));

    pool.start().await;

    // Initially should be drained (no work)
    assert!(pool.is_fully_drained());

    pool.drain().await;
    pool.shutdown().await;
}

/// Regression test for the TaskTracker migration: `drain()` must not block
/// waiting for in-flight work (callers like `QueueManager::reload_config`
/// call it while holding a lock), but `wait_drained()` must resolve once
/// every spawned worker/drain task has actually finished — and every
/// submitted message must have been acked by then.
#[tokio::test]
async fn drain_then_wait_drained_resolves_after_in_flight_work() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::with_delay(100));
    let pool = Arc::new(ProcessPool::new(config, mediator.clone()));

    pool.start().await;

    let mut receivers = Vec::new();
    for i in 0..8 {
        let (batch_msg, rx) = create_batch_message(&format!("msg-{}", i), None);
        pool.submit(batch_msg).await.unwrap();
        receivers.push(rx);
    }

    // drain() must return promptly — it does not wait for in-flight work.
    let drain_start = std::time::Instant::now();
    pool.drain().await;
    assert!(
        drain_start.elapsed() < Duration::from_millis(50),
        "drain() should not block on in-flight work"
    );

    // wait_drained() should resolve once the in-flight mediator calls (each
    // ~100ms) finish.
    let waited = tokio::time::timeout(Duration::from_secs(5), pool.wait_drained()).await;
    assert!(waited.is_ok(), "wait_drained() timed out");

    // All callbacks must have been acked (mediator succeeds by default).
    for rx in receivers {
        let result = rx.await;
        assert!(result.is_ok(), "callback channel dropped without a result");
        assert!(matches!(result.unwrap(), AckNack::Ack));
    }

    assert_eq!(mediator.call_count(), 8);
    assert!(pool.is_fully_drained());
}

/// An idle pool (nothing ever submitted) should report drained immediately
/// — `wait_drained()` must not hang waiting on a tracker that never saw any
/// work.
#[tokio::test]
async fn wait_drained_on_idle_pool_resolves_immediately() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = Arc::new(ProcessPool::new(config, mediator));

    pool.start().await;

    let waited = tokio::time::timeout(Duration::from_secs(1), pool.wait_drained()).await;
    assert!(
        waited.is_ok(),
        "wait_drained() should resolve immediately on an idle pool"
    );
    assert!(pool.is_fully_drained());
    assert_eq!(pool.tracked_tasks(), 0);
}

/// After `drain()`, `submit()` must synchronously nack new work rather than
/// queueing it — asserting the existing "reject after drain" behaviour that
/// the TaskTracker migration must not change.
#[tokio::test]
async fn submit_after_drain_is_nacked() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = Arc::new(ProcessPool::new(config, mediator));

    pool.start().await;
    pool.drain().await;

    let (batch_msg, rx) = create_batch_message("msg-1", None);
    pool.submit(batch_msg).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(1), rx).await;
    assert!(result.is_ok(), "submit after drain should nack promptly");
    let ack_nack = result.unwrap().unwrap();
    assert!(matches!(ack_nack, AckNack::Nack { .. }));
}

/// Regression test for the `group_handlers` cleanup race fixed by
/// `DashMap::remove_if`. The old get()/drop()/remove() sequence left a
/// window — between releasing the DashMap shard's read guard and taking
/// the write lock for `remove()` — where a concurrent `submit()` could
/// find-and-reuse the same handler entry, enqueue a task, flip
/// `processing = true`, and spawn a new drain task, only for the original
/// drain task's `remove()` to then yank that handler (with the freshly
/// queued task inside) out of the map. The abandoned `PoolTask`'s callback
/// fires a spurious fallback nack on `Drop`.
///
/// That window is only a few non-`.await` instructions wide (no yield point
/// exists between the drop and the remove), so it needs genuine OS-thread
/// parallelism (a multi-thread runtime) and *concurrent* submitters — a
/// single sequential submit loop, even with `yield_now()` sprinkled in, can
/// never land in it, since nothing can preempt the drain task mid-window on
/// a cooperative scheduler. Several tasks hammer submit() concurrently for
/// the same ordered message group (forcing a single shared
/// `MessageGroupHandler` and repeated drain-task exit/respawn churn), and
/// the test asserts every message was acked and none were spuriously
/// nacked.
///
/// Note on reliability: this window is narrow enough that reproducing it
/// via pure OS-scheduler luck is hard in practice — a thread that just
/// released the shard lock usually wins the race to re-acquire it back for
/// itself over another thread that has to be woken and dispatched, so this
/// test may pass against the pre-fix code too if you try reverting the
/// `remove_if` fix locally. The fix's correctness was verified separately
/// by artificially widening the window (a `std::thread::sleep` inserted
/// between the drop and the remove, and then again inside the `remove_if`
/// closure) during development: widening it reliably reproduced spurious
/// nacks against the old get()/drop()/remove() code and reliably produced
/// zero against `remove_if` (which holds the shard lock continuously across
/// the check and the removal, so a concurrent `submit()` simply blocks
/// until it's done rather than racing it). That diagnostic code is not
/// part of this test. This test still stands on its own merits: it drives
/// real concurrent load through the ordered-group path and asserts the
/// observable contract (every message acked, none spuriously nacked).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn group_handler_cleanup_race_does_not_spurious_nack() {
    const SUBMITTERS: usize = 8;
    const PER_SUBMITTER: usize = 1000;
    const N: usize = SUBMITTERS * PER_SUBMITTER;

    // `available_capacity` is `concurrency * QUEUE_CAPACITY_MULTIPLIER` (20),
    // pool-wide, not per-group. With a single ordered group here, submits
    // vastly outrun drains, so concurrency must be high enough that the
    // pool-wide queue never legitimately fills — otherwise `submit()`'s own
    // capacity check nacks messages with `Some(10)`, which is
    // indistinguishable from (and would mask) the race's cascading
    // `Some(10)` "batch+group failed" nack. `concurrency: 1000` gives a
    // capacity of 20000, comfortably above `N`.
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 1000,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(MockMediator::new());
    let pool = Arc::new(ProcessPool::new(config, mediator.clone()));

    pool.start().await;

    let acks = Arc::new(AtomicU32::new(0));
    let nacks = Arc::new(AtomicU32::new(0));

    let mut submitter_handles = Vec::with_capacity(SUBMITTERS);
    for s in 0..SUBMITTERS {
        let pool = pool.clone();
        let acks = acks.clone();
        let nacks = nacks.clone();
        submitter_handles.push(tokio::spawn(async move {
            for i in 0..PER_SUBMITTER {
                let (tx, rx) = oneshot::channel();
                let msg = BatchMessage {
                    message: {
                        // Same message_group_id ("g") for every submitter so
                        // they all contend on one MessageGroupHandler entry.
                        let mut m = create_test_message(&format!("msg-{}-{}", s, i), Some("g"));
                        m.dispatch_mode = fc_common::DispatchMode::BlockOnError;
                        m
                    },
                    receipt_handle: format!("receipt-{}-{}", s, i),
                    broker_message_id: Some(format!("broker-{}-{}", s, i)),
                    queue_identifier: "test-queue".to_string(),
                    batch_id: Some(std::sync::Arc::from("batch-1")),
                    callback: Box::new(TestCallback {
                        tx: parking_lot::Mutex::new(Some(tx)),
                    }),
                };

                pool.submit(msg).await.unwrap();

                let acks = acks.clone();
                let nacks = nacks.clone();
                tokio::spawn(async move {
                    if let Ok(ack_nack) = rx.await {
                        match ack_nack {
                            AckNack::Ack => {
                                acks.fetch_add(1, Ordering::SeqCst);
                            }
                            AckNack::Nack { .. } => {
                                nacks.fetch_add(1, Ordering::SeqCst);
                            }
                            AckNack::ExtendVisibility { .. } => {
                                panic!("unexpected ExtendVisibility in this test");
                            }
                        }
                    }
                });

                if i % 4 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        }));
    }

    for h in submitter_handles {
        h.await.unwrap();
    }

    pool.drain().await;
    let waited = tokio::time::timeout(Duration::from_secs(30), pool.wait_drained()).await;
    assert!(waited.is_ok(), "wait_drained() timed out");

    // Give the spawned ack/nack recorder tasks a moment to run after their
    // rx resolves (they're plain tokio::spawn, not tracked by the pool).
    for _ in 0..200 {
        if acks.load(Ordering::SeqCst) + nacks.load(Ordering::SeqCst) >= N as u32 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Check the primary regression signal (spurious nacks) first so a
    // failure here is unambiguous rather than being masked by whichever
    // assertion happens to come first.
    assert_eq!(
        nacks.load(Ordering::SeqCst),
        0,
        "spurious nack detected — group handler cleanup race regression"
    );
    assert_eq!(acks.load(Ordering::SeqCst), N as u32);
    assert_eq!(mediator.call_count(), N as u32);
}
