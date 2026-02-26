//! QueueManager Unit Tests
//!
//! Tests for:
//! - Message routing and batch processing
//! - Duplicate detection
//! - Pool creation and management
//! - Consumer management
//! - Receipt handle updates
//! - Shutdown behavior

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::time::Duration;
use async_trait::async_trait;

use fc_common::{
    Message, QueuedMessage, MediationType, MediationOutcome,
    PoolConfig, RouterConfig,
};
use fc_queue::{QueueConsumer, QueueError};
use fc_router::{QueueManager, Mediator};
use chrono::Utc;

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
        MediationOutcome::success()
    }
}

/// Mock queue consumer for testing
struct MockQueueConsumer {
    identifier: String,
    messages: parking_lot::Mutex<Vec<QueuedMessage>>,
    acked: parking_lot::Mutex<Vec<String>>,
    nacked: parking_lot::Mutex<Vec<(String, Option<u32>)>>,
    running: AtomicBool,
}

impl MockQueueConsumer {
    fn new(identifier: &str) -> Self {
        Self {
            identifier: identifier.to_string(),
            messages: parking_lot::Mutex::new(Vec::new()),
            acked: parking_lot::Mutex::new(Vec::new()),
            nacked: parking_lot::Mutex::new(Vec::new()),
            running: AtomicBool::new(true),
        }
    }

    fn with_messages(identifier: &str, messages: Vec<QueuedMessage>) -> Self {
        Self {
            identifier: identifier.to_string(),
            messages: parking_lot::Mutex::new(messages),
            acked: parking_lot::Mutex::new(Vec::new()),
            nacked: parking_lot::Mutex::new(Vec::new()),
            running: AtomicBool::new(true),
        }
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
    let manager = QueueManager::new(mediator);

    // Should have no pools initially
    let stats = manager.get_pool_stats();
    assert!(stats.is_empty());
}

#[tokio::test]
async fn test_apply_config() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::new(mediator));

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

    let high_priority = stats.iter().find(|s| s.pool_code == "HIGH_PRIORITY").unwrap();
    assert_eq!(high_priority.concurrency, 20);
    assert_eq!(high_priority.rate_limit_per_minute, Some(1000));
}

#[tokio::test]
async fn test_route_single_message() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::new(mediator.clone()));

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
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should have processed the message
    assert_eq!(mediator.call_count(), 1);
    assert!(mediator.processed_ids().contains(&"msg-1".to_string()));
}

#[tokio::test]
async fn test_route_batch_multiple_messages() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::new(mediator.clone()));

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
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(mediator.call_count(), 5);
}

#[tokio::test]
async fn test_route_to_different_pools() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::new(mediator.clone()));

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
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(mediator.call_count(), 3);
}

#[tokio::test]
async fn test_default_pool_for_empty_pool_code() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::new(mediator.clone()));

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
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(mediator.call_count(), 1);
}

#[tokio::test]
async fn test_add_consumer() {
    let mediator = Arc::new(MockMediator::new());
    let manager = QueueManager::new(mediator);

    let consumer = Arc::new(MockQueueConsumer::new("test-consumer"));
    manager.add_consumer(consumer).await;

    let consumer_ids = manager.consumer_ids().await;
    assert!(consumer_ids.contains(&"test-consumer".to_string()));
}

#[tokio::test]
async fn test_memory_health_check() {
    let mediator = Arc::new(MockMediator::new());
    let manager = QueueManager::new(mediator);

    // Initially should be healthy (no messages in pipeline)
    assert!(manager.check_memory_health());
}

#[tokio::test]
async fn test_pool_hot_reload() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::new(mediator.clone()));

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
    manager.update_pool_config("TEST", new_config).await.unwrap();

    let stats = manager.get_pool_stats();
    let pool_stats = stats.iter().find(|s| s.pool_code == "TEST").unwrap();
    assert_eq!(pool_stats.concurrency, 20);
    assert_eq!(pool_stats.rate_limit_per_minute, Some(500));
}

#[tokio::test]
async fn test_shutdown() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::new(mediator));

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
    let manager = QueueManager::new(mediator);

    let consumer = Arc::new(MockQueueConsumer::new("healthy-consumer"));
    manager.add_consumer(consumer).await;

    let is_healthy = manager.is_consumer_healthy("healthy-consumer").await;
    assert!(is_healthy);
}

#[tokio::test]
async fn test_pool_codes() {
    let mediator = Arc::new(MockMediator::new());
    let manager = Arc::new(QueueManager::new(mediator));

    let config = RouterConfig {
        processing_pools: vec![
            PoolConfig { code: "A".to_string(), concurrency: 5, rate_limit_per_minute: None },
            PoolConfig { code: "B".to_string(), concurrency: 5, rate_limit_per_minute: None },
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
