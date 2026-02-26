//! SQS Queue Consumer Integration Tests
//!
//! These tests require LocalStack to be running:
//! docker-compose -f docker-compose.test.yml up -d localstack
//!
//! Tests for:
//! - Message polling
//! - Message acknowledgment
//! - Message rejection (NACK)
//! - Visibility timeout extension
//! - Consumer lifecycle

#![cfg(feature = "sqs")]

use std::time::Duration;
use aws_config::{BehaviorVersion, Region};
use aws_sdk_sqs::Client;

use fc_common::{Message, MediationType};
use fc_queue::{QueueConsumer, sqs::SqsQueueConsumer};

const LOCALSTACK_ENDPOINT: &str = "http://localhost:4566";
const TEST_QUEUE_NAME: &str = "test-queue";

async fn create_test_client() -> Client {
    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url(LOCALSTACK_ENDPOINT)
        .load()
        .await;

    Client::new(&config)
}

async fn setup_test_queue(client: &Client) -> String {
    // Delete queue if exists (ignore errors)
    let _ = client
        .delete_queue()
        .queue_url(format!("{}/000000000000/{}", LOCALSTACK_ENDPOINT, TEST_QUEUE_NAME))
        .send()
        .await;

    // Wait for deletion to propagate
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Create queue
    let result = client
        .create_queue()
        .queue_name(TEST_QUEUE_NAME)
        .send()
        .await
        .expect("Failed to create queue");

    result.queue_url().unwrap().to_string()
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

async fn send_test_message(client: &Client, queue_url: &str, message: &Message) -> String {
    let body = serde_json::to_string(message).unwrap();

    let result = client
        .send_message()
        .queue_url(queue_url)
        .message_body(body)
        .send()
        .await
        .expect("Failed to send message");

    result.message_id().unwrap().to_string()
}

/// Check if LocalStack is available
async fn is_localstack_available() -> bool {
    let client = reqwest::Client::new();
    let result = client
        .get(format!("{}/_localstack/health", LOCALSTACK_ENDPOINT))
        .timeout(Duration::from_secs(2))
        .send()
        .await;

    match result {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

#[tokio::test]
async fn test_poll_empty_queue() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        30,
    );

    let messages = consumer.poll(10).await.expect("Poll failed");
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_poll_single_message() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    // Send a message
    let test_msg = create_test_message("msg-1");
    send_test_message(&client, &queue_url, &test_msg).await;

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        30,
    );

    let messages = consumer.poll(10).await.expect("Poll failed");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.id, "msg-1");
}

#[tokio::test]
async fn test_poll_multiple_messages() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    // Send multiple messages
    for i in 0..5 {
        let test_msg = create_test_message(&format!("msg-{}", i));
        send_test_message(&client, &queue_url, &test_msg).await;
    }

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        30,
    );

    let messages = consumer.poll(10).await.expect("Poll failed");
    assert!(!messages.is_empty());
    assert!(messages.len() <= 5);
}

#[tokio::test]
async fn test_message_acknowledgment() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    // Send a message
    let test_msg = create_test_message("msg-ack");
    send_test_message(&client, &queue_url, &test_msg).await;

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        30,
    );

    let messages = consumer.poll(10).await.expect("Poll failed");
    assert_eq!(messages.len(), 1);

    // Acknowledge the message
    consumer.ack(&messages[0].receipt_handle).await.expect("Ack failed");

    // Poll again - should be empty
    let messages = consumer.poll(10).await.expect("Poll failed");
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_message_nack_immediate_retry() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    // Send a message
    let test_msg = create_test_message("msg-nack");
    send_test_message(&client, &queue_url, &test_msg).await;

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        30,
    );

    let messages = consumer.poll(10).await.expect("Poll failed");
    assert_eq!(messages.len(), 1);

    // NACK with 0 delay (immediate retry)
    consumer.nack(&messages[0].receipt_handle, Some(0)).await.expect("Nack failed");

    // Wait a moment then poll again - message should be available
    tokio::time::sleep(Duration::from_millis(500)).await;
    let messages = consumer.poll(10).await.expect("Poll failed");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.id, "msg-nack");
}

#[tokio::test]
async fn test_visibility_timeout_extension() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    // Send a message
    let test_msg = create_test_message("msg-extend");
    send_test_message(&client, &queue_url, &test_msg).await;

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        5, // Short visibility timeout
    );

    let messages = consumer.poll(10).await.expect("Poll failed");
    assert_eq!(messages.len(), 1);

    // Extend visibility
    consumer.extend_visibility(&messages[0].receipt_handle, 60).await.expect("Extend failed");

    // Message should still be invisible
    let messages2 = consumer.poll(10).await.expect("Poll failed");
    assert!(messages2.is_empty());
}

#[tokio::test]
async fn test_consumer_stop() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        30,
    );

    assert!(consumer.is_healthy());

    consumer.stop().await;

    assert!(!consumer.is_healthy());

    // Poll should return error
    let result = consumer.poll(10).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_consumer_identifier() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        30,
    );

    assert_eq!(consumer.identifier(), TEST_QUEUE_NAME);
}

#[tokio::test]
async fn test_malformed_message_handling() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    // Send a malformed message (not valid JSON for Message struct)
    client
        .send_message()
        .queue_url(&queue_url)
        .message_body("not valid json at all")
        .send()
        .await
        .expect("Failed to send message");

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        30,
    );

    // Poll should return empty (malformed message is auto-acked)
    let messages = consumer.poll(10).await.expect("Poll failed");
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_batch_send_and_receive() {
    if !is_localstack_available().await {
        eprintln!("Skipping test - LocalStack not available");
        return;
    }

    let client = create_test_client().await;
    let queue_url = setup_test_queue(&client).await;

    // Send batch of messages
    let mut entries = Vec::new();
    for i in 0..10 {
        let msg = create_test_message(&format!("batch-msg-{}", i));
        let body = serde_json::to_string(&msg).unwrap();
        entries.push(
            aws_sdk_sqs::types::SendMessageBatchRequestEntry::builder()
                .id(format!("{}", i))
                .message_body(body)
                .build()
                .unwrap()
        );
    }

    client
        .send_message_batch()
        .queue_url(&queue_url)
        .set_entries(Some(entries))
        .send()
        .await
        .expect("Failed to send batch");

    let consumer = SqsQueueConsumer::new(
        client.clone(),
        queue_url,
        TEST_QUEUE_NAME.to_string(),
        30,
    );

    // Poll for messages (SQS max is 10 per poll)
    let messages = consumer.poll(10).await.expect("Poll failed");
    assert!(!messages.is_empty());
}
