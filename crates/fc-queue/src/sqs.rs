use async_trait::async_trait;
use aws_sdk_sqs::{Client, types::Message as SqsMessage, types::QueueAttributeName};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tracing::{debug, info, error};

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
    /// 5 seconds balances efficiency with shutdown responsiveness.
    /// AWS SQS max is 20 seconds.
    pub const DEFAULT_WAIT_TIME_SECONDS: i32 = 5;

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

        let result = self.client
            .receive_message()
            .queue_url(&self.queue_url)
            .max_number_of_messages(max_messages.min(10) as i32) // SQS max is 10
            .visibility_timeout(self.visibility_timeout_seconds)
            .wait_time_seconds(self.wait_time_seconds)
            .message_system_attribute_names(aws_sdk_sqs::types::MessageSystemAttributeName::All)
            .message_attribute_names("All")
            .send()
            .await
            .map_err(|e| QueueError::Sqs(e.to_string()))?;

        let sqs_messages = result.messages.unwrap_or_default();
        let mut messages = Vec::with_capacity(sqs_messages.len());

        for sqs_msg in sqs_messages {
            match self.parse_sqs_message(&sqs_msg) {
                Ok((message, receipt_handle, broker_message_id)) => {
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
        self.client
            .delete_message()
            .queue_url(&self.queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await
            .map_err(|e| QueueError::Sqs(e.to_string()))?;

        self.total_acked.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            "Message acknowledged in SQS"
        );
        Ok(())
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
