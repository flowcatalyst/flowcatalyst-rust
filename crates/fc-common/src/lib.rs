use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Instant;
use utoipa::ToSchema;

pub mod config;
pub mod logging;
pub mod tsid;

pub use tsid::{TsidGenerator, EntityType};

// ============================================================================
// Core Message Types
// ============================================================================

/// The core message structure that flows through the system.
///
/// This struct is compatible with Java's MessagePointer using camelCase field names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    #[serde(default)]
    pub pool_code: String,
    pub auth_token: Option<String>,
    /// Signing secret for HMAC-SHA256 webhook signatures (Rust extension, not in Java)
    #[serde(default)]
    pub signing_secret: Option<String>,
    pub mediation_type: MediationType,
    pub mediation_target: String,
    #[serde(default)]
    pub message_group_id: Option<String>,
    /// Whether this message should be processed with high priority
    #[serde(default)]
    pub high_priority: bool,
    /// Dispatch mode — controls ordering behavior within message groups.
    /// Default is Immediate (no ordering, concurrent processing allowed).
    #[serde(default)]
    pub dispatch_mode: DispatchMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediationType {
    HTTP,
}

/// Dispatch mode controls ordering behavior within a message group.
/// Shared across platform, scheduler, and router.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchMode {
    /// Process independently, no ordering guarantee within group
    #[default]
    Immediate,
    /// If this message fails, skip it and continue with next in group
    NextOnError,
    /// If this message fails, block all subsequent messages in group
    BlockOnError,
}

impl DispatchMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Immediate => "IMMEDIATE",
            Self::NextOnError => "NEXT_ON_ERROR",
            Self::BlockOnError => "BLOCK_ON_ERROR",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "NEXT_ON_ERROR" => Self::NextOnError,
            "BLOCK_ON_ERROR" => Self::BlockOnError,
            _ => Self::Immediate,
        }
    }

    /// Whether this mode requires sequential (FIFO) processing within a message group
    pub fn requires_ordering(&self) -> bool {
        matches!(self, Self::NextOnError | Self::BlockOnError)
    }
}

/// Dispatch job status lifecycle.
/// Shared across platform, scheduler, and router.
///
/// Matches TypeScript: `"PENDING" | "QUEUED" | "PROCESSING" | "COMPLETED" | "FAILED" | "CANCELLED" | "EXPIRED"`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchStatus {
    /// Job created, waiting to be queued
    #[default]
    Pending,
    /// Job queued for processing
    Queued,
    /// Job is being processed (webhook delivery in progress)
    Processing,
    /// Job completed successfully
    Completed,
    /// Job failed after all retries
    Failed,
    /// Job manually cancelled
    Cancelled,
    /// Job expired (TTL exceeded)
    Expired,
}

impl DispatchStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled | Self::Expired)
    }

    pub fn is_successful(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Queued => "QUEUED",
            Self::Processing => "PROCESSING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::Expired => "EXPIRED",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "PENDING" => Self::Pending,
            "QUEUED" => Self::Queued,
            "PROCESSING" | "IN_PROGRESS" => Self::Processing,
            "COMPLETED" => Self::Completed,
            "FAILED" | "ERROR" => Self::Failed,
            "CANCELLED" => Self::Cancelled,
            "EXPIRED" => Self::Expired,
            _ => Self::Pending,
        }
    }
}

/// A message that has been received from a queue with tracking metadata
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub message: Message,
    pub receipt_handle: String,
    pub broker_message_id: Option<String>,  // SQS/broker message ID for deduplication
    pub queue_identifier: String,
}

/// Callback for ACK/NACK — called by the pool worker when processing completes.
/// Mirrors the TS `MessageCallback` pattern: the pool calls these directly,
/// no spawned task or channel needed.
#[async_trait::async_trait]
pub trait MessageCallback: Send + Sync {
    /// Acknowledge — delete from queue, clean up tracking.
    async fn ack(&self);
    /// Negative acknowledge — make visible again after delay, clean up tracking.
    async fn nack(&self, delay_seconds: Option<u32>);
}

/// A message bundled with its callback for batch processing
pub struct BatchMessage {
    pub message: Message,
    pub receipt_handle: String,
    pub broker_message_id: Option<String>,
    pub queue_identifier: String,
    pub batch_id: Option<Arc<str>>,
    pub callback: Box<dyn MessageCallback>,
}

// Manual Debug since Box<dyn MessageCallback> isn't Debug
impl std::fmt::Debug for BatchMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchMessage")
            .field("message", &self.message)
            .field("receipt_handle", &self.receipt_handle)
            .field("broker_message_id", &self.broker_message_id)
            .field("batch_id", &self.batch_id)
            .finish()
    }
}

/// ACK/NACK response — still used internally for mediation result classification
#[derive(Debug, Clone)]
pub enum AckNack {
    Ack,
    Nack { delay_seconds: Option<u32> },
    ExtendVisibility { seconds: u32 },
}

// ============================================================================
// In-Flight Message Tracking
// ============================================================================

/// Tracks a message currently being processed
#[derive(Debug, Clone)]
pub struct InFlightMessage {
    pub message_id: String,
    pub broker_message_id: Option<String>,
    pub pool_code: String,
    pub queue_identifier: String,
    pub started_at: Instant,
    pub message_group_id: Option<String>,
    pub batch_id: Option<Arc<str>>,
    /// Current receipt handle - may be updated on SQS redelivery
    pub receipt_handle: String,
}

impl InFlightMessage {
    pub fn new(
        message: &Message,
        broker_message_id: Option<String>,
        queue_identifier: String,
        batch_id: Option<Arc<str>>,
        receipt_handle: String,
    ) -> Self {
        Self {
            message_id: message.id.clone(),
            broker_message_id,
            pool_code: message.pool_code.clone(),
            queue_identifier,
            started_at: Instant::now(),
            message_group_id: message.message_group_id.clone(),
            batch_id,
            receipt_handle,
        }
    }

    pub fn elapsed_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Update receipt handle when message is redelivered
    pub fn update_receipt_handle(&mut self, new_handle: String) {
        self.receipt_handle = new_handle;
    }
}

// ============================================================================
// Configuration Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PoolConfig {
    pub code: String,
    pub concurrency: u32,
    pub rate_limit_per_minute: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfig {
    pub name: String,
    pub uri: String,
    pub connections: u32,
    pub visibility_timeout: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    pub processing_pools: Vec<PoolConfig>,
    pub queues: Vec<QueueConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandbyConfig {
    pub enabled: bool,
    pub redis_url: String,
    pub lock_key: String,
    pub instance_id: String,
    pub lock_ttl_seconds: u64,
    pub refresh_interval_seconds: u64,
}

impl Default for StandbyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            redis_url: "redis://127.0.0.1:6379".to_string(),
            lock_key: "flowcatalyst:leader".to_string(),
            instance_id: uuid::Uuid::new_v4().to_string(),
            lock_ttl_seconds: 30,
            refresh_interval_seconds: 10,
        }
    }
}

/// Unified leader election configuration used by fc-outbox and fc-standby.
///
/// Union of the fields previously duplicated across those crates:
/// - `enabled`: whether leader election is active (fc-outbox semantics; fc-standby ignores)
/// - `redis_url`, `lock_key`, `lock_ttl_seconds`, `heartbeat_interval_seconds`: Redis-based lock
/// - `instance_id`: unique identifier for this process (auto-generated via uuid v4 by default)
#[derive(Debug, Clone)]
pub struct LeaderElectionConfig {
    /// Whether leader election is enabled
    pub enabled: bool,
    /// Redis connection URL
    pub redis_url: String,
    /// Key prefix for the lock
    pub lock_key: String,
    /// Lock TTL in seconds
    pub lock_ttl_seconds: u64,
    /// Heartbeat interval (should be less than TTL)
    pub heartbeat_interval_seconds: u64,
    /// Unique identifier for this instance
    pub instance_id: String,
}

impl LeaderElectionConfig {
    /// Create a new config with the given Redis URL and sensible defaults.
    pub fn new(redis_url: impl Into<String>) -> Self {
        Self {
            enabled: true,
            redis_url: redis_url.into(),
            lock_key: "fc:leader".to_string(),
            lock_ttl_seconds: 30,
            heartbeat_interval_seconds: 10,
            instance_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn with_lock_key(mut self, key: impl Into<String>) -> Self {
        self.lock_key = key.into();
        self
    }

    pub fn with_instance_id(mut self, id: impl Into<String>) -> Self {
        self.instance_id = id.into();
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl Default for LeaderElectionConfig {
    fn default() -> Self {
        Self::new("redis://127.0.0.1:6379")
    }
}

/// Configuration for stall detection
///
/// Stall detection monitors message groups that have been processing for too long.
/// When detected, it can emit warnings and optionally force-NACK stalled messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StallConfig {
    /// Whether stall detection is enabled
    pub enabled: bool,
    /// Threshold in seconds before a message is considered stalled
    pub stall_threshold_seconds: u64,
    /// Whether to force-NACK stalled messages after timeout
    pub force_nack_stalled: bool,
    /// Timeout in seconds after which to force-NACK stalled messages
    /// Only applies if force_nack_stalled is true
    pub force_nack_after_seconds: u64,
    /// Delay in seconds when NACKing stalled messages
    pub nack_delay_seconds: u32,
}

impl Default for StallConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            stall_threshold_seconds: 300, // 5 minutes
            force_nack_stalled: false,
            force_nack_after_seconds: 600, // 10 minutes
            nack_delay_seconds: 30,
        }
    }
}

/// Information about a stalled message group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StalledMessageInfo {
    pub message_id: String,
    pub message_group_id: Option<String>,
    pub pool_code: String,
    pub queue_identifier: String,
    pub elapsed_seconds: u64,
    pub detected_at: DateTime<Utc>,
}

// ============================================================================
// Mediation Types
// ============================================================================

/// Result of a mediation attempt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediationResult {
    /// Successfully delivered and acknowledged
    Success,
    /// Configuration error (4xx) - ACK to prevent infinite retries
    ErrorConfig,
    /// Transient error (5xx, timeout) - NACK for retry
    ErrorProcess,
    /// Connection error - NACK for retry
    ErrorConnection,
    /// Destination throttled the request (HTTP 429). NACK with `Retry-After`
    /// delay, but do NOT count toward circuit-breaker failures or attempt
    /// budget — the destination is healthy, just throttling us.
    RateLimited,
}

/// Outcome of mediation including result and optional delay
#[derive(Debug, Clone)]
pub struct MediationOutcome {
    pub result: MediationResult,
    pub delay_seconds: Option<u32>,
    pub status_code: Option<u16>,
    pub error_message: Option<String>,
}

impl MediationOutcome {
    pub fn success() -> Self {
        Self {
            result: MediationResult::Success,
            delay_seconds: None,
            status_code: Some(200),
            error_message: None,
        }
    }

    pub fn error_config(status_code: u16, message: String) -> Self {
        Self {
            result: MediationResult::ErrorConfig,
            delay_seconds: None,
            status_code: Some(status_code),
            error_message: Some(message),
        }
    }

    pub fn error_process(delay_seconds: Option<u32>, message: String) -> Self {
        Self {
            result: MediationResult::ErrorProcess,
            delay_seconds,
            status_code: None,
            error_message: Some(message),
        }
    }

    pub fn error_connection(message: String) -> Self {
        Self {
            result: MediationResult::ErrorConnection,
            delay_seconds: Some(30), // Java default: 30 seconds
            status_code: None,
            error_message: Some(message),
        }
    }

    pub fn rate_limited(retry_after_seconds: u32) -> Self {
        Self {
            result: MediationResult::RateLimited,
            delay_seconds: Some(retry_after_seconds),
            status_code: Some(429),
            error_message: Some("HTTP 429: Too Many Requests".to_string()),
        }
    }
}

// ============================================================================
// Outbox Types (Java-compatible)
// ============================================================================

/// Outbox status codes matching Java implementation
/// These are stored as integers in the database for Java compatibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
pub enum OutboxStatus {
    /// Item is pending processing (code: 0)
    PENDING,
    /// Item was successfully processed (code: 1)
    SUCCESS,
    /// Client error (4xx) - won't retry (code: 2)
    BAD_REQUEST,
    /// Server error (5xx) - will retry (code: 3)
    INTERNAL_ERROR,
    /// Authentication failed - will retry (code: 4)
    UNAUTHORIZED,
    /// Permission denied - won't retry (code: 5)
    FORBIDDEN,
    /// Gateway/upstream error - will retry (code: 6)
    GATEWAY_ERROR,
    /// Currently being processed (code: 9)
    IN_PROGRESS,
}

// Legacy aliases for backward compatibility
impl OutboxStatus {
    /// Alias for IN_PROGRESS (for Rust code compatibility)
    pub const PROCESSING: OutboxStatus = OutboxStatus::IN_PROGRESS;
    /// Alias for SUCCESS (for Rust code compatibility)
    pub const COMPLETED: OutboxStatus = OutboxStatus::SUCCESS;
    /// Alias for INTERNAL_ERROR (for Rust code compatibility)
    pub const FAILED: OutboxStatus = OutboxStatus::INTERNAL_ERROR;
}

impl OutboxStatus {
    /// Convert status to integer code for database storage
    pub fn code(&self) -> i32 {
        match self {
            OutboxStatus::PENDING => 0,
            OutboxStatus::SUCCESS => 1,
            OutboxStatus::BAD_REQUEST => 2,
            OutboxStatus::INTERNAL_ERROR => 3,
            OutboxStatus::UNAUTHORIZED => 4,
            OutboxStatus::FORBIDDEN => 5,
            OutboxStatus::GATEWAY_ERROR => 6,
            OutboxStatus::IN_PROGRESS => 9,
        }
    }

    /// Alias for code() - for compatibility
    pub fn to_code(&self) -> i32 {
        self.code()
    }

    /// Create status from integer code, defaulting to PENDING for unknown codes
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => OutboxStatus::PENDING,
            1 => OutboxStatus::SUCCESS,
            2 => OutboxStatus::BAD_REQUEST,
            3 => OutboxStatus::INTERNAL_ERROR,
            4 => OutboxStatus::UNAUTHORIZED,
            5 => OutboxStatus::FORBIDDEN,
            6 => OutboxStatus::GATEWAY_ERROR,
            9 => OutboxStatus::IN_PROGRESS,
            _ => OutboxStatus::PENDING, // Default for unknown codes
        }
    }

    /// Check if this status is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            OutboxStatus::INTERNAL_ERROR
                | OutboxStatus::UNAUTHORIZED
                | OutboxStatus::GATEWAY_ERROR
                | OutboxStatus::IN_PROGRESS
        )
    }

    /// Check if this status is terminal (won't retry)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            OutboxStatus::SUCCESS
                | OutboxStatus::BAD_REQUEST
                | OutboxStatus::FORBIDDEN
        )
    }
}

impl Default for OutboxStatus {
    fn default() -> Self {
        OutboxStatus::PENDING
    }
}

/// Outbox item type matching Java implementation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
pub enum OutboxItemType {
    /// Event items - sent to /api/events/batch
    EVENT,
    /// Dispatch job items - sent to /api/dispatch/jobs/batch
    DISPATCH_JOB,
    /// Audit log items - sent to /api/audit/logs/batch
    AUDIT_LOG,
}

impl OutboxItemType {
    /// All item types for iteration
    pub const ALL: [OutboxItemType; 3] = [
        OutboxItemType::EVENT,
        OutboxItemType::DISPATCH_JOB,
        OutboxItemType::AUDIT_LOG,
    ];

    /// Get the API endpoint path for this item type
    pub fn api_path(&self) -> &'static str {
        match self {
            OutboxItemType::EVENT => "/api/events/batch",
            OutboxItemType::DISPATCH_JOB => "/api/dispatch-jobs/batch",
            OutboxItemType::AUDIT_LOG => "/api/audit-logs/batch",
        }
    }

    /// Get the database type column value
    pub fn type_value(&self) -> &'static str {
        match self {
            OutboxItemType::EVENT => "EVENT",
            OutboxItemType::DISPATCH_JOB => "DISPATCH_JOB",
            OutboxItemType::AUDIT_LOG => "AUDIT_LOG",
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "EVENT" => Some(OutboxItemType::EVENT),
            "DISPATCH_JOB" | "DISPATCHJOB" | "DISPATCH-JOB" => Some(OutboxItemType::DISPATCH_JOB),
            "AUDIT_LOG" | "AUDITLOG" | "AUDIT-LOG" => Some(OutboxItemType::AUDIT_LOG),
            _ => None,
        }
    }
}

impl Default for OutboxItemType {
    fn default() -> Self {
        OutboxItemType::EVENT
    }
}

impl std::fmt::Display for OutboxItemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutboxItemType::EVENT => write!(f, "EVENT"),
            OutboxItemType::DISPATCH_JOB => write!(f, "DISPATCH_JOB"),
            OutboxItemType::AUDIT_LOG => write!(f, "AUDIT_LOG"),
        }
    }
}

/// Outbox item matching Java/TypeScript implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    /// Unique identifier (TSID Crockford Base32)
    pub id: String,
    /// Item type: EVENT, DISPATCH_JOB, or AUDIT_LOG
    pub item_type: OutboxItemType,
    /// Message group for FIFO ordering (optional)
    pub message_group: Option<String>,
    /// JSON payload
    pub payload: serde_json::Value,
    /// Current status (integer code)
    pub status: OutboxStatus,
    /// Number of retry attempts
    pub retry_count: i32,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
    /// Error message from last failure (optional)
    pub error_message: Option<String>,
    /// Client ID for multi-tenant filtering (optional)
    pub client_id: Option<String>,
    /// Size of the payload in bytes (optional)
    pub payload_size: Option<i32>,
    /// Additional headers as JSON (optional)
    pub headers: Option<serde_json::Value>,
}

// ============================================================================
// Warning System Types
// ============================================================================

/// Warning categories for the message router
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum WarningCategory {
    /// Message routing issues
    Routing,
    /// Message processing failures
    Processing,
    /// Configuration errors
    Configuration,
    /// Message group thread restart
    GroupThreadRestart,
    /// Rate limiting triggered
    RateLimiting,
    /// Queue connectivity issues
    QueueConnectivity,
    /// Pool capacity issues
    PoolCapacity,
    /// Pool health/limit issues
    PoolHealth,
    /// Queue health issues (backlog, growth)
    QueueHealth,
    /// Consumer health issues
    ConsumerHealth,
    /// Memory/resource issues
    Resource,
}

/// Warning severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema)]
pub enum WarningSeverity {
    /// Informational warning
    Info,
    /// Warning that may need attention
    Warn,
    /// Error requiring attention
    Error,
    /// Critical error requiring immediate attention
    Critical,
}

/// A system warning
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Warning {
    pub id: String,
    pub category: WarningCategory,
    pub severity: WarningSeverity,
    pub message: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub acknowledged: bool,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

impl Warning {
    pub fn new(
        category: WarningCategory,
        severity: WarningSeverity,
        message: String,
        source: String,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            category,
            severity,
            message,
            source,
            created_at: Utc::now(),
            acknowledged: false,
            acknowledged_at: None,
        }
    }

    pub fn age_minutes(&self) -> i64 {
        (Utc::now() - self.created_at).num_minutes()
    }
}

/// Overall system health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum HealthStatus {
    /// All systems operational
    Healthy,
    /// Some issues detected but operational
    Warning,
    /// Significant issues affecting operations
    Degraded,
}

/// Detailed health report
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthReport {
    pub status: HealthStatus,
    pub pools_healthy: u32,
    pub pools_unhealthy: u32,
    pub consumers_healthy: u32,
    pub consumers_unhealthy: u32,
    pub active_warnings: u32,
    pub critical_warnings: u32,
    pub issues: Vec<String>,
}

// ============================================================================
// Health & Metrics Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PoolStats {
    pub pool_code: String,
    pub concurrency: u32,
    pub active_workers: u32,
    pub queue_size: u32,
    pub queue_capacity: u32,
    pub message_group_count: u32,
    pub rate_limit_per_minute: Option<u32>,
    pub is_rate_limited: bool,
    /// Enhanced metrics (optional, available when metrics collection is enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<EnhancedPoolMetrics>,
}

/// Enhanced metrics for a processing pool
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnhancedPoolMetrics {
    /// Total messages processed successfully (all time)
    pub total_success: u64,
    /// Total messages failed (all time)
    pub total_failure: u64,
    /// Total messages rate limited (all time)
    pub total_rate_limited: u64,
    /// Success rate (0.0 - 1.0)
    pub success_rate: f64,
    /// Processing time metrics (all time)
    pub processing_time: ProcessingTimeMetrics,
    /// Metrics for the last 5 minutes
    pub last_5_min: WindowedMetrics,
    /// Metrics for the last 30 minutes
    pub last_30_min: WindowedMetrics,
}

/// Processing time metrics with percentiles
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingTimeMetrics {
    /// Average processing time in milliseconds
    pub avg_ms: f64,
    /// Minimum processing time in milliseconds
    pub min_ms: u64,
    /// Maximum processing time in milliseconds
    pub max_ms: u64,
    /// 50th percentile (median) in milliseconds
    pub p50_ms: u64,
    /// 95th percentile in milliseconds
    pub p95_ms: u64,
    /// 99th percentile in milliseconds
    pub p99_ms: u64,
    /// Total samples collected
    pub sample_count: u64,
}

impl Default for ProcessingTimeMetrics {
    fn default() -> Self {
        Self {
            avg_ms: 0.0,
            min_ms: 0,
            max_ms: 0,
            p50_ms: 0,
            p95_ms: 0,
            p99_ms: 0,
            sample_count: 0,
        }
    }
}

/// Time-windowed metrics
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WindowedMetrics {
    /// Messages processed successfully in this window
    pub success_count: u64,
    /// Messages failed in this window
    pub failure_count: u64,
    /// Messages rate limited in this window
    pub rate_limited_count: u64,
    /// Success rate in this window (0.0 - 1.0)
    pub success_rate: f64,
    /// Throughput (messages per second)
    pub throughput_per_sec: f64,
    /// Processing time metrics for this window
    pub processing_time: ProcessingTimeMetrics,
    /// Window start time
    pub window_start: DateTime<Utc>,
    /// Window duration in seconds
    pub window_duration_secs: u64,
}

impl Default for WindowedMetrics {
    fn default() -> Self {
        Self {
            success_count: 0,
            failure_count: 0,
            rate_limited_count: 0,
            success_rate: 0.0,
            throughput_per_sec: 0.0,
            processing_time: ProcessingTimeMetrics::default(),
            window_start: Utc::now(),
            window_duration_secs: 300, // 5 minutes default
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumerHealth {
    pub queue_identifier: String,
    pub is_healthy: bool,
    pub last_poll_time_ms: Option<i64>,
    pub time_since_last_poll_ms: Option<i64>,
    pub is_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfrastructureHealth {
    pub healthy: bool,
    pub message: String,
    pub issues: Vec<String>,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum FlowCatalystError {
    #[error("Queue error: {0}")]
    Queue(String),

    #[error("Pool error: {0}")]
    Pool(String),

    #[error("Mediation error: {0}")]
    Mediation(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Redis error: {0}")]
    Redis(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Shutdown in progress")]
    ShutdownInProgress,
}

pub type Result<T> = std::result::Result<T, FlowCatalystError>;
