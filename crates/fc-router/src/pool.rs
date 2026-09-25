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
use dashmap::DashMap;
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info, warn};

use crate::group_flush::GroupFlushRegistry;
use crate::mediator::Mediator;
use crate::metrics::PoolMetricsCollector;
use crate::Result;
use fc_common::{
    BatchMessage, DispatchMode, EnhancedPoolMetrics, MediationOutcome, MediationResult, Message,
    MessageCallback, PoolConfig, PoolStats,
};

const QUEUE_CAPACITY_MULTIPLIER: u32 = 20; // Java: QUEUE_CAPACITY_MULTIPLIER = 20
const MIN_QUEUE_CAPACITY: u32 = 50; // Java: MIN_QUEUE_CAPACITY = 50

/// Releases one queue slot and signals a capacity-freed [`tokio::sync::Notify`]
/// exactly on the full→not-full crossing (G12,
/// `docs/go-mirror/2026-09-06-go-fix-list.md`).
///
/// Cloned into every worker/drain closure that needs to decrement
/// `queue_size` on completion (`spawn_immediate_task`, `spawn_drain_task`,
/// group releases, `release_remainder()`, the worker guards), so the crossing-detection
/// logic lives in one place instead of being reimplemented at each of the
/// half-dozen decrement sites — and, more importantly, so it is provably
/// the *only* place `queue_size` is ever decremented, closing off the
/// classic bug this pattern exists to prevent: a decrement site that
/// forgets to notify, silently reintroducing the fixed-sleep starvation
/// G12 was written to fix.
///
/// The notify fires only when `queue_size` was *exactly* at `capacity`
/// immediately before this decrement — i.e. only the one release that
/// actually ends a capacity outage signals, not every completion. A busy
/// pool cycling through capacity therefore costs the manager's consumer
/// poll loops one wakeup per outage, not one per message.
#[derive(Clone)]
pub(crate) struct QueueSlotReleaser {
    queue_size: Arc<AtomicU32>,
    capacity: u32,
    notify: Arc<tokio::sync::Notify>,
}

impl QueueSlotReleaser {
    /// Take one queue slot back for a message that stays in the pool: an
    /// in-place retry sitting out its backoff counts as queued, as in Go.
    fn reserve(&self) {
        self.queue_size.fetch_add(1, Ordering::Relaxed);
    }

    fn release(&self) {
        let prev = self.queue_size.fetch_sub(1, Ordering::Relaxed);
        if prev == self.capacity {
            self.notify.notify_waiters();
        }
    }
}

// ============================================================================
// Disposition — ledger A-27, aligned to Go's `pool.go` `DispositionOf`
// ============================================================================
//
// `disposition_of` is the single, pure decision both delivery paths
// (`spawn_immediate_task`, `spawn_drain_task`) make about a mediation
// outcome, so they make the same decision by construction and a test can
// assert it without a pool, a mediator or a broker.
//
// It follows Go (`internal/router/pool.go`, `DispositionOf`) outcome by
// outcome:
//
// - A 429 or an `ack:false` deferral is RETRIED IN PLACE (`BrokerAction::Retry`):
//   the message stays in this process, re-fronted in its group so nothing
//   behind it is delivered first, and is attempted again after a backoff
//   (`retry_delay` / `deferred_delay`). The retry is bounded by
//   `MAX_IN_PIPELINE_ATTEMPTS`; once spent, the message is released to the
//   broker instead, with the backoff as its redelivery delay.
// - A deferral that names a delay, from a broker that holds a nacked message
//   back for that delay, goes straight back to the broker with exactly that
//   delay (owner ruling R1 2026-09-17, Go `d879b23`).
// - An unreachable or unavailable target, or an open breaker, RELEASES the
//   message and every message buffered behind it.
// - A permanent rejection is ACKed away; BLOCK_ON_ERROR then stops the
//   group.
//
// One deliberate difference from Go: under BLOCK_ON_ERROR, Go ACKs the
// untried siblings behind a failed head and reports them to the platform
// (its A-01 settled-message hook). That platform half does not exist in this
// port, so `GroupEffect::Block` here hands the siblings back to the broker
// (NACK) instead of deleting them.

/// Most times a message is retried in place before it is released to the
/// broker instead (Go `maxInPipelineAttempts`). An in-place retry never
/// returns the message, so while it loops the broker's expiry, redelivery
/// count and DLQ cannot act on it; a target answering 429 or `ack:false` for
/// ever would otherwise pin the message and its whole group in memory.
pub const MAX_IN_PIPELINE_ATTEMPTS: u32 = 10;

/// Bounds of the in-place retry backoff for 429 (Go `retryMinDelay` /
/// `retryMaxDelay`).
const RETRY_MIN_DELAY: Duration = Duration::from_millis(100);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(5 * 60);

/// Bounds of the deferred (`ack:false`) backoff (Go `deferredMinDelay` /
/// `deferredMaxDelay`): the target is healthy and answering cheap 200s, so
/// it starts later and caps sooner than the error curve.
const DEFERRED_MIN_DELAY: Duration = Duration::from_secs(5);
const DEFERRED_MAX_DELAY: Duration = Duration::from_secs(60);

/// Nack delay for messages handed back only because the message ahead of
/// them in their group failed — they were never attempted.
const SIBLING_NACK_DELAY_SECS: u32 = 10;

/// The nack delay for the untried siblings of a released head: never
/// shorter than the head's own, so no sibling can become visible before the
/// head it queued behind. With a shorter one the group's order rested on the
/// broker refusing a group's later messages while its head is held back —
/// which SQS FIFO does, but LocalStack's long poll does not, nor a broker
/// without group locks (delivery run 3, `platform-down`: the head of each
/// group was released for 30 s after the platform refused it, its siblings
/// for 10 s, and the siblings were delivered first).
fn sibling_nack_delay(head_delay: Option<u32>) -> Option<u32> {
    Some(head_delay.unwrap_or(0).max(SIBLING_NACK_DELAY_SECS))
}

#[cfg(test)]
mod sibling_delay_tests {
    use super::*;

    #[test]
    fn siblings_are_never_held_back_less_than_their_head() {
        assert_eq!(sibling_nack_delay(None), Some(SIBLING_NACK_DELAY_SECS));
        assert_eq!(sibling_nack_delay(Some(3)), Some(SIBLING_NACK_DELAY_SECS));
        assert_eq!(sibling_nack_delay(Some(30)), Some(30));
        assert_eq!(sibling_nack_delay(Some(3600)), Some(3600));
    }
}

/// Exponential backoff: `min << attempts` (shift capped at 12), floored at
/// the delay the target asked for, capped at `max` (Go `backoffDelay`).
fn backoff_delay(attempts: u32, floor_secs: u32, min: Duration, max: Duration) -> Duration {
    let shifted = min.saturating_mul(1u32 << attempts.min(12));
    shifted.max(Duration::from_secs(floor_secs as u64)).min(max)
}

/// In-place retry backoff for a 429: 100ms doubling, `Retry-After` as the
/// floor, 5-minute cap (Go `retryDelay`).
pub fn retry_delay(attempts: u32, floor_secs: u32) -> Duration {
    backoff_delay(attempts, floor_secs, RETRY_MIN_DELAY, RETRY_MAX_DELAY)
}

/// In-place retry backoff for an `ack:false` deferral: 5s doubling, the
/// target's `delaySeconds` as the floor, 60s cap (Go `deferredDelay`).
pub fn deferred_delay(attempts: u32, floor_secs: u32) -> Duration {
    backoff_delay(attempts, floor_secs, DEFERRED_MIN_DELAY, DEFERRED_MAX_DELAY)
}

/// What a [`Disposition`] does to a message at the broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerAction {
    /// Ack the message — delivered, or a permanent rejection that retrying
    /// cannot fix.
    Ack,
    /// Keep the message and attempt it again here after
    /// [`Disposition::retry_after`], without touching the broker. In an
    /// ordered group it is re-fronted, so nothing behind it overtakes it.
    Retry,
    /// Nack the message back to the broker for redelivery — the target is
    /// unreachable or unavailable, the breaker is open, the target named a
    /// delay the broker can hold, or the in-place retry budget is spent.
    Release,
}

/// What a [`Disposition`] means for the rest of an ordered message group.
/// Meaningless for IMMEDIATE dispatch, which has no group buffer to affect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupEffect {
    /// The drainer moves on: to the next buffered message after an ACK, or
    /// back to the same (re-fronted) message after a retry.
    Continue,
    /// BLOCK_ON_ERROR's defining behaviour: the head failed terminally, so
    /// every message buffered behind it is handed back to the broker rather
    /// than delivered past the failure. Only produced when
    /// `mode == DispatchMode::BlockOnError`.
    Block,
    /// This message AND everything buffered behind it go back to the
    /// broker, under every dispatch mode, so the group's order survives the
    /// redelivery.
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

/// The pure verdict for a mediation outcome: what happens to THIS message
/// at the broker ([`Self::action`]), what that means for the rest of an
/// ordered group ([`Self::group`]), the metric to record, and the backoff
/// before a retry or the delay to put on a release.
///
/// Produced by [`disposition_of`], which is pure — no I/O, no metrics
/// calls, no ack/nack calls. Every side effect stays at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disposition {
    pub action: BrokerAction,
    pub group: GroupEffect,
    pub metric: DispositionMetric,
    /// For `Retry`: how long to wait before the next attempt. For
    /// `Release`: the redelivery delay to ask the broker for (zero = none).
    /// Unused for `Ack`.
    pub retry_after: Duration,
    /// `true` when this `Release` exists only because the in-place retry
    /// budget ([`MAX_IN_PIPELINE_ATTEMPTS`]) is spent.
    pub budget_exhausted: bool,
}

impl Disposition {
    /// [`Self::retry_after`] as a broker nack delay: whole seconds, at least
    /// one, or `None` when there is nothing to wait out (Go `nackDelay`).
    pub fn nack_delay_secs(&self) -> Option<u32> {
        if self.retry_after.is_zero() {
            return None;
        }
        let secs = u32::try_from(self.retry_after.as_secs()).unwrap_or(u32::MAX);
        Some(secs.max(1))
    }
}

/// Retry in place within the budget; once it is spent, release instead with
/// the backoff as the broker's redelivery delay (Go `retryOrRelease`).
fn retry_or_release(attempts: u32, delay: Duration, metric: DispositionMetric) -> Disposition {
    if attempts + 1 >= MAX_IN_PIPELINE_ATTEMPTS {
        return Disposition {
            action: BrokerAction::Release,
            group: GroupEffect::Release,
            metric,
            retry_after: delay,
            budget_exhausted: true,
        };
    }
    Disposition {
        action: BrokerAction::Retry,
        group: GroupEffect::Continue,
        metric,
        retry_after: delay,
        budget_exhausted: false,
    }
}

/// Pure mapping from a mediation outcome to its [`Disposition`] (Go
/// `DispositionOf`).
///
/// - `attempts`: how many times this message has already been attempted in
///   place (0 on its first delivery). Drives the backoff and the
///   [`MAX_IN_PIPELINE_ATTEMPTS`] budget.
/// - `mode`: only BLOCK_ON_ERROR changes anything — a permanent rejection
///   then blocks the group.
/// - `honours_delayed_return`: whether the message's source broker holds a
///   nacked message back for the requested delay
///   ([`MessageCallback::honours_delayed_return`]). Only consulted for a
///   deferral that names a delay.
pub fn disposition_of(
    outcome: &MediationOutcome,
    attempts: u32,
    mode: DispatchMode,
    honours_delayed_return: bool,
) -> Disposition {
    let release = |metric, retry_after| Disposition {
        action: BrokerAction::Release,
        group: GroupEffect::Release,
        metric,
        retry_after,
        budget_exhausted: false,
    };
    let requested = outcome.delay_seconds.unwrap_or(0);

    match outcome.result {
        MediationResult::Success => Disposition {
            action: BrokerAction::Ack,
            group: GroupEffect::Continue,
            metric: DispositionMetric::Success,
            retry_after: Duration::ZERO,
            budget_exhausted: false,
        },

        // Permanent ACK-drop: a 4xx, an unfollowed 3xx, a pre-flight
        // rejection, or (R-57) a 5xx the mediator classified as "the app
        // answered". BLOCK_ON_ERROR stops the group rather than deliver
        // successors past the failure; every other mode moves on.
        MediationResult::ErrorConfig => Disposition {
            action: BrokerAction::Ack,
            group: if mode == DispatchMode::BlockOnError {
                GroupEffect::Block
            } else {
                GroupEffect::Continue
            },
            metric: DispositionMetric::Failure,
            retry_after: Duration::ZERO,
            budget_exhausted: false,
        },

        // 502/503/504 after the mediator's own retry burst: the target is
        // unavailable, nothing is wrong with the message. Hand back the
        // whole group with the outcome's delay.
        MediationResult::ErrorProcess => release(
            DispositionMetric::Transient,
            Duration::from_secs(requested as u64),
        ),

        // Transport failure / unreachable host / timeout.
        MediationResult::ErrorConnection => release(
            DispositionMetric::Failure,
            Duration::from_secs(requested as u64),
        ),

        // 429: a healthy target asking us to slow down. Retry in place,
        // honouring Retry-After as the backoff floor.
        MediationResult::RateLimited => retry_or_release(
            attempts,
            retry_delay(attempts, requested),
            DispositionMetric::RateLimited,
        ),

        // 2xx + ack:false. R1: a named delay the broker can hold goes back
        // to the broker on its first occurrence with exactly that delay (no
        // curve, no cap), taking the group with it. Otherwise retry in place
        // on the deferred curve, the named delay as its floor (H1: never a
        // 0s release).
        MediationResult::Deferred => {
            if requested > 0 && honours_delayed_return {
                release(
                    DispositionMetric::Transient,
                    Duration::from_secs(requested as u64),
                )
            } else {
                retry_or_release(
                    attempts,
                    deferred_delay(attempts, requested),
                    DispositionMetric::Transient,
                )
            }
        }

        // No call was made: the breaker already says the target is down.
        // Released like a transport failure — retrying in place would pin
        // the group in memory for exactly the outage the release is for.
        // No metric: nothing was attempted.
        MediationResult::CircuitOpen => release(
            DispositionMetric::None,
            Duration::from_secs(requested as u64),
        ),
    }
}

/// Whether a mediation outcome counts toward the circuit breaker, and how.
/// `None` when the call never happened (pre-flight rejection, ledger
/// R-06/A-11 — no evidence about the target's health in either direction;
/// or a `CircuitOpen` rejection — no evidence either, and in any case the
/// breaker that just rejected the call is not itself re-recorded into) or
/// shouldn't move the breaker either way (RateLimited, Deferred — the
/// target is healthy, just throttling/deferring).
///
/// Called from exactly one place now: `HttpMediator::mediate`. Kept in
/// this module (rather than moved into `mediator.rs`) because it is a pure
/// function of `MediationOutcome`/`MediationResult` that `disposition_of`
/// right above it already documents and tests against — duplicating it
/// would risk the two silently drifting apart.
pub(crate) fn breaker_effect(outcome: &MediationOutcome) -> Option<bool> {
    if outcome.pre_flight {
        return None;
    }
    match outcome.result {
        MediationResult::Success | MediationResult::ErrorConfig => Some(true),
        MediationResult::ErrorProcess | MediationResult::ErrorConnection => Some(false),
        MediationResult::RateLimited | MediationResult::Deferred => None,
        // No call was made at all — nothing to credit or blame the
        // endpoint for, and the breaker that rejected this call already
        // recorded the rejection itself (`allow_request`'s own
        // `rejected_calls` counter), not via this function.
        MediationResult::CircuitOpen => None,
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
/// otherwise touch it further (queue-size bookkeeping is still the caller's
/// job, same as any other terminal path).
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
fn maybe_flush_group(
    flush_registry: &GroupFlushRegistry,
    message: &Message,
    outcome: &MediationOutcome,
) {
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

// ============================================================================
// Worker bookkeeping and group release helpers
// ============================================================================

/// What one worker task holds against the pool's shared counters, given
/// back on every exit path — including a panic, where the Drop impl runs
/// during unwinding. Without it a panicking task leaked its queue slot (the
/// pool slowly filled until it NACKed everything), its active-worker count,
/// and a phantom entry in the never-reaped "Mediating" view.
struct WorkerGuard {
    queue_size: QueueSlotReleaser,
    active_workers: Arc<AtomicU32>,
    mediating: Arc<DashMap<u64, MediatingEntry>>,
    /// A queue slot is held for the message (queued, or waiting out a retry).
    slot_held: bool,
    /// The `mediating` key while a delivery is in flight; also means an
    /// active-worker count is held.
    in_flight: Option<u64>,
}

impl WorkerGuard {
    /// A guard for a message that already holds a queue slot, when
    /// `slot_held` is set by the caller.
    fn new(
        queue_size: QueueSlotReleaser,
        active_workers: Arc<AtomicU32>,
        mediating: Arc<DashMap<u64, MediatingEntry>>,
    ) -> Self {
        Self {
            queue_size,
            active_workers,
            mediating,
            slot_held: true,
            in_flight: None,
        }
    }

    fn release_slot(&mut self) {
        if std::mem::take(&mut self.slot_held) {
            self.queue_size.release();
        }
    }

    fn reserve_slot(&mut self) {
        if !self.slot_held {
            self.queue_size.reserve();
            self.slot_held = true;
        }
    }

    /// A delivery starts: count the worker and remember its `mediating` key.
    fn begin(&mut self, mediating_key: u64) {
        self.active_workers.fetch_add(1, Ordering::Relaxed);
        self.in_flight = Some(mediating_key);
    }

    /// The delivery finished.
    fn end(&mut self) {
        if let Some(key) = self.in_flight.take() {
            ProcessPool::end_mediating(&self.mediating, key);
            self.active_workers.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.end();
        self.release_slot();
    }
}

/// A drain task's guard: its [`WorkerGuard`] for the message in hand, plus
/// the group it owns. If the task exits abnormally (panic, or a closed
/// semaphore) it empties the group's buffer — dropping each task fires its
/// callback's fallback nack — gives back one queue slot per abandoned
/// message, and clears `processing` so a later submit can start a fresh
/// drainer.
struct DrainGuard {
    group_handlers: Arc<DashMap<Arc<str>, parking_lot::Mutex<MessageGroupHandler>>>,
    group_id: Arc<str>,
    /// The drainer's message in hand. Its slot is given back at dequeue, so
    /// it starts with none held.
    worker: WorkerGuard,
    active: bool,
}

impl Drop for DrainGuard {
    fn drop(&mut self) {
        self.worker.slot_held = false;
        if !self.active {
            return;
        }
        let abandoned = match self.group_handlers.get(&self.group_id) {
            Some(entry) => {
                let mut handler = entry.lock();
                let abandoned = handler.take_all();
                if handler.processing {
                    handler.set_processing(false);
                }
                abandoned
            }
            None => Vec::new(),
        };
        for _ in &abandoned {
            self.worker.queue_size.release();
        }
        error!(
            group_id = %self.group_id,
            abandoned = abandoned.len(),
            "Drain task exited abnormally — released its queue slots; the abandoned messages' callbacks nack them as they drop"
        );
        drop(abandoned);
    }
}

/// Empty `group`'s buffer, returning what was in it in FIFO order (Go
/// `takeBuffered`). The caller owns the queue slots those messages held.
fn take_buffered(
    group_handlers: &DashMap<Arc<str>, parking_lot::Mutex<MessageGroupHandler>>,
    group_id: &Arc<str>,
) -> Vec<PoolTask> {
    match group_handlers.get(group_id) {
        Some(entry) => entry.lock().take_all(),
        None => Vec::new(),
    }
}

/// Nack every message in `tasks`, giving back the queue slot each held.
async fn nack_all(tasks: Vec<PoolTask>, queue_size: &QueueSlotReleaser, delay: Option<u32>) {
    for task in tasks {
        queue_size.release();
        task.callback.nack(delay).await;
    }
}

/// Hand a whole group back to the broker: the head (first in the buffer)
/// with `head_delay`, everything behind it with the sibling delay.
async fn release_group(
    group_handlers: &DashMap<Arc<str>, parking_lot::Mutex<MessageGroupHandler>>,
    group_id: &Arc<str>,
    queue_size: &QueueSlotReleaser,
    head_delay: Option<u32>,
) {
    let mut buffered = take_buffered(group_handlers, group_id).into_iter();
    if let Some(head) = buffered.next() {
        queue_size.release();
        head.callback.nack(head_delay).await;
    }
    nack_all(
        buffered.collect(),
        queue_size,
        sibling_nack_delay(head_delay),
    )
    .await;
}

/// The two dispositions worth a log line of their own: a deferral (a 2xx
/// the mediator logs nothing for, yet it can hold a message back for as
/// long as the target names) and a spent retry budget.
fn log_disposition(
    pool_code: &str,
    task: &PoolTask,
    outcome: &MediationOutcome,
    disposition: &Disposition,
) {
    if outcome.result == MediationResult::Deferred {
        info!(
            message_id = %task.message.id,
            group = task.message.message_group_id.as_deref().unwrap_or(""),
            pool_code = %pool_code,
            delay_seconds = ?outcome.delay_seconds,
            status = ?outcome.status_code,
            action = ?disposition.action,
            "Target deferred message (ack=false)"
        );
    }
    if disposition.budget_exhausted {
        warn!(
            message_id = %task.message.id,
            pool_code = %pool_code,
            attempts = task.attempts + 1,
            max_attempts = MAX_IN_PIPELINE_ATTEMPTS,
            redelivery_delay_ms = disposition.retry_after.as_millis() as u64,
            "In-pipeline retry budget exhausted; releasing to broker"
        );
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

/// Task submitted to a pool worker
pub struct PoolTask {
    pub message: Message,
    pub receipt_handle: String,
    pub callback: Box<dyn MessageCallback>,
    pub batch_id: Option<Arc<str>>,
    /// How many times this message has already been attempted in place (Go
    /// `QueuedMessage.Attempts`): 0 on first delivery, incremented by each
    /// `BrokerAction::Retry`. Drives the backoff and the
    /// [`MAX_IN_PIPELINE_ATTEMPTS`] budget.
    pub attempts: u32,
    /// The message's SOURCE queue (mirrors `common.InFlightMessage.QueueIdentifier`
    /// in the Go port). A `Message`/mediation target carries no queue
    /// information of its own — this is the one place a `PoolTask` still
    /// remembers which queue it arrived on, purely for the operator
    /// "Mediating" dashboard view (`MediatingEntry::queue`); nothing in the
    /// delivery/ack/nack path reads it (that resolves the source consumer
    /// via `QueueManager`'s own `in_pipeline` map, keyed independently).
    pub queue_identifier: String,
}

/// Lightweight per-message-group handler: the group's buffer and a flag —
/// no tokio task, no channels. A drain task is spawned only when work
/// arrives for an idle group.
///
/// The buffer is a single strict FIFO, as in Go's `groupQueue`. A message
/// group is an ordering contract, so there is no priority lane inside it:
/// letting a "high priority" message jump an earlier one in the same group
/// would defeat in-order delivery. A retried head is put back at the FRONT
/// (`push_front`), so it is the next message attempted.
struct MessageGroupHandler {
    msgs: VecDeque<PoolTask>,
    processing: bool,
    /// When this group was last left with no drainer running (mirrors Go's
    /// `groupQueue.parkedAt`) — `None` while `processing` is `true`, or if
    /// the group has never been idle since creation. Set by
    /// `set_processing(false)`/cleared by `set_processing(true)`, the two
    /// call sites that flip `processing`. A `parked_at` that keeps ageing
    /// while `processing` stays `false` is the operator "blocked groups"
    /// signature: nothing has come back to resume the group.
    parked_at: Option<std::time::Instant>,
}

impl MessageGroupHandler {
    fn new() -> Self {
        Self {
            msgs: VecDeque::new(),
            processing: false,
            parked_at: None,
        }
    }

    fn enqueue(&mut self, task: PoolTask) {
        self.msgs.push_back(task);
    }

    /// Put a retried message back at the head of the group.
    fn enqueue_front(&mut self, task: PoolTask) {
        self.msgs.push_front(task);
    }

    fn dequeue(&mut self) -> Option<PoolTask> {
        self.msgs.pop_front()
    }

    /// Empty the buffer, returning what was in it in FIFO order (Go
    /// `takeBuffered`).
    fn take_all(&mut self) -> Vec<PoolTask> {
        self.msgs.drain(..).collect()
    }

    fn is_empty(&self) -> bool {
        self.msgs.is_empty()
    }

    fn len(&self) -> usize {
        self.msgs.len()
    }

    /// The single mutator of `processing` — every call site that used to
    /// write the field directly goes through this instead, so `parked_at`
    /// can never drift out of sync with it (ledger: the R-04 "blocked
    /// groups" view's `parkedAt`/`working` pair).
    fn set_processing(&mut self, processing: bool) {
        self.processing = processing;
        self.parked_at = if processing {
            None
        } else {
            Some(std::time::Instant::now())
        };
    }
}

/// One message currently inside a pool worker (awaiting a rate-limit token
/// or actively being delivered, inside `mediator.mediate`) — mirrors Go's
/// `MediatingEntry`. Snapshotted for the operator "Mediating" dashboard
/// view: the live, never-reaped set (`ProcessPool::mediating_snapshot`),
/// distinct from the manager's `in_pipeline` dedup tracker.
#[derive(Debug, Clone)]
pub struct MediatingEntry {
    pub message_id: String,
    pub pool_code: String,
    /// FIFO message-group id, empty string for ungrouped — matches Go's
    /// `MediatingEntry.Group`.
    pub group: String,
    /// The message's source queue identifier (see `PoolTask::queue_identifier`'s
    /// doc for why a `PoolTask` carries this at all).
    pub queue: String,
    pub target: String,
    /// How many times this message had already been retried in place when
    /// this attempt started (Go `MediatingEntry.Attempts`). The mediator's
    /// own retry burst inside one `mediate()` call is not counted here.
    pub attempts: u32,
    /// When this message entered the worker (started waiting on the rate
    /// limiter, immediately before `mediator.mediate` is called) — mirrors
    /// Go's `MediatingEntry.MediatedAt`. Monotonic; converted to an
    /// elapsed-ms figure at snapshot time rather than exposed as a wall
    /// clock timestamp (matches `MediatingInfo`'s wire shape, which is
    /// `elapsedTimeMs` only — no absolute-time field).
    pub mediated_at: std::time::Instant,
}

/// One live message group a pool is currently holding — the operator
/// "blocked groups" view (ledger R-04). Mirrors Go's `GroupInfo`; see its
/// doc comment for what "live" means (buffered awaiting a drainer, being
/// drained, or parked with none running — a fully-drained group is deleted
/// from `group_handlers`, so it never shows up here).
#[derive(Debug, Clone)]
pub struct GroupInfo {
    pub group: String,
    pub pool_code: String,
    /// Messages sitting in this group's FIFO right now — not counting one
    /// a drainer currently holds mid-delivery (already popped off the
    /// buffer by `dequeue`).
    pub buffered: usize,
    /// `true` while a drain task owns this group; `false` means buffered
    /// with no drainer running (freshly enqueued, or parked — see
    /// `parked_at`).
    pub working: bool,
    /// When the group was last left with no drainer (`None` while
    /// `working` is `true`, or if it has never been parked). Converted
    /// from the handler's monotonic `Instant` to a wall-clock timestamp at
    /// snapshot time — see `ProcessPool::group_snapshot`'s doc for the
    /// conversion.
    pub parked_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether `GroupFlushRegistry` currently suppresses this group.
    pub suppressed: bool,
    pub suppressed_until: Option<chrono::DateTime<chrono::Utc>>,
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

    /// Capacity-freed signal (G12) — notified exactly on the full→not-full
    /// crossing by every [`QueueSlotReleaser`] built from this pool. A
    /// standalone pool (constructed via `new` outside a
    /// `QueueManager`) gets its own private `Notify` that nothing waits on;
    /// `QueueManager` overwrites it with its one shared gate via
    /// [`Self::with_capacity_notify`] so every pool's crossing wakes the
    /// same consumer poll loops.
    capacity_notify: Arc<tokio::sync::Notify>,

    /// Active workers counter (Arc for sharing across tasks)
    active_workers: Arc<AtomicU32>,

    /// Every message currently inside a worker — mirrors Go's
    /// `Pool.mediating map[uint64]MediatingEntry`. Keyed per WORKER (via
    /// `mediating_seq`), not per message id, for the same reason as Go: the
    /// process-time dedup backstop means two copies of one message id can
    /// briefly sit in two workers, and keying by id would under-report the
    /// count and let the loser's exit delete the owner's entry. Inserted/
    /// removed in the exact same critical section as `active_workers`'
    /// increment/decrement, so `mediating.len() == active_workers.load()`
    /// always holds — the operator "Mediating" dashboard view's count is
    /// meant to match the pool stats' active-workers figure. Never reaped
    /// (unlike the manager's `in_pipeline` tracker), so a long-running
    /// delivery stays listed for its whole duration.
    mediating: Arc<DashMap<u64, MediatingEntry>>,
    /// Monotonic id source for `mediating`'s keys.
    mediating_seq: Arc<AtomicU64>,

    /// Enhanced metrics collector
    metrics_collector: Arc<PoolMetricsCollector>,

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

    /// Cancelled by [`Self::shutdown`] and [`Self::release_remainder`] — the
    /// points where buffered work is handed back to the broker. Wakes every
    /// task sitting out an in-place retry backoff, so it hands its message
    /// back too instead of holding up `wait_drained()`. [`Self::drain`] does
    /// NOT cancel it: a pool being removed keeps working through its buffer,
    /// retries included, as Go's `Drain` does.
    stop: CancellationToken,

    /// Item 3 (router bench rig, 2026-09-07): true from the moment a batch
    /// found this pool without room for it until a later batch finds room
    /// again. `QueueManager::route_batch` gates its "pool at capacity"
    /// WARN log + `WarningService` entry on the `false -> true` transition
    /// this flag captures (via [`Self::note_capacity_full`]) and its
    /// "capacity returned" INFO log on the reverse transition
    /// ([`Self::note_capacity_recovered`]), instead of logging/warning on
    /// every single deferred batch — under sustained saturation (8 NATS
    /// queues sharing one pool) that was 883 WARN lines plus 883
    /// `WarningService` entries in one bench run, one per batch that found
    /// the pool still full rather than one per time it actually became
    /// full.
    capacity_full_warned: AtomicBool,
}

impl ProcessPool {
    /// Construct a pool. `mediator` is expected to already carry the
    /// manager's shared circuit breaker registry (via
    /// `HttpMediator::with_circuit_breakers`, wired by `QueueManager`'s
    /// `MediatorFactory`): breaker admission/recording lives entirely in the
    /// mediator (see `mediator.rs`'s `Mediator::mediate` impl on
    /// `HttpMediator`), so the pool itself neither checks nor records
    /// breaker state.
    pub fn new(config: PoolConfig, mediator: Arc<dyn Mediator>) -> Self {
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
            rate_limiter: Arc::new(ArcSwapOption::new(initial_rate_limit)),
            running: AtomicBool::new(false),
            queue_size: Arc::new(AtomicU32::new(0)),
            capacity_notify: Arc::new(tokio::sync::Notify::new()),
            active_workers: Arc::new(AtomicU32::new(0)),
            mediating: Arc::new(DashMap::new()),
            mediating_seq: Arc::new(AtomicU64::new(0)),
            metrics_collector: Arc::new(PoolMetricsCollector::new()),
            flush_registry: Arc::new(GroupFlushRegistry::new()),
            tracker: TaskTracker::new(),
            stop: CancellationToken::new(),
            capacity_full_warned: AtomicBool::new(false),
        }
    }

    /// Wire this pool's capacity-freed signal into a shared [`tokio::sync::Notify`]
    /// (G12) — see the `capacity_notify` field's doc comment. `QueueManager`
    /// calls this on every pool it creates so all pools wake the same
    /// consumer poll loops; a pool built directly for a standalone test
    /// keeps its own private, never-waited-on `Notify` if this is never
    /// called.
    pub(crate) fn with_capacity_notify(mut self, notify: Arc<tokio::sync::Notify>) -> Self {
        self.capacity_notify = notify;
        self
    }

    /// This pool's queue capacity — `max(concurrency * QUEUE_CAPACITY_MULTIPLIER,
    /// MIN_QUEUE_CAPACITY)`. Factored out of `submit`/`available_capacity`
    /// so [`Self::queue_slot_releaser`] can build a [`QueueSlotReleaser`]
    /// with the same value those two already use, without a third
    /// inline copy of the formula.
    fn capacity(&self) -> u32 {
        std::cmp::max(
            self.config.concurrency * QUEUE_CAPACITY_MULTIPLIER,
            MIN_QUEUE_CAPACITY,
        )
    }

    /// Build a [`QueueSlotReleaser`] bound to this pool's queue-size counter,
    /// capacity, and capacity-freed notify — clone this into a worker/drain
    /// closure instead of cloning `queue_size` directly, so every
    /// decrement site goes through the crossing-detection logic (G12).
    fn queue_slot_releaser(&self) -> QueueSlotReleaser {
        QueueSlotReleaser {
            queue_size: self.queue_size.clone(),
            capacity: self.capacity(),
            notify: self.capacity_notify.clone(),
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
        let capacity = self.capacity();

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

        let task = PoolTask {
            message: batch_msg.message,
            receipt_handle: batch_msg.receipt_handle,
            callback: batch_msg.callback,
            batch_id: batch_msg.batch_id,
            attempts: 0,
            queue_identifier: batch_msg.queue_identifier,
        };

        // IMMEDIATE mode: no ordering needed — spawn a standalone task per message.
        // This avoids the sequential drain bottleneck where a slow HTTP call blocks
        // all other messages in the group.
        //
        // R-13 (ledger): an ordered mode WITHOUT a message group id also takes
        // this path — there is no group to order within, and funnelling all
        // such messages through one shared drain queue would serialise
        // unrelated producers behind each other (Go parity: the ungrouped-
        // ordered branch). Under FC_ROUTER_STRICT_ROUTING the manager ACKs
        // these before they ever reach the pool.
        let group_id: Arc<str> = match task
            .message
            .message_group_id
            .as_deref()
            .filter(|g| !g.is_empty())
        {
            Some(g) if task.message.dispatch_mode.requires_ordering() => Arc::from(g),
            _ => {
                self.spawn_immediate_task(task);
                return Ok(());
            }
        };

        // Ordered mode: enqueue at the back of the group's FIFO and spawn a
        // drain task if the group is idle.
        let should_spawn = {
            let entry = self
                .group_handlers
                .entry(Arc::clone(&group_id))
                .or_insert_with(|| parking_lot::Mutex::new(MessageGroupHandler::new()));
            let mut handler = entry.lock();

            handler.enqueue(task);

            if !handler.processing {
                handler.set_processing(true);
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

    /// Spawn a standalone task for an IMMEDIATE mode message (Go
    /// `runImmediate`): acquire a concurrency slot, rate-limit, mediate, and
    /// act on the [`Disposition`]. A `Retry` keeps the message in this task:
    /// it gives the slot back, waits out the backoff (counted as queued,
    /// as in Go), and goes round again.
    ///
    /// **Owns:** Arc clones of the pool's semaphore / mediator / counters /
    /// rate limiter / metrics, plus the single `PoolTask` handed in.
    /// **Exits:** when the message is acked or nacked — including when
    /// [`Self::shutdown`]/[`Self::release_remainder`] interrupt a retry
    /// backoff, which nacks it.
    /// **Tracked by:** `self.tracker`, so `wait_drained()` includes it.
    /// **Panic safety:** [`WorkerGuard`] gives back the queue slot, the
    /// active-worker count and the `mediating` entry this task held; the
    /// message's callback fires its fallback nack as it drops.
    fn spawn_immediate_task(&self, task: PoolTask) {
        let semaphore = self.semaphore.clone();
        let mediator = self.mediator.clone();
        let queue_size = self.queue_slot_releaser();
        let active_workers = self.active_workers.clone();
        let mediating = self.mediating.clone();
        let mediating_seq = self.mediating_seq.clone();
        let pool_code: Arc<str> = Arc::from(self.config.code.as_str());
        let rate_limiter = self.rate_limiter.clone();
        let metrics_collector = self.metrics_collector.clone();
        let flush_registry = self.flush_registry.clone();
        let stop = self.stop.clone();

        self.tracker.spawn(async move {
            let mut task = task;
            let mut guard = WorkerGuard::new(queue_size, active_workers, mediating);

            loop {
                // Group-flush suppression (ledger A-05/R-52/R-53), checked
                // BEFORE the semaphore/rate limiter so a suppressed group
                // spends neither a concurrency slot nor a rate-limit token —
                // that saving is the whole point of suppression.
                if ack_if_suppressed(&flush_registry, &metrics_collector, &task).await {
                    guard.release_slot();
                    return;
                }

                // Go `InFlightTracker.EnsureTracked`: a different copy of this
                // message now owns the pipeline, so this one is acked, not
                // delivered twice.
                if !task.callback.ensure_tracked() {
                    guard.release_slot();
                    task.callback.ack().await;
                    return;
                }

                // Acquire a concurrency slot FIRST, then pace on the rate
                // limiter while holding it — see `wait_for_rate_limit_permit`
                // for why this order matters.
                let permit = match semaphore.acquire().await {
                    Ok(p) => p,
                    Err(_) => {
                        guard.release_slot();
                        task.callback.nack(Some(10)).await;
                        return;
                    }
                };

                // Wait for rate limit permit (no timeout — see fn doc).
                Self::wait_for_rate_limit_permit(&rate_limiter, &metrics_collector).await;

                let key =
                    Self::begin_mediating(&guard.mediating, &mediating_seq, &pool_code, &task);
                guard.begin(key);
                guard.release_slot();

                // Circuit breaker admission/recording lives entirely inside
                // `mediator.mediate` (see `mediator.rs`); an open breaker
                // comes back as `MediationResult::CircuitOpen`.
                let start = std::time::Instant::now();
                let outcome = mediator.mediate(&task.message).await;
                let duration_ms = start.elapsed().as_millis() as u64;

                let disposition = disposition_of(
                    &outcome,
                    task.attempts,
                    task.message.dispatch_mode,
                    task.callback.honours_delayed_return(),
                );
                apply_metric(&metrics_collector, disposition.metric, duration_ms);
                log_disposition(&pool_code, &task, &outcome, &disposition);
                guard.end();
                drop(permit);

                // IMMEDIATE has no group buffer: `disposition.group` has
                // nothing to act on here.
                match disposition.action {
                    BrokerAction::Ack => {
                        maybe_flush_group(&flush_registry, &task.message, &outcome);
                        task.callback.ack().await;
                        return;
                    }
                    BrokerAction::Release => {
                        task.callback.nack(disposition.nack_delay_secs()).await;
                        return;
                    }
                    BrokerAction::Retry => {
                        task.attempts += 1;
                        // Go `InFlightTracker.MarkRetrying`: the reaper and
                        // stall detector leave a live retry alone.
                        task.callback.mark_retrying();
                        guard.reserve_slot();
                        tokio::select! {
                            biased;
                            _ = stop.cancelled() => {
                                // Handed back rather than held through
                                // shutdown.
                                guard.release_slot();
                                task.callback.nack(disposition.nack_delay_secs()).await;
                                return;
                            }
                            _ = tokio::time::sleep(disposition.retry_after) => {}
                        }
                    }
                }
            }
        });
    }

    /// Spawn a task that drains a group's buffer one message at a time,
    /// then exits (Go `drainGroup`).
    ///
    /// Per message, the [`Disposition`] decides:
    /// - `Ack`: ack it and move on — unless BLOCK_ON_ERROR blocked the
    ///   group, in which case every message buffered behind it is handed
    ///   back to the broker first.
    /// - `Retry`: put it back at the FRONT of the group, wait out the
    ///   backoff, and attempt it again. Nothing behind it is delivered in
    ///   the meantime (H3).
    /// - `Release`: nack it AND take and nack the group's whole buffer, so
    ///   the group comes back from the broker in order (H3, Go
    ///   `releaseGroup`/`takeBuffered`).
    ///
    /// **Owns:** the group's `MessageGroupHandler` (via `group_handlers`)
    /// while `processing` is set, plus Arc clones of the pool's shared state.
    /// **Exits:** when the group's buffer is empty (the handler is removed
    /// from the map), or when the semaphore closes.
    /// **Tracked by:** `self.tracker`, so `wait_drained()` includes it.
    /// **Panic safety:** [`DrainGuard`] empties the buffer (the callbacks'
    /// Drop fires their fallback nacks), gives back one queue slot per
    /// abandoned message, clears `processing`, and releases the
    /// active-worker count and `mediating` entry held for the message in
    /// hand.
    fn spawn_drain_task(&self, group_id: Arc<str>) {
        let pool_code: Arc<str> = Arc::from(self.config.code.as_str());
        let semaphore = self.semaphore.clone();
        let mediator = self.mediator.clone();
        let queue_size = self.queue_slot_releaser();
        let active_workers = self.active_workers.clone();
        let mediating = self.mediating.clone();
        let mediating_seq = self.mediating_seq.clone();
        let rate_limiter = self.rate_limiter.clone();
        let group_handlers = self.group_handlers.clone();
        let metrics_collector = self.metrics_collector.clone();
        let flush_registry = self.flush_registry.clone();
        let stop = self.stop.clone();

        self.tracker.spawn(async move {
            debug!(group_id = %group_id, pool_code = %pool_code, "Group drain task started");

            let mut guard = DrainGuard {
                group_handlers: group_handlers.clone(),
                group_id: group_id.clone(),
                worker: WorkerGuard::new(queue_size.clone(), active_workers, mediating),
                active: true,
            };
            // The drainer gives back each message's slot at dequeue itself.
            guard.worker.slot_held = false;

            loop {
                // Dequeue the head (lock held only for the dequeue).
                let next = match group_handlers.get(&group_id) {
                    Some(entry) => {
                        let mut handler = entry.lock();
                        let next = handler.dequeue();
                        if next.is_none() {
                            handler.set_processing(false);
                        }
                        next
                    }
                    None => None,
                };

                let Some(mut task) = next else {
                    // Remove the empty handler — the "still empty and idle?"
                    // check and the removal must be one atomic map
                    // operation. A separate get() + remove() would let a
                    // concurrent submit() enqueue into the handler and spawn
                    // a new drainer in between, only for this remove() to
                    // yank the handler (and the new task) out from under it.
                    group_handlers.remove_if(&group_id, |_, handler_mutex| {
                        let handler = handler_mutex.lock();
                        handler.is_empty() && !handler.processing
                    });
                    guard.active = false; // Normal exit
                    debug!(group_id = %group_id, pool_code = %pool_code, "Group drain task exited");
                    break;
                };

                queue_size.release();

                // Group-flush suppression (ledger A-05/R-52/R-53), checked
                // BEFORE the semaphore/rate limiter so a suppressed group
                // spends neither a concurrency slot nor a rate-limit token.
                if ack_if_suppressed(&flush_registry, &metrics_collector, &task).await {
                    continue;
                }

                // Go `InFlightTracker.EnsureTracked` (see the immediate path).
                if !task.callback.ensure_tracked() {
                    task.callback.ack().await;
                    continue;
                }

                // Acquire a concurrency slot FIRST, then pace on the rate
                // limiter while holding it — see `wait_for_rate_limit_permit`
                // for why this order matters.
                let permit = match semaphore.acquire().await {
                    Ok(p) => p,
                    Err(_) => {
                        error!("Semaphore closed");
                        task.callback.nack(Some(10)).await;
                        // Leave the guard active: it empties the rest of the
                        // buffer (fallback nacks) and resets `processing`.
                        break;
                    }
                };

                // Wait for rate limit permit (no timeout — see fn doc).
                Self::wait_for_rate_limit_permit(&rate_limiter, &metrics_collector).await;

                let key = Self::begin_mediating(
                    &guard.worker.mediating,
                    &mediating_seq,
                    &pool_code,
                    &task,
                );
                guard.worker.begin(key);

                let start = std::time::Instant::now();
                let outcome = mediator.mediate(&task.message).await;
                let duration_ms = start.elapsed().as_millis() as u64;

                let disposition = disposition_of(
                    &outcome,
                    task.attempts,
                    task.message.dispatch_mode,
                    task.callback.honours_delayed_return(),
                );
                apply_metric(&metrics_collector, disposition.metric, duration_ms);
                log_disposition(&pool_code, &task, &outcome, &disposition);
                guard.worker.end();
                drop(permit);

                match disposition.action {
                    BrokerAction::Ack => {
                        if outcome.result == MediationResult::Success {
                            maybe_flush_group(&flush_registry, &task.message, &outcome);
                        } else {
                            warn!(
                                message_id = %task.message.id,
                                error = ?outcome.error_message,
                                "Permanent error, ACKing to prevent retry"
                            );
                        }
                        task.callback.ack().await;

                        if disposition.group == GroupEffect::Block {
                            // BLOCK_ON_ERROR: the head failed terminally, so
                            // nothing behind it may be delivered past it.
                            // Handed back, not ACKed — see the Disposition
                            // section's module doc.
                            let siblings = take_buffered(&group_handlers, &group_id);
                            if !siblings.is_empty() {
                                warn!(
                                    group_id = %group_id,
                                    pool_code = %pool_code,
                                    released = siblings.len(),
                                    "Head failed under BLOCK_ON_ERROR; handing the group back to the broker"
                                );
                            }
                            nack_all(siblings, &queue_size, Some(SIBLING_NACK_DELAY_SECS)).await;
                        }
                    }
                    BrokerAction::Release => {
                        // The whole group goes back, head first, so it
                        // returns in order.
                        task.callback.nack(disposition.nack_delay_secs()).await;
                        let siblings = take_buffered(&group_handlers, &group_id);
                        info!(
                            group_id = %group_id,
                            pool_code = %pool_code,
                            message_id = %task.message.id,
                            buffered_released = siblings.len(),
                            delay_seconds = ?disposition.nack_delay_secs(),
                            "Released message group to broker"
                        );
                        nack_all(
                            siblings,
                            &queue_size,
                            sibling_nack_delay(disposition.nack_delay_secs()),
                        )
                        .await;
                    }
                    BrokerAction::Retry => {
                        // Re-front the head so it is the next message
                        // attempted, then wait out the backoff holding no
                        // concurrency slot. Later arrivals queue behind it.
                        task.attempts += 1;
                        // Go `InFlightTracker.MarkRetrying`: the reaper and
                        // stall detector leave a live retry alone.
                        task.callback.mark_retrying();
                        let head_delay = disposition.nack_delay_secs();
                        let homeless = match group_handlers.get(&group_id) {
                            Some(entry) => {
                                queue_size.reserve();
                                entry.lock().enqueue_front(task);
                                None
                            }
                            None => Some(task),
                        };
                        if let Some(task) = homeless {
                            // Unreachable while `processing` is set (the
                            // handler is only removed when idle); hand the
                            // message back rather than lose it.
                            task.callback.nack(head_delay).await;
                            continue;
                        }
                        tokio::select! {
                            biased;
                            _ = stop.cancelled() => {
                                // Shutdown / release_remainder: hand the group
                                // back (whatever release_remainder has not
                                // already taken) instead of holding it.
                                release_group(&group_handlers, &group_id, &queue_size, head_delay).await;
                            }
                            _ = tokio::time::sleep(disposition.retry_after) => {}
                        }
                    }
                }
            }
        });
    }

    /// Record `task` as entering a worker — the operator "Mediating" view's
    /// single write path (both `spawn_immediate_task` and the group-drain
    /// path call this, in the same critical section as their
    /// `active_workers.fetch_add`). Returns the key `end_mediating` needs
    /// to remove it again. Static (takes the Arc clones directly) so it
    /// can run inside a spawned task that only owns clones, not `&self`.
    fn begin_mediating(
        mediating: &DashMap<u64, MediatingEntry>,
        mediating_seq: &AtomicU64,
        pool_code: &str,
        task: &PoolTask,
    ) -> u64 {
        let key = mediating_seq.fetch_add(1, Ordering::Relaxed);
        mediating.insert(
            key,
            MediatingEntry {
                message_id: task.message.id.clone(),
                pool_code: pool_code.to_string(),
                group: task.message.message_group_id.clone().unwrap_or_default(),
                queue: task.queue_identifier.clone(),
                target: task.message.mediation_target.clone(),
                attempts: task.attempts,
                mediated_at: std::time::Instant::now(),
            },
        );
        key
    }

    /// Remove the entry `begin_mediating` returned the key for. Call this
    /// in the same critical section as `active_workers.fetch_sub` — see
    /// `mediating`'s own doc comment for why the two must stay in lockstep.
    fn end_mediating(mediating: &DashMap<u64, MediatingEntry>, key: u64) {
        mediating.remove(&key);
    }

    /// Every message currently inside a worker of this pool — the operator
    /// "Mediating" dashboard view (never reaped; see `mediating`'s own doc
    /// comment). Order is unspecified; callers sort as needed (the API
    /// layer sorts longest-mediating first, matching Go's dashboard).
    pub fn mediating_snapshot(&self) -> Vec<MediatingEntry> {
        self.mediating.iter().map(|e| e.value().clone()).collect()
    }

    /// A point-in-time view of every live message group this pool is
    /// holding, for the operator "blocked groups" view (ledger R-04).
    /// Thread-safe and allocation-light, same shape as Go's
    /// `Pool.GroupSnapshot`: `group_handlers`' lock is held only long
    /// enough to copy each `MessageGroupHandler`'s group/working/
    /// parked_at/buffered-length; the `GroupFlushRegistry` lookup (which
    /// takes its OWN lock) happens after that lock is released, so the two
    /// locks are never nested.
    pub fn group_snapshot(&self) -> Vec<GroupInfo> {
        let now_instant = std::time::Instant::now();
        let now_utc = chrono::Utc::now();
        // Convert a monotonic `Instant` to an approximate wall-clock
        // `DateTime<Utc>` by measuring its signed offset from "now" and
        // applying the same offset to "now" on the wall clock — used both
        // for `parked_at` (always in the past) and `GroupFlushRegistry`'s
        // suppression expiry (always in the future) below. Good enough for
        // operator display; never used for anything needing clock-accurate
        // arithmetic.
        let to_wall_clock = |instant: std::time::Instant| {
            if instant >= now_instant {
                let delta = instant - now_instant;
                now_utc + chrono::Duration::from_std(delta).unwrap_or_default()
            } else {
                let delta = now_instant - instant;
                now_utc - chrono::Duration::from_std(delta).unwrap_or_default()
            }
        };

        let mut rows: Vec<GroupInfo> = self
            .group_handlers
            .iter()
            .map(|entry| {
                let handler = entry.value().lock();
                GroupInfo {
                    group: entry.key().to_string(),
                    pool_code: self.config.code.clone(),
                    buffered: handler.len(),
                    working: handler.processing,
                    parked_at: handler.parked_at.map(to_wall_clock),
                    suppressed: false,
                    suppressed_until: None,
                }
            })
            .collect();

        for row in &mut rows {
            if let Some(until) = self.flush_registry.suppressed_until(&row.group) {
                row.suppressed = true;
                row.suppressed_until = Some(to_wall_clock(until));
            }
        }
        rows
    }

    /// Check available capacity
    pub fn available_capacity(&self) -> usize {
        let capacity = self.capacity() as usize;
        let used = self.queue_size.load(Ordering::Relaxed) as usize;
        capacity.saturating_sub(used)
    }

    /// Item 3: true only the first time this is called since the pool last
    /// had room for a whole batch — every subsequent call while it stays
    /// full returns false. Callers gate their "pool at capacity" WARN log
    /// and `WarningService` entry on the return value so a sustained
    /// saturation episode reports once, not once per deferred batch.
    pub fn note_capacity_full(&self) -> bool {
        !self.capacity_full_warned.swap(true, Ordering::SeqCst)
    }

    /// Item 3: true only when this pool was previously reported full (via
    /// `note_capacity_full`) and a batch has now found room again — the
    /// resume-log transition. Safe to call unconditionally whenever a
    /// batch finds the pool NOT full; it is a no-op (returns false) if the
    /// pool was never reported full to begin with.
    pub fn note_capacity_recovered(&self) -> bool {
        self.capacity_full_warned.swap(false, Ordering::SeqCst)
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
        self.stop.cancel();
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
        // Wake every task waiting out an in-place retry: each hands its
        // message back (and whatever is left of its group) rather than
        // holding it past shutdown. Cancelled BEFORE the buffers are taken,
        // so a head re-fronted after this sweep is seen by its own task.
        self.stop.cancel();

        let group_ids: Vec<Arc<str>> = self
            .group_handlers
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        let mut released = 0usize;
        let queue_slot_releaser = self.queue_slot_releaser();
        for group_id in group_ids {
            let drained = take_buffered(&self.group_handlers, &group_id);
            released += drained.len();
            nack_all(drained, &queue_slot_releaser, None).await;
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
    //! Pins `disposition_of` per outcome, and the backoff curves, against
    //! Go's `pool.go` (`DispositionOf`, `retryDelay`, `deferredDelay`,
    //! `retryOrRelease`, `nackDelay`). Pure/synchronous — no pool, mediator,
    //! or broker needed. End-to-end group behaviour is pinned in
    //! `tests/cascade_dispatch_mode_test.rs` and `tests/pool_tests.rs`.
    use super::*;
    use fc_common::MediationOutcome;

    const ALL_MODES: [DispatchMode; 3] = [
        DispatchMode::Immediate,
        DispatchMode::NextOnError,
        DispatchMode::BlockOnError,
    ];

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn success_acks_and_continues() {
        let d = disposition_of(
            &MediationOutcome::success(200),
            0,
            DispatchMode::Immediate,
            true,
        );
        assert_eq!(d.action, BrokerAction::Ack);
        assert_eq!(d.group, GroupEffect::Continue);
        assert_eq!(d.metric, DispositionMetric::Success);
        assert_eq!(d.nack_delay_secs(), None);
    }

    #[test]
    fn error_config_acks_and_continues_under_immediate_and_next_on_error() {
        let outcome = MediationOutcome::error_config(400, "bad request".to_string());
        for m in [DispatchMode::Immediate, DispatchMode::NextOnError] {
            let d = disposition_of(&outcome, 0, m, true);
            assert_eq!(d.action, BrokerAction::Ack, "mode {m:?}");
            assert_eq!(d.group, GroupEffect::Continue, "mode {m:?}");
            assert_eq!(d.metric, DispositionMetric::Failure, "mode {m:?}");
        }
    }

    #[test]
    fn error_config_blocks_group_under_block_on_error() {
        let outcome = MediationOutcome::error_config(500, "rejected".to_string());
        let d = disposition_of(&outcome, 0, DispatchMode::BlockOnError, true);
        assert_eq!(d.action, BrokerAction::Ack, "the head is still ACKed away");
        assert_eq!(d.group, GroupEffect::Block);
        assert_eq!(d.metric, DispositionMetric::Failure);
    }

    #[test]
    fn error_process_releases_whole_group_under_every_mode() {
        let outcome = MediationOutcome::error_process(Some(30), "unavailable".to_string());
        for m in ALL_MODES {
            let d = disposition_of(&outcome, 0, m, true);
            assert_eq!(d.action, BrokerAction::Release, "mode {m:?}");
            assert_eq!(d.group, GroupEffect::Release, "mode {m:?}");
            assert_eq!(d.metric, DispositionMetric::Transient, "mode {m:?}");
            assert_eq!(d.nack_delay_secs(), Some(30), "mode {m:?}");
        }
    }

    #[test]
    fn error_connection_releases_whole_group_with_its_30s_delay() {
        let outcome = MediationOutcome::error_connection("connection refused".to_string());
        let d = disposition_of(&outcome, 0, DispatchMode::BlockOnError, true);
        assert_eq!(d.action, BrokerAction::Release);
        assert_eq!(d.group, GroupEffect::Release);
        assert_eq!(d.metric, DispositionMetric::Failure);
        assert_eq!(d.nack_delay_secs(), Some(30));
    }

    #[test]
    fn rate_limited_retries_in_place_with_retry_after_as_floor() {
        let d = disposition_of(
            &MediationOutcome::rate_limited(12),
            0,
            DispatchMode::BlockOnError,
            true,
        );
        assert_eq!(
            d.action,
            BrokerAction::Retry,
            "429 is retried here, not released"
        );
        assert_eq!(d.group, GroupEffect::Continue, "and does not cascade");
        assert_eq!(d.metric, DispositionMetric::RateLimited);
        assert_eq!(d.retry_after, secs(12));
        assert!(!d.budget_exhausted);
    }

    #[test]
    fn deferred_without_delay_retries_in_place_on_the_deferred_curve() {
        // H1: an ack:false with no delay must never be a 0s release.
        for honours in [true, false] {
            let d = disposition_of(
                &MediationOutcome::deferred(200, Some(0)),
                0,
                DispatchMode::NextOnError,
                honours,
            );
            assert_eq!(d.action, BrokerAction::Retry, "honours={honours}");
            assert_eq!(d.retry_after, secs(5), "the curve starts at 5s");
            assert_eq!(d.metric, DispositionMetric::Transient);
        }
    }

    #[test]
    fn deferred_with_delay_goes_back_to_a_broker_that_holds_it() {
        // R1: exactly the named delay, no curve, no cap, whole group.
        let d = disposition_of(
            &MediationOutcome::deferred(200, Some(600)),
            0,
            DispatchMode::NextOnError,
            true,
        );
        assert_eq!(d.action, BrokerAction::Release);
        assert_eq!(d.group, GroupEffect::Release);
        assert_eq!(d.nack_delay_secs(), Some(600));
        assert!(!d.budget_exhausted);
    }

    #[test]
    fn deferred_with_delay_retries_in_place_when_the_broker_cannot_hold_it() {
        let d = disposition_of(
            &MediationOutcome::deferred(200, Some(15)),
            0,
            DispatchMode::NextOnError,
            false,
        );
        assert_eq!(d.action, BrokerAction::Retry);
        assert_eq!(d.retry_after, secs(15), "the named delay floors the curve");
    }

    #[test]
    fn circuit_open_releases_whole_group_with_no_metric() {
        let outcome = MediationOutcome::circuit_open();
        for m in ALL_MODES {
            let d = disposition_of(&outcome, 0, m, true);
            assert_eq!(d.action, BrokerAction::Release, "mode {m:?}");
            assert_eq!(d.group, GroupEffect::Release, "mode {m:?}");
            assert_eq!(d.metric, DispositionMetric::None, "mode {m:?}");
            assert_eq!(d.nack_delay_secs(), Some(5), "mode {m:?}");
        }
    }

    #[test]
    fn a_spent_retry_budget_releases_with_the_backoff_as_redelivery_delay() {
        let outcome = MediationOutcome::rate_limited(30);
        let d = disposition_of(
            &outcome,
            MAX_IN_PIPELINE_ATTEMPTS - 2,
            DispatchMode::NextOnError,
            true,
        );
        assert_eq!(d.action, BrokerAction::Retry, "one retry left");

        let d = disposition_of(
            &outcome,
            MAX_IN_PIPELINE_ATTEMPTS - 1,
            DispatchMode::NextOnError,
            true,
        );
        assert_eq!(
            d.action,
            BrokerAction::Release,
            "the last attempt hands it back"
        );
        assert_eq!(d.group, GroupEffect::Release, "with its group");
        assert!(d.budget_exhausted);
        assert_eq!(d.retry_after, retry_delay(MAX_IN_PIPELINE_ATTEMPTS - 1, 30));
    }

    #[test]
    fn deferred_curve_matches_go() {
        assert_eq!(deferred_delay(0, 0), secs(5));
        assert_eq!(deferred_delay(1, 0), secs(10));
        assert_eq!(deferred_delay(3, 0), secs(40));
        assert_eq!(deferred_delay(4, 0), secs(60), "80s ramp hits the 60s cap");
        assert_eq!(deferred_delay(12, 0), secs(60));
        assert_eq!(
            deferred_delay(40, 0),
            secs(60),
            "shift is capped, never overflows"
        );
        assert_eq!(deferred_delay(0, 30), secs(30));
        assert_eq!(deferred_delay(3, 30), secs(40), "ramp above the floor wins");
        assert_eq!(
            deferred_delay(0, 300),
            secs(60),
            "a floor never lifts the cap"
        );
    }

    #[test]
    fn retry_curve_matches_go() {
        assert_eq!(retry_delay(0, 0), Duration::from_millis(100));
        assert_eq!(retry_delay(0, 30), secs(30));
        assert_eq!(retry_delay(12, 0), secs(300));
        assert_eq!(retry_delay(0, 240), secs(240));
        assert_eq!(retry_delay(0, u32::MAX), secs(300));
    }

    #[test]
    fn nack_delay_is_whole_seconds_at_least_one_or_none() {
        let with = |d: Duration| Disposition {
            action: BrokerAction::Release,
            group: GroupEffect::Release,
            metric: DispositionMetric::None,
            retry_after: d,
            budget_exhausted: false,
        };
        assert_eq!(with(Duration::ZERO).nack_delay_secs(), None);
        assert_eq!(with(Duration::from_millis(200)).nack_delay_secs(), Some(1));
        assert_eq!(
            with(Duration::from_millis(51_200)).nack_delay_secs(),
            Some(51)
        );
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
        assert_eq!(
            breaker_effect(&MediationOutcome::deferred(200, Some(0))),
            None
        );
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

    #[test]
    fn breaker_effect_circuit_open_is_neutral() {
        // The breaker that just rejected the call already recorded the
        // rejection itself (`allow_request`'s own `rejected_calls`
        // counter) — this function must not double-record it as a
        // success or failure delta.
        assert_eq!(breaker_effect(&MediationOutcome::circuit_open()), None);
    }
}
