//! # FlowCatalyst common types
//!
//! Shared data types and small utility modules used across every other
//! crate in the workspace (router, platform, queue, SDK, …). Keep this
//! crate dependency-light: anything that pulls in heavy infrastructure
//! (sqlx, reqwest, axum, …) belongs in `fc-platform` or `fc-router`. The one
//! exception is [`dispatch_lifecycle`], behind the off-by-default
//! `dispatch-lifecycle` feature (see its docs for why it lives here).
//!
//! ## Mental model
//!
//! - **`Message` / `QueuedMessage`** — the message envelope that flows
//!   between consumers, pools, and mediators. Serialized as camelCase
//!   JSON (the shared wire contract).
//! - **`MediationOutcome` / `MediationResult`** — what mediation returned;
//!   drives ack/nack and retry decisions.
//! - **`PoolConfig` / `QueueConfig` / `RouterConfig`** — runtime
//!   configuration of process pools and queues. Loaded from TOML or
//!   synced from the platform.
//! - **`Warning` / `HealthStatus` / pool metrics** — operational
//!   surfaces consumed by the monitoring API.
//! - **`OutboxItem` / `OutboxStatus`** — the transactional outbox row,
//!   shared with `fc-outbox` and `fc-sdk`.
//! - **`tsid`** — prefixed TSID generation; the canonical entity-id
//!   format across the platform.
//!
//! `tsid`, `audit_redaction`, `OutboxStatus` and `OutboxItemType` are
//! defined in the Apache-2.0 `fc-common-types` (so `fc-sdk` links no AGPL
//! code) and re-exported here.
//!
//! ## Public surface
//!
//! Most callers want the top-level types ([`Message`], [`MediationOutcome`],
//! [`PoolConfig`], [`Warning`]) and the [`tsid::EntityType`] enum used
//! everywhere ids are minted. Submodules [`config`] and [`logging`]
//! configure runtime infrastructure.
//!
//! ## Where to look first
//!
//! - Wire format: this file (`lib.rs`) — every shared DTO lives here.
//! - Id minting: [`tsid`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt;
use std::fmt::Formatter;
use std::result;
use std::sync::Arc;
use std::time::Instant;
use utoipa::ToSchema;

pub mod config;
pub mod diagnostics;
#[cfg(feature = "dispatch-lifecycle")]
pub mod dispatch_lifecycle;
#[cfg(feature = "dispatch-lifecycle")]
mod dispatch_sql;
pub mod error_chain;
pub mod logging;
pub mod netguard;

// The types fc-sdk shares with the platform live in the Apache-2.0
// fc-common-types (owner decision #51); re-exported here at their old paths.
pub use fc_common_types::tsid::EntityType;
pub use fc_common_types::{audit_redaction, tsid};

/// The version every server binary reports (`/health`, the router's
/// monitoring and health documents, the platform OpenAPI `info.version`):
/// owner decision #33, "Rust reports its real build version".
///
/// A release build sets `FC_BUILD_VERSION` when compiling (the Docker images
/// take it as a build argument: the release tag, else the commit), the way Go
/// links `-X …/server.Version=<v>`; otherwise it is the workspace package
/// version. An empty `FC_BUILD_VERSION` counts as unset.
pub const BUILD_VERSION: &str = match option_env!("FC_BUILD_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => env!("CARGO_PKG_VERSION"),
};

// ============================================================================
// Core Message Types
// ============================================================================

/// The core message structure that flows through the system.
///
/// Field names are camelCase on the wire.
///
/// `Serialize` is derived (wire output is unchanged), but `Deserialize` is
/// hand-written below so it can capture whether the wire payload actually
/// carried a `dispatchMode` value — see [`Message::dispatch_mode_specified`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    #[serde(default)]
    pub pool_code: String,
    pub auth_token: Option<String>,
    /// Signing secret for HMAC-SHA256 webhook signatures (Rust-only extension to the shared wire contract)
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
    /// Ledger A-09/X-01: an absent/unrecognised wire value resolves to
    /// [`DispatchMode::default`] (`NEXT_ON_ERROR`) here, so every ordinary
    /// reader (pool, mediator, disposition logic, …) can keep using this
    /// field directly without re-deriving the default itself.
    pub dispatch_mode: DispatchMode,
    /// Ledger R-13/R-16: `true` unless this `Message` was decoded from the
    /// wire with `dispatchMode` absent or `null`. The router's strict
    /// routing gate (`FC_ROUTER_STRICT_ROUTING`) needs to tell "producer
    /// omitted the field" apart from "producer sent a value that happens to
    /// equal the default" — information the collapsed `dispatch_mode` field
    /// above cannot carry once the A-09 default has been applied.
    ///
    /// Always `true` for a `Message` built directly in Rust code (tests, the
    /// scheduler, admin/publish endpoints, …) — only [`Message`]'s
    /// `Deserialize` impl can produce `false`. Never serialized; not part of
    /// the wire contract.
    #[serde(skip)]
    pub dispatch_mode_specified: bool,
}

impl Message {
    /// Whether the router delivers this message strictly after the previous
    /// one of its group has been acked: it has a non-empty group id and a
    /// dispatch mode that requires ordering. Its ack is on the group's
    /// critical path.
    pub fn is_ordered(&self) -> bool {
        self.message_group_id
            .as_deref()
            .is_some_and(|g| !g.is_empty())
            && self.dispatch_mode.requires_ordering()
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Wire-shape twin of `Message`, differing only in `dispatch_mode`:
        // `Option<DispatchMode>` here so `None` unambiguously means "the key
        // was absent (or null)" rather than "resolved to the default".
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct MessageWire {
            id: String,
            #[serde(default)]
            pool_code: String,
            auth_token: Option<String>,
            #[serde(default)]
            signing_secret: Option<String>,
            mediation_type: MediationType,
            mediation_target: String,
            #[serde(default)]
            message_group_id: Option<String>,
            #[serde(default)]
            high_priority: bool,
            #[serde(default)]
            dispatch_mode: Option<DispatchMode>,
        }

        let wire = MessageWire::deserialize(deserializer)?;
        Ok(Message {
            id: wire.id,
            pool_code: wire.pool_code,
            auth_token: wire.auth_token,
            signing_secret: wire.signing_secret,
            mediation_type: wire.mediation_type,
            mediation_target: wire.mediation_target,
            message_group_id: wire.message_group_id,
            high_priority: wire.high_priority,
            dispatch_mode_specified: wire.dispatch_mode.is_some(),
            dispatch_mode: wire.dispatch_mode.unwrap_or_default(),
        })
    }
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
    Immediate,
    /// If this message fails, skip it and continue with next in group.
    ///
    /// Ledger A-09/X-01: this is what an unspecified/unrecognised mode
    /// means, everywhere. It was `Immediate` — the only mode with no
    /// ordering at all — so a producer that omitted the field silently gave
    /// up sequencing, and the loss showed only under load. A default that
    /// quietly weakens a guarantee is the wrong way round: ordering is cheap
    /// to opt out of (set `IMMEDIATE`) and expensive to discover you never
    /// had.
    #[default]
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

    /// Maps a wire/stored value to a mode. Empty/unrecognised means
    /// unspecified, which is [`DispatchMode::default`] (`NEXT_ON_ERROR` —
    /// ledger A-09/X-01). Lenient by design: legacy databases and free-form
    /// callers may hand this an empty or garbled string, and this API
    /// intentionally swallows that rather than forcing every caller to
    /// handle a parse failure — hence the allow. FromStr's `Result` shape
    /// doesn't match, so it's not the trait method.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "IMMEDIATE" => Self::Immediate,
            "NEXT_ON_ERROR" => Self::NextOnError,
            "BLOCK_ON_ERROR" => Self::BlockOnError,
            _ => Self::default(),
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
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Expired
        )
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

    // Lenient: legacy aliases (IN_PROGRESS, ERROR) and unknown values
    // both map to a sane default rather than parse failures. See the
    // matching note on `DispatchMode::from_str`.
    #[allow(clippy::should_implement_trait)]
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
    pub broker_message_id: Option<String>, // SQS/broker message ID for deduplication
    pub queue_identifier: String,
}

/// Callback for ACK/NACK — called by the pool worker when processing completes.
/// Mirrors the TS `MessageCallback` pattern: the pool calls these directly,
/// no spawned task or channel needed.
#[async_trait::async_trait]
pub trait MessageCallback: Send + Sync {
    /// The pool is about to dispatch this message (Go:
    /// `InFlightTracker.EnsureTracked`). Restores the router's in-flight
    /// entry if it was reaped while the message sat buffered, and answers
    /// `false` when a DIFFERENT copy of the message now owns the pipeline —
    /// the caller should then ack this copy and not deliver it. Defaults to
    /// `true` (no tracking).
    fn ensure_tracked(&self) -> bool {
        true
    }

    /// The pool is retrying this message in place (Go:
    /// `InFlightTracker.MarkRetrying`): bumps the attempt count and stamps
    /// the retry time, so the in-flight reaper leaves a live retry alone and
    /// the stall detector reports it as retrying and never force-NACKs it.
    /// Defaults to a no-op.
    fn mark_retrying(&self) {}

    /// Acknowledge — delete from queue, clean up tracking.
    async fn ack(&self);
    /// Negative acknowledge — make visible again after delay, clean up tracking.
    async fn nack(&self, delay_seconds: Option<u32>);

    /// Whether the message's SOURCE broker holds a nacked message back for
    /// the requested delay before redelivering it (Go's
    /// `queue.Consumer.HonoursDelayedReturn`, owner ruling R5 2026-09-17).
    /// It decides what the pool does with a deferral that names a delay
    /// (`{"ack": false, "delaySeconds": N}`, N > 0): a broker that honours
    /// the delay gets the message back at once (R1); one that does not
    /// would redeliver it immediately, so the pool retries it in place
    /// instead. SQS and the Postgres queue honour it; Go's NATS consumer
    /// answers `false`. Defaults to `true`.
    fn honours_delayed_return(&self) -> bool {
        true
    }

    /// The router's diagnostics context for this message, opaque here: the
    /// flight recorder's per-message context, which the pool reuses for the
    /// events it records instead of building its own. Defaults to none.
    fn diagnostics(&self) -> Option<&(dyn Any + Send + Sync)> {
        None
    }
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
impl fmt::Debug for BatchMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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
    // Identity strings are `Arc<str>` so the router can share the ones it
    // already holds per message/batch instead of copying them.
    pub message_id: Arc<str>,
    pub broker_message_id: Option<String>,
    pub pool_code: Arc<str>,
    pub queue_identifier: Arc<str>,
    pub started_at: Instant,
    pub message_group_id: Option<Arc<str>>,
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
            message_id: Arc::from(message.id.as_str()),
            broker_message_id,
            pool_code: Arc::from(message.pool_code.as_str()),
            queue_identifier: Arc::from(queue_identifier),
            started_at: Instant::now(),
            message_group_id: message.message_group_id.as_deref().map(Arc::from),
            batch_id,
            receipt_handle,
        }
    }

    /// [`new`](Self::new) with the identity strings supplied already
    /// shared, so a router that holds them per message or batch allocates
    /// none of them here.
    pub fn from_shared(
        message_id: Arc<str>,
        pool_code: Arc<str>,
        queue_identifier: Arc<str>,
        message_group_id: Option<Arc<str>>,
        broker_message_id: Option<String>,
        batch_id: Option<Arc<str>>,
        receipt_handle: String,
    ) -> Self {
        Self {
            message_id,
            broker_message_id,
            pool_code,
            queue_identifier,
            started_at: Instant::now(),
            message_group_id,
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
    /// Destination throttled the request (HTTP 429). Retried in place by
    /// the pool with `Retry-After` as the backoff floor; does NOT count
    /// toward circuit-breaker failures — the destination is healthy, just
    /// throttling us.
    RateLimited,
    /// Target answered 2xx with `{"ack": false}` (ledger 22b/Deferred): it
    /// received the request and explicitly declined the work right now
    /// (e.g. a blocked record). Not a failure — the endpoint is reachable
    /// and healthy — so this must be breaker-neutral, like `RateLimited`.
    /// Retried in place by the pool on its deferred backoff curve, with the
    /// target's requested delay as the floor — or, when the target named a
    /// delay and the broker can hold it, handed back with exactly that
    /// delay (see `fc_router::pool::disposition_of`).
    Deferred,
    /// The mediator's own circuit breaker was open for this endpoint, so no
    /// network call was attempted at all (ledger R-12's breaker admission
    /// gate, now consulted inside `HttpMediator::mediate` rather than at
    /// the pool call site — see that module's doc comment). Like
    /// `RateLimited`/`Deferred` this is breaker-neutral (no call happened,
    /// so there is nothing new to say about the endpoint's health), but
    /// unlike them it is not a delivery attempt at all — nearer in kind to
    /// a pre-flight rejection.
    ///
    /// NACK with a short fixed delay, and (in an ordered group) hand the
    /// rest of the group back with it. No pool metric is recorded: nothing
    /// was attempted (corpus `breaker-open-makes-no-call`).
    CircuitOpen,
}

/// Outcome of mediation including result and optional delay
#[derive(Debug, Clone)]
pub struct MediationOutcome {
    pub result: MediationResult,
    pub delay_seconds: Option<u32>,
    pub status_code: Option<u16>,
    pub error_message: Option<String>,
    /// Set when a 2xx body carried `{"flushGroup": true}` (ledger A-05):
    /// the target wants this message's whole group suppressed rather than
    /// delivered message-by-message. Defaults `false` — only the mediator's
    /// 2xx-body parse (`mediator/response.rs`) ever sets it. The pool-side
    /// registry that actually suppresses the group is a later lane's work;
    /// this field only carries the target's request through the outcome.
    pub flush_group: bool,
    /// Set when the mediator rejected the message BEFORE any network call
    /// was made (ledger R-06/A-11) — an unsupported mediation type, or a
    /// target URL with no host to dial. Defaults `false`. A call that
    /// never happened is no evidence about the target's health in either
    /// direction, so the pool must skip BOTH a breaker success and a
    /// breaker failure when this is set (unlike a real `ErrorConfig` from
    /// an HTTP response, which still credits the breaker with a success —
    /// the endpoint answered, so it is reachable).
    pub pre_flight: bool,
}

impl MediationOutcome {
    /// `status`: the target's real HTTP status code (ledger A-04) — 200,
    /// 201, 204, etc. Flattening these to a hardcoded 200 discarded
    /// information an operator reading a trace could never recover.
    pub fn success(status: u16) -> Self {
        Self {
            result: MediationResult::Success,
            delay_seconds: None,
            status_code: Some(status),
            error_message: None,
            flush_group: false,
            pre_flight: false,
        }
    }

    pub fn error_config(status_code: u16, message: String) -> Self {
        Self {
            result: MediationResult::ErrorConfig,
            delay_seconds: None,
            status_code: Some(status_code),
            error_message: Some(message),
            flush_group: false,
            pre_flight: false,
        }
    }

    /// A pre-flight rejection (ledger R-06/A-11): the mediator refused the
    /// message before any network call — an unsupported mediation type, or
    /// a target URL with no host to dial. Classified as `ErrorConfig` (the
    /// message is permanently undeliverable as addressed, same ACK-drop
    /// treatment as a 4xx), but flagged `pre_flight` so the pool can skip
    /// the breaker entirely rather than crediting a success for a call
    /// that was never made. `status_code` is `None` (no HTTP exchange
    /// happened at all).
    pub fn pre_flight_rejected(message: String) -> Self {
        Self {
            result: MediationResult::ErrorConfig,
            delay_seconds: None,
            status_code: None,
            error_message: Some(message),
            flush_group: false,
            pre_flight: true,
        }
    }

    pub fn error_process(delay_seconds: Option<u32>, message: String) -> Self {
        Self {
            result: MediationResult::ErrorProcess,
            delay_seconds,
            status_code: None,
            error_message: Some(message),
            flush_group: false,
            pre_flight: false,
        }
    }

    pub fn error_connection(message: String) -> Self {
        Self {
            result: MediationResult::ErrorConnection,
            delay_seconds: Some(30),
            status_code: None,
            error_message: Some(message),
            flush_group: false,
            pre_flight: false,
        }
    }

    pub fn rate_limited(retry_after_seconds: u32) -> Self {
        Self {
            result: MediationResult::RateLimited,
            delay_seconds: Some(retry_after_seconds),
            status_code: Some(429),
            error_message: Some("HTTP 429: Too Many Requests".to_string()),
            flush_group: false,
            pre_flight: false,
        }
    }

    /// Target answered 2xx with `{"ack": false}` (ledger 22b/Deferred).
    /// `status_code` carries the target's real 2xx status (ledger A-04
    /// applies here too); `delay_seconds` is the target's requested delay
    /// (or the pool's own backoff floor when the target sent none — see
    /// `mediator/response.rs`'s doc comment).
    pub fn deferred(status_code: u16, delay_seconds: Option<u32>) -> Self {
        Self {
            result: MediationResult::Deferred,
            delay_seconds,
            status_code: Some(status_code),
            error_message: Some("Target returned ack=false".to_string()),
            flush_group: false,
            pre_flight: false,
        }
    }

    /// The endpoint's circuit breaker was open, so `HttpMediator::mediate`
    /// never attempted a network call. `status_code` is `None` (no HTTP
    /// exchange happened, same as a pre-flight rejection) and
    /// `delay_seconds` is a short fixed nack delay — see
    /// `MediationResult::CircuitOpen`'s doc for the full rationale and the
    /// known gap this port carries forward from the pool's pre-existing
    /// inline short-circuit.
    pub fn circuit_open() -> Self {
        Self {
            result: MediationResult::CircuitOpen,
            delay_seconds: Some(5),
            status_code: None,
            error_message: Some("Circuit breaker open".to_string()),
            flush_group: false,
            pre_flight: false,
        }
    }
}

// ============================================================================
// Outbox Types
// ============================================================================

// The status codes and item types live in fc-common-types (fc-sdk uses them).
pub use fc_common_types::outbox::{
    OutboxItemType, OutboxStatus, UnknownOutboxItemType, UnknownOutboxStatus,
};

/// One row of the outbox table.
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
    /// X-04: a message/consumer has stalled beyond the configured threshold
    Stall,
    /// X-04: a per-endpoint circuit breaker has tripped open
    CircuitBreaker,
}

/// Warning severity levels
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
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
    /// Total messages ACKed without delivery because their group was
    /// suppressed by a target's `flushGroup` request (ledger R-53) — kept
    /// separate from `total_success` so a heavily-flushed pool reads
    /// "busy-but-suppressed" rather than idle.
    #[serde(default)]
    pub total_suppressed: u64,
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

#[cfg(test)]
mod message_tests {
    use super::*;

    fn wire_message(body: serde_json::Value) -> Message {
        serde_json::from_value(body).expect("valid Message JSON")
    }

    fn base_message_json() -> serde_json::Value {
        serde_json::json!({
            "id": "msg-1",
            "mediationType": "HTTP",
            "mediationTarget": "http://localhost/webhook",
        })
    }

    // --- A-09/X-01: DispatchMode default and from_str fallback ---

    #[test]
    fn dispatch_mode_default_is_next_on_error() {
        assert_eq!(DispatchMode::default(), DispatchMode::NextOnError);
    }

    #[test]
    fn dispatch_mode_from_str_unknown_falls_back_to_next_on_error() {
        assert_eq!(DispatchMode::from_str("garbage"), DispatchMode::NextOnError);
        assert_eq!(DispatchMode::from_str(""), DispatchMode::NextOnError);
    }

    #[test]
    fn dispatch_mode_from_str_recognises_every_named_value() {
        assert_eq!(DispatchMode::from_str("IMMEDIATE"), DispatchMode::Immediate);
        assert_eq!(
            DispatchMode::from_str("immediate"),
            DispatchMode::Immediate,
            "case-insensitive"
        );
        assert_eq!(
            DispatchMode::from_str("NEXT_ON_ERROR"),
            DispatchMode::NextOnError
        );
        assert_eq!(
            DispatchMode::from_str("BLOCK_ON_ERROR"),
            DispatchMode::BlockOnError
        );
    }

    // --- R-13/R-16: Message wire absence vs presence of dispatchMode ---

    #[test]
    fn message_missing_dispatch_mode_defaults_and_is_unspecified() {
        let msg = wire_message(base_message_json());
        assert_eq!(msg.dispatch_mode, DispatchMode::NextOnError);
        assert!(
            !msg.dispatch_mode_specified,
            "an absent wire dispatchMode must be distinguishable from a present one"
        );
    }

    #[test]
    fn message_null_dispatch_mode_is_treated_as_unspecified() {
        let mut body = base_message_json();
        body["dispatchMode"] = serde_json::Value::Null;
        let msg = wire_message(body);
        assert_eq!(msg.dispatch_mode, DispatchMode::NextOnError);
        assert!(!msg.dispatch_mode_specified);
    }

    #[test]
    fn message_present_dispatch_mode_is_specified_even_if_it_equals_the_default() {
        let mut body = base_message_json();
        body["dispatchMode"] = serde_json::Value::String("NEXT_ON_ERROR".to_string());
        let msg = wire_message(body);
        assert_eq!(msg.dispatch_mode, DispatchMode::NextOnError);
        assert!(
            msg.dispatch_mode_specified,
            "a producer that explicitly sent NEXT_ON_ERROR must not look like it sent nothing"
        );
    }

    #[test]
    fn message_present_immediate_dispatch_mode_is_specified() {
        let mut body = base_message_json();
        body["dispatchMode"] = serde_json::Value::String("IMMEDIATE".to_string());
        let msg = wire_message(body);
        assert_eq!(msg.dispatch_mode, DispatchMode::Immediate);
        assert!(msg.dispatch_mode_specified);
    }

    #[test]
    fn message_dispatch_mode_specified_is_never_serialized() {
        let mut body = base_message_json();
        body["dispatchMode"] = serde_json::Value::String("BLOCK_ON_ERROR".to_string());
        let msg = wire_message(body);
        let out = serde_json::to_value(&msg).unwrap();
        assert!(out.get("dispatchModeSpecified").is_none());
        assert!(out.get("dispatch_mode_specified").is_none());
        assert_eq!(out["dispatchMode"], "BLOCK_ON_ERROR");
    }

    #[test]
    fn message_empty_pool_code_defaults_and_is_distinguishable() {
        let msg = wire_message(base_message_json());
        assert_eq!(msg.pool_code, "");
    }
}
