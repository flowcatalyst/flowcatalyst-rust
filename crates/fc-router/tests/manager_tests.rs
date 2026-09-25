//! QueueManager Unit Tests
//!
//! Tests for:
//! - Message routing and batch processing
//! - Duplicate detection
//! - Pool creation and management
//! - Consumer management
//! - Receipt handle updates
//! - Shutdown behavior

use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fc_common::{
    BatchMessage, MediationOutcome, MediationType, Message, MessageCallback, PoolConfig,
    QueuedMessage, RouterConfig, WarningCategory,
};
use fc_queue::{QueueConsumer, QueueError};
use fc_router::{ConsumerFactory, HttpMediatorConfig, Mediator, QueueManager};

/// Mock mediator for testing
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
        tokio::time::sleep(Duration::from_millis(10)).await;
        MediationOutcome::success(200)
    }
}

/// Mock queue consumer for testing
struct MockQueueConsumer {
    identifier: String,
    messages: parking_lot::Mutex<Vec<QueuedMessage>>,
    acked: parking_lot::Mutex<Vec<String>>,
    nacked: parking_lot::Mutex<Vec<(String, Option<u32>)>>,
    running: AtomicBool,
    /// Set to `true` inside `stop()` — lets tests assert a consumer was (or
    /// deliberately was not) stopped, distinct from `running`/`is_healthy`.
    stopped: AtomicBool,
    /// Incremented at the top of every `poll()` call, success or failure —
    /// used by the restart/poll-loop tests to observe whether the spawned
    /// poll task is still looping.
    poll_count: AtomicU32,
}

impl MockQueueConsumer {
    fn new(identifier: &str) -> Self {
        Self {
            identifier: identifier.to_string(),
            messages: parking_lot::Mutex::new(Vec::new()),
            acked: parking_lot::Mutex::new(Vec::new()),
            nacked: parking_lot::Mutex::new(Vec::new()),
            running: AtomicBool::new(true),
            stopped: AtomicBool::new(false),
            poll_count: AtomicU32::new(0),
        }
    }

    fn with_messages(identifier: &str, messages: Vec<QueuedMessage>) -> Self {
        Self {
            identifier: identifier.to_string(),
            messages: parking_lot::Mutex::new(messages),
            acked: parking_lot::Mutex::new(Vec::new()),
            nacked: parking_lot::Mutex::new(Vec::new()),
            running: AtomicBool::new(true),
            stopped: AtomicBool::new(false),
            poll_count: AtomicU32::new(0),
        }
    }

    fn was_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn poll_count(&self) -> u32 {
        self.poll_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl QueueConsumer for MockQueueConsumer {
    fn identifier(&self) -> &str {
        &self.identifier
    }

    async fn poll(&self, max_messages: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
        self.poll_count.fetch_add(1, Ordering::SeqCst);

        if !self.running.load(Ordering::SeqCst) {
            return Err(QueueError::Stopped);
        }

        let mut messages = self.messages.lock();
        let count = std::cmp::min(max_messages as usize, messages.len());
        let result: Vec<_> = messages.drain(0..count).collect();
        Ok(result)
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
        self.stopped.store(true, Ordering::SeqCst);
    }
}

fn create_test_message(id: &str, pool_code: &str) -> Message {
    Message {
        id: id.to_string(),
        pool_code: pool_code.to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: "http://localhost:8080/test".to_string(),
        message_group_id: None,
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(),
        dispatch_mode_specified: true,
    }
}

fn create_queued_message(id: &str, pool_code: &str, queue_id: &str) -> QueuedMessage {
    QueuedMessage {
        message: create_test_message(id, pool_code),
        receipt_handle: format!("receipt-{}", id),
        broker_message_id: Some(format!("broker-{}", id)),
        queue_identifier: queue_id.to_string(),
    }
}

#[tokio::test]
async fn test_queue_manager_creation() {
    let mediator = Arc::new(MockMediator::new());
    let manager = QueueManager::with_shared_mediator_for_testing(mediator);

    // Should have no pools initially
    let stats = manager.get_pool_stats();
    assert!(stats.is_empty());
}

#[tokio::test]
async fn test_apply_config() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    let config = RouterConfig {
        processing_pools: vec![
            PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            },
            PoolConfig {
                code: "HIGH_PRIORITY".to_string(),
                concurrency: 20,
                rate_limit_per_minute: Some(1000),
            },
        ],
        queues: vec![],
    };

    manager.apply_config(config).await.unwrap();

    let stats = manager.get_pool_stats();
    // The two configured pools plus DEFAULT-POOL, which is always ensured
    // (Go: Reconfigure's wantPools).
    assert_eq!(stats.len(), 3);
    assert!(stats.iter().any(|s| s.pool_code == "DEFAULT-POOL"));

    let default_pool = stats.iter().find(|s| s.pool_code == "DEFAULT").unwrap();
    assert_eq!(default_pool.concurrency, 10);

    let high_priority = stats
        .iter()
        .find(|s| s.pool_code == "HIGH_PRIORITY")
        .unwrap();
    assert_eq!(high_priority.concurrency, 20);
    assert_eq!(high_priority.rate_limit_per_minute, Some(1000));
}

#[tokio::test]
async fn test_route_single_message() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    // Apply config
    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    // Create consumer with one message
    let messages = vec![create_queued_message("msg-1", "DEFAULT", "test-queue")];
    let consumer = Arc::new(MockQueueConsumer::with_messages("test-queue", messages));

    // Route the batch
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should have processed the message
    assert_eq!(mediator.call_count(), 1);
    assert!(mediator.processed_ids().contains(&"msg-1".to_string()));
}

#[tokio::test]
async fn test_route_batch_multiple_messages() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let messages: Vec<_> = (0..5)
        .map(|i| create_queued_message(&format!("msg-{}", i), "DEFAULT", "test-queue"))
        .collect();

    let consumer = Arc::new(MockQueueConsumer::with_messages("test-queue", messages));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(mediator.call_count(), 5);
}

/// No-op `MessageCallback` for directly `submit()`-ing filler messages to
/// saturate a pool without going through a real consumer/mediation cycle.
struct NoOpCallback;

#[async_trait]
impl MessageCallback for NoOpCallback {
    async fn ack(&self) {}
    async fn nack(&self, _delay_seconds: Option<u32>) {}
}

fn filler_batch_message(id: &str, pool_code: &str) -> BatchMessage {
    BatchMessage {
        message: create_test_message(id, pool_code),
        receipt_handle: format!("filler-rh-{id}"),
        broker_message_id: Some(format!("filler-bh-{id}")),
        queue_identifier: "filler-queue".to_string(),
        batch_id: None,
        callback: Box::new(NoOpCallback),
    }
}

/// Item 3 (router bench rig, 2026-09-07): `route_batch` must report the
/// "pool at capacity" `WarningCategory::QueueHealth` warning once per
/// full-episode, not once per deferred batch. Saturates a pool directly via
/// `pool.submit()` (back-to-back with no other `.await` in between, so on
/// this current-thread test runtime nothing spawned by `submit()` gets to
/// run before every filler message is admitted — same technique as the
/// G12 capacity-gate tests) to exactly its capacity, then routes TWO
/// separate over-capacity batches at it and asserts only the first records
/// a warning.
///
/// Mutant check: reverting `route_batch` to call
/// `self.warning_service.add_warning(...)` unconditionally (the pre-fix
/// behaviour, not gated by `pool.note_capacity_full()`) fails the second
/// assertion — confirmed by hand while implementing the fix (883
/// occurrences in one saturated 8-queue bench run were exactly this
/// unconditional call firing on every deferred batch).
#[tokio::test]
async fn route_batch_reports_pool_at_capacity_warning_once_per_transition() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "SATURATED".to_string(),
            concurrency: 1, // capacity = max(1 * 20, 50) = 50
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let pool = manager
        .get_pool("SATURATED")
        .expect("pool must exist after apply_config");
    for i in 0..50 {
        pool.submit(filler_batch_message(&format!("filler-{i}"), "SATURATED"))
            .await
            .expect("submit must succeed while under capacity");
    }
    assert_eq!(
        pool.available_capacity(),
        0,
        "pool must read as exactly saturated immediately after filling it"
    );

    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "over-queue",
        vec![create_queued_message("over-1", "SATURATED", "over-queue")],
    ));
    let batch = consumer.poll(10).await.unwrap();
    manager.route_batch(batch, consumer.clone()).await.unwrap();

    let warnings_after_first = manager
        .warning_service()
        .get_warnings_by_category(WarningCategory::QueueHealth)
        .len();
    assert_eq!(
        warnings_after_first, 1,
        "the first batch that finds the pool full must record exactly one warning"
    );

    let consumer2 = Arc::new(MockQueueConsumer::with_messages(
        "over-queue-2",
        vec![create_queued_message("over-2", "SATURATED", "over-queue-2")],
    ));
    let batch2 = consumer2.poll(10).await.unwrap();
    manager
        .route_batch(batch2, consumer2.clone())
        .await
        .unwrap();

    let warnings_after_second = manager
        .warning_service()
        .get_warnings_by_category(WarningCategory::QueueHealth)
        .len();
    assert_eq!(
        warnings_after_second, 1,
        "a second batch finding the SAME pool still full must not record a \
         second warning — only the transition into full is reported"
    );
}

#[tokio::test]
async fn test_route_to_different_pools() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    let config = RouterConfig {
        processing_pools: vec![
            PoolConfig {
                code: "POOL_A".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            },
            PoolConfig {
                code: "POOL_B".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            },
        ],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let messages = vec![
        create_queued_message("msg-1", "POOL_A", "test-queue"),
        create_queued_message("msg-2", "POOL_B", "test-queue"),
        create_queued_message("msg-3", "POOL_A", "test-queue"),
    ];

    let consumer = Arc::new(MockQueueConsumer::with_messages("test-queue", messages));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(mediator.call_count(), 3);
}

#[tokio::test]
async fn test_default_pool_for_empty_pool_code() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    // Message with empty pool code should go to DEFAULT
    let messages = vec![create_queued_message("msg-1", "", "test-queue")];
    let consumer = Arc::new(MockQueueConsumer::with_messages("test-queue", messages));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(mediator.call_count(), 1);
}

// ============================================================================
// R-13/R-16: FC_ROUTER_STRICT_ROUTING gate — route_batch integration
// ============================================================================

fn message_with(
    id: &str,
    pool_code: &str,
    dispatch_mode: fc_common::DispatchMode,
    dispatch_mode_specified: bool,
    group: Option<&str>,
) -> Message {
    Message {
        id: id.to_string(),
        pool_code: pool_code.to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: "http://localhost:8080/test".to_string(),
        message_group_id: group.map(|s| s.to_string()),
        high_priority: false,
        dispatch_mode,
        dispatch_mode_specified,
    }
}

fn queued_with(msg: Message) -> QueuedMessage {
    QueuedMessage {
        receipt_handle: format!("receipt-{}", msg.id),
        broker_message_id: Some(format!("broker-{}", msg.id)),
        queue_identifier: "test-queue".to_string(),
        message: msg,
    }
}

/// Strict routing is off by default — `QueueManager::strict_routing()`
/// must read `false` on a freshly built manager.
#[tokio::test]
async fn strict_routing_off_by_default() {
    let mediator = Arc::new(MockMediator::new());
    let manager = QueueManager::with_shared_mediator_for_testing(mediator);
    assert!(!manager.strict_routing());
}

/// Strict on: a message with an empty pool_code is ACKed, never delivered,
/// and never NACKed.
#[tokio::test]
async fn strict_routing_acks_empty_pool_code_without_delivery() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator.clone())
            .strict_routing(true)
            .build(),
    );

    let msg = message_with("m1", "", fc_common::DispatchMode::Immediate, true, None);
    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "q",
        vec![queued_with(msg)],
    ));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        mediator.call_count(),
        0,
        "malformed message must never be delivered"
    );
    assert_eq!(
        consumer.acked.lock().len(),
        1,
        "malformed message must be ACKed"
    );
    assert_eq!(
        consumer.nacked.lock().len(),
        0,
        "malformed message must never be NACKed"
    );
}

/// Strict on: a message with no wire dispatchMode (unspecified) is ACKed,
/// never delivered — even though it would otherwise resolve to a valid
/// default (NEXT_ON_ERROR, A-09).
#[tokio::test]
async fn strict_routing_acks_unspecified_dispatch_mode_without_delivery() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator.clone())
            .strict_routing(true)
            .build(),
    );
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let msg = message_with(
        "m1",
        "DEFAULT",
        fc_common::DispatchMode::NextOnError,
        false, // wire-unspecified
        None,
    );
    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "q",
        vec![queued_with(msg)],
    ));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(mediator.call_count(), 0);
    assert_eq!(consumer.acked.lock().len(), 1);
    assert_eq!(consumer.nacked.lock().len(), 0);
}

/// Strict on: an ordered-mode message with no message_group_id is ACKed,
/// never delivered.
#[tokio::test]
async fn strict_routing_acks_ordered_mode_without_group_id() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator.clone())
            .strict_routing(true)
            .build(),
    );
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let msg = message_with(
        "m1",
        "DEFAULT",
        fc_common::DispatchMode::BlockOnError,
        true,
        None, // no group id
    );
    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "q",
        vec![queued_with(msg)],
    ));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(mediator.call_count(), 0);
    assert_eq!(consumer.acked.lock().len(), 1);
    assert_eq!(consumer.nacked.lock().len(), 0);
}

/// Strict on: a fully well-formed message (pool code known, dispatch mode
/// specified, ordered mode carries a group id) is delivered normally — the
/// gate must not false-positive on valid traffic.
#[tokio::test]
async fn strict_routing_delivers_well_formed_message() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator.clone())
            .strict_routing(true)
            .build(),
    );
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let msg = message_with(
        "m1",
        "DEFAULT",
        fc_common::DispatchMode::NextOnError,
        true,
        Some("grp-1"),
    );
    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "q",
        vec![queued_with(msg)],
    ));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(
        mediator.call_count(),
        1,
        "well-formed message must be delivered"
    );
    assert_eq!(consumer.acked.lock().len(), 1);
}

/// Strict off (default): the same malformed shapes that strict mode would
/// reject instead route through the non-strict fallbacks and are delivered
/// — proving the gate is genuinely opt-in.
#[tokio::test]
async fn non_strict_routing_still_delivers_malformed_shapes() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    assert!(!manager.strict_routing());
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let messages = vec![
        // Empty pool code -> falls back to DEFAULT-POOL... but the manager's
        // configured pool here is literally "DEFAULT", not the manager's
        // internal fallback constant "DEFAULT-POOL" — use "DEFAULT" as the
        // pool_code directly and instead exercise the other two shapes,
        // which don't depend on the fallback pool's name.
        queued_with(message_with(
            "m1",
            "DEFAULT",
            fc_common::DispatchMode::NextOnError,
            false, // wire-unspecified — still resolves to NEXT_ON_ERROR (A-09)
            None,
        )),
        queued_with(message_with(
            "m2",
            "DEFAULT",
            fc_common::DispatchMode::BlockOnError,
            true,
            None, // ordered, no group id -> non-strict IMMEDIATE-path fallback
        )),
    ];
    let consumer = Arc::new(MockQueueConsumer::with_messages("q", messages));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(
        mediator.call_count(),
        2,
        "non-strict routing must still deliver both messages via fallback"
    );
}

// ============================================================================
// R-26/R-34: leadership loss pauses new polling; regain resumes it.
// In-flight/buffered work is never aborted by a leadership transition.
// ============================================================================

/// Losing leadership stops the consumer poll loop from calling `poll()` at
/// all; regaining it resumes polling (and delivery) without any consumer
/// rebuild.
#[tokio::test]
async fn leadership_loss_pauses_polling_and_regain_resumes_it() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let messages = vec![create_queued_message("msg-1", "DEFAULT", "leader-queue")];
    let consumer = Arc::new(MockQueueConsumer::with_messages("leader-queue", messages));
    manager.add_consumer(consumer.clone()).await;

    // Not the leader from the start.
    manager.set_leader(false);
    assert!(!manager.is_leader());

    let manager_for_start = manager.clone();
    let start_handle = tokio::spawn(async move {
        let _ = manager_for_start.start().await;
    });

    // Give the poll loop several iterations' worth of time — it must never
    // call poll() while not leader, so the message sitting in the mock
    // consumer must never be delivered.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        consumer.poll_count(),
        0,
        "consumer must not be polled while this instance is not the leader"
    );
    assert_eq!(mediator.call_count(), 0);

    // Regain leadership — polling (and delivery) must resume on its own,
    // with no consumer rebuild and no reload_config call.
    manager.set_leader(true);
    wait_until(|| mediator.call_count() >= 1).await;
    assert_eq!(mediator.call_count(), 1);
    assert!(consumer.poll_count() > 0);

    manager.shutdown().await;
    let _ = tokio::time::timeout(Duration::from_secs(2), start_handle).await;
}

/// A delivery already in flight when leadership is lost must run to
/// completion and still ack — losing leadership pauses new *polling* only,
/// it never cancels in-flight work (R-26).
#[tokio::test]
async fn leadership_loss_does_not_abort_in_flight_delivery() {
    let mediator = Arc::new(SlowMockMediator::new(Duration::from_millis(200)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let messages = vec![create_queued_message("msg-1", "DEFAULT", "test-queue")];
    let consumer = Arc::new(MockQueueConsumer::with_messages("test-queue", messages));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    // Let the pool worker actually start mediating (SlowMockMediator sleeps
    // 200ms) before pulling leadership out from under it.
    tokio::time::sleep(Duration::from_millis(30)).await;
    manager.set_leader(false);

    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(
        mediator.call_count(),
        1,
        "the in-flight delivery must have run to completion despite losing leadership mid-call"
    );
    assert_eq!(
        consumer.acked.lock().len(),
        1,
        "the completed in-flight delivery must still ack"
    );
}

#[tokio::test]
async fn test_add_consumer() {
    let mediator = Arc::new(MockMediator::new());
    let manager = QueueManager::with_shared_mediator_for_testing(mediator);

    let consumer = Arc::new(MockQueueConsumer::new("test-consumer"));
    manager.add_consumer(consumer).await;

    let consumer_ids = manager.consumer_ids().await;
    assert!(consumer_ids.contains(&"test-consumer".to_string()));
}

#[tokio::test]
async fn test_memory_health_check() {
    let mediator = Arc::new(MockMediator::new());
    let manager = QueueManager::with_shared_mediator_for_testing(mediator);

    // Initially should be healthy (no messages in pipeline)
    assert!(manager.check_memory_health());
}

#[tokio::test]
async fn test_pool_hot_reload() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    // Initial config
    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "TEST".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    // Update pool config
    let new_config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 20,
        rate_limit_per_minute: Some(500),
    };
    manager
        .update_pool_config("TEST", new_config)
        .await
        .unwrap();

    let stats = manager.get_pool_stats();
    let pool_stats = stats.iter().find(|s| s.pool_code == "TEST").unwrap();
    assert_eq!(pool_stats.concurrency, 20);
    assert_eq!(pool_stats.rate_limit_per_minute, Some(500));
}

#[tokio::test]
async fn test_shutdown() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    // Shutdown should complete without error
    manager.shutdown().await;
}

#[tokio::test]
async fn test_consumer_health_check() {
    let mediator = Arc::new(MockMediator::new());
    let manager = QueueManager::with_shared_mediator_for_testing(mediator);

    let consumer = Arc::new(MockQueueConsumer::new("healthy-consumer"));
    manager.add_consumer(consumer).await;

    let is_healthy = manager.is_consumer_healthy("healthy-consumer").await;
    assert!(is_healthy);
}

#[tokio::test]
async fn test_pool_codes() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    let config = RouterConfig {
        processing_pools: vec![
            PoolConfig {
                code: "A".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            },
            PoolConfig {
                code: "B".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            },
            PoolConfig {
                code: "C".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            },
        ],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let codes = manager.pool_codes();
    assert_eq!(
        codes.len(),
        4,
        "A, B, C and the always-present DEFAULT-POOL"
    );
    assert!(codes.contains(&"DEFAULT-POOL".to_string()));
    assert!(codes.contains(&"A".to_string()));
    assert!(codes.contains(&"B".to_string()));
    assert!(codes.contains(&"C".to_string()));
}

// ============================================================================
// CancellationToken migration tests
// ============================================================================

/// Mediator with a configurable, deliberately slow mediation delay — used to
/// keep a pool's worker task busy long enough for `shutdown()` to observe
/// real in-flight work instead of racing an already-idle pool.
struct SlowMockMediator {
    delay: Duration,
    call_count: AtomicU32,
}

impl SlowMockMediator {
    fn new(delay: Duration) -> Self {
        Self {
            delay,
            call_count: AtomicU32::new(0),
        }
    }

    fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Mediator for SlowMockMediator {
    async fn mediate(&self, _message: &Message) -> MediationOutcome {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        MediationOutcome::success(200)
    }
}

/// `ConsumerFactory` whose `create_consumer` sleeps before returning, so a
/// caller in the middle of `reload_config` is provably still in-flight for
/// the sleep's duration — used to prove readers aren't blocked by it.
struct SlowConsumerFactory {
    delay: Duration,
}

#[async_trait]
impl ConsumerFactory for SlowConsumerFactory {
    async fn create_consumer(
        &self,
        config: &fc_common::QueueConfig,
    ) -> fc_router::Result<Arc<dyn QueueConsumer>> {
        tokio::time::sleep(self.delay).await;
        Ok(Arc::new(MockQueueConsumer::new(&config.name)) as Arc<dyn QueueConsumer>)
    }
}

/// `QueueManager::shutdown()` must not hang when there are no consumers and
/// no pools — the `CancellationToken`-based signalling and the
/// `wait_drained()` timeout logic should both resolve immediately on an
/// empty manager.
#[tokio::test]
async fn shutdown_with_no_consumers_returns_promptly() {
    let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));

    tokio::time::timeout(Duration::from_secs(2), manager.shutdown())
        .await
        .expect("shutdown should complete promptly with no consumers or pools");
}

/// `shutdown()` must wait for genuinely in-flight pool work to finish
/// (via `ProcessPool::wait_drained`) rather than returning as soon as the
/// cancellation signal is sent. Routes one message into a pool backed by a
/// mediator that takes ~200ms, then asserts shutdown took at least roughly
/// that long, every pool reports fully drained afterward, and the mock
/// consumer recorded the ack.
#[tokio::test]
async fn shutdown_waits_for_in_flight_pool_work() {
    let mediator = Arc::new(SlowMockMediator::new(Duration::from_millis(200)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let messages = vec![create_queued_message("msg-1", "DEFAULT", "test-queue")];
    let consumer = Arc::new(MockQueueConsumer::with_messages("test-queue", messages));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    // Give the pool worker a moment to actually pick up the message and
    // start mediating, so shutdown() races real in-flight work rather than
    // an already-idle pool.
    tokio::time::sleep(Duration::from_millis(20)).await;

    let start = std::time::Instant::now();
    manager.shutdown().await;
    let elapsed = start.elapsed();

    assert!(
        elapsed >= Duration::from_millis(150),
        "shutdown should have waited for the ~200ms in-flight mediation, took {:?}",
        elapsed
    );
    assert_eq!(mediator.call_count(), 1);

    assert_eq!(
        manager.is_pool_fully_drained("DEFAULT"),
        Some(true),
        "pool should report fully drained once shutdown returns"
    );

    assert_eq!(
        consumer.acked.lock().len(),
        1,
        "mock consumer should have recorded the ack for the completed message"
    );
}

/// A pool removed from config during `reload_config` is now cleaned up by a
/// per-pool watcher task (spawned the moment it's moved into
/// `draining_pools`) rather than only by the periodic `cleanup_draining_pools`
/// sweep (which in production only runs on the lifecycle manager's 5-minute
/// reaper interval). This asserts the watcher does the job on its own,
/// without ever calling `cleanup_draining_pools`.
#[tokio::test]
async fn removed_pool_is_cleaned_up_by_drain_watcher_without_reaper() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    let config = RouterConfig {
        processing_pools: vec![
            PoolConfig {
                code: "KEEP".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            },
            PoolConfig {
                code: "REMOVE".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            },
        ],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let reload = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "KEEP".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.reload_config(reload).await.unwrap();

    // The watcher spawned by reload_config should remove "REMOVE" from
    // draining_pools on its own — no cleanup_draining_pools() call here.
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while manager.draining_pool_count() > 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(
        manager.draining_pool_count(),
        0,
        "drain watcher should have removed the draining pool within ~1s without a reaper sweep"
    );
    let mut codes = manager.pool_codes();
    codes.sort();
    assert_eq!(codes, vec!["DEFAULT-POOL".to_string(), "KEEP".to_string()]);
}

/// `reload_config` restructures `sync_queue_consumers` to hold the
/// `consumers`/`queue_configs` write locks only for brief synchronous
/// sections, releasing them before awaiting `ConsumerFactory::create_consumer`.
/// This proves a slow consumer-factory call during a reload does not block a
/// concurrent reader (`consumer_ids()`) for the factory's full duration.
#[tokio::test]
async fn reload_config_with_consumer_factory_does_not_block_readers() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator)
            .consumer_factory(Arc::new(SlowConsumerFactory {
                delay: Duration::from_millis(300),
            }))
            .build(),
    );

    let config = RouterConfig {
        processing_pools: vec![],
        queues: vec![fc_common::QueueConfig {
            name: "slow-queue".to_string(),
            uri: "mock://slow-queue".to_string(),
            connections: 1,
            visibility_timeout: 30,
        }],
    };

    let reload_handle = {
        let manager = manager.clone();
        tokio::spawn(async move {
            manager.reload_config(config).await.unwrap();
        })
    };

    // Give reload_config a moment to enter sync_queue_consumers and start
    // the (locked-out) create_consumer call.
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::time::timeout(Duration::from_millis(100), manager.consumer_ids())
        .await
        .expect(
            "consumer_ids() should not be blocked by an in-flight, lock-released \
             ConsumerFactory::create_consumer call",
        );

    reload_handle
        .await
        .expect("reload_config task should not panic");
    assert_eq!(manager.consumer_ids().await, vec!["slow-queue".to_string()]);
}

/// `restart_consumer` must serialise against an in-flight `reload_config`
/// (it takes `pool_configs.read()`, which reloads hold for `write()` for
/// their whole duration). Otherwise a health-triggered restart racing a
/// reload that removes the same queue could resurrect a consumer the
/// config just dropped.
///
/// Timing: the factory sleeps 500 ms per `create_consumer`. A reload that
/// adds a second queue is started, then 50 ms later a restart of the
/// first queue. Unserialised, the restart would take ≈500 ms (its own
/// factory call); serialised it must first wait out the reload's remaining
/// ≈450 ms, so ≈950 ms total. Assert ≥ 800 ms.
#[tokio::test]
async fn restart_consumer_serialises_against_in_flight_reload() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator)
            .consumer_factory(Arc::new(SlowConsumerFactory {
                delay: Duration::from_millis(500),
            }))
            .build(),
    );

    let queue = |name: &str| fc_common::QueueConfig {
        name: name.to_string(),
        uri: format!("mock://{}", name),
        connections: 1,
        visibility_timeout: 30,
    };

    // Initial config: one queue, created up front.
    manager
        .reload_config(RouterConfig {
            processing_pools: vec![],
            queues: vec![queue("q1")],
        })
        .await
        .unwrap();
    assert_eq!(manager.consumer_ids().await, vec!["q1".to_string()]);

    // Reload that adds q2 — spends ~500 ms inside the factory while holding
    // the reload lock.
    let reload_handle = {
        let manager = manager.clone();
        tokio::spawn(async move {
            manager
                .reload_config(RouterConfig {
                    processing_pools: vec![],
                    queues: vec![queue("q1"), queue("q2")],
                })
                .await
                .unwrap();
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;

    let start = std::time::Instant::now();
    let restarted = manager.restart_consumer("q1").await;
    let elapsed = start.elapsed();

    assert!(
        restarted,
        "restart should succeed once the reload releases the lock"
    );
    assert!(
        elapsed >= Duration::from_millis(800),
        "restart_consumer should have waited for the in-flight reload, took {:?}",
        elapsed
    );

    reload_handle
        .await
        .expect("reload_config task should not panic");
    let mut ids = manager.consumer_ids().await;
    ids.sort();
    assert_eq!(ids, vec!["q1".to_string(), "q2".to_string()]);
}

// ============================================================================
// restart_consumer tests
// ============================================================================

/// `ConsumerFactory` that hands out fresh `MockQueueConsumer`s and keeps an
/// `Arc` handle to every one it creates (in creation order), so a test can
/// still inspect an old instance after `restart_consumer` has replaced it.
struct CountingConsumerFactory {
    created: AtomicU32,
    handles: parking_lot::Mutex<Vec<Arc<MockQueueConsumer>>>,
}

impl CountingConsumerFactory {
    fn new() -> Self {
        Self {
            created: AtomicU32::new(0),
            handles: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn created_count(&self) -> u32 {
        self.created.load(Ordering::SeqCst)
    }

    fn handle(&self, index: usize) -> Arc<MockQueueConsumer> {
        self.handles.lock()[index].clone()
    }
}

#[async_trait]
impl ConsumerFactory for CountingConsumerFactory {
    async fn create_consumer(
        &self,
        config: &fc_common::QueueConfig,
    ) -> fc_router::Result<Arc<dyn QueueConsumer>> {
        self.created.fetch_add(1, Ordering::SeqCst);
        let mock = Arc::new(MockQueueConsumer::new(&config.name));
        self.handles.lock().push(mock.clone());
        Ok(mock as Arc<dyn QueueConsumer>)
    }
}

/// `ConsumerFactory` whose *second* `create_consumer` call fails and every
/// other call succeeds — used to exercise `restart_consumer`'s
/// factory-failure path (call #1 creates the original consumer via
/// `reload_config`, call #2 is the failing replacement attempt inside
/// `restart_consumer`, call #3 is the recreation on the next `reload_config`).
struct FlakyConsumerFactory {
    calls: AtomicU32,
}

impl FlakyConsumerFactory {
    fn new() -> Self {
        Self {
            calls: AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl ConsumerFactory for FlakyConsumerFactory {
    async fn create_consumer(
        &self,
        config: &fc_common::QueueConfig,
    ) -> fc_router::Result<Arc<dyn QueueConsumer>> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index != 1 {
            Ok(Arc::new(MockQueueConsumer::new(&config.name)) as Arc<dyn QueueConsumer>)
        } else {
            // Cheapest way to manufacture a `RouterError` from outside the
            // crate: `RouterError::Serialization` has a `#[from]` conversion
            // from `serde_json::Error`, and this reliably produces one.
            Err(serde_json::from_str::<serde_json::Value>("not json")
                .unwrap_err()
                .into())
        }
    }
}

fn queue_config(name: &str) -> fc_common::QueueConfig {
    fc_common::QueueConfig {
        name: name.to_string(),
        uri: format!("mock://{}", name),
        connections: 1,
        visibility_timeout: 30,
    }
}

/// Waits (up to ~2s) for `cond` to become true, polling every 10ms. Used
/// throughout the restart tests instead of a single fixed sleep, since the
/// exact timing of a hot-added poll task's first iteration isn't guaranteed.
async fn wait_until(mut cond: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !cond() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// `restart_consumer` with a working factory + stored queue config actually
/// replaces the consumer: the old one is stopped, a new one is created via
/// the factory, and the new one's poll task is spawned and running.
#[tokio::test]
async fn restart_consumer_replaces_consumer_and_respawns_poll_task() {
    let mediator = Arc::new(MockMediator::new());
    let factory = Arc::new(CountingConsumerFactory::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator)
            .consumer_factory(factory.clone())
            .build(),
    );

    let config = RouterConfig {
        processing_pools: vec![],
        queues: vec![queue_config("restart-queue")],
    };
    manager.reload_config(config).await.unwrap();

    wait_until(|| factory.created_count() >= 1).await;
    assert_eq!(factory.created_count(), 1);
    let first = factory.handle(0);

    wait_until(|| first.poll_count() > 0).await;
    assert!(
        first.poll_count() > 0,
        "consumer #1's poll task should have run at least once"
    );

    let restarted = manager.restart_consumer("restart-queue").await;
    assert!(
        restarted,
        "restart_consumer should succeed with a factory + stored config"
    );

    assert!(first.was_stopped(), "old consumer should have been stopped");
    assert_eq!(
        factory.created_count(),
        2,
        "factory should have created a replacement consumer"
    );
    assert_eq!(
        manager.consumer_ids().await,
        vec!["restart-queue".to_string()]
    );
    assert!(manager.is_consumer_healthy("restart-queue").await);

    let second = factory.handle(1);
    wait_until(|| second.poll_count() > 0).await;
    assert!(
        second.poll_count() > 0,
        "replacement consumer's poll task should be running within ~2s"
    );
}

/// `ConsumerFactory` that mints consumers whose `identifier()` is
/// `"{config.name}/router"` — deliberately different from the config queue
/// name `consumers`/`queue_configs` are keyed by, mirroring NATS's
/// `<stream>/<consumer>` broker-native identity vs. an operator-chosen
/// queue name (item 2, router bench rig, 2026-09-07 — the exact shape of
/// the G10-class mismatch found in `restart_consumer`).
struct IdentifierDiffersFromNameFactory {
    created: AtomicU32,
    handles: parking_lot::Mutex<Vec<Arc<MockQueueConsumer>>>,
}

impl IdentifierDiffersFromNameFactory {
    fn new() -> Self {
        Self {
            created: AtomicU32::new(0),
            handles: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn created_count(&self) -> u32 {
        self.created.load(Ordering::SeqCst)
    }

    fn handle(&self, index: usize) -> Arc<MockQueueConsumer> {
        self.handles.lock()[index].clone()
    }
}

#[async_trait]
impl ConsumerFactory for IdentifierDiffersFromNameFactory {
    async fn create_consumer(
        &self,
        config: &fc_common::QueueConfig,
    ) -> fc_router::Result<Arc<dyn QueueConsumer>> {
        self.created.fetch_add(1, Ordering::SeqCst);
        let mock = Arc::new(MockQueueConsumer::new(&format!("{}/router", config.name)));
        self.handles.lock().push(mock.clone());
        Ok(mock as Arc<dyn QueueConsumer>)
    }
}

/// Item 2 (router bench rig, 2026-09-07): the stall watchdog
/// (`HealthService::get_stalled_consumers`, and G10's ack/nack resolution)
/// keys by `Consumer::identifier()`, not the config queue name
/// `consumers`/`queue_configs` use as their own key. Reproduces the exact
/// bench-rig failure — "Consumer not found for restart" — by calling
/// `restart_consumer` with the consumer's *identifier* ("BENCH1/router")
/// rather than its config name ("BENCH1"), and pins that the restart still
/// succeeds and lands the replacement back under the SAME config-name key.
///
/// Mutant check (confirmed by hand while implementing the fix): reverting
/// `restart_consumer` to look the id up only in `self.consumers` (the
/// pre-fix behaviour) makes this test fail at the first assertion — the
/// call returns `false` and logs "Consumer not found for restart", exactly
/// the bench-rig symptom.
#[tokio::test]
async fn restart_consumer_resolves_by_identifier_when_it_differs_from_the_registry_key() {
    let mediator = Arc::new(MockMediator::new());
    let factory = Arc::new(IdentifierDiffersFromNameFactory::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator)
            .consumer_factory(factory.clone())
            .build(),
    );

    let config = RouterConfig {
        processing_pools: vec![],
        queues: vec![queue_config("BENCH1")],
    };
    manager.reload_config(config).await.unwrap();

    wait_until(|| factory.created_count() >= 1).await;
    let first = factory.handle(0);
    assert_eq!(first.identifier(), "BENCH1/router");
    wait_until(|| first.poll_count() > 0).await;

    // The watchdog / health service resolve by identifier, not by the
    // config queue name — pass exactly that here.
    let restarted = manager.restart_consumer("BENCH1/router").await;
    assert!(
        restarted,
        "restart_consumer must resolve a consumer by its identifier() even \
         when that differs from the config queue name it's registered \
         under in `consumers`"
    );

    assert!(first.was_stopped(), "old consumer should have been stopped");
    assert_eq!(
        factory.created_count(),
        2,
        "factory should have created a replacement consumer"
    );
    // The replacement must land back under the ORIGINAL config-name key
    // ("BENCH1"), not under the identifier that was passed in — otherwise
    // config-driven reconcile/removal would no longer find it.
    assert_eq!(manager.consumer_ids().await, vec!["BENCH1".to_string()]);
    assert!(manager.is_consumer_healthy("BENCH1").await);

    let second = factory.handle(1);
    assert_eq!(second.identifier(), "BENCH1/router");
    wait_until(|| second.poll_count() > 0).await;
    assert!(
        second.poll_count() > 0,
        "replacement consumer's poll task should be running within ~2s"
    );
}

/// Without a `ConsumerFactory`, `restart_consumer` cannot build a
/// replacement, so it must not stop the existing consumer — that would
/// strand it with nothing polling in its place (the original bug). It
/// returns `false` and records a `ConsumerHealth` warning instead.
#[tokio::test]
async fn restart_consumer_without_factory_does_not_stop_consumer() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));

    let mock = Arc::new(MockQueueConsumer::new("no-factory-consumer"));
    manager.add_consumer(mock.clone()).await;

    let restarted = manager.restart_consumer("no-factory-consumer").await;
    assert!(
        !restarted,
        "restart_consumer must fail when there is no factory to build a replacement"
    );
    assert!(
        !mock.was_stopped(),
        "existing consumer must not be stopped when it can't be replaced"
    );
    assert!(manager.is_consumer_healthy("no-factory-consumer").await);
    assert!(
        manager.warning_service().warning_count() >= 1,
        "a ConsumerHealth warning should have been recorded"
    );
}

/// If the factory's replacement call fails, `restart_consumer` leaves the
/// existing consumer registered and running (Go: build before retire — "a
/// rebuild that fails or hangs can never leave the queue with no consumer
/// at all"). A later restart, once the factory works again, replaces it.
#[tokio::test]
async fn restart_consumer_factory_failure_keeps_the_existing_consumer() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator)
            .consumer_factory(Arc::new(FlakyConsumerFactory::new()))
            .build(),
    );

    let config = RouterConfig {
        processing_pools: vec![],
        queues: vec![queue_config("flaky-queue")],
    };
    manager.reload_config(config.clone()).await.unwrap();
    assert_eq!(
        manager.consumer_ids().await,
        vec!["flaky-queue".to_string()]
    );

    let restarted = manager.restart_consumer("flaky-queue").await;
    assert!(
        !restarted,
        "restart should fail when the factory errors building the replacement"
    );
    assert_eq!(
        manager.consumer_ids().await,
        vec!["flaky-queue".to_string()],
        "the existing consumer must stay registered when its replacement can't be built"
    );
    assert!(manager.is_consumer_healthy("flaky-queue").await);

    // The factory works again (call #3): the restart now replaces it.
    assert!(manager.restart_consumer("flaky-queue").await);
    assert_eq!(
        manager.consumer_ids().await,
        vec!["flaky-queue".to_string()]
    );
}

/// After a consumer is stopped, its poll task must exit on the very next
/// poll instead of looping on `QueueError::Stopped` forever. Snapshots the
/// poll counter, stops the mock directly (not via `restart_consumer`, so
/// this isolates the poll-loop fix from the restart-replacement logic),
/// waits well past the old 1s error-retry interval, and asserts the counter
/// advanced by at most one more call (the poll already in flight when
/// `stop()` landed).
#[tokio::test]
async fn poll_task_exits_after_consumer_stop() {
    let mediator = Arc::new(MockMediator::new());
    let factory = Arc::new(CountingConsumerFactory::new());
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator)
            .consumer_factory(factory.clone())
            .build(),
    );

    let config = RouterConfig {
        processing_pools: vec![],
        queues: vec![queue_config("stop-queue")],
    };
    manager.reload_config(config).await.unwrap();

    wait_until(|| factory.created_count() >= 1).await;
    let consumer = factory.handle(0);
    wait_until(|| consumer.poll_count() > 0).await;

    let count_before = consumer.poll_count();
    consumer.stop().await;

    tokio::time::sleep(Duration::from_millis(2500)).await;

    let count_after = consumer.poll_count();
    assert!(
        count_after <= count_before + 1,
        "poll task should have exited on Stopped instead of looping \
         (before={count_before}, after={count_after})"
    );
}

// ---------------------------------------------------------------------
// Operator-surface parity: force-ack, blocked groups, group flushes
// ---------------------------------------------------------------------

/// `force_ack_in_flight` clears the tracker entry (so a second call finds
/// nothing to ack) and acks the broker copy — and once cleared, the
/// delivery's own eventual `ack()` call (racing the operator override)
/// finds no receipt handle and is a harmless no-op rather than a
/// double-ack or a panic.
#[tokio::test]
async fn force_ack_in_flight_clears_entry_and_later_ack_is_harmless() {
    let mediator = Arc::new(MockMediator::new()); // mediate() sleeps 10ms
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let messages = vec![create_queued_message("msg-1", "DEFAULT", "test-queue")];
    let consumer = Arc::new(MockQueueConsumer::with_messages("test-queue", messages));
    // `force_ack_in_flight` resolves the consumer for the entry's
    // `queue_identifier` from the manager's own consumer registry (unlike
    // the normal ack/nack path, which the routed `QueueMessageCallback`
    // already carries a direct `Arc<dyn QueueConsumer>` for) — so, same as
    // production's consumer-supervisor wiring, the consumer must be
    // registered here too.
    manager.add_consumer(consumer.clone()).await;
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    // Force-ack races the mediator's own 10ms delay — the message is
    // still tracked (route_batch only registers + dispatches; it doesn't
    // await delivery).
    let result = manager
        .force_ack_in_flight("msg-1")
        .await
        .expect("message must still be tracked");
    assert_eq!(result.queue_id, "test-queue");
    assert_eq!(result.pool_code, "DEFAULT");
    assert!(
        result.broker_acked,
        "the mock consumer's ack must succeed: {:?}",
        result.broker_ack_error
    );
    assert_eq!(
        consumer.acked.lock().len(),
        1,
        "force-ack must delete the broker copy via the source consumer"
    );

    // A second lookup must report "not tracked" — the entry is gone.
    assert!(
        manager.force_ack_in_flight("msg-1").await.is_none(),
        "the tracker entry must be cleared, not just answered twice"
    );

    // Let the in-flight delivery (already running when force-ack fired)
    // finish. Its own ack() finds no receipt handle in `in_pipeline`
    // (entry gone) — logged as an error, but must not panic, double-ack,
    // or otherwise crash the task.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        mediator.call_count(),
        1,
        "the delivery that was already running must still run to completion"
    );
    assert_eq!(
        consumer.acked.lock().len(),
        1,
        "the delivery's own stale-handle ack must be a no-op, not a second broker ack"
    );
}

/// A message never routed is reported as not tracked, not an error.
#[tokio::test]
async fn force_ack_in_flight_none_for_untracked_message() {
    let mediator = Arc::new(MockMediator::new());
    let manager = QueueManager::with_shared_mediator_for_testing(mediator);
    assert!(manager.force_ack_in_flight("never-seen").await.is_none());
}

/// `blocked_groups` aggregates across every pool the manager is tracking
/// (ledger R-04) — reflects a gated ordered group the same way
/// `pool::group_snapshot_reflects_a_gated_ordered_group` pins at the pool
/// level, but exercised through the manager's routing path end to end.
#[tokio::test]
async fn manager_blocked_groups_reflects_a_gated_ordered_group_across_pools() {
    let mediator = Arc::new(MockMediator::new()); // mediate() sleeps 10ms
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "ORDERED".to_string(),
                concurrency: 1, // force sequential draining within the group
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    assert!(manager.blocked_groups().is_empty());

    let mut msg1 = create_test_message("group-msg-1", "ORDERED");
    msg1.message_group_id = Some("g1".to_string());
    msg1.dispatch_mode = fc_common::DispatchMode::NextOnError;
    let mut msg2 = create_test_message("group-msg-2", "ORDERED");
    msg2.message_group_id = Some("g1".to_string());
    msg2.dispatch_mode = fc_common::DispatchMode::NextOnError;

    let messages = vec![queued_with(msg1), queued_with(msg2)];
    let consumer = Arc::new(MockQueueConsumer::with_messages("ordered-queue", messages));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(3)).await;
    let groups = manager.blocked_groups();
    assert_eq!(groups.len(), 1, "exactly one live group across all pools");
    assert_eq!(groups[0].group, "g1");
    assert_eq!(groups[0].pool_code, "ORDERED");

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        manager.blocked_groups().is_empty(),
        "a fully-drained group must not show up"
    );
}

/// `clear_group_flush` lifts a suppression via the manager-level traversal
/// (ledger R-52/R-53) — the API-layer's operator override, end to end
/// through `QueueManager` rather than a direct `ProcessPool` call. The
/// suppression itself is armed directly on the pool's registry (a target's
/// `flushGroup` response is pool.rs/mediator territory, out of scope for
/// this manager-level test) — this test's job is the manager traversal
/// (`group_flush_snapshots`/`clear_group_flush`) on top of it.
#[tokio::test]
async fn manager_clear_group_flush_lifts_suppression_and_reports_in_snapshot() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "FLUSHPOOL".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    // Unknown pool / unknown group: nothing to lift.
    assert!(!manager.clear_group_flush("FLUSHPOOL", "no-such-group"));
    assert!(!manager.clear_group_flush("NO-SUCH-POOL", "g1"));

    let pool = manager.get_pool("FLUSHPOOL").expect("pool must exist");
    assert!(pool.group_flush_registry().flush("g1", Some(3600)));

    // DEFAULT-POOL is always present too; look at the pool under test.
    let snap: Vec<_> = manager
        .group_flush_snapshots()
        .into_iter()
        .filter(|s| s.pool_code == "FLUSHPOOL")
        .collect();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].pool_code, "FLUSHPOOL");
    assert_eq!(snap[0].active_count, 1);
    assert_eq!(snap[0].total_flushes, 1);
    assert_eq!(snap[0].groups.len(), 1);
    assert_eq!(snap[0].groups[0].group, "g1");

    assert!(
        manager.clear_group_flush("FLUSHPOOL", "g1"),
        "an active suppression existed to lift"
    );
    assert!(
        !pool.group_flush_registry().suppressed("g1"),
        "clear must actually lift the suppression on the pool's registry"
    );
    let snap_after: Vec<_> = manager
        .group_flush_snapshots()
        .into_iter()
        .filter(|s| s.pool_code == "FLUSHPOOL")
        .collect();
    assert_eq!(snap_after[0].active_count, 0);
    assert!(snap_after[0].groups.is_empty());

    // Already cleared: nothing left to lift.
    assert!(!manager.clear_group_flush("FLUSHPOOL", "g1"));
}

// ============================================================================
// Concurrency-audit consolidation #2: orphaned draining predecessors
// ============================================================================

/// A pool code removed from config and re-added *before its predecessor
/// finishes draining* displaces the predecessor from the `pools` map into
/// `orphaned_draining` (see that field's doc comment in `manager/mod.rs`)
/// rather than losing the manager's only reference to it. This pins the
/// whole lifecycle end to end:
/// - the successor pool serves traffic under the same code immediately;
/// - the displaced predecessor's in-flight work stays visible through the
///   manager's public dashboard surface (`mediating_snapshot`, built on the
///   private `all_pools`) even though it's no longer `pools`' entry for
///   this code;
/// - `shutdown()` still releases the predecessor's buffered remainder
///   (NACK) rather than abandoning it — the router specification's §5.3
///   shutdown MUST (release every pool's buffered remainder back to the
///   broker, never abandon it).
#[tokio::test]
async fn displaced_draining_predecessor_stays_visible_and_gets_released_at_shutdown() {
    // Slow enough that m1 is still mid-mediation through the remove/re-add
    // below, and still mid-mediation when shutdown() is called a moment
    // later.
    let mediator = Arc::new(SlowMockMediator::new(Duration::from_millis(300)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));

    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "REBORN".to_string(),
                concurrency: 1, // force strictly serial delivery within the group
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let predecessor = manager.get_pool("REBORN").expect("pool must exist");

    // m1/m2 share an ordered group: with concurrency 1, m1 is popped and
    // starts mediating (300ms sleep) while m2 sits buffered behind it —
    // the "gated in-flight ordered message" that drains slowly.
    let m1 = message_with(
        "m1",
        "REBORN",
        fc_common::DispatchMode::NextOnError,
        true,
        Some("g1"),
    );
    let m2 = message_with(
        "m2",
        "REBORN",
        fc_common::DispatchMode::NextOnError,
        true,
        Some("g1"),
    );
    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "reborn-queue",
        vec![queued_with(m1), queued_with(m2)],
    ));
    let poll_result = consumer.poll(10).await.unwrap();
    manager
        .route_batch(poll_result, consumer.clone())
        .await
        .unwrap();

    // Give the drain task time to pop m1 and start mediating it so m2 is
    // still buffered when the pool is removed below.
    tokio::time::sleep(Duration::from_millis(30)).await;

    // Remove REBORN from config — begins draining the predecessor.
    // `pool.drain()` only stops new admission; m1 keeps mediating and m2
    // stays buffered (draining alone releases nothing — only shutdown's
    // release_remainder does).
    manager
        .reload_config(RouterConfig {
            processing_pools: vec![],
            queues: vec![],
        })
        .await
        .unwrap();
    assert!(
        manager.get_pool("REBORN").is_none(),
        "REBORN must not be active immediately after removal"
    );

    // Re-add REBORN before the predecessor has finished draining (m1 is
    // still ~270ms from finishing) — the coexistence case: a fresh Active
    // pool for the same code while the old one is still Draining.
    manager
        .reload_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "REBORN".to_string(),
                concurrency: 1,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let successor = manager
        .get_pool("REBORN")
        .expect("REBORN must be active again");
    assert!(
        !Arc::ptr_eq(&predecessor, &successor),
        "reload must have created a fresh pool instance, not resurrected the predecessor"
    );

    // The new pool actively serves traffic under the same code.
    let m3 = message_with(
        "m3",
        "REBORN",
        fc_common::DispatchMode::Immediate,
        true,
        None,
    );
    let consumer3 = Arc::new(MockQueueConsumer::with_messages(
        "reborn-queue-2",
        vec![queued_with(m3)],
    ));
    let poll3 = consumer3.poll(10).await.unwrap();
    manager.route_batch(poll3, consumer3.clone()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        manager
            .mediating_snapshot()
            .iter()
            .any(|e| e.message_id == "m3"),
        "the successor pool must be actively mediating the fresh message"
    );

    // The displaced predecessor's in-flight message is still discoverable
    // through the manager's public dashboard surface (`all_pools`) even
    // though it's no longer `pools`' entry for "REBORN" — this is the fix:
    // without `orphaned_draining`, m1 would have vanished from every
    // manager-level view the instant the successor was created.
    assert!(
        manager
            .mediating_snapshot()
            .iter()
            .any(|e| e.message_id == "m1"),
        "the displaced predecessor's in-flight message must still be visible"
    );

    // Shut down while m1 is still in-flight and m2 is still buffered on the
    // orphaned predecessor.
    manager.shutdown().await;

    // m2 was never started — shutdown's release_remainder must NACK it,
    // not abandon it (router-specification.md §5.3).
    let nacked = consumer.nacked.lock();
    assert!(
        nacked.iter().any(|(handle, _)| handle.as_str() == "receipt-m2"),
        "the orphaned predecessor's buffered remainder must be released (NACKed), not abandoned; nacked = {:?}",
        *nacked
    );
    drop(nacked);

    // m1 was already in flight — shutdown waits for it, and it resolves
    // normally (ACK), same as any other pool's in-hand delivery.
    assert!(
        consumer
            .acked
            .lock()
            .iter()
            .any(|h| h.as_str() == "receipt-m1"),
        "the orphaned predecessor's in-flight message must finish and be ACKed, not abandoned"
    );
}

// ============================================================================
// Consumer reconcile / resolution (Go: Reconfigure, resolveConsumer)
// ============================================================================

/// `ConsumerFactory` that hands out pre-seeded consumers in order, or fails
/// the build when a slot holds `None`.
struct ScriptedConsumerFactory {
    script: parking_lot::Mutex<std::collections::VecDeque<Option<Arc<MockQueueConsumer>>>>,
    built: parking_lot::Mutex<Vec<Arc<MockQueueConsumer>>>,
}

impl ScriptedConsumerFactory {
    fn new(script: Vec<Option<Arc<MockQueueConsumer>>>) -> Self {
        Self {
            script: parking_lot::Mutex::new(script.into()),
            built: parking_lot::Mutex::new(Vec::new()),
        }
    }
    fn built(&self) -> Vec<Arc<MockQueueConsumer>> {
        self.built.lock().clone()
    }
}

#[async_trait]
impl ConsumerFactory for ScriptedConsumerFactory {
    async fn create_consumer(
        &self,
        config: &fc_common::QueueConfig,
    ) -> fc_router::Result<Arc<dyn QueueConsumer>> {
        let next = self
            .script
            .lock()
            .pop_front()
            .unwrap_or_else(|| Some(Arc::new(MockQueueConsumer::new(&config.name))));
        match next {
            Some(c) => {
                self.built.lock().push(c.clone());
                Ok(c as Arc<dyn QueueConsumer>)
            }
            None => Err(serde_json::from_str::<serde_json::Value>("broker down")
                .unwrap_err()
                .into()),
        }
    }
}

fn queue_config_with(name: &str, visibility_timeout: u32) -> fc_common::QueueConfig {
    fc_common::QueueConfig {
        visibility_timeout,
        ..queue_config(name)
    }
}

/// Go compares a queue's whole config: a change under the same name (here
/// the visibility timeout; a URI change is the same path) rebuilds the
/// consumer. It used to be keyed by name alone, so the change was ignored
/// and the consumer kept its old settings for ever.
#[tokio::test]
async fn queue_config_change_under_the_same_name_rebuilds_the_consumer() {
    let factory = Arc::new(ScriptedConsumerFactory::new(vec![]));
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(Arc::new(MockMediator::new()))
            .consumer_factory(factory.clone())
            .build(),
    );
    let cfg = |vt| RouterConfig {
        processing_pools: vec![],
        queues: vec![queue_config_with("q", vt)],
    };

    manager.reload_config(cfg(30)).await.unwrap();
    assert_eq!(factory.built().len(), 1);

    // Same config again: nothing rebuilt.
    manager.reload_config(cfg(30)).await.unwrap();
    assert_eq!(factory.built().len(), 1);

    // Visibility timeout changed: rebuilt, old one detached (stopped
    // polling) but still resolvable until retired.
    manager.reload_config(cfg(90)).await.unwrap();
    let built = factory.built();
    assert_eq!(built.len(), 2);
    assert!(built[0].was_stopped(), "the old consumer stops polling");
    assert!(!built[1].was_stopped());
    assert_eq!(manager.consumer_ids().await, vec!["q".to_string()]);
    assert_eq!(manager.queue_configs()["q"].visibility_timeout, 90);
    assert_eq!(manager.detaching_consumer_count(), 1);
    assert_eq!(manager.retire_detached_consumers(), 1);
    manager.shutdown().await;
}

/// H9 (Go: Reconfigure returns the build error and the watcher forgets the
/// config): a consumer that cannot be built fails the reload — after the
/// other queues are reconciled — and the next reload builds exactly the
/// missing one. It used to log, report success, and never try again.
#[tokio::test]
async fn failed_consumer_build_fails_the_reload_and_is_retried() {
    let factory = Arc::new(ScriptedConsumerFactory::new(vec![None]));
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(Arc::new(MockMediator::new()))
            .consumer_factory(factory.clone())
            .build(),
    );
    let config = RouterConfig {
        processing_pools: vec![],
        queues: vec![queue_config("q")],
    };

    let first = manager.reload_config(config.clone()).await;
    assert!(
        first.is_err(),
        "a failed consumer build must fail the reload"
    );
    assert!(manager.consumer_ids().await.is_empty());

    manager.reload_config(config).await.unwrap();
    assert_eq!(manager.consumer_ids().await, vec!["q".to_string()]);
    manager.shutdown().await;
}

/// Go's `resolveConsumer`: a callback acks through the consumer the
/// registry holds for its queue when it runs — not an instance captured at
/// route time. Here the message was routed by an instance the manager never
/// registered, so the ack goes to the registered consumer for that queue.
#[tokio::test]
async fn ack_resolves_the_registered_consumer_for_its_queue() {
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(Arc::new(
        MockMediator::new(),
    )));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "P".to_string(),
                concurrency: 2,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();

    let registered = Arc::new(MockQueueConsumer::new("shared-q"));
    manager.add_consumer(registered.clone()).await;

    let unregistered = Arc::new(MockQueueConsumer::with_messages(
        "shared-q",
        vec![create_queued_message("m1", "P", "shared-q")],
    ));
    let polled = unregistered.poll(10).await.unwrap();
    manager
        .route_batch(polled, unregistered.clone())
        .await
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while registered.acked.lock().is_empty() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(registered.acked.lock().len(), 1);
    assert!(unregistered.acked.lock().is_empty());
    manager.shutdown().await;
}

/// A consumer detached while its message is in flight stays the one that
/// acks it (a NATS receipt can only be acked on the connection that
/// received it), and is retired only once nothing it delivered is left.
#[tokio::test]
async fn detached_consumer_acks_its_in_flight_message_then_retires() {
    let first = Arc::new(MockQueueConsumer::with_messages(
        "q",
        vec![create_queued_message("slow-1", "P", "q")],
    ));
    let factory = Arc::new(ScriptedConsumerFactory::new(vec![Some(first.clone())]));
    let mediator = Arc::new(SlowMockMediator::new(Duration::from_millis(400)));
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(mediator.clone())
            .consumer_factory(factory.clone())
            .build(),
    );
    let cfg = |vt| RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "P".to_string(),
            concurrency: 2,
            rate_limit_per_minute: None,
        }],
        queues: vec![queue_config_with("q", vt)],
    };
    manager.reload_config(cfg(30)).await.unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while mediator.call_count() == 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(mediator.call_count(), 1, "the message is mid-delivery");

    // Replace the consumer while its message is still being delivered.
    manager.reload_config(cfg(60)).await.unwrap();
    assert_eq!(manager.detaching_consumer_count(), 1);
    assert_eq!(
        manager.retire_detached_consumers(),
        0,
        "not retired while a message it delivered is in flight"
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while first.acked.lock().is_empty() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        first.acked.lock().len(),
        1,
        "acked through the instance that received it"
    );
    let second = factory.built()[1].clone();
    assert!(second.acked.lock().is_empty());
    assert_eq!(manager.retire_detached_consumers(), 1);
    manager.shutdown().await;
}

/// Consumer whose ack fails once it has been stopped — the old NATS
/// behaviour (stop() cleared the pending-ack map). Hands out one message.
struct AckFailsAfterStopConsumer {
    inner: MockQueueConsumer,
    ack_errors: AtomicU32,
}

#[async_trait]
impl QueueConsumer for AckFailsAfterStopConsumer {
    fn identifier(&self) -> &str {
        self.inner.identifier()
    }
    async fn poll(&self, max: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
        self.inner.poll(max).await
    }
    async fn ack(&self, receipt_handle: &str) -> fc_queue::Result<()> {
        if self.inner.was_stopped() {
            self.ack_errors.fetch_add(1, Ordering::SeqCst);
            return Err(QueueError::NotFound(receipt_handle.to_string()));
        }
        self.inner.ack(receipt_handle).await
    }
    async fn nack(&self, receipt_handle: &str, delay: Option<u32>) -> fc_queue::Result<()> {
        self.inner.nack(receipt_handle, delay).await
    }
    async fn extend_visibility(&self, _: &str, _: u32) -> fc_queue::Result<()> {
        Ok(())
    }
    fn is_healthy(&self) -> bool {
        true
    }
    async fn stop(&self) {
        self.inner.stop().await
    }
}

/// H8 (Go: StopPolling → drain → Shutdown): shutdown stops polling first,
/// lets the in-flight delivery finish and ack, and only then stops the
/// consumer. It used to stop consumers first, so the ack of a delivery that
/// completed during the drain failed (NATS: redelivered after ack_wait).
#[tokio::test]
async fn shutdown_stops_polling_drains_then_stops_consumers() {
    let mediator = Arc::new(SlowMockMediator::new(Duration::from_millis(300)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "P".to_string(),
                concurrency: 2,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let consumer = Arc::new(AckFailsAfterStopConsumer {
        inner: MockQueueConsumer::with_messages("q", vec![create_queued_message("m1", "P", "q")]),
        ack_errors: AtomicU32::new(0),
    });
    manager.add_consumer(consumer.clone()).await;
    let start_task = tokio::spawn(manager.clone().start());

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while mediator.call_count() == 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(mediator.call_count(), 1);

    manager.shutdown().await;
    assert!(manager.polling_stopped());
    assert_eq!(
        consumer.ack_errors.load(Ordering::SeqCst),
        0,
        "the in-flight delivery must be acked before its consumer is stopped"
    );
    assert_eq!(consumer.inner.acked.lock().len(), 1);
    assert!(
        consumer.inner.was_stopped(),
        "consumers are stopped at the end"
    );
    let _ = tokio::time::timeout(Duration::from_secs(2), start_task).await;
}

/// Once polling is stopped for shutdown, a config reload must not start new
/// consumers (Go: Watch is cancelled before the drain).
#[tokio::test]
async fn reload_after_stop_polling_is_refused() {
    let factory = Arc::new(ScriptedConsumerFactory::new(vec![]));
    let manager = Arc::new(
        QueueManager::builder_with_shared_mediator(Arc::new(MockMediator::new()))
            .consumer_factory(factory.clone())
            .build(),
    );
    manager.stop_polling();
    let applied = manager
        .reload_config(RouterConfig {
            processing_pools: vec![],
            queues: vec![queue_config("late")],
        })
        .await
        .unwrap();
    assert!(!applied);
    assert!(factory.built().is_empty());
}

/// Go's Reconfigure always keeps a DEFAULT-POOL: a config that does not
/// define it neither removes nor fails to create it. Dropping it started a
/// drain while the next unrouted message re-created it — two pools running
/// the same groups side by side.
#[tokio::test]
async fn reload_keeps_default_pool_when_config_omits_it() {
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(Arc::new(
        MockMediator::new(),
    )));
    let cfg = |code: &str| RouterConfig {
        processing_pools: vec![PoolConfig {
            code: code.to_string(),
            concurrency: 3,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(cfg("A")).await.unwrap();
    let default_pool = manager
        .get_pool("DEFAULT-POOL")
        .expect("DEFAULT-POOL is always ensured");
    assert_eq!(default_pool.concurrency(), 20);

    manager.reload_config(cfg("B")).await.unwrap();
    let after = manager.get_pool("DEFAULT-POOL").expect("still there");
    assert!(
        Arc::ptr_eq(&default_pool, &after),
        "never removed and re-created"
    );
    assert_eq!(manager.draining_pool_count(), 1, "only A drains");
}

/// update_pool_config changes a pool in place — never a second instance.
#[tokio::test]
async fn update_pool_config_is_in_place() {
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(Arc::new(
        MockMediator::new(),
    )));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "T".to_string(),
                concurrency: 4,
                rate_limit_per_minute: Some(60),
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let before = manager.get_pool("T").unwrap();
    manager
        .update_pool_config(
            "T",
            PoolConfig {
                code: "T".to_string(),
                concurrency: 9,
                rate_limit_per_minute: None,
            },
        )
        .await
        .unwrap();
    let after = manager.get_pool("T").unwrap();
    assert!(Arc::ptr_eq(&before, &after));
    assert_eq!(after.concurrency(), 9);
    assert_eq!(after.rate_limit_per_minute(), None);
    assert_eq!(manager.draining_pool_count(), 0);
    assert!(!manager.update_pool("NOPE", Some(2), None).await);
    assert!(
        !manager.update_pool("T", Some(0), None).await,
        "0 is rejected, as Go"
    );
}
