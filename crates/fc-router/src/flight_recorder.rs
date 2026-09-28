//! The flight recorder: a bounded, in-memory ring of what the router did
//! with each message recently — routed, dispatched (with outcome and
//! disposition), what its group decided, and how it was settled with the
//! broker (ack/nack/defer, with the reason). A lightweight equivalent of
//! Java's `Dispatch` / `GroupDecision` / `MessageSettled` JFR events, but
//! always on and queryable over HTTP (`/diagnostics/messages/{id}`,
//! `/diagnostics/groups/{group}`, `/diagnostics/events`), so "what happened
//! to message X" can still be answered after X has left the pipeline.
//!
//! Cost: one short uncontended mutex push per event (a handful per
//! message), into one of [`SHARDS`] rings chosen by message id, so a
//! message's history sits in one shard and a lookup scans only that shard.
//! Memory is bounded by `capacity` (`FC_ROUTER_FLIGHT_RECORDER_EVENTS`,
//! default [`DEFAULT_CAPACITY`]; `0` turns recording off). Nothing here is
//! persisted: a restart starts empty.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Default number of events kept (about 2–3 MB).
pub const DEFAULT_CAPACITY: usize = 16_384;

/// Rings the capacity is split across (fewer lock collisions between
/// workers recording at the same time).
pub const SHARDS: usize = 16;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventKind {
    /// Admitted and submitted to a pool.
    Routed,
    /// A broker redelivery of a message already in the pipeline (receipt
    /// handle refreshed, nothing delivered).
    Redelivered,
    /// Handed back to the broker because its pool was full.
    DeferredCapacity,
    /// Not routed: strict-routing malformed, pool creation or submit
    /// failed, or the message behind a failed submit in its group.
    Rejected,
    /// Acked without delivery because its group is flushed.
    Suppressed,
    /// Acked without delivery: another copy of the message owns the
    /// pipeline.
    DuplicateAcked,
    /// A delivery attempt started (after the concurrency slot and the rate
    /// limiter).
    DispatchStarted,
    /// A delivery attempt finished: outcome, status, duration and the
    /// disposition taken.
    DispatchFinished,
    /// What an ordered group did about its head: block, release, retry.
    GroupDecision,
    /// Acked on the broker (terminal).
    Acked,
    /// Acked on the broker failed; kept for deletion on redelivery.
    AckFailed,
    /// Nacked on the broker (terminal for this copy), with its delay.
    Nacked,
    /// The callback was dropped unresolved (a panic or cancel); the
    /// fallback nack ran.
    Abandoned,
    /// The mediator panicked; the message was released.
    Panicked,
    /// Removed from the in-flight tracker by the reaper or an operator.
    Untracked,
    /// Handed back at shutdown.
    ReleasedAtShutdown,
}

/// One recorded event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedEvent {
    /// Global order across shards.
    pub seq: u64,
    pub at: chrono::DateTime<chrono::Utc>,
    pub message_id: Arc<str>,
    pub kind: EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<Arc<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<Arc<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<Arc<str>>,
    /// Free text: outcome, status, delay, reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Where an event happened: the message and whatever of pool, group and
/// queue the recording site knows.
#[derive(Debug, Clone, Default)]
pub struct EventContext {
    pub pool: Option<Arc<str>>,
    pub group: Option<Arc<str>>,
    pub queue: Option<Arc<str>>,
}

impl EventContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pool(mut self, pool: impl Into<Arc<str>>) -> Self {
        self.pool = Some(pool.into());
        self
    }

    /// Empty group ids are recorded as none.
    pub fn group(mut self, group: Option<&str>) -> Self {
        self.group = group.filter(|g| !g.is_empty()).map(Arc::from);
        self
    }

    pub fn queue(mut self, queue: impl Into<Arc<str>>) -> Self {
        self.queue = Some(queue.into());
        self
    }
}

/// The recorder. Cheap to share (`Arc`); every method takes `&self`.
pub struct FlightRecorder {
    shards: Box<[Mutex<VecDeque<RecordedEvent>>]>,
    per_shard: usize,
    seq: AtomicU64,
}

impl std::fmt::Debug for FlightRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlightRecorder")
            .field("capacity", &self.capacity())
            .finish()
    }
}

impl Default for FlightRecorder {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl FlightRecorder {
    /// A recorder keeping about `capacity` events (rounded up to a multiple
    /// of [`SHARDS`]); `0` records nothing.
    pub fn new(capacity: usize) -> Self {
        let per_shard = capacity.div_ceil(SHARDS);
        let shards = (0..SHARDS)
            .map(|_| Mutex::new(VecDeque::with_capacity(per_shard.min(1024))))
            .collect();
        Self {
            shards,
            per_shard,
            seq: AtomicU64::new(0),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.per_shard > 0
    }

    pub fn capacity(&self) -> usize {
        self.per_shard * SHARDS
    }

    fn shard_of(message_id: &str) -> usize {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        message_id.hash(&mut h);
        (h.finish() as usize) % SHARDS
    }

    /// Record one event. A no-op when recording is off.
    pub fn record(
        &self,
        message_id: &str,
        kind: EventKind,
        ctx: &EventContext,
        detail: Option<String>,
    ) {
        if self.per_shard == 0 {
            return;
        }
        let event = RecordedEvent {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            at: chrono::Utc::now(),
            message_id: Arc::from(message_id),
            kind,
            pool: ctx.pool.clone(),
            group: ctx.group.clone(),
            queue: ctx.queue.clone(),
            detail,
        };
        let mut ring = self.shards[Self::shard_of(message_id)].lock();
        if ring.len() >= self.per_shard {
            ring.pop_front();
        }
        ring.push_back(event);
    }

    /// Every event still held for `message_id`, oldest first.
    pub fn for_message(&self, message_id: &str) -> Vec<RecordedEvent> {
        if self.per_shard == 0 {
            return Vec::new();
        }
        self.shards[Self::shard_of(message_id)]
            .lock()
            .iter()
            .filter(|e| &*e.message_id == message_id)
            .cloned()
            .collect()
    }

    /// The most recent events matching the filter, oldest first, at most
    /// `limit`. Scans every shard: for operator queries, not hot paths.
    pub fn query(&self, filter: &EventFilter, limit: usize) -> Vec<RecordedEvent> {
        if self.per_shard == 0 || limit == 0 {
            return Vec::new();
        }
        let mut out: Vec<RecordedEvent> = Vec::new();
        for shard in self.shards.iter() {
            let ring = shard.lock();
            out.extend(ring.iter().filter(|e| filter.matches(e)).cloned());
        }
        out.sort_by_key(|e| e.seq);
        if out.len() > limit {
            out.drain(..out.len() - limit);
        }
        out
    }

    /// Events held now, across every shard.
    pub fn len(&self) -> usize {
        self.shards.iter().map(|s| s.lock().len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Events recorded since start (including those since overwritten).
    pub fn recorded_total(&self) -> u64 {
        self.seq.load(Ordering::Relaxed)
    }
}

/// A query over the recorder. Every set field must match.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub message_id: Option<String>,
    pub group: Option<String>,
    pub pool: Option<String>,
    pub kind: Option<EventKind>,
}

impl EventFilter {
    fn matches(&self, e: &RecordedEvent) -> bool {
        self.message_id
            .as_deref()
            .is_none_or(|m| &*e.message_id == m)
            && self
                .group
                .as_deref()
                .is_none_or(|g| e.group.as_deref() == Some(g))
            && self.pool.as_deref().is_none_or(|p| {
                e.pool
                    .as_deref()
                    .is_some_and(|ep| ep.eq_ignore_ascii_case(p))
            })
            && self.kind.is_none_or(|k| e.kind == k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EventContext {
        EventContext::new().pool("P").group(Some("g1")).queue("q")
    }

    #[test]
    fn a_message_history_is_kept_in_order() {
        let r = FlightRecorder::new(64);
        r.record("m1", EventKind::Routed, &ctx(), None);
        r.record("m2", EventKind::Routed, &ctx(), None);
        r.record("m1", EventKind::DispatchStarted, &ctx(), None);
        r.record("m1", EventKind::Acked, &ctx(), Some("2xx".into()));
        let h = r.for_message("m1");
        let kinds: Vec<_> = h.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                EventKind::Routed,
                EventKind::DispatchStarted,
                EventKind::Acked
            ]
        );
        assert_eq!(h[2].detail.as_deref(), Some("2xx"));
        assert_eq!(h[0].group.as_deref(), Some("g1"));
    }

    #[test]
    fn the_ring_is_bounded() {
        let r = FlightRecorder::new(SHARDS * 2);
        for i in 0..1000 {
            r.record(&format!("m{i}"), EventKind::Routed, &ctx(), None);
        }
        assert!(r.len() <= SHARDS * 2);
        assert_eq!(r.recorded_total(), 1000);
    }

    #[test]
    fn zero_capacity_records_nothing() {
        let r = FlightRecorder::new(0);
        r.record("m", EventKind::Routed, &ctx(), None);
        assert!(r.is_empty());
        assert!(!r.is_enabled());
        assert!(r.for_message("m").is_empty());
    }

    #[test]
    fn queries_filter_by_group_and_keep_the_newest() {
        let r = FlightRecorder::new(1024);
        for i in 0..10 {
            let group = if i % 2 == 0 { "even" } else { "odd" };
            r.record(
                &format!("m{i}"),
                EventKind::Routed,
                &EventContext::new().pool("P").group(Some(group)),
                None,
            );
        }
        let even = r.query(
            &EventFilter {
                group: Some("even".into()),
                ..EventFilter::default()
            },
            3,
        );
        let ids: Vec<_> = even.iter().map(|e| e.message_id.to_string()).collect();
        assert_eq!(ids, vec!["m4", "m6", "m8"]);
    }
}
