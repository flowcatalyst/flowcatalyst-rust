//! The manager's consumer registry: one [`RunningConsumer`] per configured
//! queue, indexed by config name and by the consumer's own `identifier()`,
//! plus the consumers that were detached (queue removed or changed, or a
//! stalled poll loop replaced) but may still own in-flight messages.
//!
//! Mirrors Go's `Manager.consumers` / `consumersByID` / `detaching` and
//! `runningConsumer` (`internal/router/manager.go`). The manager, not the
//! health service, owns each consumer's poll heartbeat: the restart watchdog
//! and every health answer read the same `last_poll`, so the two can never
//! disagree about whether one consumer is alive (Go: `ConsumerStats`).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use fc_common::QueueConfig;
use fc_queue::QueueConsumer;

/// One queue's consumer plus its poll loop's state (Go: `runningConsumer`).
///
/// A consumer is replaced, never mutated: a rebuild or a queue-config change
/// makes a new `RunningConsumer` with a new `generation`, and the old one is
/// detached. Anything that must act on "this exact instance" (a poll loop's
/// exit, a callback resolving the consumer that received its message)
/// compares generations, so a late action by an old instance can never
/// clobber its replacement.
pub(crate) struct RunningConsumer {
    pub(crate) consumer: Arc<dyn QueueConsumer>,
    /// Registry key: the config queue name (or the identifier, for a
    /// consumer registered directly via `add_consumer`).
    pub(crate) name: String,
    /// The config this consumer was built from; `None` for a consumer
    /// registered directly, which the watchdog cannot rebuild.
    pub(crate) queue_config: Option<QueueConfig>,
    pub(crate) generation: u64,
    /// Ends this consumer's poll loop only (Go: `stopPoll`). Messages it
    /// already routed keep running and can still be acked.
    pub(crate) stop_poll: CancellationToken,
    /// Set once a poll loop has been spawned for this instance.
    pub(crate) poll_task_started: AtomicBool,
    /// Cancelled when this instance's poll loop has exited (its last
    /// receive finished and anything it got after `stop_poll` was handed
    /// back). Shutdown waits on it after releasing group remainders.
    pub(crate) poll_exited: CancellationToken,
    /// Heartbeat: the last completed successful poll, capacity pause or
    /// leadership pause. Seeded at creation (Go: `newRunningConsumer`).
    last_poll: Mutex<Instant>,
    /// `polls_started` / `polls_returned` bracket every `poll()` call, so a
    /// stall can say whether the loop is inside a hung poll or not polling.
    pub(crate) polls_started: AtomicU64,
    pub(crate) polls_returned: AtomicU64,
    /// Pool codes the most recent routed batch went to — the pools whose
    /// capacity gates this consumer's next poll (Go: `runningConsumer.pools`).
    dest_pools: Mutex<Vec<String>>,
    /// When the messages this consumer deferred for capacity are due back
    /// from the broker (Go: `deferralLedger`).
    deferrals: Mutex<VecDeque<Instant>>,
    /// Stamped when the consumer is detached; `None` while active.
    detached_at: Mutex<Option<Instant>>,
}

impl RunningConsumer {
    pub(crate) fn new(
        consumer: Arc<dyn QueueConsumer>,
        name: String,
        queue_config: Option<QueueConfig>,
        generation: u64,
        stop_poll: CancellationToken,
    ) -> Self {
        Self {
            consumer,
            name,
            queue_config,
            generation,
            stop_poll,
            poll_task_started: AtomicBool::new(false),
            poll_exited: CancellationToken::new(),
            last_poll: Mutex::new(Instant::now()),
            polls_started: AtomicU64::new(0),
            polls_returned: AtomicU64::new(0),
            dest_pools: Mutex::new(Vec::new()),
            deferrals: Mutex::new(VecDeque::new()),
            detached_at: Mutex::new(None),
        }
    }

    pub(crate) fn identifier(&self) -> &str {
        self.consumer.identifier()
    }

    /// Stamp the heartbeat.
    pub(crate) fn beat(&self) {
        *self.last_poll.lock() = Instant::now();
    }

    pub(crate) fn last_poll(&self) -> Instant {
        *self.last_poll.lock()
    }

    #[cfg(test)]
    pub(crate) fn set_last_poll(&self, at: Instant) {
        *self.last_poll.lock() = at;
    }

    /// Record the destination pools of the batch just routed. An empty
    /// batch leaves the previous set in place — a quiet poll says nothing
    /// about where this queue's traffic goes.
    pub(crate) fn set_dest_pools(&self, codes: Vec<String>) {
        if !codes.is_empty() {
            *self.dest_pools.lock() = codes;
        }
    }

    pub(crate) fn dest_pools(&self) -> Vec<String> {
        self.dest_pools.lock().clone()
    }

    /// Book a deferral the broker will hand back at `due`.
    pub(crate) fn note_deferral(&self, due: Instant) {
        let mut d = self.deferrals.lock();
        let pos = d.iter().rposition(|t| *t <= due).map_or(0, |i| i + 1);
        d.insert(pos, due);
    }

    /// How many deferred messages are still out on the broker.
    pub(crate) fn deferrals_outstanding(&self, now: Instant) -> usize {
        let mut d = self.deferrals.lock();
        while d.front().is_some_and(|t| *t <= now) {
            d.pop_front();
        }
        d.len()
    }

    /// When the next deferred message is due back, if any.
    pub(crate) fn earliest_deferral(&self) -> Option<Instant> {
        self.deferrals.lock().front().copied()
    }

    pub(crate) fn detached_at(&self) -> Option<Instant> {
        *self.detached_at.lock()
    }

    /// Why this consumer's heartbeat is stale, for the stall warning.
    pub(crate) fn poll_state(&self) -> &'static str {
        let started = self.polls_started.load(Ordering::SeqCst);
        let returned = self.polls_returned.load(Ordering::SeqCst);
        if !self.poll_task_started.load(Ordering::SeqCst) || started == 0 {
            "never polled (loop never reached poll)"
        } else if started > returned {
            "inside poll (poll is hung)"
        } else {
            "between polls (loop is not calling poll)"
        }
    }
}

/// One consumer's liveness as the manager sees it (Go: `ConsumerStat`).
#[derive(Debug, Clone)]
pub struct ConsumerStat {
    /// Registry key — the config queue name.
    pub queue_name: String,
    /// The consumer's own identifier (NATS: `stream/consumer`).
    pub identifier: String,
    /// A poll loop has been started for it — it is meant to be polling.
    pub running: bool,
    /// Its last heartbeat.
    pub last_poll: Instant,
}

#[derive(Default)]
struct Maps {
    by_name: HashMap<String, Arc<RunningConsumer>>,
    by_id: HashMap<String, Arc<RunningConsumer>>,
}

/// Active consumers (by name and by identifier, kept in lockstep under one
/// lock) plus the detaching list. Shared with every message callback so an
/// ack/nack resolves its consumer through the registry at the moment it
/// runs, not through whatever instance happened to be current at route time.
#[derive(Default)]
pub(crate) struct ConsumerRegistry {
    maps: RwLock<Maps>,
    detaching: Mutex<Vec<Arc<RunningConsumer>>>,
}

impl ConsumerRegistry {
    /// Register `rc` under its name, returning whatever held that name.
    pub(crate) fn insert(&self, rc: Arc<RunningConsumer>) -> Option<Arc<RunningConsumer>> {
        let mut m = self.maps.write();
        let prev = m.by_name.insert(rc.name.clone(), rc.clone());
        if let Some(ref p) = prev {
            if m.by_id
                .get(p.identifier())
                .is_some_and(|cur| cur.generation == p.generation)
            {
                m.by_id.remove(p.identifier());
            }
        }
        m.by_id.insert(rc.identifier().to_string(), rc);
        prev
    }

    /// Replace `old` with `new` under `old`'s name, only if `old` is still
    /// the registered instance. Returns false (and changes nothing) if
    /// something else replaced or removed it in the meantime.
    pub(crate) fn replace_if_current(
        &self,
        old: &Arc<RunningConsumer>,
        new: Arc<RunningConsumer>,
    ) -> bool {
        let mut m = self.maps.write();
        match m.by_name.get(&old.name) {
            Some(cur) if cur.generation == old.generation => {}
            _ => return false,
        }
        m.by_name.insert(new.name.clone(), new.clone());
        if m.by_id
            .get(old.identifier())
            .is_some_and(|cur| cur.generation == old.generation)
        {
            m.by_id.remove(old.identifier());
        }
        m.by_id.insert(new.identifier().to_string(), new);
        true
    }

    /// Remove the instance registered under `name`.
    pub(crate) fn remove(&self, name: &str) -> Option<Arc<RunningConsumer>> {
        let mut m = self.maps.write();
        let rc = m.by_name.remove(name)?;
        if m.by_id
            .get(rc.identifier())
            .is_some_and(|cur| cur.generation == rc.generation)
        {
            m.by_id.remove(rc.identifier());
        }
        Some(rc)
    }

    pub(crate) fn get(&self, name: &str) -> Option<Arc<RunningConsumer>> {
        self.maps.read().by_name.get(name).cloned()
    }

    pub(crate) fn get_by_id(&self, identifier: &str) -> Option<Arc<RunningConsumer>> {
        self.maps.read().by_id.get(identifier).cloned()
    }

    pub(crate) fn active(&self) -> Vec<Arc<RunningConsumer>> {
        self.maps.read().by_name.values().cloned().collect()
    }

    pub(crate) fn names(&self) -> Vec<String> {
        self.maps.read().by_name.keys().cloned().collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.maps.read().by_name.len()
    }

    /// Remove every active consumer (shutdown). The maps are left empty,
    /// not dropped, so the manager stays usable (Go: `Shutdown`).
    pub(crate) fn drain_active(&self) -> Vec<Arc<RunningConsumer>> {
        let mut m = self.maps.write();
        m.by_id.clear();
        m.by_name.drain().map(|(_, v)| v).collect()
    }

    /// Remove every detaching consumer (shutdown).
    pub(crate) fn drain_detaching(&self) -> Vec<Arc<RunningConsumer>> {
        std::mem::take(&mut *self.detaching.lock())
    }

    /// Move `rc` (already out of the active maps) onto the detaching list,
    /// stamping when it stopped polling.
    pub(crate) fn detach(&self, rc: Arc<RunningConsumer>) {
        *rc.detached_at.lock() = Some(Instant::now());
        self.detaching.lock().push(rc);
    }

    pub(crate) fn detaching(&self) -> Vec<Arc<RunningConsumer>> {
        self.detaching.lock().clone()
    }

    /// Remove and return the detaching consumers `retire` accepts.
    pub(crate) fn take_retirable(
        &self,
        mut retire: impl FnMut(&RunningConsumer) -> bool,
    ) -> Vec<Arc<RunningConsumer>> {
        let mut d = self.detaching.lock();
        let (out, keep): (Vec<_>, Vec<_>) = d.drain(..).partition(|rc| retire(rc));
        *d = keep;
        out
    }

    /// Resolve the consumer an ack/nack for a message from `queue_id` should
    /// go through (Go: `resolveConsumer`), preferring, in order:
    ///
    /// 1. the instance that received the message (`origin_generation`), while
    ///    it is still registered, active or detaching — a NATS receipt can
    ///    only be acked on the connection that received it;
    /// 2. the active consumer for `queue_id`;
    /// 3. the most recently detached consumer for `queue_id`.
    ///
    /// `None` when nothing registered answers to `queue_id`.
    pub(crate) fn resolve(
        &self,
        queue_id: &str,
        origin_generation: u64,
    ) -> Option<Arc<dyn QueueConsumer>> {
        let active = self.get_by_id(queue_id);
        if let Some(ref rc) = active {
            if rc.generation == origin_generation {
                return Some(rc.consumer.clone());
            }
        }
        let detaching = self.detaching.lock();
        if let Some(rc) = detaching
            .iter()
            .find(|rc| rc.generation == origin_generation && rc.identifier() == queue_id)
        {
            return Some(rc.consumer.clone());
        }
        if let Some(rc) = active {
            return Some(rc.consumer.clone());
        }
        detaching
            .iter()
            .rev()
            .find(|rc| rc.identifier() == queue_id)
            .map(|rc| rc.consumer.clone())
    }

    /// Liveness of every active consumer.
    pub(crate) fn stats(&self) -> Vec<ConsumerStat> {
        self.maps
            .read()
            .by_name
            .values()
            .map(|rc| ConsumerStat {
                queue_name: rc.name.clone(),
                identifier: rc.identifier().to_string(),
                running: rc.poll_task_started.load(Ordering::SeqCst),
                last_poll: rc.last_poll(),
            })
            .collect()
    }
}

/// A consumer whose last heartbeat is older than `threshold`.
pub(crate) fn is_stale(last_poll: Instant, threshold: Duration) -> bool {
    last_poll.elapsed() >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fc_common::QueuedMessage;

    struct Named(&'static str);

    #[async_trait]
    impl QueueConsumer for Named {
        fn identifier(&self) -> &str {
            self.0
        }
        async fn poll(&self, _: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
            Ok(vec![])
        }
        async fn ack(&self, _: &str) -> fc_queue::Result<()> {
            Ok(())
        }
        async fn nack(&self, _: &str, _: Option<u32>) -> fc_queue::Result<()> {
            Ok(())
        }
        async fn extend_visibility(&self, _: &str, _: u32) -> fc_queue::Result<()> {
            Ok(())
        }
        fn is_healthy(&self) -> bool {
            true
        }
        async fn stop(&self) {}
    }

    fn rc(name: &str, id: &'static str, generation: u64) -> Arc<RunningConsumer> {
        Arc::new(RunningConsumer::new(
            Arc::new(Named(id)),
            name.to_string(),
            None,
            generation,
            CancellationToken::new(),
        ))
    }

    /// The origin instance wins while registered — active or detaching —
    /// then the active consumer for the queue, then a detaching one.
    #[test]
    fn resolve_prefers_origin_then_active_then_detaching() {
        let reg = ConsumerRegistry::default();
        let old = rc("q", "Q/id", 1);
        reg.insert(old.clone());
        let new = rc("q", "Q/id", 2);
        assert!(reg.replace_if_current(&old, new.clone()));
        reg.detach(old.clone());

        let for_old = reg.resolve("Q/id", 1).unwrap();
        assert!(Arc::ptr_eq(&for_old, &old.consumer));
        let for_new = reg.resolve("Q/id", 2).unwrap();
        assert!(Arc::ptr_eq(&for_new, &new.consumer));
        // An unknown generation (origin retired) goes to the active one.
        let other = reg.resolve("Q/id", 99).unwrap();
        assert!(Arc::ptr_eq(&other, &new.consumer));

        // With no active consumer, the detaching one still answers.
        reg.remove("q");
        let detached = reg.resolve("Q/id", 99).unwrap();
        assert!(Arc::ptr_eq(&detached, &old.consumer));
        assert!(reg.resolve("other", 1).is_none());
    }

    #[test]
    fn deferral_ledger_counts_only_outstanding() {
        let r = rc("q", "Q", 1);
        let now = Instant::now();
        r.note_deferral(now - Duration::from_secs(1));
        r.note_deferral(now + Duration::from_secs(5));
        r.note_deferral(now + Duration::from_secs(3));
        assert_eq!(r.deferrals_outstanding(now), 2);
        assert_eq!(r.earliest_deferral(), Some(now + Duration::from_secs(3)));
    }

    /// A replacement is only swapped in over the instance it was built to
    /// replace.
    #[test]
    fn replace_if_current_refuses_a_stale_old() {
        let reg = ConsumerRegistry::default();
        let a = rc("q", "Q", 1);
        let b = rc("q", "Q", 2);
        let c = rc("q", "Q", 3);
        reg.insert(a.clone());
        assert!(reg.replace_if_current(&a, b.clone()));
        assert!(!reg.replace_if_current(&a, c));
        assert_eq!(reg.get("q").unwrap().generation, 2);
        assert_eq!(reg.get_by_id("Q").unwrap().generation, 2);
    }
}
