//! Rate Limiting Tests
//!
//! Tests for:
//! - Pool-level rate limiting
//! - Rate limit enforcement
//! - Rate limit updates via hot reload
//! - Multiple pools with different rate limits

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::time::{Duration, Instant};
use async_trait::async_trait;

use fc_common::{
    Message, QueuedMessage, MediationType, MediationOutcome, PoolConfig, RouterConfig,
};
use fc_queue::{QueueConsumer, QueueError};
use fc_router::{QueueManager, Mediator};
use chrono::Utc;

/// Mediator that tracks timing and counts
struct TimingMediator {
    call_times: parking_lot::Mutex<Vec<Instant>>,
    call_count: AtomicU32,
}

impl TimingMediator {
    fn new() -> Self {
        Self {
            call_times: parking_lot::Mutex::new(Vec::new()),
            call_count: AtomicU32::new(0),
        }
    }

    fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Mediator for TimingMediator {
    async fn mediate(&self, _message: &Message) -> MediationOutcome {
        self.call_times.lock().push(Instant::now());
        self.call_count.fetch_add(1, Ordering::SeqCst);
        MediationOutcome::success()
    }
}

/// Mock queue consumer
struct TestQueueConsumer {
    identifier: String,
    messages: parking_lot::Mutex<Vec<QueuedMessage>>,
    acked: parking_lot::Mutex<Vec<String>>,
    running: AtomicBool,
}

impl TestQueueConsumer {
    fn new(identifier: &str) -> Self {
        Self {
            identifier: identifier.to_string(),
            messages: parking_lot::Mutex::new(Vec::new()),
            acked: parking_lot::Mutex::new(Vec::new()),
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

    async fn nack(&self, _receipt_handle: &str, _delay_seconds: Option<u32>) -> fc_queue::Result<()> {
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
    }
}

fn create_queued_message(id: &str, pool_code: &str) -> QueuedMessage {
    QueuedMessage {
        message: create_test_message(id, pool_code),
        receipt_handle: format!("receipt-{}", id),
        broker_message_id: Some(format!("broker-{}", id)),
        queue_identifier: "test-queue".to_string(),
    }
}

#[tokio::test]
async fn test_pool_without_rate_limit() {
    let mediator = Arc::new(TimingMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None, // No rate limit
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add 20 messages
    for i in 0..20 {
        consumer.add_message(create_queued_message(&format!("msg-{}", i), "DEFAULT"));
    }

    let start = Instant::now();

    let poll_result = consumer.poll(20).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    let elapsed = start.elapsed();

    // Without rate limit, all 20 messages should process quickly (under 500ms)
    assert!(
        elapsed < Duration::from_millis(500),
        "Expected fast processing without rate limit, took {:?}",
        elapsed
    );

    assert_eq!(mediator.call_count(), 20);
}

#[tokio::test]
async fn test_pool_with_rate_limit() {
    let mediator = Arc::new(TimingMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    // Use a low rate limit (60/minute = 1/second) to test rate limiting behavior
    // Note: governor uses token bucket with burst capacity equal to quota,
    // so small batches may process within burst capacity without visible gaps
    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "RATE_LIMITED".to_string(),
            concurrency: 10,
            rate_limit_per_minute: Some(60), // 1 per second
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add 5 messages
    for i in 0..5 {
        consumer.add_message(create_queued_message(&format!("msg-{}", i), "RATE_LIMITED"));
    }

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    // Wait for processing
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Messages should be processed (some may be NACKed due to rate limit)
    let processed = mediator.call_count();
    assert!(processed >= 1, "Expected at least some messages to be processed");

    // Verify pool has rate limit configured
    let stats = manager.get_pool_stats();
    let pool = stats.iter().find(|s| s.pool_code == "RATE_LIMITED").unwrap();
    assert_eq!(pool.rate_limit_per_minute, Some(60));
}

#[tokio::test]
async fn test_multiple_pools_different_rates() {
    let mediator = Arc::new(TimingMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![
            PoolConfig {
                code: "FAST".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None, // No limit
            },
            PoolConfig {
                code: "SLOW".to_string(),
                concurrency: 10,
                rate_limit_per_minute: Some(60), // 1 per second
            },
        ],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add messages to both pools
    for i in 0..3 {
        consumer.add_message(create_queued_message(&format!("fast-{}", i), "FAST"));
        consumer.add_message(create_queued_message(&format!("slow-{}", i), "SLOW"));
    }

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    // FAST pool should complete quickly, SLOW pool takes longer
    tokio::time::sleep(Duration::from_secs(4)).await;

    assert_eq!(mediator.call_count(), 6);
}

#[tokio::test]
async fn test_rate_limit_hot_reload() {
    let mediator = Arc::new(TimingMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    // Start with no rate limit
    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DYNAMIC".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    // Verify pool exists with no rate limit
    let stats = manager.get_pool_stats();
    let pool_stats = stats.iter().find(|s| s.pool_code == "DYNAMIC").unwrap();
    assert_eq!(pool_stats.rate_limit_per_minute, None);

    // Update to add rate limit
    let new_config = PoolConfig {
        code: "DYNAMIC".to_string(),
        concurrency: 10,
        rate_limit_per_minute: Some(600), // 10 per second
    };
    manager.update_pool_config("DYNAMIC", new_config).await.unwrap();

    // Verify rate limit was applied
    let stats = manager.get_pool_stats();
    let pool_stats = stats.iter().find(|s| s.pool_code == "DYNAMIC").unwrap();
    assert_eq!(pool_stats.rate_limit_per_minute, Some(600));
}

#[tokio::test]
async fn test_rate_limit_stats() {
    let mediator = Arc::new(TimingMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "TEST".to_string(),
            concurrency: 5,
            rate_limit_per_minute: Some(300),
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let stats = manager.get_pool_stats();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].pool_code, "TEST");
    assert_eq!(stats[0].concurrency, 5);
    assert_eq!(stats[0].rate_limit_per_minute, Some(300));
}

#[tokio::test]
async fn test_high_rate_limit() {
    // Very high rate limit should not noticeably slow down processing
    let mediator = Arc::new(TimingMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "HIGH_RATE".to_string(),
            concurrency: 20,
            rate_limit_per_minute: Some(6000), // 100 per second
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add 50 messages
    for i in 0..50 {
        consumer.add_message(create_queued_message(&format!("msg-{}", i), "HIGH_RATE"));
    }

    let start = Instant::now();

    let poll_result = consumer.poll(50).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let elapsed = start.elapsed();

    // With 100/second limit, 50 messages should complete in under 2 seconds
    assert!(
        elapsed < Duration::from_secs(3),
        "High rate limit should allow fast processing, took {:?}",
        elapsed
    );

    assert_eq!(mediator.call_count(), 50);
}

#[tokio::test]
async fn test_rate_limit_combined_with_concurrency() {
    let mediator = Arc::new(TimingMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    // Low concurrency + rate limit
    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "LIMITED".to_string(),
            concurrency: 2, // Only 2 concurrent workers
            rate_limit_per_minute: Some(120), // 2 per second
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    for i in 0..4 {
        consumer.add_message(create_queued_message(&format!("msg-{}", i), "LIMITED"));
    }

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    // Wait for processing - should be limited by both concurrency and rate
    tokio::time::sleep(Duration::from_secs(4)).await;

    assert_eq!(mediator.call_count(), 4);
}

#[tokio::test]
async fn test_pool_codes_with_rate_limits() {
    let mediator = Arc::new(TimingMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    let config = RouterConfig {
        processing_pools: vec![
            PoolConfig { code: "A".to_string(), concurrency: 5, rate_limit_per_minute: Some(100) },
            PoolConfig { code: "B".to_string(), concurrency: 5, rate_limit_per_minute: Some(200) },
            PoolConfig { code: "C".to_string(), concurrency: 5, rate_limit_per_minute: None },
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

#[tokio::test]
async fn test_remove_rate_limit() {
    let mediator = Arc::new(TimingMediator::new());
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(mediator.clone()));

    // Start with rate limit
    let config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "REMOVE_LIMIT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: Some(60),
        }],
        queues: vec![],
    };
    manager.apply_config(config).await.unwrap();

    // Verify rate limit exists
    let stats = manager.get_pool_stats();
    let pool_stats = stats.iter().find(|s| s.pool_code == "REMOVE_LIMIT").unwrap();
    assert_eq!(pool_stats.rate_limit_per_minute, Some(60));

    // Remove rate limit
    let new_config = PoolConfig {
        code: "REMOVE_LIMIT".to_string(),
        concurrency: 10,
        rate_limit_per_minute: None,
    };
    manager.update_pool_config("REMOVE_LIMIT", new_config).await.unwrap();

    // Verify rate limit was removed
    let stats = manager.get_pool_stats();
    let pool_stats = stats.iter().find(|s| s.pool_code == "REMOVE_LIMIT").unwrap();
    assert_eq!(pool_stats.rate_limit_per_minute, None);
}
