//! End-to-End Integration Tests
//!
//! Tests the complete message flow:
//! Message → Queue → Router → Pool → Mediator → Target
//!
//! These tests use wiremock for HTTP target simulation.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::time::Duration;
use async_trait::async_trait;
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

use fc_common::{
    Message, QueuedMessage, MediationType, PoolConfig, RouterConfig,
};
use fc_queue::{QueueConsumer, QueueError};
use fc_router::{QueueManager, HttpMediator, HttpMediatorConfig};
use chrono::Utc;

/// Mock queue consumer that provides test messages
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

    fn acked_handles(&self) -> Vec<String> {
        self.acked.lock().clone()
    }

    fn nacked_handles(&self) -> Vec<(String, Option<u32>)> {
        self.nacked.lock().clone()
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

fn create_test_message(id: &str, pool_code: &str, target: &str) -> Message {
    Message {
        id: id.to_string(),
        pool_code: pool_code.to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: target.to_string(),
        message_group_id: None,
        high_priority: false,
    }
}

fn create_queued_message(id: &str, pool_code: &str, target: &str, queue_id: &str) -> QueuedMessage {
    QueuedMessage {
        message: create_test_message(id, pool_code, target),
        receipt_handle: format!("receipt-{}", id),
        broker_message_id: Some(format!("broker-{}", id)),
        queue_identifier: queue_id.to_string(),
    }
}

#[tokio::test]
async fn test_end_to_end_successful_delivery() {
    // Start mock HTTP server
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ack": true})))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Create mediator pointing to mock server
    let config = HttpMediatorConfig {
        max_retries: 1,
        ..Default::default()
    };
    let mediator = Arc::new(HttpMediator::with_config(config));
    let manager = Arc::new(QueueManager::new(mediator.clone()));

    // Configure pool
    let router_config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(router_config).await.unwrap();

    // Create consumer with one message
    let target = format!("{}/webhook", mock_server.uri());
    let messages = vec![create_queued_message("msg-1", "DEFAULT", &target, "test-queue")];
    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));
    for msg in messages {
        consumer.add_message(msg);
    }

    // Poll and route
    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify ACK was sent
    let acked = consumer.acked_handles();
    assert_eq!(acked.len(), 1);
    assert_eq!(acked[0], "receipt-msg-1");
}

#[tokio::test]
async fn test_end_to_end_failed_delivery() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let config = HttpMediatorConfig {
        max_retries: 1,
        retry_delays: vec![Duration::from_millis(10)],
        ..Default::default()
    };
    let mediator = Arc::new(HttpMediator::with_config(config));
    let manager = Arc::new(QueueManager::new(mediator.clone()));

    let router_config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(router_config).await.unwrap();

    let target = format!("{}/webhook", mock_server.uri());
    let messages = vec![create_queued_message("msg-fail", "DEFAULT", &target, "test-queue")];
    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));
    for msg in messages {
        consumer.add_message(msg);
    }

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Verify NACK was sent (failure)
    let nacked = consumer.nacked_handles();
    assert_eq!(nacked.len(), 1);
    assert_eq!(nacked[0].0, "receipt-msg-fail");
}

#[tokio::test]
async fn test_end_to_end_config_error_no_retry() {
    let mock_server = MockServer::start().await;

    // 400 is a config error - should not retry
    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = HttpMediatorConfig {
        max_retries: 3,
        ..Default::default()
    };
    let mediator = Arc::new(HttpMediator::with_config(config));
    let manager = Arc::new(QueueManager::new(mediator.clone()));

    let router_config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(router_config).await.unwrap();

    let target = format!("{}/webhook", mock_server.uri());
    let messages = vec![create_queued_message("msg-400", "DEFAULT", &target, "test-queue")];
    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));
    for msg in messages {
        consumer.add_message(msg);
    }

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Config errors (4xx) should be ACK'd to prevent infinite retries
    // The message is removed from the queue without retry
    let acked = consumer.acked_handles();
    assert_eq!(acked.len(), 1);
    assert_eq!(acked[0], "receipt-msg-400");
}

#[tokio::test]
async fn test_end_to_end_multiple_pools() {
    let mock_server = MockServer::start().await;

    let request_count = Arc::new(AtomicU32::new(0));
    let count_clone = request_count.clone();

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(move |_req: &wiremock::Request| {
            count_clone.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200)
        })
        .mount(&mock_server)
        .await;

    let mediator = Arc::new(HttpMediator::new());
    let manager = Arc::new(QueueManager::new(mediator.clone()));

    let router_config = RouterConfig {
        processing_pools: vec![
            PoolConfig { code: "POOL_A".to_string(), concurrency: 5, rate_limit_per_minute: None },
            PoolConfig { code: "POOL_B".to_string(), concurrency: 5, rate_limit_per_minute: None },
        ],
        queues: vec![],
    };
    manager.apply_config(router_config).await.unwrap();

    let target = format!("{}/webhook", mock_server.uri());
    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add messages to different pools
    consumer.add_message(create_queued_message("msg-a1", "POOL_A", &target, "test-queue"));
    consumer.add_message(create_queued_message("msg-b1", "POOL_B", &target, "test-queue"));
    consumer.add_message(create_queued_message("msg-a2", "POOL_A", &target, "test-queue"));

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    // All 3 messages should be processed
    assert_eq!(request_count.load(Ordering::SeqCst), 3);
    assert_eq!(consumer.acked_handles().len(), 3);
}

#[tokio::test]
async fn test_end_to_end_custom_delay_response() {
    let mock_server = MockServer::start().await;

    // Target returns ack=false with custom delay
    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"ack": false, "delaySeconds": 120}))
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = HttpMediatorConfig {
        max_retries: 1,
        ..Default::default()
    };
    let mediator = Arc::new(HttpMediator::with_config(config));
    let manager = Arc::new(QueueManager::new(mediator.clone()));

    let router_config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(router_config).await.unwrap();

    let target = format!("{}/webhook", mock_server.uri());
    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));
    consumer.add_message(create_queued_message("msg-delay", "DEFAULT", &target, "test-queue"));

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Should NACK with delay
    let nacked = consumer.nacked_handles();
    assert_eq!(nacked.len(), 1);
    // Delay should be Some(120)
    assert_eq!(nacked[0].1, Some(120));
}

#[tokio::test]
async fn test_end_to_end_batch_processing() {
    let mock_server = MockServer::start().await;

    let request_count = Arc::new(AtomicU32::new(0));
    let count_clone = request_count.clone();

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(move |_req: &wiremock::Request| {
            count_clone.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200)
        })
        .mount(&mock_server)
        .await;

    let mediator = Arc::new(HttpMediator::new());
    let manager = Arc::new(QueueManager::new(mediator.clone()));

    let router_config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 10,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(router_config).await.unwrap();

    let target = format!("{}/webhook", mock_server.uri());
    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));

    // Add a batch of 20 messages
    for i in 0..20 {
        consumer.add_message(create_queued_message(
            &format!("msg-{}", i),
            "DEFAULT",
            &target,
            "test-queue"
        ));
    }

    let poll_result = consumer.poll(20).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    // All messages should be processed
    assert_eq!(request_count.load(Ordering::SeqCst), 20);
    assert_eq!(consumer.acked_handles().len(), 20);
}

#[tokio::test]
async fn test_end_to_end_connection_error() {
    // Use a port that's definitely not listening
    let target = "http://127.0.0.1:59999/webhook";

    let config = HttpMediatorConfig {
        max_retries: 1,
        retry_delays: vec![Duration::from_millis(10)],
        timeout: Duration::from_millis(100),
        ..Default::default()
    };
    let mediator = Arc::new(HttpMediator::with_config(config));
    let manager = Arc::new(QueueManager::new(mediator.clone()));

    let router_config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(router_config).await.unwrap();

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));
    consumer.add_message(create_queued_message("msg-conn-err", "DEFAULT", target, "test-queue"));

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Should NACK due to connection error
    let nacked = consumer.nacked_handles();
    assert_eq!(nacked.len(), 1);
}

#[tokio::test]
async fn test_end_to_end_shutdown() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let mediator = Arc::new(HttpMediator::new());
    let manager = Arc::new(QueueManager::new(mediator.clone()));

    let router_config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(router_config).await.unwrap();

    // Shutdown should complete gracefully
    manager.shutdown().await;
}

#[tokio::test]
async fn test_end_to_end_auth_token() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/secure-webhook"))
        .and(wiremock::matchers::header("Authorization", "Bearer test-token-123"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = Arc::new(HttpMediator::new());
    let manager = Arc::new(QueueManager::new(mediator.clone()));

    let router_config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        }],
        queues: vec![],
    };
    manager.apply_config(router_config).await.unwrap();

    let target = format!("{}/secure-webhook", mock_server.uri());
    let mut message = create_test_message("msg-auth", "DEFAULT", &target);
    message.auth_token = Some("test-token-123".to_string());

    let consumer = Arc::new(TestQueueConsumer::new("test-queue"));
    consumer.add_message(QueuedMessage {
        message,
        receipt_handle: "receipt-msg-auth".to_string(),
        broker_message_id: Some("broker-msg-auth".to_string()),
        queue_identifier: "test-queue".to_string(),
    });

    let poll_result = consumer.poll(10).await.unwrap();
    manager.route_batch(poll_result, consumer.clone()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Should ACK - auth header was correctly sent
    assert_eq!(consumer.acked_handles().len(), 1);
}
