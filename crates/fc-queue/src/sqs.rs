use async_trait::async_trait;
use aws_sdk_sqs::{Client, types::Message as SqsMessage, types::QueueAttributeName};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use parking_lot::Mutex;
use tracing::{debug, info, warn, error};

use fc_common::{Message, QueuedMessage};
use crate::{QueueConsumer, QueueMetrics, Result, QueueError};

/// AWS SQS queue consumer
pub struct SqsQueueConsumer {
    client: Client,
    queue_url: String,
    queue_name: String,
    visibility_timeout_seconds: i32,
    wait_time_seconds: i32,
    running: AtomicBool,
    /// SQS message IDs that we've acked (successfully or not). SQS standard queues
    /// are at-least-once — even a successful DeleteMessage can be followed by a
    /// redelivery, and a failed delete obviously needs the same guard. Every
    /// redelivery within the TTL is batch-deleted without re-routing to the
    /// mediator. Entries age out after `PENDING_DELETE_TTL`.
    pending_delete_ids: Mutex<HashMap<String, std::time::Instant>>,
    /// Maps receipt handle -> SQS message ID so `ack` (which only receives the
    /// handle) can record the message ID in `pending_delete_ids`. Entries are
    /// pruned periodically to prevent unbounded growth.
    receipt_to_message_id: Mutex<HashMap<String, (String, std::time::Instant)>>,
    /// Total messages polled from queue
    total_polled: AtomicU64,
    /// Total messages successfully ACKed
    total_acked: AtomicU64,
    /// Total messages NACKed (actual failures)
    total_nacked: AtomicU64,
    /// Total messages deferred (rate limiting, capacity - not failures)
    total_deferred: AtomicU64,
}

impl SqsQueueConsumer {
    /// Default long poll wait time in seconds.
    /// 20 seconds matches TS version and minimises SQS API calls.
    /// AWS SQS max is 20 seconds.
    pub const DEFAULT_WAIT_TIME_SECONDS: i32 = 20;

    /// How long to remember an acked SQS MessageId so redeliveries are
    /// short-circuited to DeleteMessage without re-routing to the mediator.
    const PENDING_DELETE_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

    pub fn new(
        client: Client,
        queue_url: String,
        queue_name: String,
        visibility_timeout_seconds: i32,
    ) -> Self {
        Self {
            client,
            queue_url,
            queue_name,
            visibility_timeout_seconds,
            wait_time_seconds: Self::DEFAULT_WAIT_TIME_SECONDS,
            running: AtomicBool::new(true),
            pending_delete_ids: Mutex::new(HashMap::new()),
            receipt_to_message_id: Mutex::new(HashMap::new()),
            total_polled: AtomicU64::new(0),
            total_acked: AtomicU64::new(0),
            total_nacked: AtomicU64::new(0),
            total_deferred: AtomicU64::new(0),
        }
    }

    /// Create from queue URL, extracting name
    pub async fn from_queue_url(client: Client, queue_url: String, visibility_timeout_seconds: i32) -> Self {
        let queue_name = queue_url
            .split('/')
            .last()
            .unwrap_or("unknown")
            .to_string();

        Self::new(client, queue_url, queue_name, visibility_timeout_seconds)
    }

    /// Set the long poll wait time in seconds (max 20).
    /// Shorter times mean faster shutdown response but more API calls.
    pub fn with_wait_time_seconds(mut self, seconds: i32) -> Self {
        self.wait_time_seconds = seconds.clamp(0, 20);
        self
    }

    fn parse_sqs_message(&self, sqs_msg: &SqsMessage) -> Result<(Message, String, Option<String>)> {
        let body = sqs_msg.body()
            .ok_or_else(|| QueueError::Sqs("Message body is empty".to_string()))?;

        let message: Message = serde_json::from_str(body)?;

        let receipt_handle = sqs_msg.receipt_handle()
            .ok_or_else(|| QueueError::Sqs("Missing receipt handle".to_string()))?
            .to_string();

        let message_id = sqs_msg.message_id().map(|s| s.to_string());

        Ok((message, receipt_handle, message_id))
    }
}

#[async_trait]
impl QueueConsumer for SqsQueueConsumer {
    fn identifier(&self) -> &str {
        &self.queue_name
    }

    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(QueueError::Stopped);
        }

        let max_per_poll = max_messages.min(10) as i32; // SQS max is 10

        // Java: 25s per-request API call timeout to prevent indefinite blocking
        let timeout_config = aws_sdk_sqs::config::timeout::TimeoutConfig::builder()
            .operation_timeout(std::time::Duration::from_secs(25))
            .build();

        let result = self.client
            .receive_message()
            .queue_url(&self.queue_url)
            .max_number_of_messages(max_per_poll)
            .visibility_timeout(self.visibility_timeout_seconds)
            .wait_time_seconds(self.wait_time_seconds)
            .message_system_attribute_names(aws_sdk_sqs::types::MessageSystemAttributeName::All)
            .message_attribute_names("All")
            .customize()
            .config_override(
                aws_sdk_sqs::config::Builder::default()
                    .timeout_config(timeout_config)
            )
            .send()
            .await
            .map_err(|e| QueueError::Sqs(e.to_string()))?;

        let sqs_messages = result.messages.unwrap_or_default();
        let sqs_messages_count = sqs_messages.len();
        let mut messages = Vec::with_capacity(sqs_messages_count);

        for sqs_msg in sqs_messages {
            // If this MessageId is in pending-delete (we already acked it once),
            // delete the redelivery immediately and move on. Keep the entry until
            // it ages out past the TTL so every redelivery within the window is
            // short-circuited — not just the first one.
            if let Some(msg_id) = sqs_msg.message_id() {
                let should_delete = {
                    let mut pending = self.pending_delete_ids.lock();
                    pending.retain(|_, ts| ts.elapsed() < Self::PENDING_DELETE_TTL);
                    pending.contains_key(msg_id)
                };
                if should_delete {
                    info!(
                        queue = %self.queue_name,
                        message_id = %msg_id,
                        "Redelivery of acked message — deleting immediately"
                    );
                    if let Some(handle) = sqs_msg.receipt_handle() {
                        // Delete directly; don't call self.ack() to avoid re-inserting
                        // into pending_delete_ids or racing with the tracking map.
                        let _ = self.client
                            .delete_message()
                            .queue_url(&self.queue_url)
                            .receipt_handle(handle)
                            .send()
                            .await;
                    }
                    continue;
                }
            }

            match self.parse_sqs_message(&sqs_msg) {
                Ok((message, receipt_handle, broker_message_id)) => {
                    // Track receipt handle → message ID so `ack` can record the
                    // message ID in pending_delete_ids (ack only has the handle).
                    if let Some(ref msg_id) = broker_message_id {
                        let mut map = self.receipt_to_message_id.lock();
                        if map.len() > 1000 {
                            map.retain(|_, (_, ts)| ts.elapsed() < Self::PENDING_DELETE_TTL);
                        }
                        map.insert(receipt_handle.clone(), (msg_id.clone(), std::time::Instant::now()));
                    }
                    messages.push(QueuedMessage {
                        message,
                        receipt_handle,
                        broker_message_id,
                        queue_identifier: self.queue_name.clone(),
                    });
                }
                Err(e) => {
                    error!(
                        queue = %self.queue_name,
                        error = %e,
                        "Failed to parse SQS message"
                    );
                    // ACK the malformed message to prevent infinite retries
                    if let Some(handle) = sqs_msg.receipt_handle() {
                        let _ = self.ack(handle).await;
                    }
                }
            }
        }

        if !messages.is_empty() {
            self.total_polled.fetch_add(messages.len() as u64, Ordering::Relaxed);
            debug!(
                queue = %self.queue_name,
                count = messages.len(),
                "Polled messages from SQS"
            );
        }

        Ok(messages)
    }

    async fn ack(&self, receipt_handle: &str) -> Result<()> {
        // Always record the MessageId in pending_delete_ids — regardless of
        // whether DeleteMessage succeeds or fails. SQS standard queues are
        // at-least-once, so even a successful delete can be followed by a
        // redelivery; a failed delete obviously needs the same guard.
        // Redeliveries within the TTL are batch-deleted in `poll` without
        // being re-routed to the mediator.
        let msg_id = self.receipt_to_message_id.lock().remove(receipt_handle)
            .map(|(id, _)| id);
        if let Some(ref id) = msg_id {
            self.pending_delete_ids.lock().insert(id.clone(), std::time::Instant::now());
        }

        let result = self.client
            .delete_message()
            .queue_url(&self.queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await;

        match result {
            Ok(_) => {
                self.total_acked.fetch_add(1, Ordering::Relaxed);
                debug!(
                    receipt_handle = %receipt_handle,
                    queue = %self.queue_name,
                    "Message acknowledged in SQS"
                );
                Ok(())
            }
            Err(e) => {
                warn!(
                    queue = %self.queue_name,
                    message_id = ?msg_id,
                    error = %e,
                    "ACK failed — pending-delete guard will short-circuit redeliveries"
                );
                Err(QueueError::Sqs(e.to_string()))
            }
        }
    }

    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        // In SQS, NACK is done by setting visibility timeout to 0 (immediate retry)
        // or to a delay value for delayed retry
        let visibility_timeout = delay_seconds.unwrap_or(0) as i32;

        self.client
            .change_message_visibility()
            .queue_url(&self.queue_url)
            .receipt_handle(receipt_handle)
            .visibility_timeout(visibility_timeout)
            .send()
            .await
            .map_err(|e| QueueError::Sqs(e.to_string()))?;

        self.total_nacked.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            visibility_timeout = visibility_timeout,
            "Message NACKed in SQS"
        );
        Ok(())
    }

    async fn defer(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        // Same SQS operation as nack, but tracked separately as not a failure
        let visibility_timeout = delay_seconds.unwrap_or(0) as i32;

        self.client
            .change_message_visibility()
            .queue_url(&self.queue_url)
            .receipt_handle(receipt_handle)
            .visibility_timeout(visibility_timeout)
            .send()
            .await
            .map_err(|e| QueueError::Sqs(e.to_string()))?;

        self.total_deferred.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            visibility_timeout = visibility_timeout,
            "Message deferred in SQS (not counted as failure)"
        );
        Ok(())
    }

    async fn extend_visibility(&self, receipt_handle: &str, seconds: u32) -> Result<()> {
        self.client
            .change_message_visibility()
            .queue_url(&self.queue_url)
            .receipt_handle(receipt_handle)
            .visibility_timeout(seconds as i32)
            .send()
            .await
            .map_err(|e| QueueError::Sqs(e.to_string()))?;

        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            seconds = seconds,
            "Visibility extended in SQS"
        );
        Ok(())
    }

    fn is_healthy(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        info!(queue = %self.queue_name, "SQS queue consumer stopped");
    }

    fn get_counters(&self) -> Option<QueueMetrics> {
        Some(QueueMetrics {
            pending_messages: 0,       // Not available without SQS API call
            in_flight_messages: 0,     // Not available without SQS API call
            queue_identifier: self.queue_name.clone(),
            total_polled: self.total_polled.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_nacked: self.total_nacked.load(Ordering::Relaxed),
            total_deferred: self.total_deferred.load(Ordering::Relaxed),
        })
    }

    async fn get_metrics(&self) -> Result<Option<QueueMetrics>> {
        let result = self.client
            .get_queue_attributes()
            .queue_url(&self.queue_url)
            .attribute_names(QueueAttributeName::ApproximateNumberOfMessages)
            .attribute_names(QueueAttributeName::ApproximateNumberOfMessagesNotVisible)
            .send()
            .await
            .map_err(|e| QueueError::Sqs(e.to_string()))?;

        let attributes = result.attributes();

        let pending_messages = attributes
            .and_then(|attrs| attrs.get(&QueueAttributeName::ApproximateNumberOfMessages))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let in_flight_messages = attributes
            .and_then(|attrs| attrs.get(&QueueAttributeName::ApproximateNumberOfMessagesNotVisible))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        debug!(
            queue = %self.queue_name,
            pending = pending_messages,
            in_flight = in_flight_messages,
            "Retrieved SQS queue metrics"
        );

        Ok(Some(QueueMetrics {
            pending_messages,
            in_flight_messages,
            queue_identifier: self.queue_name.clone(),
            total_polled: self.total_polled.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_nacked: self.total_nacked.load(Ordering::Relaxed),
            total_deferred: self.total_deferred.load(Ordering::Relaxed),
        }))
    }
}
