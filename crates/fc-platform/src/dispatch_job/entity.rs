//! Dispatch Job Entity
//!
//! Represents async delivery of an event/task to a target endpoint.
//! Tracks full lifecycle with attempt history.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;

/// Dispatch job kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchKind {
    /// Dispatching an event
    Event,
    /// Dispatching a task/command
    Task,
}

impl Default for DispatchKind {
    fn default() -> Self {
        Self::Event
    }
}

/// Dispatch job status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchStatus {
    /// Job created, waiting to be queued
    Pending,
    /// Job queued for processing
    Queued,
    /// Job is being processed
    InProgress,
    /// Job completed successfully
    Completed,
    /// Job failed after all retries
    Failed,
    /// Job expired (TTL exceeded)
    Expired,
}

impl Default for DispatchStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl DispatchStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Expired)
    }

    pub fn is_successful(&self) -> bool {
        matches!(self, Self::Completed)
    }
}

/// Dispatch mode for ordering behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchMode {
    /// Process immediately, independent of other jobs
    Immediate,
    /// If this job fails, continue with next in group
    NextOnError,
    /// If this job fails, block subsequent jobs in group
    BlockOnError,
}

impl Default for DispatchMode {
    fn default() -> Self {
        Self::Immediate
    }
}

/// Target protocol for dispatch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchProtocol {
    HttpWebhook,
}

impl Default for DispatchProtocol {
    fn default() -> Self {
        Self::HttpWebhook
    }
}

/// Retry strategy for failed jobs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RetryStrategy {
    /// Immediate retry
    Immediate,
    /// Fixed delay between retries
    FixedDelay,
    /// Exponential backoff
    ExponentialBackoff,
}

impl Default for RetryStrategy {
    fn default() -> Self {
        Self::ExponentialBackoff
    }
}

/// Error type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorType {
    /// Network/connection error (retriable)
    Connection,
    /// Timeout (retriable)
    Timeout,
    /// Client error 4xx (not retriable)
    ClientError,
    /// Server error 5xx (retriable)
    ServerError,
    /// Configuration error (not retriable)
    Configuration,
    /// Unknown error
    Unknown,
}

/// Dispatch attempt record
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchAttempt {
    /// Attempt number (1-based)
    pub attempt_number: u32,

    /// When the attempt started
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub attempted_at: DateTime<Utc>,

    /// When the attempt completed
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub completed_at: Option<DateTime<Utc>>,

    /// Duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_millis: Option<i64>,

    /// HTTP response code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_code: Option<u16>,

    /// Response body (truncated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,

    /// Whether this attempt succeeded
    pub success: bool,

    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,

    /// Error type classification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<ErrorType>,
}

impl DispatchAttempt {
    pub fn new(attempt_number: u32) -> Self {
        Self {
            attempt_number,
            attempted_at: Utc::now(),
            completed_at: None,
            duration_millis: None,
            response_code: None,
            response_body: None,
            success: false,
            error_message: None,
            error_type: None,
        }
    }

    pub fn complete_success(mut self, response_code: u16, response_body: Option<String>) -> Self {
        let now = Utc::now();
        self.completed_at = Some(now);
        self.duration_millis = Some((now - self.attempted_at).num_milliseconds());
        self.response_code = Some(response_code);
        self.response_body = response_body;
        self.success = true;
        self
    }

    pub fn complete_failure(mut self, error_message: String, error_type: ErrorType, response_code: Option<u16>) -> Self {
        let now = Utc::now();
        self.completed_at = Some(now);
        self.duration_millis = Some((now - self.attempted_at).num_milliseconds());
        self.response_code = response_code;
        self.error_message = Some(error_message);
        self.error_type = Some(error_type);
        self.success = false;
        self
    }
}

/// Metadata key-value pair
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchMetadata {
    pub key: String,
    pub value: String,
}

/// Dispatch job entity
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJob {
    /// TSID as Crockford Base32 string
    #[serde(rename = "_id")]
    pub id: String,

    /// External reference ID (optional, for idempotency)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,

    // === Classification ===

    /// Event or Task
    #[serde(default)]
    pub kind: DispatchKind,

    /// Event type code or task identifier
    pub code: String,

    /// Source system/application
    pub source: String,

    /// Subject/context identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    // === Target ===

    /// Target URL for webhook delivery
    pub target_url: String,

    /// Protocol (HTTP webhook)
    #[serde(default)]
    pub protocol: DispatchProtocol,

    // === Payload ===

    /// Payload to deliver (JSON string)
    pub payload: String,

    /// Content type of payload
    #[serde(default = "default_content_type")]
    pub payload_content_type: String,

    /// If true, send raw data only. If false, wrap in envelope with metadata.
    #[serde(default)]
    pub data_only: bool,

    // === Context ===

    /// Triggering event ID (for EVENT kind)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,

    /// Correlation ID for tracing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    /// Multi-tenant: Client ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Subscription that created this job
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,

    /// Service account for authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,

    // === Dispatch behavior ===

    /// Rate limiting pool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,

    /// Message group for FIFO ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,

    /// Dispatch mode for ordering
    #[serde(default)]
    pub mode: DispatchMode,

    /// Sequence number within message group
    #[serde(default = "default_sequence")]
    pub sequence: i32,

    // === Execution settings ===

    /// Timeout in seconds for HTTP call
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,

    /// Maximum retry attempts
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Retry strategy
    #[serde(default)]
    pub retry_strategy: RetryStrategy,

    // === Status tracking ===

    /// Current status
    #[serde(default)]
    pub status: DispatchStatus,

    /// Number of attempts made
    #[serde(default)]
    pub attempt_count: u32,

    /// Last error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,

    /// Attempt history
    #[serde(default)]
    pub attempts: Vec<DispatchAttempt>,

    // === Metadata ===

    /// Custom metadata
    #[serde(default)]
    pub metadata: Vec<DispatchMetadata>,

    /// Idempotency key for deduplication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,

    // === Timestamps ===

    /// When the job was created
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,

    /// When the job was last updated
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,

    /// When the job was queued
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub queued_at: Option<DateTime<Utc>>,

    /// When the last attempt was made
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub last_attempt_at: Option<DateTime<Utc>>,

    /// When the job completed
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub completed_at: Option<DateTime<Utc>>,

    /// Total duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_millis: Option<i64>,

    /// Next retry scheduled time
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub next_retry_at: Option<DateTime<Utc>>,
}

fn default_content_type() -> String {
    "application/json".to_string()
}

fn default_sequence() -> i32 {
    99
}

fn default_timeout() -> u32 {
    30
}

fn default_max_retries() -> u32 {
    3
}

impl DispatchJob {
    /// Create a new dispatch job for an event
    pub fn for_event(
        event_id: impl Into<String>,
        event_type: impl Into<String>,
        source: impl Into<String>,
        target_url: impl Into<String>,
        payload: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(),
            external_id: None,
            kind: DispatchKind::Event,
            code: event_type.into(),
            source: source.into(),
            subject: None,
            target_url: target_url.into(),
            protocol: DispatchProtocol::HttpWebhook,
            payload: payload.into(),
            payload_content_type: default_content_type(),
            data_only: false,
            event_id: Some(event_id.into()),
            correlation_id: None,
            client_id: None,
            subscription_id: None,
            service_account_id: None,
            dispatch_pool_id: None,
            message_group: None,
            mode: DispatchMode::Immediate,
            sequence: default_sequence(),
            timeout_seconds: default_timeout(),
            max_retries: default_max_retries(),
            retry_strategy: RetryStrategy::ExponentialBackoff,
            status: DispatchStatus::Pending,
            attempt_count: 0,
            last_error: None,
            attempts: vec![],
            metadata: vec![],
            idempotency_key: None,
            created_at: now,
            updated_at: now,
            queued_at: None,
            last_attempt_at: None,
            completed_at: None,
            duration_millis: None,
            next_retry_at: None,
        }
    }

    /// Create a new dispatch job for a task
    pub fn for_task(
        code: impl Into<String>,
        source: impl Into<String>,
        target_url: impl Into<String>,
        payload: impl Into<String>,
    ) -> Self {
        let mut job = Self::for_event("", code, source, target_url, payload);
        job.kind = DispatchKind::Task;
        job.event_id = None;
        job
    }

    // Builder methods
    pub fn with_client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = Some(id.into());
        self
    }

    pub fn with_subscription_id(mut self, id: impl Into<String>) -> Self {
        self.subscription_id = Some(id.into());
        self
    }

    pub fn with_service_account_id(mut self, id: impl Into<String>) -> Self {
        self.service_account_id = Some(id.into());
        self
    }

    pub fn with_dispatch_pool_id(mut self, id: impl Into<String>) -> Self {
        self.dispatch_pool_id = Some(id.into());
        self
    }

    pub fn with_message_group(mut self, group: impl Into<String>) -> Self {
        self.message_group = Some(group.into());
        self
    }

    pub fn with_mode(mut self, mode: DispatchMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    pub fn with_data_only(mut self, data_only: bool) -> Self {
        self.data_only = data_only;
        self
    }

    /// Mark the job as queued
    pub fn mark_queued(&mut self) {
        self.status = DispatchStatus::Queued;
        self.queued_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark the job as in progress
    pub fn mark_in_progress(&mut self) {
        self.status = DispatchStatus::InProgress;
        self.updated_at = Utc::now();
    }

    /// Record a successful attempt and complete the job
    pub fn complete_success(&mut self, response_code: u16, response_body: Option<String>) {
        self.attempt_count += 1;
        let attempt = DispatchAttempt::new(self.attempt_count)
            .complete_success(response_code, response_body);
        self.attempts.push(attempt);

        self.status = DispatchStatus::Completed;
        let now = Utc::now();
        self.completed_at = Some(now);
        self.last_attempt_at = Some(now);
        self.duration_millis = Some((now - self.created_at).num_milliseconds());
        self.updated_at = now;
    }

    /// Record a failed attempt
    pub fn record_failure(&mut self, error_message: String, error_type: ErrorType, response_code: Option<u16>) {
        self.attempt_count += 1;
        let attempt = DispatchAttempt::new(self.attempt_count)
            .complete_failure(error_message.clone(), error_type, response_code);
        self.attempts.push(attempt);

        self.last_error = Some(error_message);
        self.last_attempt_at = Some(Utc::now());
        self.updated_at = Utc::now();

        // Check if we've exhausted retries
        if self.attempt_count >= self.max_retries {
            self.status = DispatchStatus::Failed;
            self.completed_at = Some(Utc::now());
            self.duration_millis = Some((Utc::now() - self.created_at).num_milliseconds());
        } else {
            // Schedule retry
            self.status = DispatchStatus::Pending;
            self.next_retry_at = Some(self.calculate_next_retry());
        }
    }

    /// Calculate the next retry time based on strategy
    fn calculate_next_retry(&self) -> DateTime<Utc> {
        let delay_seconds = match self.retry_strategy {
            RetryStrategy::Immediate => 0,
            RetryStrategy::FixedDelay => 5,
            RetryStrategy::ExponentialBackoff => {
                // 5s, 25s, 125s, 625s, ...
                5i64.pow(self.attempt_count.min(5))
            }
        };
        Utc::now() + chrono::Duration::seconds(delay_seconds)
    }

    /// Check if the job can be retried
    pub fn can_retry(&self) -> bool {
        !self.status.is_terminal() && self.attempt_count < self.max_retries
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.push(DispatchMetadata {
            key: key.into(),
            value: value.into(),
        });
    }
}

/// Dispatch job read projection - optimized for queries (matches Java DispatchJobRead)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobRead {
    #[serde(rename = "_id")]
    pub id: String,
    pub external_id: Option<String>,
    pub source: String,
    pub kind: DispatchKind,
    pub code: String,
    pub subject: Option<String>,
    pub event_id: Option<String>,
    pub correlation_id: Option<String>,
    pub target_url: String,
    pub protocol: DispatchProtocol,
    pub client_id: Option<String>,
    pub subscription_id: Option<String>,
    pub service_account_id: Option<String>,
    pub dispatch_pool_id: Option<String>,
    pub message_group: Option<String>,
    pub mode: DispatchMode,
    #[serde(default = "default_sequence")]
    pub sequence: i32,
    pub status: DispatchStatus,
    pub attempt_count: u32,
    pub max_retries: u32,
    pub last_error: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
    pub retry_strategy: RetryStrategy,
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub scheduled_for: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub duration_millis: Option<i64>,
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub is_terminal: bool,
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub projected_at: Option<DateTime<Utc>>,
}

impl From<&DispatchJob> for DispatchJobRead {
    fn from(job: &DispatchJob) -> Self {
        Self {
            id: job.id.clone(),
            external_id: job.external_id.clone(),
            source: job.source.clone(),
            kind: job.kind,
            code: job.code.clone(),
            subject: job.subject.clone(),
            event_id: job.event_id.clone(),
            correlation_id: job.correlation_id.clone(),
            target_url: job.target_url.clone(),
            protocol: job.protocol,
            client_id: job.client_id.clone(),
            subscription_id: job.subscription_id.clone(),
            service_account_id: job.service_account_id.clone(),
            dispatch_pool_id: job.dispatch_pool_id.clone(),
            message_group: job.message_group.clone(),
            mode: job.mode,
            sequence: job.sequence,
            status: job.status,
            attempt_count: job.attempt_count,
            max_retries: job.max_retries,
            last_error: job.last_error.clone(),
            timeout_seconds: job.timeout_seconds,
            retry_strategy: job.retry_strategy,
            created_at: job.created_at,
            updated_at: job.updated_at,
            scheduled_for: job.next_retry_at,
            expires_at: None, // Not tracked in DispatchJob yet
            completed_at: job.completed_at,
            last_attempt_at: job.last_attempt_at,
            duration_millis: job.duration_millis,
            idempotency_key: job.idempotency_key.clone(),
            is_completed: job.status == DispatchStatus::Completed,
            is_terminal: job.status.is_terminal(),
            projected_at: Some(Utc::now()),
        }
    }
}
