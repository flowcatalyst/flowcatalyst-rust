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

impl NatsConfig {
    /// Parse a `nats://` queue URI into a [`NatsConfig`], per
    /// `docs/spec/router.md` §7.4:
    ///
    /// `nats://host:port[,host2…]?stream=&consumer=&subject=&max-messages=&poll-timeout-ms=&ack-wait-secs=&max-deliver=&max-ack-pending=&storage=file|memory&replicas=&max-age-days=`
    ///
    /// Everything before the first `?` (including the `nats://` scheme) is
    /// passed straight through as `servers` — `async_nats`'s connector
    /// already accepts a comma-separated multi-host string in that shape.
    /// Every query parameter is optional; an absent or unparseable one
    /// falls back to [`NatsConfig::default`]'s value for that field (same
    /// defaults the other routers ship: stream `FLOWCATALYST`, consumer
    /// `fc-router`, subject `flowcatalyst.>`, batch 10, poll timeout 20s,
    /// ack-wait 120s, max-deliver 10, max-ack-pending 1000, storage file,
    /// replicas 1, max-age 7d).
    pub fn from_uri(uri: &str) -> Result<Self> {
        if !uri.starts_with("nats://") {
            return Err(QueueError::Config(format!(
                "not a nats:// URI: {}",
                uri
            )));
        }

        let mut config = NatsConfig::default();

        let (servers, query) = match uri.split_once('?') {
            Some((before, after)) => (before, Some(after)),
            None => (uri, None),
        };
        config.servers = servers.to_string();

        let Some(query) = query else {
            return Ok(config);
        };

        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            match key {
                "stream" if !value.is_empty() => config.stream_name = value.to_string(),
                "consumer" if !value.is_empty() => config.consumer_name = value.to_string(),
                "subject" if !value.is_empty() => config.subject = value.to_string(),
                "max-messages" => {
                    if let Ok(v) = value.parse::<u32>() {
                        config.max_messages_per_poll = v;
                    }
                }
                "poll-timeout-ms" => {
                    if let Ok(v) = value.parse::<u64>() {
                        config.poll_timeout_ms = v;
                    }
                }
                "ack-wait-secs" => {
                    if let Ok(v) = value.parse::<u64>() {
                        config.ack_wait_secs = v;
                    }
                }
                "max-deliver" => {
                    if let Ok(v) = value.parse::<i64>() {
                        config.max_deliver = v;
                    }
                }
                "max-ack-pending" => {
                    if let Ok(v) = value.parse::<i64>() {
                        config.max_ack_pending = v;
                    }
                }
                "storage" if !value.is_empty() => config.storage = value.to_string(),
                "replicas" => {
                    if let Ok(v) = value.parse::<usize>() {
                        config.replicas = v;
                    }
                }
                "max-age-days" => {
                    if let Ok(v) = value.parse::<u64>() {
                        config.max_age_days = v;
                    }
                }
                _ => {
                    // Unknown parameter — ignore rather than fail, same
                    // tolerance the other backends' URI parsing gives an
                    // operator adding a forward-looking query param.
                }
            }
        }

        Ok(config)
    }
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            servers: "nats://localhost:4222".to_string(),
            stream_name: "FLOWCATALYST".to_string(),
            consumer_name: "fc-router".to_string(),
            subject: "flowcatalyst.>".to_string(),
            max_messages_per_poll: 10,
            poll_timeout_ms: 20_000, // Java default: 20 seconds
            ack_wait_secs: 120,      // Java default: 120 seconds
            max_deliver: 10,         // Java default: 10 redeliveries
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
    /// Cached queue identifier in Java format: `streamName/consumerName`
    queue_id: String,
    client: async_nats::Client,
    consumer: Arc<RwLock<PullConsumer>>,
    running: AtomicBool,
    /// Maps receipt handle (`streamName:streamSequence`) -> JetStream message for ack/nack
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

        // Connect to NATS with reconnect settings matching Java:
        // unlimited reconnects, 2s reconnect wait, 10s connection timeout
        let client = async_nats::ConnectOptions::new()
            .connection_timeout(Duration::from_secs(10))
            .reconnect_delay_callback(|_attempts| Duration::from_secs(2))
            .connect(&config.servers)
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
        // WorkQueue retention: messages are removed once consumed (Java default)
        let stream = jetstream
            .get_or_create_stream(stream::Config {
                name: config.stream_name.clone(),
                subjects: vec![config.subject.clone()],
                retention: stream::RetentionPolicy::WorkQueue,
                storage: storage_type,
                num_replicas: config.replicas,
                max_age,
                ..Default::default()
            })
            .await
            .map_err(|e| {
                QueueError::Nats(format!(
                    "Failed to get/create stream '{}': {}",
                    config.stream_name, e
                ))
            })?;

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
            .map_err(|e| {
                QueueError::Nats(format!(
                    "Failed to get/create consumer '{}': {}",
                    config.consumer_name, e
                ))
            })?;

        info!(
            consumer = %config.consumer_name,
            ack_wait_secs = config.ack_wait_secs,
            max_deliver = config.max_deliver,
            max_ack_pending = config.max_ack_pending,
            "JetStream consumer ready"
        );

        let queue_id = format!("{}/{}", config.stream_name, config.consumer_name);

        Ok(Self {
            config,
            queue_id,
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

    /// Extract receipt handle from a JetStream message.
    /// Format matches Java: `streamName:streamSequence`
    fn receipt_handle_from_message(
        msg: &async_nats::jetstream::Message,
        stream_name: &str,
    ) -> Option<String> {
        msg.info()
            .ok()
            .map(|info| format!("{}:{}", stream_name, info.stream_sequence))
    }

    /// Dedup id for a JetStream delivery (R-19).
    ///
    /// This MUST be the stream sequence only. The stream sequence is stable
    /// across redeliveries of the same message; the consumer sequence
    /// increments on every delivery attempt (including redeliveries), so
    /// folding it in gave every redelivery a fresh id and the dedup layer
    /// could never recognise a redelivery as the message it already saw —
    /// turning at-least-once delivery into at-most-once.
    fn broker_message_id_from_sequence(stream_sequence: u64, _consumer_sequence: u64) -> String {
        stream_sequence.to_string()
    }
}

#[async_trait]
impl QueueConsumer for NatsQueueConsumer {
    fn identifier(&self) -> &str {
        &self.queue_id
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
                    // Extract receipt handle: streamName:streamSequence
                    let receipt_handle = match Self::receipt_handle_from_message(
                        &js_msg,
                        &self.config.stream_name,
                    ) {
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
                            // Dedup id must be the stream sequence ONLY (R-19) — see
                            // broker_message_id_from_sequence.
                            let broker_message_id = js_msg.info().ok().map(|info| {
                                Self::broker_message_id_from_sequence(
                                    info.stream_sequence,
                                    info.consumer_sequence,
                                )
                            });

                            // Store the JetStream message for later ack/nack
                            self.pending_messages.insert(receipt_handle.clone(), js_msg);

                            messages.push(QueuedMessage {
                                message,
                                receipt_handle,
                                broker_message_id,
                                queue_identifier: self.queue_id.clone(),
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
            self.total_polled
                .fetch_add(messages.len() as u64, Ordering::Relaxed);
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
            .ok_or_else(|| {
                QueueError::NotFound(format!(
                    "No pending message for receipt handle: {}",
                    receipt_handle
                ))
            })?;

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
            .ok_or_else(|| {
                QueueError::NotFound(format!(
                    "No pending message for receipt handle: {}",
                    receipt_handle
                ))
            })?;

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
            .ok_or_else(|| {
                QueueError::NotFound(format!(
                    "No pending message for receipt handle: {}",
                    receipt_handle
                ))
            })?;

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
        let js_msg = self.pending_messages.get(receipt_handle).ok_or_else(|| {
            QueueError::NotFound(format!(
                "No pending message for receipt handle: {}",
                receipt_handle
            ))
        })?;

        // AckKind::Progress resets the ack_wait timer, giving the consumer more time
        // to process the message without it being redelivered.
        js_msg
            .value()
            .ack_with(AckKind::Progress)
            .await
            .map_err(|e| {
                QueueError::Nats(format!("Failed to extend visibility (in-progress): {}", e))
            })?;

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
            queue_identifier: self.queue_id.clone(),
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
        assert_eq!(config.poll_timeout_ms, 20_000); // Java: 20 seconds
        assert_eq!(config.ack_wait_secs, 120); // Java: 120 seconds
        assert_eq!(config.max_deliver, 10); // Java: 10 redeliveries
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

    /// R-19: broker_message_id must be the stream sequence only, so that two
    /// deliveries of the same message — which necessarily share a stream
    /// sequence but carry different (incrementing) consumer sequences — are
    /// recognised by the dedup layer as the same message.
    #[test]
    fn test_broker_message_id_ignores_consumer_sequence() {
        let first_delivery = NatsQueueConsumer::broker_message_id_from_sequence(42, 1);
        let redelivery = NatsQueueConsumer::broker_message_id_from_sequence(42, 7);

        assert_eq!(first_delivery, redelivery);
        assert_eq!(first_delivery, "42");
    }

    #[test]
    fn test_broker_message_id_distinguishes_different_messages() {
        let a = NatsQueueConsumer::broker_message_id_from_sequence(42, 1);
        let b = NatsQueueConsumer::broker_message_id_from_sequence(43, 1);
        assert_ne!(a, b);
    }

    // --- NatsConfig::from_uri (item 1: scheme-dispatched queue wiring) ---

    #[test]
    fn from_uri_bare_servers_uses_every_default() {
        let config = NatsConfig::from_uri("nats://localhost:4222").unwrap();
        let defaults = NatsConfig::default();
        assert_eq!(config.servers, "nats://localhost:4222");
        assert_eq!(config.stream_name, defaults.stream_name);
        assert_eq!(config.consumer_name, defaults.consumer_name);
        assert_eq!(config.subject, defaults.subject);
        assert_eq!(config.max_messages_per_poll, defaults.max_messages_per_poll);
        assert_eq!(config.poll_timeout_ms, defaults.poll_timeout_ms);
        assert_eq!(config.ack_wait_secs, defaults.ack_wait_secs);
        assert_eq!(config.max_deliver, defaults.max_deliver);
        assert_eq!(config.max_ack_pending, defaults.max_ack_pending);
        assert_eq!(config.storage, defaults.storage);
        assert_eq!(config.replicas, defaults.replicas);
        assert_eq!(config.max_age_days, defaults.max_age_days);
    }

    #[test]
    fn from_uri_parses_every_query_parameter() {
        let uri = "nats://host1:4222,host2:4222?stream=BENCH1&consumer=router&subject=flowcatalyst.bench.>&max-messages=25&poll-timeout-ms=5000&ack-wait-secs=60&max-deliver=5&max-ack-pending=500&storage=memory&replicas=3&max-age-days=1";
        let config = NatsConfig::from_uri(uri).unwrap();

        assert_eq!(config.servers, "nats://host1:4222,host2:4222");
        assert_eq!(config.stream_name, "BENCH1");
        assert_eq!(config.consumer_name, "router");
        assert_eq!(config.subject, "flowcatalyst.bench.>");
        assert_eq!(config.max_messages_per_poll, 25);
        assert_eq!(config.poll_timeout_ms, 5000);
        assert_eq!(config.ack_wait_secs, 60);
        assert_eq!(config.max_deliver, 5);
        assert_eq!(config.max_ack_pending, 500);
        assert_eq!(config.storage, "memory");
        assert_eq!(config.replicas, 3);
        assert_eq!(config.max_age_days, 1);
    }

    #[test]
    fn from_uri_identifier_is_stream_slash_consumer() {
        let config =
            NatsConfig::from_uri("nats://localhost:4222?stream=BENCH4&consumer=router").unwrap();
        assert_eq!(
            format!("{}/{}", config.stream_name, config.consumer_name),
            "BENCH4/router"
        );
    }

    #[test]
    fn from_uri_rejects_non_nats_scheme() {
        assert!(NatsConfig::from_uri("postgres://host/db").is_err());
    }

    #[test]
    fn from_uri_ignores_unparseable_numeric_param_keeping_default() {
        // A malformed value must not poison the whole parse — it falls back
        // to the default for that one field, same tolerance the other
        // fields get when a param is absent altogether.
        let config = NatsConfig::from_uri("nats://localhost:4222?max-messages=not-a-number").unwrap();
        assert_eq!(
            config.max_messages_per_poll,
            NatsConfig::default().max_messages_per_poll
        );
    }
}
