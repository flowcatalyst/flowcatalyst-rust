//! The per-group FIFO buffers behind ordered delivery, and the drainer
//! bookkeeping that keeps exactly one drainer per group (Java
//! `OrderedGroups`, Go `groupQueue`).
//!
//! Each group is a strict FIFO plus a `draining` flag, behind one mutex:
//! "is this group being drained?" and "what is in it?" are one decision.
//! Answering them separately is how a group ends up with two drainers, or
//! with none while it still holds work. A group with nothing buffered and
//! no drainer is not kept: [`GroupQueues::poll_head`] removes it as the
//! drainer leaves, so the map holds only live groups.
//!
//! **Invariants.**
//! - A group is in the map iff it has buffered work or a drainer.
//! - `draining` is set iff exactly one drainer owns the group. Only
//!   [`GroupQueues::offer`] (on an idle group) and
//!   [`GroupQueues::claim_parked`] set it, each under the group's lock, so
//!   two callers are never both told to start a drainer.
//! - A drainer stops only when [`GroupQueues::poll_head`] returns `None`,
//!   and by then the group is gone from the map, so the next offer starts a
//!   fresh drainer. No offer can land between "empty" and "removed".
//!
//! Queue slots are the pool's business: a buffered task holds one, and the
//! caller releases it for each task it takes out, with one exception
//! ([`GroupQueues::re_front`] reserves the slot itself; see there).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use parking_lot::Mutex;

use super::{BufferedMessage, PoolTask, QueueSlotReleaser};

/// One group's buffer, and whether a drainer owns it.
///
/// The buffer is a single strict FIFO, as in Go's `groupQueue`. A message
/// group is an ordering contract, so there is no priority lane inside it:
/// letting a "high priority" message jump an earlier one in the same group
/// would defeat in-order delivery. A retried head is put back at the FRONT,
/// so it is the next message attempted.
struct Group {
    msgs: VecDeque<PoolTask>,
    draining: bool,
    /// When this group was last left with no drainer (Go's
    /// `groupQueue.parkedAt`): `None` while `draining`, or if it has never
    /// been idle since it was created. A `parked_at` that keeps ageing is
    /// the operator "blocked groups" signature: nothing has come back to
    /// resume the group.
    parked_at: Option<Instant>,
}

impl Group {
    fn new() -> Self {
        Self {
            msgs: VecDeque::new(),
            draining: false,
            parked_at: None,
        }
    }

    /// The single mutator of `draining`, so `parked_at` can never drift out
    /// of sync with it (the R-04 "blocked groups" view's `parkedAt` /
    /// `working` pair).
    fn set_draining(&mut self, draining: bool) {
        self.draining = draining;
        self.parked_at = if draining { None } else { Some(Instant::now()) };
    }
}

/// One group's state, for the operator "blocked groups" view.
pub(super) struct GroupState {
    pub group: Arc<str>,
    /// Messages in the FIFO, not counting the one a drainer holds (already
    /// taken by `poll_head`).
    pub buffered: usize,
    pub draining: bool,
    pub parked_at: Option<Instant>,
}

/// Every ordered group a pool holds (see the module docs).
pub(super) struct GroupQueues {
    groups: DashMap<Arc<str>, Mutex<Group>>,
}

impl GroupQueues {
    pub(super) fn new() -> Self {
        Self {
            groups: DashMap::new(),
        }
    }

    /// Append `task` to the back of `group`.
    ///
    /// Returns whether the caller must start a drainer: true only when this
    /// task found the group idle. Claiming the group and appending happen
    /// under one lock, so two concurrent offers are never both told to.
    pub(super) fn offer(&self, group: Arc<str>, task: PoolTask) -> bool {
        let entry = self
            .groups
            .entry(group)
            .or_insert_with(|| Mutex::new(Group::new()));
        let mut group = entry.lock();
        group.msgs.push_back(task);
        if group.draining {
            false
        } else {
            group.set_draining(true);
            true
        }
    }

    /// The drainer's next message for `group`, or `None` when it is done.
    ///
    /// `None` also releases the group: it is removed from the map, so the
    /// drainer must exit and the next offer starts a fresh one. The removal
    /// re-checks emptiness under the map's shard lock, which [`Self::offer`]
    /// also takes, so a task offered after the buffer was seen empty is
    /// either taken here (another round) or finds the group gone and starts
    /// its own drainer. It is never left in a group nobody drains.
    pub(super) fn poll_head(&self, group: &str) -> Option<PoolTask> {
        loop {
            if let Some(task) = self.groups.get(group)?.lock().msgs.pop_front() {
                return Some(task);
            }
            if self
                .groups
                .remove_if(group, |_, g| g.lock().msgs.is_empty())
                .is_some()
            {
                return None;
            }
            // An offer landed between the two: take it.
        }
    }

    /// Put `task` back at the FRONT of `group`, the next message attempted,
    /// for an in-place retry. It holds a queue slot again, reserved on
    /// `slots` before the task becomes visible, so a concurrent
    /// [`Self::take_buffered`] that releases it never sees the counter
    /// before the reservation.
    ///
    /// Returns `task` (homeless) if the group is gone, which cannot happen
    /// while the caller is its drainer (only the drainer removes its group).
    pub(super) fn re_front(
        &self,
        group: &str,
        task: PoolTask,
        slots: &QueueSlotReleaser,
    ) -> Option<PoolTask> {
        match self.groups.get(group) {
            Some(entry) => {
                slots.reserve();
                entry.lock().msgs.push_front(task);
                None
            }
            None => Some(task),
        }
    }

    /// Empty `group`'s buffer, returning it in FIFO order (Go
    /// `takeBuffered`). The group and its drainer are left alone: the
    /// drainer finds the buffer empty and leaves. The caller owns the queue
    /// slots the tasks held.
    pub(super) fn take_buffered(&self, group: &str) -> Vec<PoolTask> {
        match self.groups.get(group) {
            Some(entry) => entry.lock().msgs.drain(..).collect(),
            None => Vec::new(),
        }
    }

    /// A drainer is dying (a panic, or the semaphore closed): empty the
    /// buffer, let go of the group, and remove it unless an offer has
    /// already claimed it again. Returns what was buffered, whose queue
    /// slots the caller owns.
    pub(super) fn abandon(&self, group: &str) -> Vec<PoolTask> {
        let abandoned = match self.groups.get(group) {
            Some(entry) => {
                let mut g = entry.lock();
                let abandoned = g.msgs.drain(..).collect();
                if g.draining {
                    g.set_draining(false);
                }
                abandoned
            }
            None => Vec::new(),
        };
        // Leave no emptied, idle group behind: the blocked-groups view would
        // show it as parked until the group's next message.
        self.groups.remove_if(group, |_, g| {
            let g = g.lock();
            g.msgs.is_empty() && !g.draining
        });
        abandoned
    }

    /// Claim every parked group (buffered work, no drainer) for a new
    /// drainer, for the lifecycle sweep. Each claim is made under the
    /// group's lock, as [`Self::offer`]'s is, so a sweep racing a resuming
    /// offer starts exactly one drainer between them.
    pub(super) fn claim_parked(&self) -> Vec<Arc<str>> {
        self.groups
            .iter()
            .filter_map(|entry| {
                let mut g = entry.value().lock();
                if !g.draining && !g.msgs.is_empty() {
                    g.set_draining(true);
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// The groups held now.
    pub(super) fn ids(&self) -> Vec<Arc<str>> {
        self.groups
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// How many groups are held now.
    pub(super) fn len(&self) -> usize {
        self.groups.len()
    }

    /// Every group's state, each copied under its own lock.
    pub(super) fn snapshot(&self) -> Vec<GroupState> {
        self.groups
            .iter()
            .map(|entry| {
                let g = entry.value().lock();
                GroupState {
                    group: entry.key().clone(),
                    buffered: g.msgs.len(),
                    draining: g.draining,
                    parked_at: g.parked_at,
                }
            })
            .collect()
    }

    /// Where `message_id` is buffered, if it is (scans every group: an
    /// operator lookup, not a hot path).
    pub(super) fn find(&self, pool_code: &str, message_id: &str) -> Option<BufferedMessage> {
        for entry in self.groups.iter() {
            let g = entry.value().lock();
            if let Some(position) = g.msgs.iter().position(|t| t.message.id == message_id) {
                return Some(BufferedMessage {
                    pool_code: pool_code.to_string(),
                    group: entry.key().to_string(),
                    position,
                    depth: g.msgs.len(),
                    attempts: g.msgs[position].attempts,
                    drainer_running: g.draining,
                });
            }
        }
        None
    }

    /// `group`'s buffered messages, head first, with their in-place attempt
    /// counts; `None` when the group is not held.
    pub(super) fn buffer(&self, group: &str) -> Option<Vec<(String, u32)>> {
        let entry = self.groups.get(group)?;
        let g = entry.value().lock();
        Some(
            g.msgs
                .iter()
                .map(|t| (t.message.id.clone(), t.attempts))
                .collect(),
        )
    }

    /// Let go of `group` without touching its buffer (Java
    /// `releaseDrainer`): the state a drainer that stopped with work left,
    /// or a lost wake-up, leaves behind. Removes the group if it is empty.
    /// Only tests need it: no drainer stops that way.
    #[cfg(test)]
    pub(super) fn release_drainer(&self, group: &str) -> bool {
        let still_holds_work = match self.groups.get(group) {
            Some(entry) => {
                let mut g = entry.lock();
                if !g.msgs.is_empty() {
                    g.set_draining(false);
                }
                !g.msgs.is_empty()
            }
            None => return false,
        };
        if !still_holds_work {
            self.groups
                .remove_if(group, |_, g| g.lock().msgs.is_empty());
        }
        still_holds_work
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_common::{DispatchMode, MediationType, Message, MessageCallback};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use tokio::sync::Notify;

    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    struct Quiet;

    #[async_trait::async_trait]
    impl MessageCallback for Quiet {
        async fn ack(&self) {}
        async fn nack(&self, _: Option<u32>) {}
    }

    fn task(id: &str, group: &str) -> PoolTask {
        PoolTask {
            message: Message {
                id: id.to_string(),
                pool_code: "P".to_string(),
                auth_token: None,
                signing_secret: None,
                mediation_type: MediationType::HTTP,
                mediation_target: "http://example.invalid".to_string(),
                message_group_id: Some(group.to_string()),
                high_priority: false,
                dispatch_mode: DispatchMode::BlockOnError,
                dispatch_mode_specified: true,
            },
            receipt_handle: format!("rh-{id}"),
            callback: Box::new(Quiet),
            batch_id: None,
            attempts: 0,
            queue_identifier: "q".to_string(),
            pending_retry: None,
        }
    }

    fn slots() -> QueueSlotReleaser {
        QueueSlotReleaser {
            queue_size: Arc::new(AtomicU32::new(0)),
            capacity: Arc::new(AtomicU32::new(1000)),
            notify: Arc::new(Notify::new()),
        }
    }

    fn id(task: Option<PoolTask>) -> Option<String> {
        task.map(|t| t.message.id)
    }

    #[test]
    fn fifo_within_a_group() {
        let groups = GroupQueues::new();
        for n in ["a", "b", "c"] {
            groups.offer(Arc::from("g"), task(n, "g"));
        }
        assert_eq!(id(groups.poll_head("g")).as_deref(), Some("a"));
        assert_eq!(id(groups.poll_head("g")).as_deref(), Some("b"));
        assert_eq!(id(groups.poll_head("g")).as_deref(), Some("c"));
        assert!(groups.poll_head("g").is_none());
    }

    #[test]
    fn only_the_offer_that_finds_the_group_idle_starts_a_drainer() {
        let groups = GroupQueues::new();
        assert!(groups.offer(Arc::from("g"), task("a", "g")));
        assert!(!groups.offer(Arc::from("g"), task("b", "g")));
        assert!(
            groups.offer(Arc::from("h"), task("c", "h")),
            "groups are independent"
        );
    }

    #[test]
    fn emptying_releases_the_group() {
        let groups = GroupQueues::new();
        groups.offer(Arc::from("g"), task("a", "g"));
        assert!(groups.poll_head("g").is_some());
        assert_eq!(groups.len(), 1, "held while its drainer works");
        assert!(groups.poll_head("g").is_none());
        assert_eq!(groups.len(), 0, "gone once its drainer is done");
        assert!(
            groups.offer(Arc::from("g"), task("b", "g")),
            "the next offer starts a fresh drainer"
        );
    }

    #[test]
    fn re_front_is_the_next_head_and_reserves_its_slot() {
        let groups = GroupQueues::new();
        let slots = slots();
        groups.offer(Arc::from("g"), task("a", "g"));
        groups.offer(Arc::from("g"), task("b", "g"));
        let head = groups.poll_head("g").unwrap();
        assert!(groups.re_front("g", head, &slots).is_none());
        assert_eq!(slots.queue_size.load(Ordering::Relaxed), 1);
        assert_eq!(id(groups.poll_head("g")).as_deref(), Some("a"));
        assert_eq!(id(groups.poll_head("g")).as_deref(), Some("b"));
    }

    #[test]
    fn re_front_into_a_missing_group_gives_the_task_back() {
        let groups = GroupQueues::new();
        let slots = slots();
        assert!(groups.re_front("g", task("a", "g"), &slots).is_some());
        assert_eq!(slots.queue_size.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn take_buffered_empties_the_buffer_but_keeps_the_drainer() {
        let groups = GroupQueues::new();
        for n in ["a", "b", "c"] {
            groups.offer(Arc::from("g"), task(n, "g"));
        }
        let _head = groups.poll_head("g").unwrap();
        let taken: Vec<_> = groups
            .take_buffered("g")
            .into_iter()
            .map(|t| t.message.id)
            .collect();
        assert_eq!(taken, ["b", "c"]);
        assert!(
            !groups.offer(Arc::from("g"), task("d", "g")),
            "the drainer still owns the group"
        );
    }

    #[test]
    fn abandon_empties_and_removes_the_group() {
        let groups = GroupQueues::new();
        groups.offer(Arc::from("g"), task("a", "g"));
        groups.offer(Arc::from("g"), task("b", "g"));
        let _head = groups.poll_head("g").unwrap();
        assert_eq!(groups.abandon("g").len(), 1);
        assert_eq!(groups.len(), 0);
        assert!(groups.offer(Arc::from("g"), task("c", "g")));
    }

    #[test]
    fn the_sweep_claims_only_parked_groups() {
        let groups = GroupQueues::new();
        groups.offer(Arc::from("working"), task("a", "working"));
        groups.offer(Arc::from("parked"), task("b", "parked"));
        assert!(groups.release_drainer("parked"));
        let parked = groups
            .snapshot()
            .into_iter()
            .find(|s| &*s.group == "parked");
        assert!(parked.is_some_and(|s| !s.draining && s.parked_at.is_some()));

        assert_eq!(groups.claim_parked(), [Arc::<str>::from("parked")]);
        assert!(groups.claim_parked().is_empty(), "claimed once");
        assert!(
            !groups.offer(Arc::from("parked"), task("c", "parked")),
            "the sweep's drainer owns it"
        );
    }

    /// Java `OrderedGroupsTest.concurrentSubmitsElectOneDrainer`.
    #[test]
    fn concurrent_offers_elect_one_drainer() {
        let groups = Arc::new(GroupQueues::new());
        let elected = Arc::new(AtomicUsize::new(0));
        thread::scope(|s| {
            for n in 0..64 {
                let (groups, elected) = (&groups, &elected);
                s.spawn(move || {
                    if groups.offer(Arc::from("g"), task(&format!("m{n}"), "g")) {
                        elected.fetch_add(1, Ordering::SeqCst);
                    }
                });
            }
        });
        assert_eq!(elected.load(Ordering::SeqCst), 1);
        assert_eq!(groups.buffer("g").map(|b| b.len()), Some(64));
    }

    /// Offers, drainers, re-fronted retries, lost wake-ups and the parked
    /// sweep all race on a handful of groups. Every task must be taken
    /// exactly once, no group may ever have two drainers, and nothing may be
    /// left behind (Java `OrderedGroupsTest`: a sweep racing a resume never
    /// loses or duplicates).
    ///
    /// - Producers offer tasks and start a drainer whenever `offer` says so.
    /// - A drainer takes heads one at a time. Sometimes it re-fronts the head
    ///   (an in-place retry) and must get the same task back next. Sometimes
    ///   it re-fronts the head and then lets go of the group, as a lost
    ///   wake-up would, leaving the group parked for a resuming offer or the
    ///   sweep to claim, and for the sweep alone once the producers stop.
    /// - The sweep claims parked groups continuously.
    ///
    /// A second drainer on a group shows as two drainers holding one of its
    /// heads at once, or as a task taken twice.
    #[test]
    fn a_sweep_racing_a_resume_never_loses_or_duplicates() {
        const GROUPS: usize = 4;
        const PRODUCERS: usize = 4;
        const PER_PRODUCER: usize = 2_000;
        const TOTAL: usize = PRODUCERS * PER_PRODUCER;
        const DRAINERS: usize = 6;

        for seed in 0..5u64 {
            let groups = GroupQueues::new();
            let slots = slots();
            let taken: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
            let taken_count = AtomicUsize::new(0);
            let holding: Vec<AtomicUsize> = (0..GROUPS).map(|_| AtomicUsize::new(0)).collect();
            let double_drainers = AtomicUsize::new(0);
            let parked_by_drainers = AtomicUsize::new(0);
            let swept = AtomicUsize::new(0);
            let done = AtomicBool::new(false);
            let (start, starts) = mpsc::channel::<Arc<str>>();
            let starts = Mutex::new(starts);
            let group_index = |g: &str| g[1..].parse::<usize>().unwrap();

            let drain = |group: Arc<str>, rng: &mut StdRng| {
                while let Some(head) = groups.poll_head(&group) {
                    let held = &holding[group_index(&group)];
                    if held.fetch_add(1, Ordering::SeqCst) != 0 {
                        double_drainers.fetch_add(1, Ordering::SeqCst);
                    }
                    let roll = rng.random_range(0..100);
                    if roll < 10 {
                        // An in-place retry: the head comes straight back.
                        let head_id = head.message.id.clone();
                        held.fetch_sub(1, Ordering::SeqCst);
                        assert!(groups.re_front(&group, head, &slots).is_none());
                        let again = groups.poll_head(&group).expect("re-fronted");
                        assert_eq!(again.message.id, head_id, "a re-fronted head is next");
                        assert!(groups.re_front(&group, again, &slots).is_none());
                        continue;
                    }
                    if roll < 13 {
                        // A lost wake-up: the head goes back and the
                        // group is left parked.
                        held.fetch_sub(1, Ordering::SeqCst);
                        assert!(groups.re_front(&group, head, &slots).is_none());
                        groups.release_drainer(&group);
                        parked_by_drainers.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                    let previous = taken.lock().insert(head.message.id.clone(), 1);
                    assert!(previous.is_none(), "{} taken twice", head.message.id);
                    taken_count.fetch_add(1, Ordering::SeqCst);
                    held.fetch_sub(1, Ordering::SeqCst);
                }
            };

            thread::scope(|s| {
                for d in 0..DRAINERS {
                    let (starts, drain, done) = (&starts, &drain, &done);
                    s.spawn(move || {
                        let mut rng = StdRng::seed_from_u64(seed * 1_000 + d as u64);
                        loop {
                            let next = starts.lock().recv_timeout(Duration::from_millis(5));
                            match next {
                                Ok(group) => drain(group, &mut rng),
                                Err(_) if done.load(Ordering::SeqCst) => return,
                                Err(_) => {}
                            }
                        }
                    });
                }

                {
                    let (groups, start, swept, done) = (&groups, start.clone(), &swept, &done);
                    s.spawn(move || {
                        while !done.load(Ordering::SeqCst) {
                            for group in groups.claim_parked() {
                                swept.fetch_add(1, Ordering::SeqCst);
                                start.send(group).unwrap();
                            }
                            thread::yield_now();
                        }
                    });
                }

                let producers: Vec<_> = (0..PRODUCERS)
                    .map(|p| {
                        let (groups, start) = (&groups, start.clone());
                        s.spawn(move || {
                            let mut rng = StdRng::seed_from_u64(seed * 1_000 + 100 + p as u64);
                            for n in 0..PER_PRODUCER {
                                let group = format!("g{}", rng.random_range(0..GROUPS));
                                let group: Arc<str> = Arc::from(group);
                                let t = task(&format!("p{p}-{n}"), &group);
                                if groups.offer(group.clone(), t) {
                                    start.send(group).unwrap();
                                }
                                if n % 64 == 0 {
                                    thread::yield_now();
                                }
                            }
                        })
                    })
                    .collect();
                for producer in producers {
                    producer.join().unwrap();
                }

                // Once the producers stop, only the sweep resumes parked
                // groups, so every task left is taken through it.
                let deadline = Instant::now() + Duration::from_secs(30);
                while taken_count.load(Ordering::SeqCst) < TOTAL && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(1));
                }
                while groups.len() > 0 && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(1));
                }
                done.store(true, Ordering::SeqCst);
            });

            assert_eq!(
                taken_count.load(Ordering::SeqCst),
                TOTAL,
                "seed {seed}: none lost"
            );
            assert_eq!(taken.lock().len(), TOTAL, "seed {seed}: none duplicated");
            assert_eq!(
                double_drainers.load(Ordering::SeqCst),
                0,
                "seed {seed}: one drainer per group"
            );
            assert_eq!(groups.len(), 0, "seed {seed}: nothing left behind");
            assert!(
                parked_by_drainers.load(Ordering::SeqCst) > 0,
                "seed {seed}: the race was exercised"
            );
            assert!(
                swept.load(Ordering::SeqCst) > 0,
                "seed {seed}: the sweep claimed groups"
            );
        }
    }
}
