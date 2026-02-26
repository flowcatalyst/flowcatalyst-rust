//! ActiveMQ Queue Consumer via AMQP
//!
//! Provides an AMQP-based consumer for ActiveMQ (and other AMQP brokers like RabbitMQ).
//! Supports:
//! - Queue-based message consumption
//! - Manual acknowledgment
//! - Message rejection with requeue
//! - Visibility timeout simulation via consumer prefetch

use async_trait::async_trait;
use futures::StreamExt;
use lapin::{
    options::*,
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties, Consumer,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use fc_common::{Message, QueuedMessage};
use crate::{QueueConsumer, QueueError, Result};

/// Configuration for ActiveMQ consumer
#[derive(Debug, Clone)]
pub struct ActiveMqConfig {
    /// AMQP URI (e.g., "amqp://guest:guest@localhost:5672")
    pub uri: String,
    /// Queue name to consume from
    pub queue_name: String,
    /// Consumer tag for identification
    pub consumer_tag: String,
    /// Prefetch count (similar to visibility - limits concurrent processing)
    pub prefetch_count: u16,
    /// Whether to auto-create the queue if it doesn't exist
    pub auto_create_queue: bool,
    /// Queue durability
    pub durable: bool,
}

impl Default for ActiveMqConfig {
    fn default() -> Self {
        Self {
            uri: "amqp://guest:guest@localhost:5672".to_string(),
            queue_name: "flowcatalyst".to_string(),
            consumer_tag: format!("fc-consumer-{}", uuid::Uuid::new_v4()),
            prefetch_count: 10,
            auto_create_queue: true,
            durable: true,
        }
    }
}

/// ActiveMQ/AMQP queue consumer
pub struct ActiveMqConsumer {
    config: ActiveMqConfig,
    connection: Arc<RwLock<Option<Connection>>>,
    channel: Arc<RwLock<Option<Channel>>>,
    consumer: Arc<RwLock<Option<Consumer>>>,
    running: AtomicBool,
    delivery_tag_counter: AtomicU64,
    /// Maps our internal receipt handles to AMQP delivery tags
    delivery_tags: Arc<dashmap::DashMap<String, u64>>,
}

impl ActiveMqConsumer {
    /// Create a new ActiveMQ consumer with the given configuration
    pub async fn new(config: ActiveMqConfig) -> Result<Self> {
        let consumer = Self {
            config,
            connection: Arc::new(RwLock::new(None)),
            channel: Arc::new(RwLock::new(None)),
            consumer: Arc::new(RwLock::new(None)),
            running: AtomicBool::new(false),
            delivery_tag_counter: AtomicU64::new(0),
            delivery_tags: Arc::new(dashmap::DashMap::new()),
        };

        consumer.connect().await?;
        Ok(consumer)
    }

    /// Create with default configuration
    pub async fn with_uri(uri: &str, queue_name: &str) -> Result<Self> {
        let config = ActiveMqConfig {
            uri: uri.to_string(),
            queue_name: queue_name.to_string(),
            ..Default::default()
        };
        Self::new(config).await
    }

    /// Establish connection to the broker
    async fn connect(&self) -> Result<()> {
        info!(uri = %self.config.uri, queue = %self.config.queue_name, "Connecting to AMQP broker");

        let connection = Connection::connect(
            &self.config.uri,
            ConnectionProperties::default()
                .with_connection_name("flowcatalyst-router".into()),
        )
        .await
        .map_err(|e| QueueError::Database(format!("AMQP connection failed: {}", e)))?;

        let channel = connection
            .create_channel()
            .await
            .map_err(|e| QueueError::Database(format!("Failed to create channel: {}", e)))?;

        // Set prefetch count (QoS)
        channel
            .basic_qos(self.config.prefetch_count, BasicQosOptions::default())
            .await
            .map_err(|e| QueueError::Database(format!("Failed to set QoS: {}", e)))?;

        // Declare queue if auto-create is enabled
        if self.config.auto_create_queue {
            channel
                .queue_declare(
                    &self.config.queue_name,
                    QueueDeclareOptions {
                        durable: self.config.durable,
                        ..Default::default()
                    },
                    FieldTable::default(),
                )
                .await
                .map_err(|e| QueueError::Database(format!("Failed to declare queue: {}", e)))?;
        }

        // Create consumer
        let consumer = channel
            .basic_consume(
                &self.config.queue_name,
                &self.config.consumer_tag,
                BasicConsumeOptions {
                    no_ack: false, // We need manual ack
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| QueueError::Database(format!("Failed to create consumer: {}", e)))?;

        // Store connection state
        *self.connection.write().await = Some(connection);
        *self.channel.write().await = Some(channel);
        *self.consumer.write().await = Some(consumer);
        self.running.store(true, Ordering::SeqCst);

        info!(queue = %self.config.queue_name, "Connected to AMQP broker");
        Ok(())
    }

    /// Reconnect to the broker
    async fn reconnect(&self) -> Result<()> {
        warn!(queue = %self.config.queue_name, "Reconnecting to AMQP broker");

        // Clear old state
        *self.consumer.write().await = None;
        *self.channel.write().await = None;
        *self.connection.write().await = None;

        // Reconnect
        self.connect().await
    }

    /// Generate a unique receipt handle for our tracking
    fn generate_receipt_handle(&self, delivery_tag: u64) -> String {
        let handle = format!(
            "{}:{}:{}",
            self.config.queue_name,
            delivery_tag,
            self.delivery_tag_counter.fetch_add(1, Ordering::SeqCst)
        );
        self.delivery_tags.insert(handle.clone(), delivery_tag);
        handle
    }

    /// Get the AMQP delivery tag from our receipt handle
    fn get_delivery_tag(&self, receipt_handle: &str) -> Option<u64> {
        self.delivery_tags.get(receipt_handle).map(|r| *r.value())
    }

    /// Remove tracking for a receipt handle
    fn remove_receipt_handle(&self, receipt_handle: &str) {
        self.delivery_tags.remove(receipt_handle);
    }
}

#[async_trait]
impl QueueConsumer for ActiveMqConsumer {
    fn identifier(&self) -> &str {
        &self.config.queue_name
    }

    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(QueueError::Stopped);
        }

        let consumer_guard = self.consumer.read().await;
        let consumer = match consumer_guard.as_ref() {
            Some(c) => c,
            None => {
                drop(consumer_guard);
                self.reconnect().await?;
                return Ok(vec![]);
            }
        };

        let mut messages = Vec::with_capacity(max_messages as usize);
        let mut consumer_stream = consumer.clone();

        // Poll for messages with a timeout
        let timeout = tokio::time::Duration::from_millis(100);

        for _ in 0..max_messages {
            let result = tokio::time::timeout(timeout, consumer_stream.next()).await;

            match result {
                Ok(Some(Ok(delivery))) => {
                    // Parse the message body
                    match serde_json::from_slice::<Message>(&delivery.data) {
                        Ok(message) => {
                            let receipt_handle = self.generate_receipt_handle(delivery.delivery_tag);
                            let broker_message_id = delivery
                                .properties
                                .message_id()
                                .as_ref()
                                .map(|s| s.to_string());

                            messages.push(QueuedMessage {
                                message,
                                receipt_handle,
                                broker_message_id,
                                queue_identifier: self.config.queue_name.clone(),
                            });
                        }
                        Err(e) => {
                            error!(
                                queue = %self.config.queue_name,
                                error = %e,
                                "Failed to parse AMQP message"
                            );
                            // Reject the malformed message (don't requeue)
                            if let Some(channel) = self.channel.read().await.as_ref() {
                                let _ = channel
                                    .basic_reject(
                                        delivery.delivery_tag,
                                        BasicRejectOptions { requeue: false },
                                    )
                                    .await;
                            }
                        }
                    }
                }
                Ok(Some(Err(e))) => {
                    error!(queue = %self.config.queue_name, error = %e, "Error receiving message");
                    break;
                }
                Ok(None) => {
                    // Consumer stream ended
                    warn!(queue = %self.config.queue_name, "Consumer stream ended");
                    break;
                }
                Err(_) => {
                    // Timeout - no more messages available
                    break;
                }
            }
        }

        if !messages.is_empty() {
            debug!(
                queue = %self.config.queue_name,
                count = messages.len(),
                "Polled messages from AMQP"
            );
        }

        Ok(messages)
    }

    async fn ack(&self, receipt_handle: &str) -> Result<()> {
        let delivery_tag = self
            .get_delivery_tag(receipt_handle)
            .ok_or_else(|| QueueError::NotFound(receipt_handle.to_string()))?;

        let channel_guard = self.channel.read().await;
        let channel = channel_guard
            .as_ref()
            .ok_or_else(|| QueueError::Database("Not connected".to_string()))?;

        channel
            .basic_ack(delivery_tag, BasicAckOptions::default())
            .await
            .map_err(|e| QueueError::Database(format!("ACK failed: {}", e)))?;

        self.remove_receipt_handle(receipt_handle);

        debug!(
            receipt_handle = %receipt_handle,
            delivery_tag = delivery_tag,
            queue = %self.config.queue_name,
            "Message acknowledged in AMQP"
        );

        Ok(())
    }

    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        let delivery_tag = self
            .get_delivery_tag(receipt_handle)
            .ok_or_else(|| QueueError::NotFound(receipt_handle.to_string()))?;

        let channel_guard = self.channel.read().await;
        let channel = channel_guard
            .as_ref()
            .ok_or_else(|| QueueError::Database("Not connected".to_string()))?;

        // For delayed retry, we could use dead-letter exchanges or message TTL
        // For simplicity, we just reject with requeue
        // A more advanced implementation would use DLX with TTL
        let requeue = true;

        channel
            .basic_nack(
                delivery_tag,
                BasicNackOptions {
                    requeue,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| QueueError::Database(format!("NACK failed: {}", e)))?;

        self.remove_receipt_handle(receipt_handle);

        debug!(
            receipt_handle = %receipt_handle,
            delivery_tag = delivery_tag,
            queue = %self.config.queue_name,
            delay_seconds = ?delay_seconds,
            "Message NACKed in AMQP (requeued)"
        );

        Ok(())
    }

    async fn extend_visibility(&self, receipt_handle: &str, _seconds: u32) -> Result<()> {
        // AMQP doesn't have visibility timeout like SQS
        // With prefetch, messages are held by the consumer until ACK/NACK
        // This is a no-op for AMQP
        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.config.queue_name,
            "Visibility extension not applicable for AMQP (message held by consumer)"
        );
        Ok(())
    }

    fn is_healthy(&self) -> bool {
        if !self.running.load(Ordering::SeqCst) {
            return false;
        }

        // Check if connection is still open
        // This is a simple check - a more robust implementation would
        // ping the broker periodically
        true
    }

    async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);

        // Cancel the consumer
        if let Some(channel) = self.channel.read().await.as_ref() {
            let _ = channel
                .basic_cancel(&self.config.consumer_tag, BasicCancelOptions::default())
                .await;
        }

        // Close channel
        if let Some(channel) = self.channel.write().await.take() {
            let _ = channel.close(200, "Shutdown").await;
        }

        // Close connection
        if let Some(connection) = self.connection.write().await.take() {
            let _ = connection.close(200, "Shutdown").await;
        }

        info!(queue = %self.config.queue_name, "ActiveMQ consumer stopped");
    }
}

/// ActiveMQ/AMQP queue publisher
pub struct ActiveMqPublisher {
    config: ActiveMqConfig,
    connection: Arc<RwLock<Option<Connection>>>,
    channel: Arc<RwLock<Option<Channel>>>,
}

impl ActiveMqPublisher {
    /// Create a new ActiveMQ publisher
    pub async fn new(config: ActiveMqConfig) -> Result<Self> {
        let publisher = Self {
            config,
            connection: Arc::new(RwLock::new(None)),
            channel: Arc::new(RwLock::new(None)),
        };

        publisher.connect().await?;
        Ok(publisher)
    }

    /// Create with URI and queue name
    pub async fn with_uri(uri: &str, queue_name: &str) -> Result<Self> {
        let config = ActiveMqConfig {
            uri: uri.to_string(),
            queue_name: queue_name.to_string(),
            ..Default::default()
        };
        Self::new(config).await
    }

    async fn connect(&self) -> Result<()> {
        let connection = Connection::connect(
            &self.config.uri,
            ConnectionProperties::default()
                .with_connection_name("flowcatalyst-publisher".into()),
        )
        .await
        .map_err(|e| QueueError::Database(format!("AMQP connection failed: {}", e)))?;

        let channel = connection
            .create_channel()
            .await
            .map_err(|e| QueueError::Database(format!("Failed to create channel: {}", e)))?;

        // Declare queue if auto-create is enabled
        if self.config.auto_create_queue {
            channel
                .queue_declare(
                    &self.config.queue_name,
                    QueueDeclareOptions {
                        durable: self.config.durable,
                        ..Default::default()
                    },
                    FieldTable::default(),
                )
                .await
                .map_err(|e| QueueError::Database(format!("Failed to declare queue: {}", e)))?;
        }

        *self.connection.write().await = Some(connection);
        *self.channel.write().await = Some(channel);

        Ok(())
    }

    /// Publish a message to the queue
    pub async fn publish(&self, message: &Message) -> Result<String> {
        let channel_guard = self.channel.read().await;
        let channel = channel_guard
            .as_ref()
            .ok_or_else(|| QueueError::Database("Not connected".to_string()))?;

        let body = serde_json::to_vec(message)?;
        let message_id = message.id.clone();

        channel
            .basic_publish(
                "", // Default exchange
                &self.config.queue_name,
                BasicPublishOptions::default(),
                &body,
                BasicProperties::default()
                    .with_message_id(message_id.clone().into())
                    .with_delivery_mode(2) // Persistent
                    .with_content_type("application/json".into()),
            )
            .await
            .map_err(|e| QueueError::Database(format!("Publish failed: {}", e)))?
            .await
            .map_err(|e| QueueError::Database(format!("Publish confirm failed: {}", e)))?;

        debug!(
            message_id = %message_id,
            queue = %self.config.queue_name,
            "Message published to AMQP"
        );

        Ok(message_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ActiveMqConfig::default();
        assert_eq!(config.prefetch_count, 10);
        assert!(config.durable);
        assert!(config.auto_create_queue);
    }
}
