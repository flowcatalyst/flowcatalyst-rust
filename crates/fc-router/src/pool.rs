//! ProcessPool - Worker pool with FIFO ordering, rate limiting, and concurrency control
//!
//! Uses lightweight per-message-group handlers (VecDeque + processing flag) instead of
//! dedicated tokio tasks with channels. A task is spawned only when there's work to do
//! and exits when the group's queue is empty. This matches the TS MessageGroupHandler
//! pattern and uses ~200 bytes per idle group vs ~100KB with the old design.
//!
//! Every task spawned by the pool (`spawn_immediate_task`'s standalone workers and
//! `spawn_drain_task`'s per-group drain loops) is spawned via `self.tracker`, a
//! `tokio_util::task::TaskTracker`, instead of bare `tokio::spawn`. Nothing explicitly
//! joins these tasks — they're self-terminating — but the tracker gives the pool a
//! tokio-native answer to "has everything finished?": `is_fully_drained()` is a
//! non-blocking `tracker.is_empty()` check, and `wait_drained()` closes the tracker
//! and awaits `tracker.wait()`. This replaces the older design of polling
//! `queue_size == 0 && active_workers == 0` on Relaxed atomics, which could read
//! "drained" momentarily between a counter decrement and the task's actual exit.

use arc_swap::ArcSwapOption;
use dashmap::{DashMap, DashSet};
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info, warn};

use crate::circuit_breaker_registry::breaker_key;
use crate::group_flush::GroupFlushRegistry;
use crate::mediator::Mediator;
use crate::metrics::PoolMetricsCollector;
use crate::Result;
use fc_common::{
    BatchMessage, DispatchMode, EnhancedPoolMetrics, MediationOutcome, MediationResult, Message,
    MessageCallback, PoolConfig, PoolStats,
};

const DEFAULT_GROUP: &str = "__DEFAULT__";
const QUEUE_CAPACITY_MULTIPLIER: u32 = 20; // Java: QUEUE_CAPACITY_MULTIPLIER = 20
const MIN_QUEUE_CAPACITY: u32 = 50; // Java: MIN_QUEUE_CAPACITY = 50

// ============================================================================
// Disposition — ledger A-27
// ============================================================================
//
// `disposition_of` is the single, pure decision that used to live twice as
// near-identical inline `match outcome.result { ... }` blocks in
// `spawn_immediate_task` and `spawn_drain_task` — one deciding the circuit
// breaker's ledger, one deciding the ack/nack/metric/cascade side effects.
// Extracting it means both call sites make the exact same decision by
// construction, and a test can assert the decision without spinning up a
// pool, a mediator, or a broker.
//
// Shaped after Go's `pool.go` `DispositionOf` (ledger A-27's reference
// shape — see `docs/router-gap-analysis.md`), adapted to what Rust's pool
// actually does today:
//
// - Go's `BrokerRetry` is an IN-PIPELINE retry (re-front the message on the
//   group's own buffer, no broker round-trip) gated by a per-message
//   attempts budget (`maxInPipelineAttempts`). Rust's pool has no such
//   path: every retryable outcome already nacks back to the broker for
//   redelivery. `BrokerAction::Retry` and the `attempts` parameter stay in
//   this shape for interface parity and so a future in-pipeline retry
//   budget has somewhere to land, but nothing produces `Retry` today.
// - Go's BLOCK_ON_ERROR ACKs untried siblings off the broker (its `A-01`
//   recovery path — a settled-message hook + platform reaper that does not
//   exist in this port yet). Ledger A-01 forbids shipping that branch here
//   before the platform half exists, so `GroupEffect::Block` here still
//   means "cascade a NACK to the buffered siblings as they're dequeued"
//   (the pre-existing `failed_batch_groups` mechanism), never an ACK.

/// What a [`Disposition`] does to a message at the broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerAction {
    /// Ack the message — delivered, or a permanent rejection that retrying
    /// cannot fix.
    Ack,
    /// Retry in place, without touching the broker. Reserved for a future
    /// in-pipeline retry budget — see the module doc above. Nothing
    /// produces this today; a call site that receives it treats it the
    /// same as `Release`.
    Retry,
    /// Nack the message back to the broker for redelivery — the target is
    /// unreachable, unavailable, or throttling.
    Release,
}

/// What a [`Disposition`] means for the rest of an ordered message group.
/// Meaningless for IMMEDIATE dispatch, which has no group buffer to affect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupEffect {
    /// The drainer moves on to the next buffered message (or exits idle if
    /// there is none). Covers a success and a discarded failure that
    /// doesn't cascade — either because the mode doesn't call for it
    /// (NEXT_ON_ERROR / IMMEDIATE) or because the outcome itself doesn't
    /// (RateLimited, Deferred — the target is healthy, just asking to
    /// wait).
    Continue,
    /// BLOCK_ON_ERROR's defining behaviour: the head failed terminally (a
    /// permanent, non-retryable rejection), so every message still
    /// buffered behind it is cascaded — NACKed as it's dequeued, never
    /// mediated — rather than delivered past the failure. Only ever
    /// produced when `mode == DispatchMode::BlockOnError`.
    Block,
    /// The target is unreachable/unavailable: this message AND everything
    /// still buffered behind it are cascaded back to the broker, under
    /// EVERY dispatch mode — an unreachable target says nothing about
    /// whether the message itself was wrong, so ordering must be
    /// preserved by returning the whole group rather than skipping ahead.
    Release,
}

/// Which [`PoolMetricsCollector`] method a [`Disposition`]'s outcome
/// records, as data rather than a call — the reason `disposition_of` can
/// stay pure. The call site applies it via `apply_metric`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositionMetric {
    /// Nothing recorded — no mediation was attempted (a suppressed-group
    /// ACK, a circuit-open release), so there is nothing to measure.
    None,
    Success,
    Failure,
    Transient,
    RateLimited,
}

/// `processOne`'s (here: the drain/immediate task's) pure verdict for a
/// mediation outcome: what happens to THIS message at the broker
/// ([`Self::action`]), what that means for the rest of an ordered group
/// ([`Self::group`]), the metric to record, and the nack delay to apply.
///
/// Produced by [`disposition_of`], which is pure — no I/O, no metrics
/// calls, no ack/nack calls. Every side effect stays at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disposition {
    pub action: BrokerAction,
    pub group: GroupEffect,
    pub metric: DispositionMetric,
    /// Seconds to pass to `MessageCallback::nack` when `action` is
    /// `Release` (or `Retry`). `None` for `Ack` (irrelevant) and for a
    /// `Release` with no specific delay (the call site's own default
    /// applies — see `disposition_of`'s `ErrorConnection` arm).
    pub retry_after_secs: Option<u32>,
}

/// Pure mapping from a mediation outcome to its [`Disposition`] (ledger
/// A-27 / A-01). No I/O, no metrics, no ack/nack/flush calls — everything
/// needed is passed in, so a test can call it directly instead of
/// re-deriving the decision from the delivery loop.
///
/// `attempts` is accepted for interface parity with Go's `DispositionOf`
/// (which gates an in-pipeline retry budget on it) but is not consulted
/// here — see the module doc above for why. It stays in the signature so a
/// future retry budget has somewhere to land without a second breaking
/// signature change.
///
/// `mode` only changes the `Group` effect of a permanent rejection
/// (`ErrorConfig`): ledger A-01 says BLOCK_ON_ERROR must stop the group at
/// a terminally failed head rather than deliver successors past it (the
/// pre-existing NACK cascade — see the module doc for why not the
/// ACK-the-siblings branch); every other mode continues past it.
pub fn disposition_of(
    outcome: &MediationOutcome,
    _attempts: u32,
    mode: DispatchMode,
) -> Disposition {
    match outcome.result {
        MediationResult::Success => Disposition {
            action: BrokerAction::Ack,
            group: GroupEffect::Continue,
            metric: DispositionMetric::Success,
            retry_after_secs: None,
        },

        MediationResult::ErrorConfig => {
            // Permanent ACK-drop: a 4xx, an unfollowed 3xx, a pre-flight
            // rejection, or (R-57) a 5xx the mediator has already
            // classified as "the app answered" rather than "unavailable".
            // BLOCK_ON_ERROR must stop the group at this failure rather
            // than deliver successors past it; every other mode moves on.
            let group = if mode == DispatchMode::BlockOnError {
                GroupEffect::Block
            } else {
                GroupEffect::Continue
            };
            Disposition {
                action: BrokerAction::Ack,
                group,
                metric: DispositionMetric::Failure,
                retry_after_secs: None,
            }
        }

        MediationResult::ErrorProcess => {
            // Pre-classified by the mediator (R-57): reaching this case at
            // all means "target unavailable" (502/503/504, or the
            // generic-failure fallback) — release rather than discard, and
            // release the WHOLE group (not just this message) so ordering
            // survives redelivery. Applies under every dispatch mode.
            Disposition {
                action: BrokerAction::Release,
                group: GroupEffect::Release,
                metric: DispositionMetric::Transient,
                retry_after_secs: outcome.delay_seconds,
            }
        }

        MediationResult::ErrorConnection => Disposition {
            // Transport failure / unreachable host / timeout — the target
            // is down. Same whole-group release as ErrorProcess, fixed 30s
            // delay (the mediator's own default for this outcome).
            action: BrokerAction::Release,
            group: GroupEffect::Release,
            metric: DispositionMetric::Failure,
            retry_after_secs: Some(30),
        },

        MediationResult::RateLimited => Disposition {
            // 429 — healthy destination asking us to slow down. NOT a
            // breaker failure (see `breaker_effect`) and does NOT cascade:
            // a rate limit says nothing about the rest of the group.
            action: BrokerAction::Release,
            group: GroupEffect::Continue,
            metric: DispositionMetric::RateLimited,
            retry_after_secs: Some(outcome.delay_seconds.unwrap_or(30)),
        },

        MediationResult::Deferred => Disposition {
            // 2xx + ack=false (ledger 22b) — the target explicitly
            // deferred this message. Not a failure: breaker-neutral, same
            // as RateLimited, and does not cascade either — requeue with
            // the target's requested delay (already floored to 0 by the
            // mediator when absent).
            action: BrokerAction::Release,
            group: GroupEffect::Continue,
            metric: DispositionMetric::Transient,
            retry_after_secs: outcome.delay_seconds,
        },
    }
}

/// Whether a mediation outcome counts toward the circuit breaker, and how.
/// `None` when the call never happened (pre-flight rejection, ledger
/// R-06/A-11 — no evidence about the target's health in either direction)
/// or shouldn't move the breaker either way (RateLimited, Deferred — the
/// target is healthy, just throttling/deferring).
fn breaker_effect(outcome: &MediationOutcome) -> Option<bool> {
    if outcome.pre_flight {
        return None;
    }
    match outcome.result {
        MediationResult::Success | MediationResult::ErrorConfig => Some(true),
        MediationResult::ErrorProcess | MediationResult::ErrorConnection => Some(false),
        MediationResult::RateLimited | MediationResult::Deferred => None,
    }
}

/// Apply a [`DispositionMetric`] to a [`PoolMetricsCollector`] — the single
/// place a `Disposition`'s metric turns into an actual `record_*` call, so
/// `disposition_of` itself never touches the collector.
fn apply_metric(collector: &PoolMetricsCollector, metric: DispositionMetric, duration_ms: u64) {
    match metric {
        DispositionMetric::None => {}
        DispositionMetric::Success => collector.record_success(duration_ms),
        DispositionMetric::Failure => collector.record_failure(duration_ms),
        DispositionMetric::Transient => collector.record_transient(duration_ms),
        DispositionMetric::RateLimited => collector.record_rate_limited(),
    }
}

// ============================================================================
// Group-flush suppression wiring (ledger A-05/R-52/R-53)
// ============================================================================

/// Check `task`'s message group against `flush_registry`; if the group is
/// currently suppressed, ACK it without ever calling the mediator and
/// record the suppressed-ACK metric. Returns `true` when the task was
/// fully handled this way — the caller must not mediate, rate-limit, or
/// otherwise touch it further (queue-size/batch-group bookkeeping is still
/// the caller's job, same as any other terminal path).
///
/// A message with no group id (or an empty one) is never suppressed —
/// suppression is a per-group concept.
async fn ack_if_suppressed(
    flush_registry: &GroupFlushRegistry,
    metrics_collector: &PoolMetricsCollector,
    task: &PoolTask,
) -> bool {
    let Some(group) = task
        .message
        .message_group_id
        .as_deref()
        .filter(|g| !g.is_empty())
    else {
        return false;
    };
    if !flush_registry.suppressed(group) {
        return false;
    }
    debug!(
        message_id = %task.message.id,
        group = %group,
        "Message group flushed; ACKing without delivery"
    );
    metrics_collector.record_suppressed();
    task.callback.ack().await;
    true
}

/// After a successful delivery, honour a `flushGroup: true` request on the
/// response (ledger A-05) by suppressing the rest of the message's group.
/// A no-op unless `outcome` is `Success` with `flush_group` set. Warns
/// (rather than suppressing nothing silently) when the target asked to
/// flush a message that has no group id — there is nothing to suppress.
fn maybe_flush_group(flush_registry: &GroupFlushRegistry, message: &Message, outcome: &MediationOutcome) {
    if outcome.result != MediationResult::Success || !outcome.flush_group {
        return;
    }
    match message
        .message_group_id
        .as_deref()
        .filter(|g| !g.is_empty())
    {
        Some(group) => {
            if flush_registry.flush(group, outcome.delay_seconds) {
                info!(
                    group = %group,
                    message_id = %message.id,
                    delay_seconds = ?outcome.delay_seconds,
                    "Message group flushed by target"
                );
            }
        }
        None => {
            warn!(message_id = %message.id, "flushGroup ignored: message has no message group");
        }
    }
}

/// Pool-wide rate limiter state shared across all message groups in a pool.
///
/// Bundles the configured `rpm` with the live `RateLimiter` so a single
/// `ArcSwapOption` swap atomically updates both — replacing the older
/// `Arc<RwLock<Option<Arc<RateLimiter>>>>` + `Arc<RwLock<Option<u32>>>`
/// pair. Readers (workers) take a lock-free snapshot via `.load()`; the
/// next acquire after an `update_rate_limit` picks up the new state.
type SharedRateLimiter = Arc<ArcSwapOption<RateLimitState>>;

#[derive(Debug)]
struct RateLimitState {
    rpm: u32,
    limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
}

impl RateLimitState {
    /// Build a `RateLimitState` from a per-minute quota. `Some(0)` and
    /// values that don't fit a `NonZeroU32` collapse to `None`.
    fn from_rpm(rpm: u32) -> Option<Arc<Self>> {
        if rpm == 0 {
            return None;
        }
        NonZeroU32::new(rpm).map(|nz| {
            Arc::new(Self {
                rpm,
                limiter: RateLimiter::direct(Quota::per_minute(nz)),
            })
        })
    }
}

/// Composite key for batch+group tracking - avoids format!() string allocation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BatchGroupKey {
    pub batch_id: Arc<str>,
    pub group_id: Arc<str>,
}

impl BatchGroupKey {
    #[inline]
    pub fn new(batch_id: &str, group_id: &str) -> Self {
        Self {
            batch_id: Arc::from(batch_id),
            group_id: Arc::from(group_id),
        }
    }
}

impl std::fmt::Display for BatchGroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.batch_id, self.group_id)
    }
}

/// Task submitted to a pool worker
pub struct PoolTask {
    pub message: Message,
    pub receipt_handle: String,
    pub callback: Box<dyn MessageCallback>,
    pub batch_id: Option<Arc<str>>,
    /// Pre-computed batch+group key for FIFO tracking
    pub batch_group_key: Option<BatchGroupKey>,
}

/// Lightweight per-message-group handler.
/// Just a queue of pending tasks and a flag — no tokio task, no channels.
/// A drain task is spawned only when work arrives for an idle group.
struct MessageGroupHandler {
    high_priority: VecDeque<PoolTask>,
    regular: VecDeque<PoolTask>,
    processing: bool,
}

impl MessageGroupHandler {
    fn new() -> Self {
        Self {
            high_priority: VecDeque::new(),
            regular: VecDeque::new(),
            processing: false,
        }
    }

    fn enqueue(&mut self, task: PoolTask, high_priority: bool) {
        if high_priority {
            self.high_priority.push_back(task);
        } else {
            self.regular.push_back(task);
        }
    }

    /// Dequeue next task, high priority first.
    fn dequeue(&mut self) -> Option<PoolTask> {
        self.high_priority
            .pop_front()
            .or_else(|| self.regular.pop_front())
    }

    fn is_empty(&self) -> bool {
        self.high_priority.is_empty() && self.regular.is_empty()
    }

    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.high_priority.len() + self.regular.len()
    }
}

/// Process pool with FIFO ordering and rate limiting
pub struct ProcessPool {
    config: PoolConfig,
    mediator: Arc<dyn Mediator>,

    /// Current concurrency level (may differ from config after updates)
    concurrency: AtomicU32,

    /// Pool-level concurrency semaphore
    semaphore: Arc<Semaphore>,

    /// Per-message-group handlers (lightweight: VecDeque + processing flag)
    group_handlers: Arc<DashMap<Arc<str>, parking_lot::Mutex<MessageGroupHandler>>>,

    /// Batch+group failure tracking for cascading NACKs
    failed_batch_groups: Arc<DashSet<BatchGroupKey>>,

    /// Track remaining messages per batch+group for cleanup
    batch_group_message_count: Arc<DashMap<BatchGroupKey, AtomicU32>>,

    /// Rate limiter + configured rpm bundled together. `ArcSwapOption`
    /// allows lock-free reads on the hot path (every dispatched message)
    /// and atomic hot-swap on `update_rate_limit`. The bundled `rpm`
    /// replaces the older separate `Arc<RwLock<Option<u32>>>` field —
    /// one primitive now holds both the live limiter and the value used
    /// to detect "did the config change?" during reconfig.
    rate_limiter: SharedRateLimiter,

    /// Running state
    running: AtomicBool,

    /// Queue size counter (Arc for sharing across tasks)
    queue_size: Arc<AtomicU32>,

    /// Active workers counter (Arc for sharing across tasks)
    active_workers: Arc<AtomicU32>,

    /// Enhanced metrics collector
    metrics_collector: Arc<PoolMetricsCollector>,

    /// Per-endpoint circuit breaker registry — shared across pools, keyed by mediation target URL.
    circuit_breaker_registry: Arc<crate::circuit_breaker_registry::CircuitBreakerRegistry>,

    /// Per-message-group delivery suppression registry (ledger A-05/R-52/R-53).
    /// Pool-private — unlike the circuit breaker registry, flushGroup
    /// suppression is scoped per pool (ledger R-55: same group id in two
    /// pools suppresses independently, deferred but current-behaviour-is-
    /// correct-by-construction here since there is no sharing to begin
    /// with).
    flush_registry: Arc<GroupFlushRegistry>,

    /// Tracks every worker/drain task spawned by this pool (`spawn_immediate_task`,
    /// `spawn_drain_task`). Tasks spawned through it are always tracked, before
    /// or after `close()` — closing only arms `wait()`, which then resolves as
    /// soon as the tracker is empty. `drain()`/`shutdown()` close it;
    /// `wait_drained()` awaits `tracker.wait()`; `is_fully_drained()` is a
    /// non-blocking snapshot of the same state via `tracker.is_empty()`.
    tracker: TaskTracker,
}

impl ProcessPool {
    /// Construct a pool with a private circuit breaker registry.
    /// **Test/standalone use only** — production pools are created by the
    /// `QueueManager` via [`ProcessPool::with_dependencies`] so they share the
    /// manager's single registry. Keeping this thin constructor lets unit tests
    /// build a pool without plumbing collaborators they don't exercise.
    pub fn new(config: PoolConfig, mediator: Arc<dyn Mediator>) -> Self {
        Self::with_dependencies(
            config,
            mediator,
            Arc::new(crate::circuit_breaker_registry::CircuitBreakerRegistry::default()),
        )
    }

    /// Construct a fully-wired pool. The `QueueManager` passes its shared
    /// circuit breaker registry here, so breaker state is shared across all
    /// pools (and visible to monitoring). Requiring the registry up front makes
    /// the previously-possible "pool created without the shared registry" bug
    /// unrepresentable on the production path.
    pub fn with_dependencies(
        config: PoolConfig,
        mediator: Arc<dyn Mediator>,
        circuit_breaker_registry: Arc<crate::circuit_breaker_registry::CircuitBreakerRegistry>,
    ) -> Self {
        // Java: effectiveConcurrency() — if concurrency is 0, fall back to max(rateLimitPerMinute/60, 1)
        let concurrency_val = if config.concurrency == 0 {
            config
                .rate_limit_per_minute
                .map(|rpm| (rpm / 60).max(1))
                .unwrap_or(1)
        } else {
            config.concurrency
        };

        let initial_rate_limit = config
            .rate_limit_per_minute
            .and_then(RateLimitState::from_rpm);

        Self {
            config: config.clone(),
            mediator,
            concurrency: AtomicU32::new(concurrency_val),
            semaphore: Arc::new(Semaphore::new(concurrency_val as usize)),
            group_handlers: Arc::new(DashMap::new()),
            failed_batch_groups: Arc::new(DashSet::new()),
            batch_group_message_count: Arc::new(DashMap::new()),
            rate_limiter: Arc::new(ArcSwapOption::new(initial_rate_limit)),
            running: AtomicBool::new(false),
            queue_size: Arc::new(AtomicU32::new(0)),
            active_workers: Arc::new(AtomicU32::new(0)),
            metrics_collector: Arc::new(PoolMetricsCollector::new()),
            circuit_breaker_registry,
            flush_registry: Arc::new(GroupFlushRegistry::new()),
            tracker: TaskTracker::new(),
        }
    }

    /// Start the pool
    pub async fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // Already running
        }

        info!(
            pool_code = %self.config.code,
            concurrency = self.config.concurrency,
            rate_limit = ?self.config.rate_limit_per_minute,
            "Starting process pool"
        );
    }

    /// Submit a message to the pool
    pub async fn submit(&self, batch_msg: BatchMessage) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            batch_msg.callback.nack(Some(10)).await;
            return Ok(());
        }

        // Check capacity
        let current_size = self.queue_size.load(Ordering::Relaxed);
        let capacity = std::cmp::max(
            self.config.concurrency * QUEUE_CAPACITY_MULTIPLIER,
            MIN_QUEUE_CAPACITY,
        );

        if current_size >= capacity {
            debug!(
                pool_code = %self.config.code,
                current = current_size,
                capacity = capacity,
                "Pool at capacity, rejecting"
            );
            batch_msg.callback.nack(Some(10)).await;
            return Ok(());
        }

        // Increment queue size
        self.queue_size.fetch_add(1, Ordering::Relaxed);

        // Get message group
        let group_id: Arc<str> = batch_msg
            .message
            .message_group_id
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| Arc::from(s.as_str()))
            .unwrap_or_else(|| Arc::from(DEFAULT_GROUP));

        // Track batch+group message count for cleanup
        let batch_group_key = batch_msg
            .batch_id
            .as_ref()
            .map(|batch_id| BatchGroupKey::new(batch_id, &group_id));

        if let Some(ref key) = batch_group_key {
            self.batch_group_message_count
                .entry(key.clone())
                .or_insert_with(|| AtomicU32::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }

        // Check if batch+group has failed (early check before queueing)
        if let Some(ref key) = batch_group_key {
            if self.failed_batch_groups.contains(key) {
                debug!(
                    message_id = %batch_msg.message.id,
                    batch_id = %key.batch_id,
                    group_id = %key.group_id,
                    "Batch+group failed, NACKing for FIFO"
                );
                self.queue_size.fetch_sub(1, Ordering::Relaxed);
                self.decrement_and_cleanup_batch_group(key);
                batch_msg.callback.nack(Some(10)).await;
                return Ok(());
            }
        }

        // IMMEDIATE mode: no ordering needed — spawn a standalone task per message.
        // This avoids the sequential drain bottleneck where a slow HTTP call blocks
        // all other messages in the group.
        if !batch_msg.message.dispatch_mode.requires_ordering() {
            let task = PoolTask {
                message: batch_msg.message,
                receipt_handle: String::new(), // not used in standalone path
                callback: batch_msg.callback,
                batch_id: batch_msg.batch_id,
                batch_group_key,
            };
            self.spawn_immediate_task(task);
            return Ok(());
        }

        let is_high_priority = batch_msg.message.high_priority;

        let task = PoolTask {
            message: batch_msg.message,
            receipt_handle: batch_msg.receipt_handle,
            callback: batch_msg.callback,
            batch_id: batch_msg.batch_id,
            batch_group_key,
        };

        // Ordered mode: enqueue to group handler and spawn drain task if idle
        let should_spawn = {
            let entry = self
                .group_handlers
                .entry(Arc::clone(&group_id))
                .or_insert_with(|| parking_lot::Mutex::new(MessageGroupHandler::new()));
            let mut handler = entry.lock();

            handler.enqueue(task, is_high_priority);

            if !handler.processing {
                handler.processing = true;
                true
            } else {
                false
            }
        };

        if should_spawn {
            self.spawn_drain_task(group_id);
        }

        Ok(())
    }

    /// Spawn a standalone task for an IMMEDIATE mode message.
    /// No group ordering — acquires semaphore, rate-limits, mediates, callbacks directly.
    ///
    /// **Owns:** Arc clones of the pool's semaphore / mediator / counters /
    /// rate limiter / metrics / circuit-breaker registry, plus the single
    /// `PoolTask` value that was handed in.
    /// **Exits:** when mediation finishes (success, failure, or callback
    /// fired). Self-terminating — there is no shutdown channel; the task
    /// is short-lived (one message).
    /// **Tracked by:** `self.tracker` (a `tokio_util::task::TaskTracker`).
    /// `wait_drained()` awaits every task the tracker knows about, so this
    /// task is included in that wait even though nothing explicitly joins
    /// it. The pool also tracks in-flight work via the `active_workers`
    /// counter and the semaphore permit lifetime, for stats.
    fn spawn_immediate_task(&self, task: PoolTask) {
        let semaphore = self.semaphore.clone();
        let mediator = self.mediator.clone();
        let queue_size = self.queue_size.clone();
        let active_workers = self.active_workers.clone();
        let rate_limiter = self.rate_limiter.clone();
        let metrics_collector = self.metrics_collector.clone();
        let cb_registry = self.circuit_breaker_registry.clone();
        let flush_registry = self.flush_registry.clone();
        let failed_batch_groups = self.failed_batch_groups.clone();
        let batch_group_message_count = self.batch_group_message_count.clone();

        self.tracker.spawn(async move {
            // Group-flush suppression (ledger A-05/R-52/R-53), checked
            // BEFORE the semaphore/rate limiter so a suppressed group
            // spends neither a concurrency slot nor a rate-limit token —
            // that saving is the whole point of suppression.
            if ack_if_suppressed(&flush_registry, &metrics_collector, &task).await {
                queue_size.fetch_sub(1, Ordering::Relaxed);
                if let Some(ref key) = task.batch_group_key {
                    Self::decrement_and_cleanup_batch_group_static(
                        key,
                        &batch_group_message_count,
                        &failed_batch_groups,
                    );
                }
                return;
            }

            // Acquire a concurrency slot FIRST, then pace on the rate
            // limiter while holding it — see `wait_for_rate_limit_permit`
            // for why this order matters.
            let permit = match semaphore.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    queue_size.fetch_sub(1, Ordering::Relaxed);
                    if let Some(ref key) = task.batch_group_key {
                        Self::decrement_and_cleanup_batch_group_static(
                            key,
                            &batch_group_message_count,
                            &failed_batch_groups,
                        );
                    }
                    task.callback.nack(Some(10)).await;
                    return;
                }
            };

            // Wait for rate limit permit (no timeout — see fn doc).
            Self::wait_for_rate_limit_permit(&rate_limiter, &metrics_collector).await;

            active_workers.fetch_add(1, Ordering::Relaxed);
            queue_size.fetch_sub(1, Ordering::Relaxed);

            // Check per-endpoint circuit breaker (keyed by origin+path,
            // ledger R-12 — query string stripped so per-message query
            // data can't fragment the failure signal).
            let endpoint = breaker_key(&task.message.mediation_target);
            if !cb_registry.allow_request(&endpoint) {
                debug!(message_id = %task.message.id, endpoint = %endpoint, "Endpoint circuit breaker open");
                metrics_collector.record_failure(0);
                task.callback.nack(Some(5)).await;
            } else {
                let start = std::time::Instant::now();
                let outcome = mediator.mediate(&task.message).await;
                let duration_ms = start.elapsed().as_millis() as u64;

                match breaker_effect(&outcome) {
                    Some(true) => cb_registry.record_success(&endpoint),
                    Some(false) => cb_registry.record_failure(&endpoint),
                    None => {}
                }

                // IMMEDIATE mode has no group buffer, so `disposition.group`
                // is never consulted here — DispatchMode at this call site
                // is always `Immediate`.
                let disposition = disposition_of(&outcome, 0, task.message.dispatch_mode);
                apply_metric(&metrics_collector, disposition.metric, duration_ms);

                match disposition.action {
                    BrokerAction::Ack => {
                        maybe_flush_group(&flush_registry, &task.message, &outcome);
                        task.callback.ack().await;
                    }
                    BrokerAction::Release | BrokerAction::Retry => {
                        task.callback.nack(disposition.retry_after_secs).await;
                    }
                }
            }

            if let Some(ref key) = task.batch_group_key {
                Self::decrement_and_cleanup_batch_group_static(
                    key,
                    &batch_group_message_count,
                    &failed_batch_groups,
                );
            }

            active_workers.fetch_sub(1, Ordering::Relaxed);
            drop(permit);
        });
    }

    /// Spawn a task that drains all queued messages for a group, then exits.
    ///
    /// **Owns:** the group's `MessageGroupHandler` (via `group_handlers`)
    /// and Arc clones of the pool's shared state (semaphore, mediator,
    /// counters, rate limiter, metrics, circuit-breaker registry,
    /// failed-batch tracking).
    /// **Exits:** when the group's queue drains to empty (the handler's
    /// `processing` flag is cleared and the loop breaks). Self-terminating
    /// — one drain task per active group, recreated by the next submit
    /// that finds the queue idle.
    /// **Tracked by:** `self.tracker` (a `tokio_util::task::TaskTracker`).
    /// `wait_drained()` awaits every task the tracker knows about, so this
    /// task is included in that wait. The `processing` flag in the handler
    /// remains the "is a drain task running" signal used by `submit()`; the
    /// Drop guard at the top of the spawned body resets that flag even on
    /// panic.
    fn spawn_drain_task(&self, group_id: Arc<str>) {
        let pool_code: Arc<str> = Arc::from(self.config.code.as_str());
        let semaphore = self.semaphore.clone();
        let mediator = self.mediator.clone();
        let queue_size = self.queue_size.clone();
        let active_workers = self.active_workers.clone();
        let failed_batch_groups = self.failed_batch_groups.clone();
        let batch_group_message_count = self.batch_group_message_count.clone();
        let rate_limiter = self.rate_limiter.clone();
        let group_handlers = self.group_handlers.clone();
        let metrics_collector = self.metrics_collector.clone();
        let cb_registry = self.circuit_breaker_registry.clone();
        let flush_registry = self.flush_registry.clone();

        self.tracker.spawn(async move {
            debug!(group_id = %group_id, pool_code = %pool_code, "Group drain task started");

            // Safety guard: if this task panics or exits via an early break,
            // (a) reset the `processing` flag so a future submit() can spawn
            //     a fresh drain task,
            // (b) drain remaining tasks from the VecDeque — dropping them is
            //     the trigger for `QueueMessageCallback::drop` to clear
            //     `in_pipeline` and fire fallback nacks, releasing SQS
            //     redelivery for those messages,
            // (c) decrement active_workers if a permit was held.
            //
            // Without (b), abandoned tasks would sit in the VecDeque
            // indefinitely (the handler is only freed when its queue is
            // empty AND `processing == false`), and SQS redeliveries would
            // be silently swallowed by the manager's duplicate filter.
            struct PanicGuard {
                group_handlers: Arc<DashMap<Arc<str>, parking_lot::Mutex<MessageGroupHandler>>>,
                group_id: Arc<str>,
                active_workers: Arc<AtomicU32>,
                /// Whether a semaphore permit was held when panic occurred
                holding_permit: bool,
                active: bool,
            }
            impl Drop for PanicGuard {
                fn drop(&mut self) {
                    if !self.active {
                        return;
                    }
                    let mut abandoned = 0usize;
                    if let Some(entry) = self.group_handlers.get(&self.group_id) {
                        let mut handler = entry.lock();
                        // Drain queued tasks; their callbacks' Drop impl
                        // does the cleanup. We can't await here so we just
                        // drop them and let the callback's Drop spawn the
                        // fallback nack on the runtime.
                        while handler.dequeue().is_some() {
                            abandoned += 1;
                        }
                        if handler.processing {
                            handler.processing = false;
                        }
                    }
                    if abandoned > 0 {
                        error!(
                            group_id = %self.group_id,
                            abandoned = abandoned,
                            "Drain task exited abnormally — drained queued tasks; their callbacks' Drop will release SQS redelivery"
                        );
                    } else {
                        error!(group_id = %self.group_id, "Drain task exited abnormally — reset processing flag");
                    }
                    if self.holding_permit {
                        self.active_workers.fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
            let mut panic_guard = PanicGuard {
                group_handlers: group_handlers.clone(),
                group_id: group_id.clone(),
                active_workers: active_workers.clone(),
                holding_permit: false,
                active: true,
            };

            loop {
                // Dequeue next task (lock is held only for the dequeue, dropped before any await)
                let dequeue_result = {
                    let handler_entry = group_handlers.get(&group_id);
                    match handler_entry {
                        Some(entry) => {
                            let mut handler = entry.lock();
                            match handler.dequeue() {
                                Some(task) => Some(task),
                                None => {
                                    handler.processing = false;
                                    None
                                }
                            }
                        }
                        None => None,
                    }
                }; // Lock dropped here

                // Check for failed batch+group OUTSIDE the lock
                let task = match dequeue_result {
                    Some(task) => {
                        if let Some(ref key) = task.batch_group_key {
                            if failed_batch_groups.contains(key) {
                                queue_size.fetch_sub(1, Ordering::Relaxed);
                                Self::decrement_and_cleanup_batch_group_static(
                                    key,
                                    &batch_group_message_count,
                                    &failed_batch_groups,
                                );
                                task.callback.nack(Some(10)).await;
                                continue;
                            }
                        }
                        Some(task)
                    }
                    None => None,
                };

                let task = match task {
                    Some(t) => t,
                    None => {
                        // Clean up the empty handler from the map — but the
                        // check ("is it still empty and idle?") and the
                        // removal must be a single atomic map operation.
                        // A separate get() + drop() + remove() opens a
                        // window between the drop and the remove where a
                        // concurrent submit() can find the handler via
                        // entry().or_insert_with, enqueue a task, set
                        // `processing = true`, and spawn a new drain task —
                        // only for this remove() to then yank the handler
                        // (with the freshly queued task inside) out from
                        // under it. The new drain task's next get() then
                        // sees `None` and exits immediately, and the
                        // abandoned `PoolTask`'s callback fires a spurious
                        // fallback nack on Drop. `remove_if` holds the
                        // shard lock across the predicate and the removal,
                        // closing that window.
                        group_handlers.remove_if(&group_id, |_, handler_mutex| {
                            let handler = handler_mutex.lock();
                            handler.is_empty() && !handler.processing
                        });
                        panic_guard.active = false; // Normal exit, don't trigger guard
                        debug!(group_id = %group_id, pool_code = %pool_code, "Group drain task exited");
                        break;
                    }
                };

                // Decrement queue size
                queue_size.fetch_sub(1, Ordering::Relaxed);

                // Group-flush suppression (ledger A-05/R-52/R-53), checked
                // BEFORE the semaphore/rate limiter so a suppressed group
                // spends neither a concurrency slot nor a rate-limit token.
                if ack_if_suppressed(&flush_registry, &metrics_collector, &task).await {
                    if let Some(ref key) = task.batch_group_key {
                        Self::decrement_and_cleanup_batch_group_static(
                            key,
                            &batch_group_message_count,
                            &failed_batch_groups,
                        );
                    }
                    continue;
                }

                // Acquire a concurrency slot FIRST, then pace on the rate
                // limiter while holding it — see `wait_for_rate_limit_permit`
                // for why this order matters.
                let permit = match semaphore.acquire().await {
                    Ok(p) => p,
                    Err(_) => {
                        error!("Semaphore closed");
                        if let Some(ref key) = task.batch_group_key {
                            Self::decrement_and_cleanup_batch_group_static(
                                key,
                                &batch_group_message_count,
                                &failed_batch_groups,
                            );
                        }
                        task.callback.nack(Some(10)).await;
                        // Leave panic_guard.active = true: the queue may
                        // still hold tasks, and the guard will drain them
                        // (their callbacks' Drop fires the fallback nack)
                        // and reset the processing flag so a future submit
                        // can spawn a new drain task.
                        break;
                    }
                };

                // Wait for rate limit permit (no timeout — see fn doc).
                Self::wait_for_rate_limit_permit(&rate_limiter, &metrics_collector).await;

                active_workers.fetch_add(1, Ordering::Relaxed);
                panic_guard.holding_permit = true;

                // Check per-endpoint circuit breaker before attempting
                // mediation (keyed by origin+path, ledger R-12).
                let endpoint = breaker_key(&task.message.mediation_target);
                if !cb_registry.allow_request(&endpoint) {
                    debug!(
                        message_id = %task.message.id,
                        endpoint = %endpoint,
                        "Endpoint circuit breaker open — NACKing for retry"
                    );
                    metrics_collector.record_failure(0);

                    if let Some(ref key) = task.batch_group_key {
                        failed_batch_groups.insert(key.clone());
                    }

                    task.callback.nack(Some(5)).await;
                } else {
                    // Process the message
                    let start = std::time::Instant::now();
                    let outcome = mediator.mediate(&task.message).await;
                    let duration_ms = start.elapsed().as_millis() as u64;

                    match breaker_effect(&outcome) {
                        Some(true) => cb_registry.record_success(&endpoint),
                        Some(false) => cb_registry.record_failure(&endpoint),
                        None => {}
                    }

                    let disposition =
                        disposition_of(&outcome, 0, task.message.dispatch_mode);
                    apply_metric(&metrics_collector, disposition.metric, duration_ms);

                    // GroupEffect::Block (BLOCK_ON_ERROR's terminally-failed
                    // head) and GroupEffect::Release (an unreachable target,
                    // under every mode) both cascade the same way today:
                    // mark the batch+group failed so every message still
                    // buffered behind this one is NACKed as it's dequeued,
                    // never mediated — see the module's disposition_of doc
                    // for why this isn't the ACK-the-siblings branch.
                    match disposition.group {
                        GroupEffect::Continue => {}
                        GroupEffect::Block | GroupEffect::Release => {
                            if let Some(ref key) = task.batch_group_key {
                                let was_new = failed_batch_groups.insert(key.clone());
                                if was_new {
                                    warn!(
                                        batch_group = %key,
                                        group_effect = ?disposition.group,
                                        "Batch+group marked as failed - remaining messages will be NACKed"
                                    );
                                }
                            }
                        }
                    }

                    match disposition.action {
                        BrokerAction::Ack => {
                            if outcome.result == MediationResult::Success {
                                debug!(
                                    message_id = %task.message.id,
                                    duration_ms = duration_ms,
                                    "Message processed successfully"
                                );
                                maybe_flush_group(&flush_registry, &task.message, &outcome);
                            } else {
                                warn!(
                                    message_id = %task.message.id,
                                    error = ?outcome.error_message,
                                    "Permanent error, ACKing to prevent retry"
                                );
                            }
                            task.callback.ack().await;
                        }
                        BrokerAction::Release | BrokerAction::Retry => {
                            warn!(
                                message_id = %task.message.id,
                                error = ?outcome.error_message,
                                retry_after = ?disposition.retry_after_secs,
                                "NACKing for retry"
                            );
                            task.callback.nack(disposition.retry_after_secs).await;
                        }
                    }
                }

                // Decrement batch+group count and cleanup if done
                if let Some(ref key) = task.batch_group_key {
                    Self::decrement_and_cleanup_batch_group_static(
                        key,
                        &batch_group_message_count,
                        &failed_batch_groups,
                    );
                }

                // Cleanup
                active_workers.fetch_sub(1, Ordering::Relaxed);
                panic_guard.holding_permit = false;
                drop(permit);
            }
        });
    }

    /// Decrement batch+group message count and cleanup tracking maps when count reaches zero.
    /// Instance version for use in submit().
    fn decrement_and_cleanup_batch_group(&self, batch_group_key: &BatchGroupKey) {
        Self::decrement_and_cleanup_batch_group_static(
            batch_group_key,
            &self.batch_group_message_count,
            &self.failed_batch_groups,
        );
    }

    /// Decrement batch+group message count and cleanup tracking maps when count reaches zero.
    /// Static version for use in drain tasks.
    fn decrement_and_cleanup_batch_group_static(
        batch_group_key: &BatchGroupKey,
        batch_group_message_count: &DashMap<BatchGroupKey, AtomicU32>,
        failed_batch_groups: &DashSet<BatchGroupKey>,
    ) {
        let should_cleanup = if let Some(counter) = batch_group_message_count.get(batch_group_key) {
            let remaining = counter.fetch_sub(1, Ordering::Relaxed).saturating_sub(1);
            debug!(batch_group = %batch_group_key, remaining = remaining, "Batch+group count decremented");
            remaining == 0
        } else {
            false
        };

        if should_cleanup {
            batch_group_message_count.remove(batch_group_key);
            failed_batch_groups.remove(batch_group_key);
            debug!(batch_group = %batch_group_key, "Batch+group fully processed, cleaned up");
        }
    }

    /// Check available capacity
    pub fn available_capacity(&self) -> usize {
        let capacity = std::cmp::max(
            self.config.concurrency * QUEUE_CAPACITY_MULTIPLIER,
            MIN_QUEUE_CAPACITY,
        ) as usize;
        let used = self.queue_size.load(Ordering::Relaxed) as usize;
        capacity.saturating_sub(used)
    }

    /// Check if rate limited
    pub fn is_rate_limited(&self) -> bool {
        self.rate_limiter
            .load()
            .as_ref()
            .map(|s| s.limiter.check().is_err())
            .unwrap_or(false)
    }

    /// Wait for a rate-limit permit using governor's async API (zero CPU
    /// while waiting). No timeout: the rate limiter is internal pacing and
    /// NACKing on timeout was strictly worse than waiting — bouncing a
    /// message back to SQS only to re-arrive at the same wait creates
    /// churn without changing the achievable throughput. Capacity backpressure
    /// is enforced upstream at `submit()` (bounded queue, NACK on overflow).
    ///
    /// **Ordering:** callers acquire the concurrency semaphore *before*
    /// calling this, and hold the permit while pacing. Governor consumes a
    /// token the moment `until_ready()` resolves, so pacing first and then
    /// queueing on the semaphore would spend tokens while the message sits
    /// waiting for a slot — under saturation the achieved rate lags the
    /// configured rpm, and when slots free up several token-holders fire
    /// at once, bursting above the limit. Holding a slot while pacing costs
    /// nothing: the rate limit is the ceiling either way.
    ///
    /// Within an ordered message group, messages drain serially anyway, so
    /// waiting here doesn't block anything that wasn't already going to
    /// wait. Across groups, each drain task has its own future, so one
    /// waiter doesn't block other groups.
    async fn wait_for_rate_limit_permit(
        rate_limiter: &SharedRateLimiter,
        metrics_collector: &Arc<PoolMetricsCollector>,
    ) {
        // Lock-free snapshot. `load_full` clones the inner Arc; the
        // returned handle is independent of any subsequent `store` so a
        // hot-swap during `.until_ready().await` does not affect this
        // call (the next acquire picks up the new limiter).
        let snapshot = rate_limiter.load_full();
        let state = match snapshot {
            None => return,
            Some(s) => s,
        };

        // Fast path: permit available immediately.
        if state.limiter.check().is_ok() {
            return;
        }

        // Slow path: wait for permit (no timeout).
        metrics_collector.record_rate_limited();
        debug!("Rate limited — waiting for permit");
        state.limiter.until_ready().await;
    }

    /// Drain the pool: stop accepting new work and close the task tracker
    /// so that [`ProcessPool::wait_drained`] can resolve.
    ///
    /// This does **not** wait for in-flight work to finish — it only flips
    /// `running` to `false` and calls `TaskTracker::close()`, both
    /// non-blocking. `TaskTracker::spawn` still works (and still tracks)
    /// after `close()` — close only arms `wait()` — so a drain task already
    /// mid-loop for an ordered group keeps dequeuing until its queue is
    /// empty; new submissions are rejected via the `running` flag.
    /// Callers that need to block until every tracked task has exited
    /// should await [`ProcessPool::wait_drained`] afterwards. Kept
    /// non-blocking deliberately: `QueueManager::reload_config` calls this
    /// while holding a lock, and a blocking wait here would stall it.
    pub async fn drain(&self) {
        info!(pool_code = %self.config.code, "Draining pool");
        self.running.store(false, Ordering::SeqCst);
        self.tracker.close();
    }

    /// Non-blocking snapshot of whether every tracked worker/drain task has
    /// exited. Backed by `TaskTracker::is_empty()` rather than the
    /// `queue_size`/`active_workers` counters — those stay as-is for stats,
    /// but the tracker is the source of truth for "has every spawned task
    /// actually returned", since it also accounts for tasks that are
    /// mid-teardown (e.g. running their final callback) after decrementing
    /// those counters.
    pub fn is_fully_drained(&self) -> bool {
        self.tracker.is_empty()
    }

    /// Wait for every worker/drain task spawned by this pool to finish.
    ///
    /// Closes the tracker (defensive — `TaskTracker::wait` never resolves on
    /// an un-closed tracker, and this method should work correctly even if
    /// called without a preceding `drain()`/`shutdown()`) and then awaits
    /// `TaskTracker::wait()`, which resolves once every task the tracker
    /// has ever seen has completed.
    ///
    /// This does **not** itself stop the pool from accepting new work —
    /// closing the tracker doesn't touch the `running` flag, so a submit
    /// that lands after `wait()` observes "empty" still runs (untracked by
    /// this wait). Callers pair this with [`ProcessPool::drain`] (or
    /// [`ProcessPool::shutdown`]) when they want "stop accepting work, then
    /// wait for what's already running to finish".
    pub async fn wait_drained(&self) {
        self.tracker.close();
        self.tracker.wait().await;
    }

    /// Number of worker/drain tasks the tracker currently considers
    /// in-flight (spawned but not yet finished). Useful for stats/tests.
    pub fn tracked_tasks(&self) -> usize {
        self.tracker.len()
    }

    /// Shut down the pool: stop accepting new work and close the task
    /// tracker. Same non-blocking semantics as [`ProcessPool::drain`] — see
    /// its doc comment. Distinct method kept for call-site clarity (drain
    /// vs. full shutdown) even though the bodies are currently identical;
    /// callers needing to block until tasks finish should follow this with
    /// [`ProcessPool::wait_drained`].
    pub async fn shutdown(&self) {
        info!(pool_code = %self.config.code, "Shutting down pool");
        self.running.store(false, Ordering::SeqCst);
        self.tracker.close();
    }

    /// Release every group's buffered remainder back to the broker (ledger
    /// R-49): stop admitting new work (the same `running` flag
    /// `drain`/`shutdown` use), then NACK every not-yet-started task still
    /// queued behind an in-flight message in each ordered group's handler.
    ///
    /// In-flight deliveries — already popped off a group's buffer and
    /// inside a drain/immediate task's `mediator.mediate()` call — are
    /// left completely alone; this touches only what's still buffered and
    /// waiting.
    ///
    /// This is the mechanical primitive R-49 asks for ("finish what's in
    /// the air, release the rest of the buffer") — not the full shutdown
    /// sequence. R-49's own rationale: draining a deep buffer against a
    /// slow target could take arbitrarily long, and the orchestrator's
    /// SIGTERM→SIGKILL window would sever in-flight deliveries mid-call
    /// anyway, so the broker holding the remainder (rather than this
    /// process trying to work through it) is the safe place for it.
    /// Sequencing this with the drain budget — finish what's in flight,
    /// THEN call this once the budget expires or at hard shutdown — is the
    /// manager lane's job; this method only guarantees every
    /// buffered-but-unstarted message gets a NACK (no fixed delay — the
    /// broker's own redelivery timing applies) rather than being drained
    /// to completion or silently abandoned.
    ///
    /// Returns how many buffered messages were released. Safe to call more
    /// than once — later calls find every group buffer already empty and
    /// are cheap no-ops. Does NOT close the task tracker or affect
    /// `wait_drained()`/`is_fully_drained()` — pair with
    /// [`ProcessPool::drain`] or [`ProcessPool::shutdown`] for that half.
    pub async fn release_remainder(&self) -> usize {
        self.running.store(false, Ordering::SeqCst);

        let group_ids: Vec<Arc<str>> = self
            .group_handlers
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        let mut released = 0usize;
        for group_id in group_ids {
            let drained: Vec<PoolTask> = match self.group_handlers.get(&group_id) {
                Some(entry) => {
                    let mut handler = entry.lock();
                    let mut tasks = Vec::new();
                    while let Some(task) = handler.dequeue() {
                        tasks.push(task);
                    }
                    tasks
                }
                None => Vec::new(),
            };

            for task in drained {
                released += 1;
                self.queue_size.fetch_sub(1, Ordering::Relaxed);
                if let Some(ref key) = task.batch_group_key {
                    self.decrement_and_cleanup_batch_group(key);
                }
                task.callback.nack(None).await;
            }
        }

        if released > 0 {
            info!(
                pool_code = %self.config.code,
                released,
                "Released buffered group remainder to broker"
            );
        }
        released
    }

    /// Get pool statistics
    pub fn get_stats(&self) -> PoolStats {
        let current_concurrency = self.concurrency.load(Ordering::SeqCst);
        PoolStats {
            pool_code: self.config.code.clone(),
            concurrency: current_concurrency,
            active_workers: self.active_workers.load(Ordering::Relaxed),
            queue_size: self.queue_size.load(Ordering::Relaxed),
            queue_capacity: std::cmp::max(
                current_concurrency * QUEUE_CAPACITY_MULTIPLIER,
                MIN_QUEUE_CAPACITY,
            ),
            message_group_count: self.group_handlers.len() as u32,
            rate_limit_per_minute: self.rate_limit_per_minute(),
            is_rate_limited: self.is_rate_limited(),
            metrics: Some(self.metrics_collector.get_metrics()),
        }
    }

    /// Get enhanced metrics for this pool
    pub fn get_enhanced_metrics(&self) -> EnhancedPoolMetrics {
        self.metrics_collector.get_metrics()
    }

    /// Reset metrics (useful for testing)
    pub fn reset_metrics(&self) {
        self.metrics_collector.reset();
    }

    /// Get the pool code
    pub fn code(&self) -> &str {
        &self.config.code
    }

    /// Get the circuit breaker registry (for monitoring APIs)
    pub fn circuit_breaker_registry(
        &self,
    ) -> &Arc<crate::circuit_breaker_registry::CircuitBreakerRegistry> {
        &self.circuit_breaker_registry
    }

    /// Get the group-flush suppression registry (ledger R-52: for
    /// monitoring/operator APIs — listing active suppressions and clearing
    /// one early. Wiring that into an actual HTTP endpoint is a later
    /// lane's work; this exposes the primitive).
    pub fn group_flush_registry(&self) -> &Arc<GroupFlushRegistry> {
        &self.flush_registry
    }

    /// Get current concurrency setting
    pub fn concurrency(&self) -> u32 {
        self.concurrency.load(Ordering::SeqCst)
    }

    /// Get current rate limit setting
    pub fn rate_limit_per_minute(&self) -> Option<u32> {
        self.rate_limiter.load().as_ref().map(|s| s.rpm)
    }

    /// Get current queue size
    pub fn queue_size(&self) -> u32 {
        self.queue_size.load(Ordering::Relaxed)
    }

    /// Get current active worker count
    pub fn active_workers(&self) -> u32 {
        self.active_workers.load(Ordering::Relaxed)
    }

    /// Update concurrency at runtime
    pub async fn update_concurrency(&self, new_concurrency: u32) -> bool {
        let old_concurrency = self.concurrency.load(Ordering::SeqCst);
        if new_concurrency == old_concurrency {
            return true;
        }

        if new_concurrency == 0 {
            warn!(pool_code = %self.config.code, "Rejecting invalid concurrency limit: 0");
            return false;
        }

        let diff = (new_concurrency as i32) - (old_concurrency as i32);

        if diff > 0 {
            self.semaphore.add_permits(diff as usize);
            self.concurrency.store(new_concurrency, Ordering::SeqCst);
            info!(
                pool_code = %self.config.code,
                old = old_concurrency,
                new = new_concurrency,
                added_permits = diff,
                "Increased pool concurrency"
            );
            true
        } else {
            let permits_to_acquire = (-diff) as usize;
            let timeout = Duration::from_secs(60);

            match tokio::time::timeout(timeout, self.acquire_permits(permits_to_acquire)).await {
                Ok(permits) => {
                    std::mem::forget(permits);
                    self.concurrency.store(new_concurrency, Ordering::SeqCst);
                    info!(
                        pool_code = %self.config.code,
                        old = old_concurrency,
                        new = new_concurrency,
                        acquired_permits = permits_to_acquire,
                        "Decreased pool concurrency"
                    );
                    true
                }
                Err(_) => {
                    warn!(
                        pool_code = %self.config.code,
                        old = old_concurrency,
                        new = new_concurrency,
                        timeout_secs = 60,
                        active_workers = self.active_workers.load(Ordering::Relaxed),
                        "Concurrency decrease timed out waiting for idle slots - retaining current limit"
                    );
                    false
                }
            }
        }
    }

    /// Helper to acquire multiple permits (needed for concurrency decrease)
    async fn acquire_permits(&self, count: usize) -> Vec<tokio::sync::SemaphorePermit<'_>> {
        let mut permits = Vec::with_capacity(count);
        for _ in 0..count {
            permits.push(self.semaphore.acquire().await.expect("semaphore closed"));
        }
        permits
    }

    /// Update rate limit at runtime.
    ///
    /// Atomic swap via `ArcSwapOption::store` — in-flight workers
    /// holding a snapshot from before the swap finish on the old
    /// limiter; the next acquire picks up the new state.
    pub fn update_rate_limit(&self, new_rate_limit: Option<u32>) {
        let old_rate_limit = self.rate_limit_per_minute();

        if old_rate_limit == new_rate_limit {
            return;
        }

        let new_state = new_rate_limit.and_then(RateLimitState::from_rpm);
        self.rate_limiter.store(new_state);

        info!(
            pool_code = %self.config.code,
            old = ?old_rate_limit.map(|r| format!("{}/min", r)).unwrap_or_else(|| "none".to_string()),
            new = ?new_rate_limit.map(|r| format!("{}/min", r)).unwrap_or_else(|| "none".to_string()),
            "Rate limit updated in-place"
        );
    }
}

/// Configuration update that can be applied at runtime
#[derive(Debug, Clone)]
pub struct PoolConfigUpdate {
    /// New concurrency level (if changed)
    pub concurrency: Option<u32>,
    /// New rate limit per minute (None to clear, Some(0) means no limit)
    pub rate_limit_per_minute: Option<Option<u32>>,
}

impl PoolConfigUpdate {
    pub fn new() -> Self {
        Self {
            concurrency: None,
            rate_limit_per_minute: None,
        }
    }

    pub fn with_concurrency(mut self, concurrency: u32) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    pub fn with_rate_limit(mut self, rate_limit: Option<u32>) -> Self {
        self.rate_limit_per_minute = Some(rate_limit);
        self
    }
}

impl Default for PoolConfigUpdate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod disposition_tests {
    //! Pins `disposition_of` per outcome (ledger A-27). Pure/synchronous —
    //! no pool, mediator, or broker needed. Integration-level cascade
    //! behaviour (does BLOCK_ON_ERROR actually NACK the siblings end to
    //! end?) is pinned separately in
    //! `tests/cascade_dispatch_mode_test.rs`.
    use super::*;
    use fc_common::MediationOutcome;

    #[test]
    fn success_acks_and_continues() {
        let d = disposition_of(&MediationOutcome::success(200), 0, DispatchMode::Immediate);
        assert_eq!(d.action, BrokerAction::Ack);
        assert_eq!(d.group, GroupEffect::Continue);
        assert_eq!(d.metric, DispositionMetric::Success);
        assert_eq!(d.retry_after_secs, None);
    }

    #[test]
    fn error_config_acks_and_continues_under_immediate_and_next_on_error() {
        let outcome = MediationOutcome::error_config(400, "bad request".to_string());
        for m in [DispatchMode::Immediate, DispatchMode::NextOnError] {
            let d = disposition_of(&outcome, 0, m);
            assert_eq!(d.action, BrokerAction::Ack, "mode {m:?}");
            assert_eq!(d.group, GroupEffect::Continue, "mode {m:?}");
            assert_eq!(d.metric, DispositionMetric::Failure, "mode {m:?}");
        }
    }

    #[test]
    fn error_config_blocks_group_under_block_on_error() {
        let outcome = MediationOutcome::error_config(500, "rejected".to_string());
        let d = disposition_of(&outcome, 0, DispatchMode::BlockOnError);
        assert_eq!(d.action, BrokerAction::Ack, "the head is still ACKed away");
        assert_eq!(
            d.group,
            GroupEffect::Block,
            "BLOCK_ON_ERROR must stop the group at a terminally failed head"
        );
        assert_eq!(d.metric, DispositionMetric::Failure);
    }

    #[test]
    fn error_process_releases_whole_group_under_every_mode() {
        let outcome = MediationOutcome::error_process(Some(30), "unavailable".to_string());
        for m in [
            DispatchMode::Immediate,
            DispatchMode::NextOnError,
            DispatchMode::BlockOnError,
        ] {
            let d = disposition_of(&outcome, 0, m);
            assert_eq!(d.action, BrokerAction::Release, "mode {m:?}");
            assert_eq!(
                d.group,
                GroupEffect::Release,
                "a retryable head failure releases the whole group under every mode; mode {m:?}"
            );
            assert_eq!(d.metric, DispositionMetric::Transient, "mode {m:?}");
            assert_eq!(d.retry_after_secs, Some(30), "mode {m:?}");
        }
    }

    #[test]
    fn error_connection_releases_whole_group_with_fixed_30s_delay() {
        let outcome = MediationOutcome::error_connection("connection refused".to_string());
        let d = disposition_of(&outcome, 0, DispatchMode::BlockOnError);
        assert_eq!(d.action, BrokerAction::Release);
        assert_eq!(d.group, GroupEffect::Release);
        assert_eq!(d.metric, DispositionMetric::Failure);
        assert_eq!(d.retry_after_secs, Some(30));
    }

    #[test]
    fn rate_limited_releases_without_cascading() {
        let outcome = MediationOutcome::rate_limited(12);
        let d = disposition_of(&outcome, 0, DispatchMode::BlockOnError);
        assert_eq!(d.action, BrokerAction::Release);
        assert_eq!(
            d.group,
            GroupEffect::Continue,
            "429 is breaker-neutral and must not cascade to the rest of the group"
        );
        assert_eq!(d.metric, DispositionMetric::RateLimited);
        assert_eq!(d.retry_after_secs, Some(12));
    }

    #[test]
    fn rate_limited_defaults_delay_to_30s_when_absent() {
        // `rate_limited()` always sets a delay, but disposition_of's own
        // `.unwrap_or(30)` is pinned directly against a hand-built outcome
        // in case that constructor's default ever changes independently.
        let outcome = MediationOutcome {
            result: MediationResult::RateLimited,
            delay_seconds: None,
            status_code: Some(429),
            error_message: None,
            flush_group: false,
            pre_flight: false,
        };
        let d = disposition_of(&outcome, 0, DispatchMode::Immediate);
        assert_eq!(d.retry_after_secs, Some(30));
    }

    #[test]
    fn deferred_releases_without_cascading_and_retains_delay() {
        let outcome = MediationOutcome::deferred(200, Some(15));
        let d = disposition_of(&outcome, 0, DispatchMode::BlockOnError);
        assert_eq!(d.action, BrokerAction::Release);
        assert_eq!(
            d.group,
            GroupEffect::Continue,
            "ack:false is not a failure and must not cascade to the rest of the group"
        );
        assert_eq!(d.metric, DispositionMetric::Transient);
        assert_eq!(d.retry_after_secs, Some(15));
    }

    // ------------------------------------------------------------------
    // breaker_effect (ledger 22b / R-06 / A-11)
    // ------------------------------------------------------------------

    #[test]
    fn breaker_effect_success_and_error_config_are_success() {
        assert_eq!(breaker_effect(&MediationOutcome::success(200)), Some(true));
        assert_eq!(
            breaker_effect(&MediationOutcome::error_config(404, "nf".to_string())),
            Some(true)
        );
    }

    #[test]
    fn breaker_effect_error_process_and_connection_are_failure() {
        assert_eq!(
            breaker_effect(&MediationOutcome::error_process(Some(30), "x".to_string())),
            Some(false)
        );
        assert_eq!(
            breaker_effect(&MediationOutcome::error_connection("x".to_string())),
            Some(false)
        );
    }

    #[test]
    fn breaker_effect_rate_limited_and_deferred_are_neutral() {
        assert_eq!(breaker_effect(&MediationOutcome::rate_limited(30)), None);
        assert_eq!(breaker_effect(&MediationOutcome::deferred(200, Some(0))), None);
    }

    #[test]
    fn breaker_effect_pre_flight_is_neutral_even_though_result_is_error_config() {
        let outcome = MediationOutcome::pre_flight_rejected("no host".to_string());
        assert_eq!(outcome.result, MediationResult::ErrorConfig);
        assert_eq!(
            breaker_effect(&outcome),
            None,
            "a call that never happened is no evidence about the target's health"
        );
    }
}
