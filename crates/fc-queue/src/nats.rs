//! NATS JetStream Queue Consumer
//!
//! Provides a pull-based JetStream consumer for NATS.
//! Supports:
//! - Pull-based message consumption with configurable batch sizes
//! - Manual acknowledgment with ack/nak/in-progress semantics
//! - Durable consumers with configurable ack wait and max deliver
//! - Automatic stream and consumer provisioning
//! - Queue metrics from JetStream consumer info

use async_nats::jetstream::{self, consumer::PullConsumer, stream, AckKind};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::{QueueConsumer, QueueError, QueueMetrics, Result};
use fc_common::{Message, QueuedMessage};

/// Configuration for the NATS JetStream consumer
#[derive(Debug, Clone)]
pub struct NatsConfig {
    /// NATS server URL(s), comma-separated (e.g., "nats://localhost:4222")
    pub servers: String,
    /// JetStream stream name
    pub stream_name: String,
    /// Durable consumer name
    pub consumer_name: String,
    /// Subject filter for the consumer (e.g., "flowcatalyst.>")
    pub subject: String,
    /// Max messages to request per poll batch
    pub max_messages_per_poll: u32,
    /// Timeout in milliseconds for each poll/fetch request
    pub poll_timeout_ms: u64,
    /// Ack wait time in seconds before redelivery
    pub ack_wait_secs: u64,
    /// Maximum number of delivery attempts before giving up
    pub max_deliver: i64,
    /// Maximum number of unacknowledged messages the consumer can have in-flight
    pub max_ack_pending: i64,
    /// Stream storage type: "file" or "memory"
    pub storage: String,
    /// Number of stream replicas (for clustering)
    pub replicas: usize,
    /// Maximum message age in days (0 = unlimited)
    pub max_age_days: u64,
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            servers: "nats://localhost:4222".to_string(),
            stream_name: "FLOWCATALYST".to_string(),
            consumer_name: "fc-router".to_string(),
            subject: "flowcatalyst.>".to_string(),
            max_messages_per_poll: 10,
            poll_timeout_ms: 5000,
            ack_wait_secs: 30,
            max_deliver: 5,
            max_ack_pending: 1000,
            storage: "file".to_string(),
            replicas: 1,
            max_age_days: 7,
        }
    }
}

/// NATS JetStream pull-based queue consumer
pub struct NatsQueueConsumer {
    config: NatsConfig,
    client: async_nats::Client,
    consumer: Arc<RwLock<PullConsumer>>,
    running: AtomicBool,
    /// Maps receipt handle (stream sequence as string) -> JetStream message for ack/nack
    pending_messages: Arc<DashMap<String, async_nats::jetstream::Message>>,
    /// Total messages polled from queue
    total_polled: AtomicU64,
    /// Total messages successfully ACKed
    total_acked: AtomicU64,
    /// Total messages NACKed (actual failures)
    total_nacked: AtomicU64,
    /// Total messages deferred (rate limiting, capacity - not failures)
    total_deferred: AtomicU64,
}

impl NatsQueueConsumer {
    /// Create a new NATS JetStream consumer.
    ///
    /// This connects to the NATS server, ensures the stream exists (creates it if needed),
    /// and ensures the durable consumer exists (creates it if needed).
    pub async fn new(config: NatsConfig) -> Result<Self> {
        info!(
            servers = %config.servers,
            stream = %config.stream_name,
            consumer = %config.consumer_name,
            subject = %config.subject,
            "Connecting to NATS JetStream"
        );

        // Connect to NATS
        let client = async_nats::connect(&config.servers)
            .await
            .map_err(|e| QueueError::Nats(format!("Failed to connect to NATS: {}", e)))?;

        info!(servers = %config.servers, "Connected to NATS");

        // Create JetStream context
        let jetstream = jetstream::new(client.clone());

        // Resolve storage type
        let storage_type = match config.storage.to_lowercase().as_str() {
            "memory" => stream::StorageType::Memory,
            _ => stream::StorageType::File,
        };

        // Calculate max age duration
        let max_age = if config.max_age_days > 0 {
            Duration::from_secs(config.max_age_days * 24 * 60 * 60)
        } else {
            Duration::ZERO // 0 means unlimited in NATS
        };

        // Ensure stream exists (create or get)
        let stream = jetstream
            .get_or_create_stream(stream::Config {
                name: config.stream_name.clone(),
                subjects: vec![config.subject.clone()],
                storage: storage_type,
                num_replicas: config.replicas,
                max_age,
                ..Default::default()
            })
            .await
            .map_err(|e| QueueError::Nats(format!("Failed to get/create stream '{}': {}", config.stream_name, e)))?;

        info!(
            stream = %config.stream_name,
            storage = %config.storage,
            replicas = config.replicas,
            "JetStream stream ready"
        );

        // Ensure durable consumer exists (create or get)
        let consumer = stream
            .get_or_create_consumer(
                &config.consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(config.consumer_name.clone()),
                    ack_wait: Duration::from_secs(config.ack_wait_secs),
                    max_deliver: config.max_deliver,
                    max_ack_pending: config.max_ack_pending,
                    filter_subject: config.subject.clone(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| QueueError::Nats(format!("Failed to get/create consumer '{}': {}", config.consumer_name, e)))?;

        info!(
            consumer = %config.consumer_name,
            ack_wait_secs = config.ack_wait_secs,
            max_deliver = config.max_deliver,
            max_ack_pending = config.max_ack_pending,
            "JetStream consumer ready"
        );

        Ok(Self {
            config,
            client,
            consumer: Arc::new(RwLock::new(consumer)),
            running: AtomicBool::new(true),
            pending_messages: Arc::new(DashMap::new()),
            total_polled: AtomicU64::new(0),
            total_acked: AtomicU64::new(0),
            total_nacked: AtomicU64::new(0),
            total_deferred: AtomicU64::new(0),
        })
    }

    /// Extract stream sequence number from a JetStream message to use as receipt handle.
    fn receipt_handle_from_message(msg: &async_nats::jetstream::Message) -> Option<String> {
        msg.info()
            .ok()
            .map(|info| info.stream_sequence.to_string())
    }
}

#[async_trait]
impl QueueConsumer for NatsQueueConsumer {
    fn identifier(&self) -> &str {
        &self.config.consumer_name
    }

    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(QueueError::Stopped);
        }

        let consumer = self.consumer.read().await;
        let batch_size = max_messages.min(self.config.max_messages_per_poll) as usize;
        let timeout = Duration::from_millis(self.config.poll_timeout_ms);

        // Use fetch() for pull-based consumption with a timeout
        let mut batch = consumer
            .fetch()
            .max_messages(batch_size)
            .expires(timeout)
            .messages()
            .await
            .map_err(|e| QueueError::Nats(format!("Failed to fetch messages: {}", e)))?;

        let mut messages = Vec::with_capacity(batch_size);

        while let Some(msg_result) = batch.next().await {
            match msg_result {
                Ok(js_msg) => {
                    // Extract receipt handle from stream sequence
                    let receipt_handle = match Self::receipt_handle_from_message(&js_msg) {
                        Some(handle) => handle,
                        None => {
                            warn!(
                                consumer = %self.config.consumer_name,
                                "Could not extract stream sequence from NATS message, skipping"
                            );
                            // Term the message since we can't track it
                            let _ = js_msg.ack_with(AckKind::Term).await;
                            continue;
                        }
                    };

                    // Parse the message body
                    match serde_json::from_slice::<Message>(&js_msg.payload) {
                        Ok(message) => {
                            let broker_message_id = js_msg
                                .info()
                                .ok()
                                .map(|info| format!("{}:{}", info.stream_sequence, info.consumer_sequence));

                            // Store the JetStream message for later ack/nack
                            self.pending_messages.insert(receipt_handle.clone(), js_msg);

                            messages.push(QueuedMessage {
                                message,
                                receipt_handle,
                                broker_message_id,
                                queue_identifier: self.config.consumer_name.clone(),
                            });
                        }
                        Err(e) => {
                            error!(
                                consumer = %self.config.consumer_name,
                                error = %e,
                                "Failed to parse NATS message payload, terminating message"
                            );
                            // Term the malformed message to prevent infinite redelivery
                            let _ = js_msg.ack_with(AckKind::Term).await;
                        }
                    }
                }
                Err(e) => {
                    error!(
                        consumer = %self.config.consumer_name,
                        error = %e,
                        "Error receiving NATS message"
                    );
                    break;
                }
            }
        }

        if !messages.is_empty() {
            self.total_polled.fetch_add(messages.len() as u64, Ordering::Relaxed);
            debug!(
                consumer = %self.config.consumer_name,
                count = messages.len(),
                "Polled messages from NATS JetStream"
            );
        }

        Ok(messages)
    }

    async fn ack(&self, receipt_handle: &str) -> Result<()> {
        let (_, js_msg) = self
            .pending_messages
            .remove(receipt_handle)
            .ok_or_else(|| QueueError::NotFound(format!("No pending message for receipt handle: {}", receipt_handle)))?;

        js_msg
            .ack()
            .await
            .map_err(|e| QueueError::Nats(format!("Failed to ACK message: {}", e)))?;

        self.total_acked.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            consumer = %self.config.consumer_name,
            "Message acknowledged in NATS JetStream"
        );

        Ok(())
    }

    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        let (_, js_msg) = self
            .pending_messages
            .remove(receipt_handle)
            .ok_or_else(|| QueueError::NotFound(format!("No pending message for receipt handle: {}", receipt_handle)))?;

        let ack_kind = match delay_seconds {
            Some(secs) if secs > 0 => AckKind::Nak(Some(Duration::from_secs(secs as u64))),
            _ => AckKind::Nak(None),
        };

        js_msg
            .ack_with(ack_kind)
            .await
            .map_err(|e| QueueError::Nats(format!("Failed to NAK message: {}", e)))?;

        self.total_nacked.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            consumer = %self.config.consumer_name,
            delay_seconds = ?delay_seconds,
            "Message NACKed in NATS JetStream"
        );

        Ok(())
    }

    async fn defer(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        let (_, js_msg) = self
            .pending_messages
            .remove(receipt_handle)
            .ok_or_else(|| QueueError::NotFound(format!("No pending message for receipt handle: {}", receipt_handle)))?;

        let ack_kind = match delay_seconds {
            Some(secs) if secs > 0 => AckKind::Nak(Some(Duration::from_secs(secs as u64))),
            _ => AckKind::Nak(None),
        };

        js_msg
            .ack_with(ack_kind)
            .await
            .map_err(|e| QueueError::Nats(format!("Failed to defer message: {}", e)))?;

        self.total_deferred.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            consumer = %self.config.consumer_name,
            delay_seconds = ?delay_seconds,
            "Message deferred in NATS JetStream (not counted as failure)"
        );

        Ok(())
    }

    async fn extend_visibility(&self, receipt_handle: &str, _seconds: u32) -> Result<()> {
        let js_msg = self
            .pending_messages
            .get(receipt_handle)
            .ok_or_else(|| QueueError::NotFound(format!("No pending message for receipt handle: {}", receipt_handle)))?;

        // AckKind::Progress resets the ack_wait timer, giving the consumer more time
        // to process the message without it being redelivered.
        js_msg
            .value()
            .ack_with(AckKind::Progress)
            .await
            .map_err(|e| QueueError::Nats(format!("Failed to extend visibility (in-progress): {}", e)))?;

        debug!(
            receipt_handle = %receipt_handle,
            consumer = %self.config.consumer_name,
            "Visibility extended (ack_wait reset) in NATS JetStream"
        );

        Ok(())
    }

    fn is_healthy(&self) -> bool {
        if !self.running.load(Ordering::SeqCst) {
            return false;
        }

        // Check the underlying NATS connection state
        matches!(
            self.client.connection_state(),
            async_nats::connection::State::Connected
        )
    }

    async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);

        // Clear any pending messages that haven't been acked/nacked.
        // They will be redelivered by the server after ack_wait expires.
        let pending_count = self.pending_messages.len();
        self.pending_messages.clear();

        if pending_count > 0 {
            warn!(
                consumer = %self.config.consumer_name,
                pending_count = pending_count,
                "Stopped with pending messages; they will be redelivered after ack_wait"
            );
        }

        info!(
            consumer = %self.config.consumer_name,
            "NATS JetStream consumer stopped"
        );
    }

    async fn get_metrics(&self) -> Result<Option<QueueMetrics>> {
        let mut consumer = self.consumer.write().await;

        let info = consumer
            .info()
            .await
            .map_err(|e| QueueError::Nats(format!("Failed to get consumer info: {}", e)))?;

        let pending_messages = info.num_pending;
        let in_flight_messages = info.num_ack_pending as u64;

        debug!(
            consumer = %self.config.consumer_name,
            pending = pending_messages,
            in_flight = in_flight_messages,
            redelivered = info.num_redelivered,
            "Retrieved NATS JetStream consumer metrics"
        );

        Ok(Some(QueueMetrics {
            pending_messages,
            in_flight_messages,
            queue_identifier: self.config.consumer_name.clone(),
            total_polled: self.total_polled.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_nacked: self.total_nacked.load(Ordering::Relaxed),
            total_deferred: self.total_deferred.load(Ordering::Relaxed),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NatsConfig::default();
        assert_eq!(config.servers, "nats://localhost:4222");
        assert_eq!(config.stream_name, "FLOWCATALYST");
        assert_eq!(config.consumer_name, "fc-router");
        assert_eq!(config.subject, "flowcatalyst.>");
        assert_eq!(config.max_messages_per_poll, 10);
        assert_eq!(config.poll_timeout_ms, 5000);
        assert_eq!(config.ack_wait_secs, 30);
        assert_eq!(config.max_deliver, 5);
        assert_eq!(config.max_ack_pending, 1000);
        assert_eq!(config.storage, "file");
        assert_eq!(config.replicas, 1);
        assert_eq!(config.max_age_days, 7);
    }

    #[test]
    fn test_storage_type_parsing() {
        // Test that our storage parsing logic works
        let file_storage = match "file".to_lowercase().as_str() {
            "memory" => stream::StorageType::Memory,
            _ => stream::StorageType::File,
        };
        assert!(matches!(file_storage, stream::StorageType::File));

        let memory_storage = match "memory".to_lowercase().as_str() {
            "memory" => stream::StorageType::Memory,
            _ => stream::StorageType::File,
        };
        assert!(matches!(memory_storage, stream::StorageType::Memory));

        let memory_upper = match "MEMORY".to_lowercase().as_str() {
            "memory" => stream::StorageType::Memory,
            _ => stream::StorageType::File,
        };
        assert!(matches!(memory_upper, stream::StorageType::Memory));
    }
}
