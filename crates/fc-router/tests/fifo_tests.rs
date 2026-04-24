//! FIFO Ordering Tests
//!
//! Tests for message ordering within groups:
//! - Messages in same group processed in order
//! - Different groups can process in parallel
//! - Group ordering maintained across batches

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use async_trait::async_trait;

use fc_common::{
    Message, QueuedMessage, MediationType, MediationOutcome, PoolConfig, RouterConfig,
};
use fc_queue::{QueueConsumer, QueueError};
use fc_router::{QueueManager, Mediator};
use chrono::Utc;

/// Mediator that tracks processing order
struct OrderTrackingMediator {
    processed_ids: parking_lot::Mutex<Vec<String>>,
    delay_ms: u64,
}

impl OrderTrackingMediator {
    fn new(delay_ms: u64) -> Self {
        Self {
            processed_ids: parking_lot::Mutex::new(Vec::new()),
            delay_ms,
        }
    }

    fn processed_ids(&self) -> Vec<String> {
        self.processed_ids.lock().clone()
    }
}

#[async_trait]
impl Mediator for OrderTrackingMediator {
    async fn mediate(&self, message: &Message) -> MediationOutcome {
        // Simulate some processing time
        if self.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        }
        self.processed_ids.lock().push(message.id.clone());
        MediationOutcome::success()
    }
}

/// Mock queue consumer
struct TestQueueConsumer {
    identifier: String,
    messages: parking_lot::Mutex<Vec<QueuedMessage>>,
    acked: parking_lot::Mutex<Vec<String>>,
    nacked: parking_lot::Mutex<Vec<(String, Option<u32>)>>,
    running: AtomicBool,
}

impl TestQueueConsumer {
    fn new(identifier: &str) -> Self {
        Self {
            identifier: identifier.to_string(),
            messages: parking_lot::Mutex::new(Vec::new()),
            acked: parking_lot::Mutex::new(Vec::new()),
            nacked: parking_lot::Mutex::new(Vec::new()),
            running: AtomicBool::new(true),
        }
    }

    fn add_message(&self, msg: QueuedMessage) {
        self.messages.lock().push(msg);
    }
}

#[async_trait]
impl QueueConsumer for TestQueueConsumer {
    fn identifier(&self) -> &str {
        &self.identifier
    }

    async fn poll(&self, max_messages: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
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
        self.nacked.lock().push((receipt_handle.to_string(), delay_seconds));
        Ok(())
    }

    async fn extend_visibility(&self, _receipt_handle: &str, _seconds: u32) -> fc_queue::Result<()> {
        Ok(())
    }

    fn is_healthy(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn create_message_with_group(id: &str, pool_code: &str, group_id: Option<&str>) -> Message {
    Message {
        id: id.to_string(),
        pool_code: pool_code.to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: "http://localhost:8080/test".to_string(),
        message_group_id: group_id.map(|s| s.to_string()),
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(),
    }
}

fn create_queued_message_with_group(id: &str, pool_code: &str, group_id: Option<&str>) -> QueuedMessage {
    QueuedMessage {
        message: create_message_with_group(id, pool_code, group_id),
        receipt_handle: format!("receipt-{}", id),
        broker_message_id: Some(format!("broker-{}", id)),
        queue_identifier: "test-queue".to_string(),
    }
}

#[tokio::test]
async fn test_fifo_single_group_ordering() {
    // Messages in the same group should be processed in order
    let mediator = Arc::new(OrderTrackingMediator::new(20));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 5, // Multiple workers, but group should still be sequential
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add messages in a specific order to the same group
    for i in 0..10 {
        consumer.add_message(create_queued_message_with_group(
            &format!("msg-{}", i),
            "DEFAULT",
            Some("group-1"),
        ));
    }

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify all messages were processed
    let processed = mediator.processed_ids();
    assert_eq!(processed.len(), 10);

    // Verify order is preserved
    for i in 0..10 {
        assert_eq!(processed[i], format!("msg-{}", i), "Message order mismatch at index {}", i);
    }
}

#[tokio::test]
async fn test_fifo_different_groups_parallel() {
    // Different groups should be processed in parallel
    let mediator = Arc::new(OrderTrackingMediator::new(50));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add messages to different groups
    for i in 0..5 {
        consumer.add_message(create_queued_message_with_group(
            &format!("group-a-{}", i),
            "DEFAULT",
            Some(&format!("group-{}", i)), // Each message in its own group
        ));
    }

    let start = std::time::Instant::now();

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    // Wait for processing - shorter sleep since messages process in parallel
    tokio::time::sleep(Duration::from_millis(150)).await;

    let elapsed = start.elapsed();

    // With 50ms delay per message and parallel processing of 5 different groups,
    // all should complete in ~50ms (parallel), well under sequential time of 250ms.
    // Total time = sleep(150ms) + ~50ms processing = ~200ms, threshold 250ms gives margin
    assert!(
        elapsed < Duration::from_millis(250),
        "Expected parallel processing, took {:?}",
        elapsed
    );

    // All messages should be processed
    assert_eq!(mediator.processed_ids().len(), 5);
}

#[tokio::test]
async fn test_fifo_mixed_groups() {
    // Mix of grouped and ungrouped messages
    let mediator = Arc::new(OrderTrackingMediator::new(10));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Group A: 3 messages (ordered)
    for i in 0..3 {
        consumer.add_message(create_queued_message_with_group(
            &format!("group-a-{}", i),
            "DEFAULT",
            Some("group-a"),
        ));
    }

    // Ungrouped messages (can be parallel)
    for i in 0..2 {
        consumer.add_message(create_queued_message_with_group(
            &format!("no-group-{}", i),
            "DEFAULT",
            None,
        ));
    }

    // Group B: 2 messages (ordered)
    for i in 0..2 {
        consumer.add_message(create_queued_message_with_group(
            &format!("group-b-{}", i),
            "DEFAULT",
            Some("group-b"),
        ));
    }

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(400)).await;

    let processed = mediator.processed_ids();
    assert_eq!(processed.len(), 7);

    // Find indices of group-a messages and verify order
    let group_a_indices: Vec<usize> = processed.iter()
        .enumerate()
        .filter(|(_, id)| id.starts_with("group-a-"))
        .map(|(i, _)| i)
        .collect();

    if group_a_indices.len() == 3 {
        // Verify group-a messages are in order relative to each other
        let group_a_ids: Vec<&String> = group_a_indices.iter()
            .map(|&i| &processed[i])
            .collect();

        for i in 0..group_a_ids.len() - 1 {
            let current = group_a_ids[i].strip_prefix("group-a-").unwrap().parse::<u32>().unwrap();
            let next = group_a_ids[i+1].strip_prefix("group-a-").unwrap().parse::<u32>().unwrap();
            assert!(current < next, "Group A messages out of order: {} before {}", current, next);
        }
    }
}

#[tokio::test]
async fn test_fifo_multiple_pools_same_group() {
    // Same group ID across different pools should still be independent
    let mediator = Arc::new(OrderTrackingMediator::new(10));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![
            PoolConfig { code: "POOL_A".to_string(), concurrency: 5, rate_limit_per_minute: None },
            PoolConfig { code: "POOL_B".to_string(), concurrency: 5, rate_limit_per_minute: None },
        ],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Same group ID but different pools
    for i in 0..3 {
        consumer.add_message(create_queued_message_with_group(
            &format!("pool-a-{}", i),
            "POOL_A",
            Some("shared-group"),
        ));
    }
    for i in 0..3 {
        consumer.add_message(create_queued_message_with_group(
            &format!("pool-b-{}", i),
            "POOL_B",
            Some("shared-group"),
        ));
    }

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    let processed = mediator.processed_ids();
    assert_eq!(processed.len(), 6);

    // Verify pool-a messages are in order
    let pool_a: Vec<&String> = processed.iter().filter(|id| id.starts_with("pool-a-")).collect();
    for i in 0..pool_a.len() - 1 {
        let current = pool_a[i].strip_prefix("pool-a-").unwrap().parse::<u32>().unwrap();
        let next = pool_a[i+1].strip_prefix("pool-a-").unwrap().parse::<u32>().unwrap();
        assert!(current < next);
    }

    // Verify pool-b messages are in order
    let pool_b: Vec<&String> = processed.iter().filter(|id| id.starts_with("pool-b-")).collect();
    for i in 0..pool_b.len() - 1 {
        let current = pool_b[i].strip_prefix("pool-b-").unwrap().parse::<u32>().unwrap();
        let next = pool_b[i+1].strip_prefix("pool-b-").unwrap().parse::<u32>().unwrap();
        assert!(current < next);
    }
}

#[tokio::test]
async fn test_fifo_large_group() {
    // Large group should still maintain order
    let mediator = Arc::new(OrderTrackingMediator::new(5));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add 50 messages to the same group
    for i in 0..50 {
        consumer.add_message(create_queued_message_with_group(
            &format!("msg-{:04}", i),
            "DEFAULT",
            Some("large-group"),
        ));
    }

    let poll_result = consumer.poll(50).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(800)).await;

    let processed = mediator.processed_ids();
    assert_eq!(processed.len(), 50);

    // Verify strict ordering
    for i in 0..50 {
        assert_eq!(processed[i], format!("msg-{:04}", i));
    }
}

#[tokio::test]
async fn test_fifo_unique_groups_parallel() {
    // Messages in DIFFERENT groups should be processed in parallel.
    // Note: Messages without a group ID go to __DEFAULT__ group and are processed
    // sequentially (correct FIFO behavior matching Java). To get parallel processing,
    // messages must have different group IDs.
    let mediator = Arc::new(OrderTrackingMediator::new(50));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add 10 messages, each in its own group (allows parallel processing)
    let group_ids: Vec<String> = (0..10).map(|i| format!("unique-group-{}", i)).collect();
    for i in 0..10 {
        consumer.add_message(create_queued_message_with_group(
            &format!("msg-{}", i),
            "DEFAULT",
            Some(group_ids[i].as_str()),  // Each message in its own group
        ));
    }

    let start = std::time::Instant::now();

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let elapsed = start.elapsed();

    // With parallel processing across 10 different groups and 50ms delay,
    // all 10 should complete in ~50ms (parallel), well under 300ms
    assert!(
        elapsed < Duration::from_millis(300),
        "Expected parallel processing across unique groups, took {:?}",
        elapsed
    );

    assert_eq!(mediator.processed_ids().len(), 10);
}

#[tokio::test]
async fn test_fifo_group_throughput() {
    // Verify that groups don't completely serialize all processing
    // i.e., different groups can interleave
    let mediator = Arc::new(OrderTrackingMediator::new(20));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add messages to 5 different groups (3 messages each)
    for group in 0..5 {
        for msg in 0..3 {
            consumer.add_message(create_queued_message_with_group(
                &format!("g{}-m{}", group, msg),
                "DEFAULT",
                Some(&format!("group-{}", group)),
            ));
        }
    }

    let poll_result = consumer.poll(15).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let processed = mediator.processed_ids();
    assert_eq!(processed.len(), 15);

    // Verify each group's messages are in order
    for group in 0..5 {
        let group_msgs: Vec<&String> = processed.iter()
            .filter(|id| id.starts_with(&format!("g{}-", group)))
            .collect();

        for i in 0..group_msgs.len() - 1 {
            let current_num = group_msgs[i].chars().last().unwrap().to_digit(10).unwrap();
            let next_num = group_msgs[i+1].chars().last().unwrap().to_digit(10).unwrap();
            assert!(current_num < next_num, "Group {} out of order", group);
        }
    }
}
