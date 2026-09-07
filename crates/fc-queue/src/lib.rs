use async_trait::async_trait;
use fc_common::{Message, QueuedMessage};

pub mod error;
pub mod scheme;

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "sqs")]
pub mod sqs;

#[cfg(feature = "activemq")]
pub mod activemq;

#[cfg(feature = "nats")]
pub mod nats;

pub use error::QueueError;
pub use scheme::{resolve_scheme, QueueScheme};

pub type Result<T> = std::result::Result<T, QueueError>;

/// Queue metrics for monitoring
#[derive(Debug, Clone, Default)]
pub struct QueueMetrics {
    /// Approximate number of messages visible in the queue (pending)
    pub pending_messages: u64,
    /// Approximate number of messages currently being processed (in-flight)
    pub in_flight_messages: u64,
    /// Queue identifier
    pub queue_identifier: String,
    /// Total messages polled from this queue
    pub total_polled: u64,
    /// Total messages successfully acknowledged (consumed)
    pub total_acked: u64,
    /// Total messages negatively acknowledged (failed/retried)
    pub total_nacked: u64,
    /// Total messages deferred (rate limiting, capacity - not counted as failures)
    pub total_deferred: u64,
}

/// Trait for consuming messages from a queue
#[async_trait]
pub trait QueueConsumer: Send + Sync {
    /// Get the unique identifier for this consumer
    fn identifier(&self) -> &str;

    /// Poll for messages from the queue
    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>>;

    /// Acknowledge a message (remove from queue)
    async fn ack(&self, receipt_handle: &str) -> Result<()>;

    /// Negative acknowledge a message (make visible again after delay)
    /// This is counted as a failure in metrics.
    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()>;

    /// Defer a message (make visible again after delay) without counting as a failure.
    /// Use this for rate limiting, capacity limits, or other non-error backpressure scenarios.
    /// Default implementation calls nack() - override to track separately.
    async fn defer(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        self.nack(receipt_handle, delay_seconds).await
    }

    /// Extend visibility timeout for a message
    async fn extend_visibility(&self, receipt_handle: &str, seconds: u32) -> Result<()>;

    /// Check if the consumer is healthy
    fn is_healthy(&self) -> bool;

    /// Evidence the broker connection is alive, independent of whether
    /// `poll()` has returned.
    ///
    /// The stall watchdog (`fc_router::health::HealthService`) normally
    /// judges a consumer's liveness purely off when `poll()` last
    /// *returned* — correct for a backend whose `poll()` is bounded (SQS's
    /// long-poll cap, Postgres's own poll loop) but wrong for one whose
    /// `poll()` legitimately blocks **untimed** waiting on the broker
    /// (NATS's standing subscription, `docs/spec/router.md` §3.2/§7.4):
    /// an idle queue and a genuinely hung one look identical to a
    /// poll-return-only heartbeat once the poll has been running longer
    /// than the stall threshold, which restarts a perfectly healthy,
    /// merely-idle consumer forever (G13,
    /// `docs/go-mirror/2026-09-06-go-fix-list.md`).
    ///
    /// This is consulted ONLY as a second lifeline while a poll is
    /// genuinely in progress: it can rescue a consumer the plain
    /// poll-return heartbeat would call stale, but it can never hide one
    /// that's actually wedged — a backend that never overrides this
    /// keeps the default `None`, which adds nothing to the existing
    /// poll-return check, and an override that itself goes stale (broker
    /// disconnected, nothing delivered in a long time) still lets the
    /// watchdog restart normally.
    fn last_broker_activity(&self) -> Option<std::time::Instant> {
        None
    }

    /// Stop the consumer
    async fn stop(&self);

    /// Get queue metrics (pending/in-flight message counts) — calls SQS API.
    /// Returns None if metrics are not available for this queue type.
    async fn get_metrics(&self) -> Result<Option<QueueMetrics>> {
        Ok(None) // Default implementation returns None
    }

    /// Get just the counter metrics (polled/acked/nacked/deferred) without calling SQS API.
    /// These are instant atomic reads. Returns None if not supported.
    fn get_counters(&self) -> Option<QueueMetrics> {
        None
    }
}

/// Trait for publishing messages to a queue
#[async_trait]
pub trait QueuePublisher: Send + Sync {
    /// Get the queue identifier
    fn identifier(&self) -> &str;

    /// Publish a single message
    async fn publish(&self, message: Message) -> Result<String>;

    /// Publish a batch of messages
    async fn publish_batch(&self, messages: Vec<Message>) -> Result<Vec<String>>;
}

/// Combined consumer and publisher for embedded/dev mode
#[async_trait]
pub trait EmbeddedQueue: QueueConsumer + QueuePublisher {
    /// Initialize the queue schema (create tables, etc.)
    async fn init_schema(&self) -> Result<()>;
}
