//! ProcessPool Unit Tests
//!
//! Tests for:
//! - Pool creation and configuration
//! - Concurrent message processing
//! - Rate limiting behavior
//! - Message group ordering (FIFO)
//! - Capacity management
//! - Shutdown behavior

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;
use async_trait::async_trait;

use fc_common::{
    Message, BatchMessage, AckNack, MessageCallback, PoolConfig, MediationType,
    MediationResult, MediationOutcome,
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
use fc_router::{ProcessPool, Mediator};

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
            }
        } else {
            MediationOutcome::success()
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
    }
}

fn create_batch_message(id: &str, group_id: Option<&str>) -> (BatchMessage, oneshot::Receiver<AckNack>) {
    let (tx, rx) = oneshot::channel();
    let msg = BatchMessage {
        message: create_test_message(id, group_id),
        receipt_handle: format!("receipt-{}", id),
        broker_message_id: Some(format!("broker-{}", id)),
        queue_identifier: "test-queue".to_string(),
        batch_id: Some(std::sync::Arc::from("batch-1")),
        callback: Box::new(TestCallback { tx: parking_lot::Mutex::new(Some(tx)) }),
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
    assert!(elapsed < Duration::from_millis(200), "Expected parallel processing, took {:?}", elapsed);
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
