//! The flight recorder: a bounded, in-memory ring of what the router did
//! with each message recently — routed, dispatched (with outcome and
//! disposition), what its group decided, and how it was settled with the
//! broker (ack/nack/defer, with the reason). A lightweight equivalent of
//! Java's `Dispatch` / `GroupDecision` / `MessageSettled` JFR events, but
//! always on and queryable over HTTP (`/diagnostics/messages/{id}`,
//! `/diagnostics/groups/{group}`, `/diagnostics/events`), so "what happened
//! to message X" can still be answered after X has left the pipeline.
//!
//! Cost: a handful of events per message, each one short mutex push into
//! one of [`SHARDS`] rings chosen by message id (so a message's history
//! sits in one shard and a lookup scans only that shard), one clock read,
//! and one reference-count increment on the message's own context — no
//! formatting, no shared counters (`throughput_bench.rs` prices it).
//! Memory is bounded by `capacity` (`FC_ROUTER_FLIGHT_RECORDER_EVENTS`,
//! default [`DEFAULT_CAPACITY`]; `0` turns recording off). Nothing here is
//! persisted: a restart starts empty.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::fmt::Formatter;
use std::sync::OnceLock;

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
    /// Order within the message's shard (a message's own events are in
    /// one shard; across shards, `at` orders).
    pub seq: u64,
    pub at: chrono::DateTime<chrono::Utc>,
    pub kind: EventKind,
    #[serde(flatten)]
    pub context: EventContext,
    #[serde(flatten)]
    pub facts: Facts,
}

impl RecordedEvent {
    pub fn message_id(&self) -> &str {
        &self.context.0.message_id
    }
    pub fn pool(&self) -> Option<&str> {
        self.context.0.pool.as_deref()
    }
    pub fn group(&self) -> Option<&str> {
        self.context.0.group.as_deref()
    }
    pub fn queue(&self) -> Option<&str> {
        self.context.0.queue.as_deref()
    }
}

/// What an event says beyond its kind. Structured, so the hot recording
/// sites (routed, dispatched, settled) format nothing: text is built only
/// when an operator reads it, or on a rare path.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    /// 1-based delivery attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    /// The mediation outcome (`Success`, `ErrorProcess`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// What the pool did next (`Ack`, `Release`, `Retry`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<&'static str>,
    /// A nack's or release's redelivery delay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_secs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<Arc<str>>,
    /// Free text: an error, a reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Cow<'static, str>>,
}

impl Facts {
    /// Just a text.
    pub fn text(detail: impl Into<Cow<'static, str>>) -> Self {
        Self {
            detail: Some(detail.into()),
            ..Self::default()
        }
    }
}

/// Where an event happened: the message and whatever of pool, group and
/// queue the recording site knows. Build it once per message and pass it to
/// each of that message's events: they share it (one reference count on
/// this message's context, not one per field on names every message shares).
#[derive(Debug, Clone)]
pub struct EventContext(Arc<ContextFields>);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContextFields {
    message_id: Arc<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pool: Option<Arc<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group: Option<Arc<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    queue: Option<Arc<str>>,
}

impl Serialize for EventContext {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl EventContext {
    pub fn new(message_id: impl Into<Arc<str>>) -> Self {
        Self::from_parts(message_id.into(), None, None, None)
    }

    /// A context from names the caller already shares.
    pub fn from_parts(
        message_id: Arc<str>,
        pool: Option<Arc<str>>,
        group: Option<Arc<str>>,
        queue: Option<Arc<str>>,
    ) -> Self {
        Self(Arc::new(ContextFields {
            message_id,
            pool,
            group,
            queue,
        }))
    }

    pub fn pool(mut self, pool: impl Into<Arc<str>>) -> Self {
        Arc::make_mut(&mut self.0).pool = Some(pool.into());
        self
    }

    /// Empty group ids are recorded as none.
    pub fn group(mut self, group: Option<&str>) -> Self {
        Arc::make_mut(&mut self.0).group = group.filter(|g| !g.is_empty()).map(Arc::from);
        self
    }

    pub fn queue(mut self, queue: impl Into<Arc<str>>) -> Self {
        Arc::make_mut(&mut self.0).queue = Some(queue.into());
        self
    }

    pub fn message_id(&self) -> &str {
        &self.0.message_id
    }

    /// A shared placeholder for when recording is off: no allocation.
    pub fn unrecorded() -> Self {
        static NONE: OnceLock<EventContext> = OnceLock::new();
        NONE.get_or_init(|| Self::new("")).clone()
    }
}

/// The recorder. Cheap to share (`Arc`); every method takes `&self`.
pub struct FlightRecorder {
    shards: Box<[Mutex<Shard>]>,
    per_shard: usize,
}

struct Shard {
    ring: VecDeque<RecordedEvent>,
    /// Events ever recorded here (also each event's `seq`).
    recorded: u64,
}

impl fmt::Debug for FlightRecorder {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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
            .map(|_| {
                Mutex::new(Shard {
                    ring: VecDeque::with_capacity(per_shard.min(1024)),
                    recorded: 0,
                })
            })
            .collect();
        Self { shards, per_shard }
    }

    pub fn is_enabled(&self) -> bool {
        self.per_shard > 0
    }

    pub fn capacity(&self) -> usize {
        self.per_shard * SHARDS
    }

    fn shard_of(message_id: &str) -> usize {
        let mut h = DefaultHasher::new();
        message_id.hash(&mut h);
        (h.finish() as usize) % SHARDS
    }

    /// Record one event about `ctx`'s message. A no-op when recording is
    /// off.
    pub fn record(&self, kind: EventKind, ctx: &EventContext, facts: Facts) {
        if self.per_shard == 0 {
            return;
        }
        let at = chrono::Utc::now();
        let mut shard = self.shards[Self::shard_of(ctx.message_id())].lock();
        let seq = shard.recorded;
        shard.recorded += 1;
        if shard.ring.len() >= self.per_shard {
            shard.ring.pop_front();
        }
        shard.ring.push_back(RecordedEvent {
            seq,
            at,
            kind,
            context: ctx.clone(),
            facts,
        });
    }

    /// Every event still held for `message_id`, oldest first.
    pub fn for_message(&self, message_id: &str) -> Vec<RecordedEvent> {
        if self.per_shard == 0 {
            return Vec::new();
        }
        self.shards[Self::shard_of(message_id)]
            .lock()
            .ring
            .iter()
            .filter(|e| e.message_id() == message_id)
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
            let shard = shard.lock();
            out.extend(shard.ring.iter().filter(|e| filter.matches(e)).cloned());
        }
        out.sort_by_key(|e| (e.at, e.seq));
        if out.len() > limit {
            out.drain(..out.len() - limit);
        }
        out
    }

    /// Events held now, across every shard.
    pub fn len(&self) -> usize {
        self.shards.iter().map(|s| s.lock().ring.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Events recorded since start (including those since overwritten).
    pub fn recorded_total(&self) -> u64 {
        self.shards.iter().map(|s| s.lock().recorded).sum()
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
            .is_none_or(|m| e.message_id() == m)
            && self.group.as_deref().is_none_or(|g| e.group() == Some(g))
            && self
                .pool
                .as_deref()
                .is_none_or(|p| e.pool().is_some_and(|ep| ep.eq_ignore_ascii_case(p)))
            && self.kind.is_none_or(|k| e.kind == k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn ctx(id: &str) -> EventContext {
        EventContext::new(id).pool("P").group(Some("g1")).queue("q")
    }

    #[test]
    fn a_message_history_is_kept_in_order() {
        let r = FlightRecorder::new(64);
        r.record(EventKind::Routed, &ctx("m1"), Facts::default());
        r.record(EventKind::Routed, &ctx("m2"), Facts::default());
        r.record(EventKind::DispatchStarted, &ctx("m1"), Facts::default());
        r.record(EventKind::Acked, &ctx("m1"), Facts::text("2xx"));
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
        assert_eq!(h[2].facts.detail.as_deref(), Some("2xx"));
        assert_eq!(h[0].group(), Some("g1"));
    }

    #[test]
    fn facts_serialise_flat_and_skip_what_is_unset() {
        let r = FlightRecorder::new(64);
        r.record(
            EventKind::DispatchFinished,
            &ctx("m1"),
            Facts {
                outcome: Some("Success"),
                status: Some(200),
                duration_ms: Some(12),
                action: Some("Ack"),
                ..Facts::default()
            },
        );
        let json = serde_json::to_value(&r.for_message("m1")[0]).unwrap();
        assert_eq!(json["kind"], "DISPATCH_FINISHED");
        assert_eq!(json["status"], 200);
        assert_eq!(json["durationMs"], 12);
        assert!(json.get("delaySecs").is_none());
        assert!(json.get("detail").is_none());
    }

    #[test]
    fn the_ring_is_bounded() {
        let r = FlightRecorder::new(SHARDS * 2);
        for i in 0..1000 {
            r.record(EventKind::Routed, &ctx(&format!("m{i}")), Facts::default());
        }
        assert!(r.len() <= SHARDS * 2);
        assert_eq!(r.recorded_total(), 1000);
    }

    #[test]
    fn zero_capacity_records_nothing() {
        let r = FlightRecorder::new(0);
        r.record(EventKind::Routed, &ctx("m"), Facts::default());
        assert!(r.is_empty());
        assert!(!r.is_enabled());
        assert!(r.for_message("m").is_empty());
    }

    #[test]
    fn queries_filter_by_group_and_keep_the_newest() {
        let r = FlightRecorder::new(1024);
        for i in 0..10 {
            // Across shards events order by time: keep them apart.
            thread::sleep(Duration::from_millis(2));
            let group = if i % 2 == 0 { "even" } else { "odd" };
            r.record(
                EventKind::Routed,
                &EventContext::new(format!("m{i}"))
                    .pool("P")
                    .group(Some(group)),
                Facts::default(),
            );
        }
        let even = r.query(
            &EventFilter {
                group: Some("even".into()),
                ..EventFilter::default()
            },
            3,
        );
        let ids: Vec<_> = even.iter().map(|e| e.message_id().to_string()).collect();
        assert_eq!(ids, vec!["m4", "m6", "m8"]);
    }
}
