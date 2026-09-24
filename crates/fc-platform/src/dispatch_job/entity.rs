//! Dispatch Job Entity
//!
//! Represents async delivery of an event/task to a target endpoint.
//! Tracks full lifecycle with attempt history.

use crate::shared::enum_str::UnknownEnumValue;
use chrono::{DateTime, Utc};
pub use fc_common::DispatchMode;
pub use fc_common::DispatchStatus;
use serde::{Deserialize, Serialize};

/// Strict parse of a dispatch job status (X-06): anything but a known
/// spelling is an error. `fc_common::DispatchStatus::from_str` maps unknown
/// input to PENDING, so fc-platform parses through this instead.
/// `IN_PROGRESS` and `ERROR` are legacy stored spellings of PROCESSING and
/// FAILED.
pub fn parse_dispatch_status(s: &str) -> Result<DispatchStatus, UnknownEnumValue> {
    Ok(match s {
        "PENDING" => DispatchStatus::Pending,
        "QUEUED" => DispatchStatus::Queued,
        "PROCESSING" | "IN_PROGRESS" => DispatchStatus::Processing,
        "COMPLETED" => DispatchStatus::Completed,
        "FAILED" | "ERROR" => DispatchStatus::Failed,
        "CANCELLED" => DispatchStatus::Cancelled,
        "EXPIRED" => DispatchStatus::Expired,
        _ => {
            return Err(UnknownEnumValue::new(
                "dispatch job status",
                s,
                &[
                    "PENDING",
                    "QUEUED",
                    "PROCESSING",
                    "COMPLETED",
                    "FAILED",
                    "CANCELLED",
                    "EXPIRED",
                ],
            ))
        }
    })
}

/// Dispatch mode, leniently (ruling X-01, the one exemption from X-06).
/// Absent or empty means unspecified and takes the default, NEXT_ON_ERROR;
/// an unrecognised value also takes the default but logs a warning, because
/// silently reading a typo as some mode is how a producer loses ordering
/// without noticing. Matches Go's `common.ParseDispatchMode`.
pub fn parse_dispatch_mode(s: Option<&str>) -> DispatchMode {
    match s {
        Some("IMMEDIATE") => DispatchMode::Immediate,
        Some("NEXT_ON_ERROR") => DispatchMode::NextOnError,
        Some("BLOCK_ON_ERROR") => DispatchMode::BlockOnError,
        None | Some("") => DispatchMode::default(),
        Some(other) => {
            tracing::warn!(
                value = other,
                default = DispatchMode::default().as_str(),
                "unrecognised dispatch mode; using the default"
            );
            DispatchMode::default()
        }
    }
}

/// Dispatch job kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum DispatchKind {
    /// Dispatching an event
    #[default]
    Event,
    /// Dispatching a task/command
    Task,
}

crate::shared::enum_str::str_enum!(DispatchKind, "dispatch kind", {
    Event => "EVENT",
    Task => "TASK",
});

/// Target protocol for dispatch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum DispatchProtocol {
    #[default]
    HttpWebhook,
}

crate::shared::enum_str::str_enum!(DispatchProtocol, "dispatch protocol", {
    HttpWebhook => "HTTP_WEBHOOK",
});

/// Retry strategy for failed jobs.
///
/// Stored and sent lowercase (`immediate` / `fixed` / `exponential`), as the
/// stored data and the Go port spell it. The uppercase forms are legacy
/// spellings that are still accepted on input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RetryStrategy {
    /// Immediate retry
    #[serde(rename = "immediate", alias = "IMMEDIATE")]
    Immediate,
    /// Fixed delay between retries
    #[serde(rename = "fixed", alias = "FIXED_DELAY")]
    FixedDelay,
    /// Exponential backoff
    #[default]
    #[serde(rename = "exponential", alias = "EXPONENTIAL_BACKOFF")]
    ExponentialBackoff,
}

crate::shared::enum_str::str_enum!(RetryStrategy, "retry strategy", {
    Immediate => "immediate" | "IMMEDIATE",
    FixedDelay => "fixed" | "FIXED_DELAY",
    ExponentialBackoff => "exponential" | "EXPONENTIAL_BACKOFF",
});

/// Error type classification — matches TS DispatchErrorType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorType {
    /// Network/connection error (retriable)
    Connection,
    /// Timeout (retriable)
    Timeout,
    /// HTTP error (4xx or 5xx)
    HttpError,
    /// Validation/configuration error (not retriable)
    Validation,
    /// Unknown error
    Unknown,
}

crate::shared::enum_str::str_enum!(ErrorType, "error type", {
    Connection => "CONNECTION",
    Timeout => "TIMEOUT",
    HttpError => "HTTP_ERROR",
    Validation => "VALIDATION",
    Unknown => "UNKNOWN",
});

/// Outcome of one delivery attempt, as stored in
/// `msg_dispatch_job_attempts.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchAttemptStatus {
    Success,
    /// Earlier Rust builds wrote `FAILED`; accepted on read.
    #[serde(alias = "FAILED")]
    Failure,
}

crate::shared::enum_str::str_enum!(DispatchAttemptStatus, "dispatch attempt status", {
    Success => "SUCCESS",
    Failure => "FAILURE" | "FAILED",
});

/// Dispatch attempt record
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchAttempt {
    /// Attempt number (1-based)
    pub attempt_number: u32,

    /// When the attempt started
    pub attempted_at: DateTime<Utc>,

    /// When the attempt completed
    #[serde(skip_serializing_if = "Option::is_none")]
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

    pub fn complete_failure(
        mut self,
        error_message: String,
        error_type: ErrorType,
        response_code: Option<u16>,
    ) -> Self {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,

    /// Content type of payload
    #[serde(default = "default_content_type")]
    pub payload_content_type: String,

    /// If true, send raw data only. If false, wrap in envelope with metadata.
    #[serde(default = "default_data_only")]
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

    /// Schema ID for payload validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_id: Option<String>,

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

    /// Attempt history (loaded separately from msg_dispatch_job_attempts table)
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
    pub created_at: DateTime<Utc>,

    /// When the job was last updated
    pub updated_at: DateTime<Utc>,

    /// When the job is scheduled for dispatch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<DateTime<Utc>>,

    /// When the job expires
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,

    /// When the last attempt was made
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<DateTime<Utc>>,

    /// When the job completed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,

    /// Total duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_millis: Option<i64>,
}

pub(crate) fn default_content_type() -> String {
    "application/json".to_string()
}

fn default_data_only() -> bool {
    false
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
        event_id: Option<&str>,
        event_type: impl Into<String>,
        source: Option<&str>,
        target_url: impl Into<String>,
        payload: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate_untyped(),
            external_id: None,
            kind: DispatchKind::Event,
            code: event_type.into(),
            source: source.map(String::from),
            subject: None,
            target_url: target_url.into(),
            protocol: DispatchProtocol::HttpWebhook,
            payload: Some(payload.into()),
            payload_content_type: default_content_type(),
            data_only: false,
            event_id: event_id.map(String::from),
            correlation_id: None,
            client_id: None,
            subscription_id: None,
            service_account_id: None,
            dispatch_pool_id: None,
            message_group: None,
            mode: DispatchMode::Immediate,
            sequence: default_sequence(),
            timeout_seconds: default_timeout(),
            schema_id: None,
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
            scheduled_for: None,
            expires_at: None,
            last_attempt_at: None,
            completed_at: None,
            duration_millis: None,
        }
    }

    /// Create a new dispatch job for a task
    pub fn for_task(
        code: impl Into<String>,
        source: Option<&str>,
        target_url: impl Into<String>,
        payload: impl Into<String>,
    ) -> Self {
        Self {
            kind: DispatchKind::Task,
            ..Self::for_event(None, code, source, target_url, payload)
        }
    }

    /// Parse the code field into (application, subdomain, aggregate) parts.
    /// Codes follow the pattern "application:subdomain:aggregate:action" or similar colon-separated format.
    pub fn parse_code_parts(&self) -> (Option<String>, Option<String>, Option<String>) {
        let parts: Vec<&str> = self.code.split(':').collect();
        let application = parts.first().map(|s| s.to_string());
        let subdomain = parts.get(1).map(|s| s.to_string());
        let aggregate = parts.get(2).map(|s| s.to_string());
        (application, subdomain, aggregate)
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

    /// Mark the job as queued (schedule it for now)
    pub fn mark_queued(&mut self) {
        self.status = DispatchStatus::Queued;
        self.scheduled_for = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark the job as in progress
    pub fn mark_in_progress(&mut self) {
        self.status = DispatchStatus::Processing;
        self.updated_at = Utc::now();
    }

    /// Record a successful attempt and complete the job
    pub fn complete_success(&mut self, response_code: u16, response_body: Option<String>) {
        self.attempt_count += 1;
        let attempt =
            DispatchAttempt::new(self.attempt_count).complete_success(response_code, response_body);
        self.attempts.push(attempt);

        self.status = DispatchStatus::Completed;
        let now = Utc::now();
        self.completed_at = Some(now);
        self.last_attempt_at = Some(now);
        self.duration_millis = Some((now - self.created_at).num_milliseconds());
        self.updated_at = now;
    }

    /// Record a failed attempt
    pub fn record_failure(
        &mut self,
        error_message: String,
        error_type: ErrorType,
        response_code: Option<u16>,
    ) {
        self.attempt_count += 1;
        let attempt = DispatchAttempt::new(self.attempt_count).complete_failure(
            error_message.clone(),
            error_type,
            response_code,
        );
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
            self.scheduled_for = Some(self.calculate_next_retry());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_dispatch_job_for_event() {
        let job = DispatchJob::for_event(
            Some("evt123"),
            "orders:fulfillment:shipment:shipped",
            Some("my-app"),
            "https://example.com/webhook",
            r#"{"orderId":"123"}"#,
        );

        assert!(!job.id.is_empty());
        assert_eq!(
            job.id.len(),
            13,
            "Untyped ID should be 13 chars, got: {}",
            job.id.len()
        );
        assert!(
            !job.id.contains('_'),
            "Untyped ID should not contain underscore prefix"
        );
        assert_eq!(job.kind, DispatchKind::Event);
        assert_eq!(job.code, "orders:fulfillment:shipment:shipped");
        assert_eq!(job.source, Some("my-app".to_string()));
        assert_eq!(job.target_url, "https://example.com/webhook");
        assert_eq!(job.payload, Some(r#"{"orderId":"123"}"#.to_string()));
        assert_eq!(job.event_id, Some("evt123".to_string()));
        assert_eq!(job.status, DispatchStatus::Pending);
        assert_eq!(job.attempt_count, 0);
        assert_eq!(job.max_retries, 3);
        assert_eq!(job.timeout_seconds, 30);
        assert_eq!(job.retry_strategy, RetryStrategy::ExponentialBackoff);
        assert_eq!(job.protocol, DispatchProtocol::HttpWebhook);
        assert_eq!(job.mode, DispatchMode::Immediate);
        assert_eq!(job.sequence, 99);
        assert!(!job.data_only);
        assert_eq!(job.payload_content_type, "application/json");
        assert!(job.attempts.is_empty());
        assert!(job.metadata.is_empty());
    }

    #[test]
    fn test_dispatch_job_for_task() {
        let job = DispatchJob::for_task(
            "process-order",
            Some("task-runner"),
            "https://example.com/tasks",
            r#"{"taskId":"t1"}"#,
        );

        assert_eq!(job.kind, DispatchKind::Task);
        assert!(job.event_id.is_none(), "Task should not have event_id");
        assert_eq!(job.code, "process-order");
        assert_eq!(job.source, Some("task-runner".to_string()));
    }

    #[test]
    fn test_dispatch_job_unique_ids() {
        let j1 = DispatchJob::for_event(Some("e1"), "t1", Some("s"), "u", "p");
        let j2 = DispatchJob::for_event(Some("e2"), "t2", Some("s"), "u", "p");
        assert_ne!(j1.id, j2.id);
    }

    // --- DispatchKind ---

    #[test]
    fn test_dispatch_kind_as_str() {
        assert_eq!(DispatchKind::Event.as_str(), "EVENT");
        assert_eq!(DispatchKind::Task.as_str(), "TASK");
    }

    #[test]
    fn test_dispatch_kind_from_str() {
        assert_eq!(DispatchKind::from_str("EVENT"), Ok(DispatchKind::Event));
        assert_eq!(DispatchKind::from_str("TASK"), Ok(DispatchKind::Task));
        assert!(DispatchKind::from_str("unknown").is_err());
    }

    #[test]
    fn test_dispatch_kind_default() {
        assert_eq!(DispatchKind::default(), DispatchKind::Event);
    }

    #[test]
    fn test_dispatch_kind_roundtrip() {
        for kind in [DispatchKind::Event, DispatchKind::Task] {
            assert_eq!(DispatchKind::from_str(kind.as_str()), Ok(kind));
        }
    }

    // --- DispatchStatus ---

    #[test]
    fn test_dispatch_status_as_str() {
        assert_eq!(DispatchStatus::Pending.as_str(), "PENDING");
        assert_eq!(DispatchStatus::Queued.as_str(), "QUEUED");
        assert_eq!(DispatchStatus::Processing.as_str(), "PROCESSING");
        assert_eq!(DispatchStatus::Completed.as_str(), "COMPLETED");
        assert_eq!(DispatchStatus::Failed.as_str(), "FAILED");
        assert_eq!(DispatchStatus::Cancelled.as_str(), "CANCELLED");
        assert_eq!(DispatchStatus::Expired.as_str(), "EXPIRED");
    }

    #[test]
    fn test_dispatch_status_from_str() {
        assert_eq!(
            parse_dispatch_status("PENDING"),
            Ok(DispatchStatus::Pending)
        );
        assert_eq!(parse_dispatch_status("QUEUED"), Ok(DispatchStatus::Queued));
        assert_eq!(
            parse_dispatch_status("PROCESSING"),
            Ok(DispatchStatus::Processing)
        );
        assert_eq!(
            parse_dispatch_status("IN_PROGRESS"),
            Ok(DispatchStatus::Processing)
        );
        assert_eq!(
            parse_dispatch_status("COMPLETED"),
            Ok(DispatchStatus::Completed)
        );
        assert_eq!(parse_dispatch_status("FAILED"), Ok(DispatchStatus::Failed));
        assert_eq!(
            parse_dispatch_status("CANCELLED"),
            Ok(DispatchStatus::Cancelled)
        );
        assert_eq!(
            parse_dispatch_status("EXPIRED"),
            Ok(DispatchStatus::Expired)
        );
        assert!(parse_dispatch_status("unknown").is_err());
    }

    #[test]
    fn test_dispatch_status_default() {
        assert_eq!(DispatchStatus::default(), DispatchStatus::Pending);
    }

    #[test]
    fn test_dispatch_status_roundtrip() {
        for s in [
            DispatchStatus::Pending,
            DispatchStatus::Queued,
            DispatchStatus::Processing,
            DispatchStatus::Completed,
            DispatchStatus::Failed,
            DispatchStatus::Cancelled,
            DispatchStatus::Expired,
        ] {
            assert_eq!(
                parse_dispatch_status(s.as_str()),
                Ok(s),
                "Roundtrip failed for {:?}",
                s
            );
        }
    }

    #[test]
    fn test_dispatch_status_is_terminal() {
        assert!(!DispatchStatus::Pending.is_terminal());
        assert!(!DispatchStatus::Queued.is_terminal());
        assert!(!DispatchStatus::Processing.is_terminal());
        assert!(DispatchStatus::Completed.is_terminal());
        assert!(DispatchStatus::Failed.is_terminal());
        assert!(DispatchStatus::Cancelled.is_terminal());
        assert!(DispatchStatus::Expired.is_terminal());
    }

    #[test]
    fn test_dispatch_status_is_successful() {
        assert!(!DispatchStatus::Pending.is_successful());
        assert!(!DispatchStatus::Failed.is_successful());
        assert!(DispatchStatus::Completed.is_successful());
    }

    // --- DispatchProtocol ---

    #[test]
    fn test_dispatch_protocol_as_str() {
        assert_eq!(DispatchProtocol::HttpWebhook.as_str(), "HTTP_WEBHOOK");
    }

    #[test]
    fn test_dispatch_protocol_from_str() {
        assert_eq!(
            DispatchProtocol::from_str("HTTP_WEBHOOK"),
            Ok(DispatchProtocol::HttpWebhook)
        );
        assert!(DispatchProtocol::from_str("anything").is_err());
    }

    #[test]
    fn test_dispatch_protocol_default() {
        assert_eq!(DispatchProtocol::default(), DispatchProtocol::HttpWebhook);
    }

    // --- RetryStrategy ---

    #[test]
    fn test_retry_strategy_as_str() {
        assert_eq!(RetryStrategy::Immediate.as_str(), "immediate");
        assert_eq!(RetryStrategy::FixedDelay.as_str(), "fixed");
        assert_eq!(RetryStrategy::ExponentialBackoff.as_str(), "exponential");
    }

    #[test]
    fn test_retry_strategy_from_str() {
        assert_eq!(
            RetryStrategy::from_str("immediate"),
            Ok(RetryStrategy::Immediate)
        );
        assert_eq!(
            RetryStrategy::from_str("IMMEDIATE"),
            Ok(RetryStrategy::Immediate)
        );
        assert_eq!(
            RetryStrategy::from_str("fixed"),
            Ok(RetryStrategy::FixedDelay)
        );
        assert_eq!(
            RetryStrategy::from_str("FIXED_DELAY"),
            Ok(RetryStrategy::FixedDelay)
        );
        assert_eq!(
            RetryStrategy::from_str("exponential"),
            Ok(RetryStrategy::ExponentialBackoff)
        );
        assert_eq!(
            RetryStrategy::from_str("EXPONENTIAL_BACKOFF"),
            Ok(RetryStrategy::ExponentialBackoff)
        );
        assert!(RetryStrategy::from_str("unknown").is_err());
    }

    #[test]
    fn retry_strategy_serde_matches_stored_lowercase() {
        for (v, s) in [
            (RetryStrategy::Immediate, "immediate"),
            (RetryStrategy::FixedDelay, "fixed"),
            (RetryStrategy::ExponentialBackoff, "exponential"),
        ] {
            assert_eq!(v.as_str(), s);
            assert_eq!(serde_json::to_value(v).unwrap(), serde_json::json!(s));
            assert_eq!(
                serde_json::from_value::<RetryStrategy>(serde_json::json!(s)).unwrap(),
                v
            );
        }
        // Earlier serde spellings still read.
        for (s, v) in [
            ("IMMEDIATE", RetryStrategy::Immediate),
            ("FIXED_DELAY", RetryStrategy::FixedDelay),
            ("EXPONENTIAL_BACKOFF", RetryStrategy::ExponentialBackoff),
        ] {
            assert_eq!(
                serde_json::from_value::<RetryStrategy>(serde_json::json!(s)).unwrap(),
                v
            );
        }
    }

    #[test]
    fn dispatch_mode_is_lenient_and_defaults_to_next_on_error() {
        assert_eq!(
            parse_dispatch_mode(Some("IMMEDIATE")),
            DispatchMode::Immediate
        );
        assert_eq!(
            parse_dispatch_mode(Some("NEXT_ON_ERROR")),
            DispatchMode::NextOnError
        );
        assert_eq!(
            parse_dispatch_mode(Some("BLOCK_ON_ERROR")),
            DispatchMode::BlockOnError
        );
        // X-01: absent, empty and unknown all mean NEXT_ON_ERROR, never IMMEDIATE.
        for v in [
            None,
            Some(""),
            Some("immediate"),
            Some("BLOCKONERROR"),
            Some("junk"),
        ] {
            assert_eq!(parse_dispatch_mode(v), DispatchMode::NextOnError, "{v:?}");
        }
    }

    #[test]
    fn dispatch_status_parse_is_strict() {
        for s in [
            "PENDING",
            "QUEUED",
            "PROCESSING",
            "COMPLETED",
            "FAILED",
            "CANCELLED",
            "EXPIRED",
        ] {
            assert_eq!(parse_dispatch_status(s).unwrap().as_str(), s);
        }
        assert_eq!(
            parse_dispatch_status("IN_PROGRESS"),
            Ok(DispatchStatus::Processing)
        );
        assert_eq!(parse_dispatch_status("ERROR"), Ok(DispatchStatus::Failed));
        assert!(parse_dispatch_status("pending").is_err());
        assert!(parse_dispatch_status("").is_err());
    }

    #[test]
    fn dispatch_attempt_status_writes_failure_and_reads_legacy_failed() {
        crate::shared::enum_str::assert_str_enum(
            DispatchAttemptStatus::ALL,
            DispatchAttemptStatus::as_str,
        );
        assert_eq!(DispatchAttemptStatus::Failure.as_str(), "FAILURE");
        assert_eq!("FAILED".parse(), Ok(DispatchAttemptStatus::Failure));
    }

    #[test]
    fn test_retry_strategy_default() {
        assert_eq!(RetryStrategy::default(), RetryStrategy::ExponentialBackoff);
    }

    // --- DispatchMode (from fc_common) ---

    #[test]
    fn test_dispatch_mode_roundtrip() {
        for mode in [DispatchMode::Immediate, DispatchMode::BlockOnError] {
            let s = mode.as_str();
            assert_eq!(
                parse_dispatch_mode(Some(s)),
                mode,
                "Roundtrip failed for {:?}",
                mode
            );
        }
    }

    // --- Lifecycle methods ---

    #[test]
    fn test_mark_queued() {
        let mut job = DispatchJob::for_event(Some("e1"), "t", Some("s"), "u", "p");
        job.mark_queued();
        assert_eq!(job.status, DispatchStatus::Queued);
        assert!(job.scheduled_for.is_some());
    }

    #[test]
    fn test_mark_in_progress() {
        let mut job = DispatchJob::for_event(Some("e1"), "t", Some("s"), "u", "p");
        job.mark_in_progress();
        assert_eq!(job.status, DispatchStatus::Processing);
    }

    #[test]
    fn test_complete_success() {
        let mut job = DispatchJob::for_event(Some("e1"), "t", Some("s"), "u", "p");
        job.mark_in_progress();
        job.complete_success(200, Some("OK".to_string()));

        assert_eq!(job.status, DispatchStatus::Completed);
        assert_eq!(job.attempt_count, 1);
        assert!(job.completed_at.is_some());
        assert!(job.last_attempt_at.is_some());
        assert!(job.duration_millis.is_some());
        assert_eq!(job.attempts.len(), 1);
        assert!(job.attempts[0].success);
        assert_eq!(job.attempts[0].response_code, Some(200));
    }

    #[test]
    fn test_record_failure_with_retry() {
        let mut job = DispatchJob::for_event(Some("e1"), "t", Some("s"), "u", "p");
        // max_retries = 3, so first failure should schedule retry
        job.record_failure("Connection timeout".to_string(), ErrorType::Timeout, None);

        assert_eq!(
            job.status,
            DispatchStatus::Pending,
            "Should be back to Pending for retry"
        );
        assert_eq!(job.attempt_count, 1);
        assert_eq!(job.last_error, Some("Connection timeout".to_string()));
        assert!(job.scheduled_for.is_some(), "Should have scheduled retry");
        assert!(job.completed_at.is_none(), "Should not be completed yet");
    }

    #[test]
    fn test_record_failure_exhausted_retries() {
        let mut job = DispatchJob::for_event(Some("e1"), "t", Some("s"), "u", "p");
        // Exhaust all retries (max_retries = 3)
        job.record_failure("err".to_string(), ErrorType::HttpError, Some(500));
        job.record_failure("err".to_string(), ErrorType::HttpError, Some(500));
        job.record_failure("err".to_string(), ErrorType::HttpError, Some(500));

        assert_eq!(job.status, DispatchStatus::Failed);
        assert_eq!(job.attempt_count, 3);
        assert!(job.completed_at.is_some());
        assert!(job.duration_millis.is_some());
    }

    #[test]
    fn test_can_retry() {
        let mut job = DispatchJob::for_event(Some("e1"), "t", Some("s"), "u", "p");
        assert!(job.can_retry());

        job.complete_success(200, None);
        assert!(!job.can_retry(), "Completed job cannot be retried");
    }

    // --- Builder methods ---

    #[test]
    fn test_dispatch_job_builder_methods() {
        let job = DispatchJob::for_event(Some("e1"), "t", Some("s"), "u", "p")
            .with_client_id("client-1")
            .with_subscription_id("sub-1")
            .with_service_account_id("sa-1")
            .with_dispatch_pool_id("pool-1")
            .with_message_group("group-1")
            .with_mode(DispatchMode::BlockOnError)
            .with_correlation_id("corr-1")
            .with_data_only(false);

        assert_eq!(job.client_id, Some("client-1".to_string()));
        assert_eq!(job.subscription_id, Some("sub-1".to_string()));
        assert_eq!(job.service_account_id, Some("sa-1".to_string()));
        assert_eq!(job.dispatch_pool_id, Some("pool-1".to_string()));
        assert_eq!(job.message_group, Some("group-1".to_string()));
        assert_eq!(job.mode, DispatchMode::BlockOnError);
        assert_eq!(job.correlation_id, Some("corr-1".to_string()));
        assert!(!job.data_only);
    }

    // --- parse_code_parts ---

    #[test]
    fn test_parse_code_parts() {
        let job = DispatchJob::for_event(
            Some("e1"),
            "orders:fulfillment:shipment:shipped",
            Some("s"),
            "u",
            "p",
        );
        let (app, sub, agg) = job.parse_code_parts();
        assert_eq!(app, Some("orders".to_string()));
        assert_eq!(sub, Some("fulfillment".to_string()));
        assert_eq!(agg, Some("shipment".to_string()));
    }

    #[test]
    fn test_parse_code_parts_single() {
        let job = DispatchJob::for_event(Some("e1"), "simple", Some("s"), "u", "p");
        let (app, sub, agg) = job.parse_code_parts();
        assert_eq!(app, Some("simple".to_string()));
        assert!(sub.is_none());
        assert!(agg.is_none());
    }

    // --- DispatchAttempt ---

    #[test]
    fn test_dispatch_attempt_new() {
        let attempt = DispatchAttempt::new(1);
        assert_eq!(attempt.attempt_number, 1);
        assert!(!attempt.success);
        assert!(attempt.completed_at.is_none());
        assert!(attempt.response_code.is_none());
        assert!(attempt.error_message.is_none());
    }

    #[test]
    fn test_dispatch_attempt_complete_success() {
        let attempt = DispatchAttempt::new(1).complete_success(200, Some("OK".to_string()));
        assert!(attempt.success);
        assert!(attempt.completed_at.is_some());
        assert_eq!(attempt.response_code, Some(200));
        assert_eq!(attempt.response_body, Some("OK".to_string()));
        assert!(attempt.duration_millis.is_some());
    }

    #[test]
    fn test_dispatch_attempt_complete_failure() {
        let attempt = DispatchAttempt::new(2).complete_failure(
            "timeout".to_string(),
            ErrorType::Timeout,
            Some(504),
        );
        assert!(!attempt.success);
        assert!(attempt.completed_at.is_some());
        assert_eq!(attempt.error_message, Some("timeout".to_string()));
        assert_eq!(attempt.error_type, Some(ErrorType::Timeout));
        assert_eq!(attempt.response_code, Some(504));
    }

    // --- DispatchJobRead projection ---

    #[test]
    fn test_dispatch_job_read_from_job() {
        let job = DispatchJob::for_event(
            Some("e1"),
            "orders:billing:invoice:created",
            Some("app"),
            "https://x.com",
            "{}",
        )
        .with_client_id("c1");
        let read = DispatchJobRead::from(&job);

        assert_eq!(read.id, job.id);
        assert_eq!(read.code, job.code);
        assert_eq!(read.application, Some("orders".to_string()));
        assert_eq!(read.subdomain, Some("billing".to_string()));
        assert_eq!(read.aggregate, Some("invoice".to_string()));
        assert!(!read.is_completed);
        assert!(!read.is_terminal);
        assert_eq!(read.status, DispatchStatus::Pending);
    }

    #[test]
    fn test_dispatch_job_read_completed_flags() {
        let mut job = DispatchJob::for_event(Some("e1"), "t", Some("s"), "u", "p");
        job.complete_success(200, None);
        let read = DispatchJobRead::from(&job);

        assert!(read.is_completed);
        assert!(read.is_terminal);
    }

    // --- add_metadata ---

    #[test]
    fn test_add_metadata() {
        let mut job = DispatchJob::for_event(Some("e1"), "t", Some("s"), "u", "p");
        job.add_metadata("key1", "value1");
        job.add_metadata("key2", "value2");

        assert_eq!(job.metadata.len(), 2);
        assert_eq!(job.metadata[0].key, "key1");
        assert_eq!(job.metadata[0].value, "value1");
    }

    // ── Retry delay computation per strategy ──────────────────────────────
    // calculate_next_retry is private; observe its output via scheduled_for
    // after record_failure.

    fn make_retryable_job(strategy: RetryStrategy, max_retries: u32) -> DispatchJob {
        let mut job =
            DispatchJob::for_event(Some("e1"), "a:b:c:d", Some("src"), "https://x.com", "{}");
        job.retry_strategy = strategy;
        job.max_retries = max_retries;
        job
    }

    #[test]
    fn immediate_retry_strategy_schedules_for_now() {
        let mut job = make_retryable_job(RetryStrategy::Immediate, 3);
        let before = Utc::now();
        job.record_failure("boom".into(), ErrorType::HttpError, Some(500));
        let scheduled = job.scheduled_for.expect("retry scheduled");
        // Immediate → delay 0. Allow 5s slack for test clock.
        let diff = (scheduled - before).num_seconds();
        assert!(
            (0..5).contains(&diff),
            "immediate delay should be ~0, got {}s",
            diff
        );
    }

    #[test]
    fn fixed_delay_strategy_schedules_5s_out() {
        let mut job = make_retryable_job(RetryStrategy::FixedDelay, 3);
        let before = Utc::now();
        job.record_failure("boom".into(), ErrorType::HttpError, Some(500));
        let scheduled = job.scheduled_for.expect("retry scheduled");
        let diff = (scheduled - before).num_seconds();
        // FixedDelay = 5s. Allow 1s slack either side.
        assert!(
            (4..=7).contains(&diff),
            "fixed delay should be ~5s, got {}s",
            diff
        );
    }

    #[test]
    fn exponential_backoff_grows_by_powers_of_five() {
        // attempt_count starts at 0, record_failure increments it, then computes delay as 5^attempt_count.
        // So after 1st failure (attempt_count=1): delay ≈ 5s
        //    after 2nd failure (attempt_count=2): delay ≈ 25s
        //    after 3rd failure (attempt_count=3): delay ≈ 125s
        let mut job = make_retryable_job(RetryStrategy::ExponentialBackoff, 10);

        let t0 = Utc::now();
        job.record_failure("boom".into(), ErrorType::HttpError, None);
        let d1 = (job.scheduled_for.unwrap() - t0).num_seconds();
        assert!(
            (4..=7).contains(&d1),
            "attempt 1 delay should be ~5s, got {}s",
            d1
        );

        let t1 = Utc::now();
        job.record_failure("boom".into(), ErrorType::HttpError, None);
        let d2 = (job.scheduled_for.unwrap() - t1).num_seconds();
        assert!(
            (23..=28).contains(&d2),
            "attempt 2 delay should be ~25s, got {}s",
            d2
        );

        let t2 = Utc::now();
        job.record_failure("boom".into(), ErrorType::HttpError, None);
        let d3 = (job.scheduled_for.unwrap() - t2).num_seconds();
        assert!(
            (120..=130).contains(&d3),
            "attempt 3 delay should be ~125s, got {}s",
            d3
        );
    }

    #[test]
    fn exponential_backoff_caps_at_attempt_count_5() {
        // 5^5 = 3125 seconds; any further increase in attempt_count stops growing the exponent.
        let mut job = make_retryable_job(RetryStrategy::ExponentialBackoff, 100);
        // Push attempt_count to 6 before measuring the 7th failure's delay.
        for _ in 0..6 {
            job.record_failure("boom".into(), ErrorType::HttpError, None);
        }
        let t = Utc::now();
        job.record_failure("boom".into(), ErrorType::HttpError, None);
        let d = (job.scheduled_for.unwrap() - t).num_seconds();
        // After 7 failures, attempt_count=7 but .min(5) keeps delay at 5^5 = 3125s.
        assert!(
            (3100..=3150).contains(&d),
            "capped delay should be ~3125s, got {}s",
            d
        );
    }

    // ── Attempt-list integrity through a failure→success sequence ─────────

    #[test]
    fn attempts_list_grows_with_each_record_and_preserves_order() {
        let mut job = make_retryable_job(RetryStrategy::Immediate, 5);
        job.record_failure("e1".into(), ErrorType::Connection, None);
        job.record_failure("e2".into(), ErrorType::Timeout, Some(504));
        job.complete_success(200, Some("ok".into()));

        assert_eq!(job.attempts.len(), 3);
        assert_eq!(job.attempt_count, 3);
        assert_eq!(job.attempts[0].attempt_number, 1);
        assert_eq!(job.attempts[1].attempt_number, 2);
        assert_eq!(job.attempts[2].attempt_number, 3);
        assert_eq!(job.attempts[0].error_message, Some("e1".into()));
        assert_eq!(job.attempts[1].error_message, Some("e2".into()));
        assert!(job.attempts[2].success);
        assert_eq!(job.status, DispatchStatus::Completed);
        assert!(job.completed_at.is_some());
    }

    // ── Terminal states block can_retry ───────────────────────────────────

    #[test]
    fn terminal_statuses_block_retry_regardless_of_attempts() {
        let mut job = make_retryable_job(RetryStrategy::Immediate, 10);
        // Still has budget, but status forced to terminal
        job.status = DispatchStatus::Completed;
        assert!(!job.can_retry(), "Completed must not be retryable");
        job.status = DispatchStatus::Cancelled;
        assert!(!job.can_retry(), "Cancelled must not be retryable");
        job.status = DispatchStatus::Expired;
        assert!(!job.can_retry(), "Expired must not be retryable");
        job.status = DispatchStatus::Failed;
        assert!(!job.can_retry(), "Failed must not be retryable");
    }

    #[test]
    fn can_retry_false_when_attempts_exhausted_even_if_pending() {
        let mut job = make_retryable_job(RetryStrategy::Immediate, 2);
        job.status = DispatchStatus::Pending;
        job.attempt_count = 2;
        assert!(!job.can_retry(), "exhausted attempts must block retry");

        job.attempt_count = 1;
        assert!(
            job.can_retry(),
            "with budget remaining, retry should be allowed"
        );
    }

    // ── Exhaustion transitions status to Failed, not Pending ──────────────

    #[test]
    fn record_failure_transitions_to_failed_when_budget_exhausted() {
        let mut job = make_retryable_job(RetryStrategy::Immediate, 2);
        job.record_failure("first".into(), ErrorType::HttpError, Some(500));
        assert_eq!(job.status, DispatchStatus::Pending, "first failure retries");
        assert!(job.completed_at.is_none());

        job.record_failure("second".into(), ErrorType::HttpError, Some(500));
        assert_eq!(
            job.status,
            DispatchStatus::Failed,
            "final failure is terminal"
        );
        assert!(job.completed_at.is_some());
        assert_eq!(job.last_error, Some("second".into()));
    }
}

/// Dispatch job read projection - optimized for queries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobRead {
    pub id: String,
    pub external_id: Option<String>,
    pub source: Option<String>,
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
    pub application: Option<String>,
    pub subdomain: Option<String>,
    pub aggregate: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub duration_millis: Option<i64>,
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub is_terminal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projected_at: Option<DateTime<Utc>>,
}

impl From<&DispatchJob> for DispatchJobRead {
    fn from(job: &DispatchJob) -> Self {
        let (application, subdomain, aggregate) = job.parse_code_parts();
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
            application,
            subdomain,
            aggregate,
            created_at: job.created_at,
            updated_at: job.updated_at,
            scheduled_for: job.scheduled_for,
            expires_at: job.expires_at,
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
