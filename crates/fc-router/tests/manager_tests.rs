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
    MediationOutcome, MediationType, Message, PoolConfig, QueuedMessage, RouterConfig,
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
    assert_eq!(stats.len(), 2);

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
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    manager.set_strict_routing(true);

    let msg = message_with(
        "m1",
        "",
        fc_common::DispatchMode::Immediate,
        true,
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

    assert_eq!(mediator.call_count(), 0, "malformed message must never be delivered");
    assert_eq!(consumer.acked.lock().len(), 1, "malformed message must be ACKed");
    assert_eq!(consumer.nacked.lock().len(), 0, "malformed message must never be NACKed");
}

/// Strict on: a message with no wire dispatchMode (unspecified) is ACKed,
/// never delivered — even though it would otherwise resolve to a valid
/// default (NEXT_ON_ERROR, A-09).
#[tokio::test]
async fn strict_routing_acks_unspecified_dispatch_mode_without_delivery() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    manager.set_strict_routing(true);
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
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    manager.set_strict_routing(true);
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
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone(),
    ));
    manager.set_strict_routing(true);
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

    assert_eq!(mediator.call_count(), 1, "well-formed message must be delivered");
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
    assert_eq!(codes.len(), 3);
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
    ) -> fc_router::Result<Arc<dyn QueueConsumer + Send + Sync>> {
        tokio::time::sleep(self.delay).await;
        Ok(Arc::new(MockQueueConsumer::new(&config.name)) as Arc<dyn QueueConsumer + Send + Sync>)
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
    assert_eq!(manager.pool_codes(), vec!["KEEP".to_string()]);
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

    assert!(restarted, "restart should succeed once the reload releases the lock");
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
    ) -> fc_router::Result<Arc<dyn QueueConsumer + Send + Sync>> {
        self.created.fetch_add(1, Ordering::SeqCst);
        let mock = Arc::new(MockQueueConsumer::new(&config.name));
        self.handles.lock().push(mock.clone());
        Ok(mock as Arc<dyn QueueConsumer + Send + Sync>)
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
    ) -> fc_router::Result<Arc<dyn QueueConsumer + Send + Sync>> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index != 1 {
            Ok(Arc::new(MockQueueConsumer::new(&config.name))
                as Arc<dyn QueueConsumer + Send + Sync>)
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

/// If the factory's replacement call fails, `restart_consumer` removes the
/// now-dead entry from `consumers` (so it stops being reported as a live
/// consumer) but leaves its `queue_configs` entry alone. The next
/// `reload_config` with the same queue config should then recreate it via
/// the ordinary hot-add path, self-healing a transient factory failure.
#[tokio::test]
async fn restart_consumer_factory_failure_removes_dead_consumer_and_keeps_config() {
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
    assert!(
        manager.consumer_ids().await.is_empty(),
        "dead consumer entry should be removed from `consumers`"
    );

    // A second reload with the *same* queue config should recreate it: the
    // config-sync path treats "in config, not in `consumers`" as new.
    manager.reload_config(config).await.unwrap();
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
