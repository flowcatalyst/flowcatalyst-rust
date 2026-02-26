//! ActiveMQ Queue Consumer Integration Tests
//!
//! These tests require ActiveMQ Artemis to be running:
//! docker-compose -f docker-compose.test.yml up -d activemq
//!
//! Tests for:
//! - Connection establishment
//! - Message polling
//! - Message acknowledgment
//! - Message rejection (NACK)
//! - Consumer lifecycle
//! - Publisher functionality

#![cfg(feature = "activemq")]

use std::time::Duration;

use fc_common::{Message, MediationType};
use fc_queue::{QueueConsumer, activemq::{ActiveMqConfig, ActiveMqConsumer, ActiveMqPublisher}};
use chrono::Utc;
use reqwest;

const AMQP_URI: &str = "amqp://admin:admin@localhost:5672";
const TEST_QUEUE_NAME: &str = "test-queue";

fn create_test_config() -> ActiveMqConfig {
    ActiveMqConfig {
        uri: AMQP_URI.to_string(),
        queue_name: TEST_QUEUE_NAME.to_string(),
        consumer_tag: format!("test-consumer-{}", uuid::Uuid::new_v4()),
        prefetch_count: 10,
        auto_create_queue: true,
        durable: false, // Non-durable for tests
    }
}

fn create_test_message(id: &str) -> Message {
    Message {
        id: id.to_string(),
        pool_code: "DEFAULT".to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: "http://localhost:8080/test".to_string(),
        message_group_id: None,
        high_priority: false,
    }
}

/// Check if ActiveMQ is available
async fn is_activemq_available() -> bool {
    let client = reqwest::Client::new();
    let result = client
        .get("http://localhost:8161/console")
        .timeout(Duration::from_secs(2))
        .send()
        .await;

    match result {
        Ok(resp) => resp.status().is_success() || resp.status().as_u16() == 401,
        Err(_) => false,
    }
}

#[tokio::test]
async fn test_consumer_creation() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let config = create_test_config();
    let consumer = ActiveMqConsumer::new(config).await;

    assert!(consumer.is_ok(), "Failed to create consumer: {:?}", consumer.err());

    let consumer = consumer.unwrap();
    assert!(consumer.is_healthy());
    consumer.stop().await;
}

#[tokio::test]
async fn test_consumer_with_uri() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let consumer = ActiveMqConsumer::with_uri(AMQP_URI, "test-queue-uri").await;

    assert!(consumer.is_ok(), "Failed to create consumer: {:?}", consumer.err());

    let consumer = consumer.unwrap();
    assert!(consumer.is_healthy());
    consumer.stop().await;
}

#[tokio::test]
async fn test_poll_empty_queue() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let config = ActiveMqConfig {
        queue_name: format!("empty-queue-{}", uuid::Uuid::new_v4()),
        ..create_test_config()
    };

    let consumer = ActiveMqConsumer::new(config).await.expect("Failed to create consumer");

    let messages = consumer.poll(10).await.expect("Poll failed");
    assert!(messages.is_empty());

    consumer.stop().await;
}

#[tokio::test]
async fn test_publish_and_consume() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let queue_name = format!("pub-con-test-{}", uuid::Uuid::new_v4());

    // Create publisher
    let pub_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        ..create_test_config()
    };
    let publisher = ActiveMqPublisher::new(pub_config).await.expect("Failed to create publisher");

    // Publish a message
    let test_msg = create_test_message("amqp-msg-1");
    let msg_id = publisher.publish(&test_msg).await.expect("Publish failed");
    assert_eq!(msg_id, "amqp-msg-1");

    // Create consumer
    let con_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        ..create_test_config()
    };
    let consumer = ActiveMqConsumer::new(con_config).await.expect("Failed to create consumer");

    // Poll for the message
    tokio::time::sleep(Duration::from_millis(100)).await;
    let messages = consumer.poll(10).await.expect("Poll failed");

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.id, "amqp-msg-1");

    consumer.stop().await;
}

#[tokio::test]
async fn test_message_acknowledgment() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let queue_name = format!("ack-test-{}", uuid::Uuid::new_v4());

    // Publish a message
    let pub_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        ..create_test_config()
    };
    let publisher = ActiveMqPublisher::new(pub_config).await.expect("Failed to create publisher");
    let test_msg = create_test_message("amqp-msg-ack");
    publisher.publish(&test_msg).await.expect("Publish failed");

    // Consume and acknowledge
    let con_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        ..create_test_config()
    };
    let consumer = ActiveMqConsumer::new(con_config).await.expect("Failed to create consumer");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let messages = consumer.poll(10).await.expect("Poll failed");
    assert_eq!(messages.len(), 1);

    // Acknowledge the message
    consumer.ack(&messages[0].receipt_handle).await.expect("Ack failed");

    // Create a new consumer to check message is gone
    tokio::time::sleep(Duration::from_millis(100)).await;
    let messages = consumer.poll(10).await.expect("Poll failed");
    assert!(messages.is_empty());

    consumer.stop().await;
}

#[tokio::test]
async fn test_message_nack_requeue() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let queue_name = format!("nack-test-{}", uuid::Uuid::new_v4());

    // Publish a message
    let pub_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        ..create_test_config()
    };
    let publisher = ActiveMqPublisher::new(pub_config).await.expect("Failed to create publisher");
    let test_msg = create_test_message("amqp-msg-nack");
    publisher.publish(&test_msg).await.expect("Publish failed");

    // Consume and NACK
    let con_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        prefetch_count: 1,
        ..create_test_config()
    };
    let consumer = ActiveMqConsumer::new(con_config).await.expect("Failed to create consumer");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let messages = consumer.poll(10).await.expect("Poll failed");
    assert_eq!(messages.len(), 1);

    // NACK the message (will requeue)
    consumer.nack(&messages[0].receipt_handle, None).await.expect("Nack failed");

    // Message should be requeued and available again
    tokio::time::sleep(Duration::from_millis(200)).await;
    let messages = consumer.poll(10).await.expect("Poll failed");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.id, "amqp-msg-nack");

    consumer.stop().await;
}

#[tokio::test]
async fn test_consumer_stop() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let config = create_test_config();
    let consumer = ActiveMqConsumer::new(config).await.expect("Failed to create consumer");

    assert!(consumer.is_healthy());

    consumer.stop().await;

    assert!(!consumer.is_healthy());

    // Poll should return error
    let result = consumer.poll(10).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_consumer_identifier() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let config = create_test_config();
    let consumer = ActiveMqConsumer::new(config).await.expect("Failed to create consumer");

    assert_eq!(consumer.identifier(), TEST_QUEUE_NAME);

    consumer.stop().await;
}

#[tokio::test]
async fn test_extend_visibility_noop() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let queue_name = format!("extend-test-{}", uuid::Uuid::new_v4());

    let pub_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        ..create_test_config()
    };
    let publisher = ActiveMqPublisher::new(pub_config).await.expect("Failed to create publisher");
    let test_msg = create_test_message("amqp-msg-extend");
    publisher.publish(&test_msg).await.expect("Publish failed");

    let con_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        ..create_test_config()
    };
    let consumer = ActiveMqConsumer::new(con_config).await.expect("Failed to create consumer");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let messages = consumer.poll(10).await.expect("Poll failed");
    assert_eq!(messages.len(), 1);

    // Extend visibility should succeed (no-op for AMQP)
    let result = consumer.extend_visibility(&messages[0].receipt_handle, 60).await;
    assert!(result.is_ok());

    consumer.stop().await;
}

#[tokio::test]
async fn test_multiple_messages() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let queue_name = format!("multi-test-{}", uuid::Uuid::new_v4());

    // Publish multiple messages
    let pub_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        ..create_test_config()
    };
    let publisher = ActiveMqPublisher::new(pub_config).await.expect("Failed to create publisher");

    for i in 0..5 {
        let test_msg = create_test_message(&format!("amqp-multi-{}", i));
        publisher.publish(&test_msg).await.expect("Publish failed");
    }

    // Consume all messages
    let con_config = ActiveMqConfig {
        queue_name: queue_name.clone(),
        prefetch_count: 10,
        ..create_test_config()
    };
    let consumer = ActiveMqConsumer::new(con_config).await.expect("Failed to create consumer");

    tokio::time::sleep(Duration::from_millis(200)).await;
    let messages = consumer.poll(10).await.expect("Poll failed");

    assert!(!messages.is_empty());
    assert!(messages.len() <= 5);

    consumer.stop().await;
}

#[tokio::test]
async fn test_publisher_with_uri() {
    if !is_activemq_available().await {
        eprintln!("Skipping test - ActiveMQ not available");
        return;
    }

    let queue_name = format!("pub-uri-test-{}", uuid::Uuid::new_v4());
    let publisher = ActiveMqPublisher::with_uri(AMQP_URI, &queue_name).await;

    assert!(publisher.is_ok(), "Failed to create publisher: {:?}", publisher.err());

    let publisher = publisher.unwrap();
    let test_msg = create_test_message("pub-uri-msg");
    let result = publisher.publish(&test_msg).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_default_config() {
    let config = ActiveMqConfig::default();
    assert_eq!(config.prefetch_count, 10);
    assert!(config.durable);
    assert!(config.auto_create_queue);
}
