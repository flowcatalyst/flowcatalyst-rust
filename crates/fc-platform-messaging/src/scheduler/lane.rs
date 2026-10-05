//! Dispatcher lanes, and the state they share with the poller.
//!
//! The poller claims queue rows (`claimed_at` is stamped, so a claimed row is
//! not claimed again) and hands each to a lane; a lane publishes what it is
//! given and marks it QUEUED, in bulk, with no transaction and no row lock
//! held anywhere. The poller never waits for a publish. Three things bound
//! and order the work:
//!
//! - **Permits** ([`Pipeline`]): one per job from claim to the end of its lane
//!   batch. The poller blocks when none are free, so a slow broker holds back
//!   the claim instead of growing a buffer.
//! - **The in-flight set**: ids claimed and not yet finished by a lane. It is
//!   no longer passed to the claim (the claim stamp does that job); it is how
//!   the poller recognises a doomed job (below) and what a stale-claim
//!   release must not touch.
//! - **Generations and poison** (below): the per-group order under failure.
//!
//! # Ordering under failure
//!
//! A group always goes to one lane and a lane publishes in claim order, so
//! the order holds while everything succeeds. When a job `j` of group `g` is
//! not published, later jobs of `g` that are already claimed (in the lane's
//! channel, or in a claim the poller is running right now) must not be
//! published ahead of `j`:
//!
//! - every claim takes a generation (an increment of a shared counter)
//!   **before** it snapshots the in-flight set, and stamps its jobs with it;
//! - a lane that leaves jobs of `g` unpublished RELEASES their claims (so
//!   they are claimed again), removes the batch's ids from the in-flight set
//!   and **then** reads the counter, `P`, and records `poison[g] = P`;
//! - a job of `g` with `generation <= poison[g]` is dropped when it reaches
//!   the lane (its claim is released and it stays PENDING; it is claimed
//!   again later);
//! - a claim whose generation is `> P` incremented the counter after `P` was
//!   read, hence after the release committed, so its statement sees `j`
//!   unclaimed: it claims `j` again, in order. Its jobs pass, and the first
//!   one clears the poison. A claim whose statement started before the
//!   release took its generation before it, so its generation is `<= P` and
//!   its jobs of `g` are dropped.
//! - a claim that returns a job still in the in-flight set (the release ran,
//!   the removal has not yet) withholds that job and the rest of its group,
//!   and releases them.
//! - a release that FAILS leaves the ids in the in-flight set, poisons the
//!   groups, and is retried by the poller before its next claim; until it
//!   succeeds the group is held back by the "doomed" check below.
//!
//! # The claim must not skip a doomed job
//!
//! A job of `g` still waiting in the lane when `g` is poisoned is doomed (it
//! will be dropped), yet it is claimed, so a later claim skips it and, if
//! that claim is newer than the poison, takes the job *behind* it, which
//! would then be published ahead of the doomed one. The
//! poller therefore checks every claim against the in-flight set it
//! snapshotted: if the snapshot holds a job of `g` that is doomed (its
//! generation is `<=` the group's poison), the claim's jobs of `g` are not
//! submitted; their claims are released and they are claimed again, in
//! order, once the doomed jobs have been dropped.
//!
//! (Making a *drop* poison the group again, at the generation read after the
//! drop, also closes the hole, but it livelocks whenever the poller claims
//! faster than a lane drains: every claim made while a batch is being dropped
//! is older than that batch's poison, so it is dropped in turn, without end.)

use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::sync::atomic::AtomicU32;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
#[cfg(test)]
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, Semaphore};
use tokio::time::{self, Instant};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::dispatcher::{DispatchJobToken, MessageGroupDispatcher};
use super::poller::{JobStore, MarkKey};

/// How long a group stays poisoned without being seen again before its entry
/// is forgotten.
const POISON_TTL: Duration = Duration::from_secs(10 * 60);

/// The longest a lane's status update may take, including one that starts
/// after shutdown was requested.
const MARK_TIMEOUT: Duration = Duration::from_secs(10);

/// A claimed row on its way to a lane.
#[derive(Debug, Clone)]
pub(crate) struct ClaimedJob {
    pub token: DispatchJobToken,
    pub created_at: DateTime<Utc>,
    /// The row's `updated_at` as the claim read it: the version the QUEUED
    /// update must still find.
    pub updated_at: DateTime<Utc>,
}

impl ClaimedJob {
    pub fn id(&self) -> &str {
        &self.token.job_id
    }

    /// The message group; an empty one is no group (nothing orders it).
    pub fn group(&self) -> Option<&str> {
        self.token
            .message_group
            .as_deref()
            .filter(|g| !g.is_empty())
    }
}

/// A [`ClaimedJob`] stamped with the generation of the claim that took it.
#[derive(Debug, Clone)]
pub(crate) struct LaneJob {
    pub job: ClaimedJob,
    pub generation: u64,
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct InFlight {
    group: Option<String>,
    generation: u64,
}

struct Poison {
    generation: u64,
    set_at: Instant,
}

#[derive(Default)]
struct State {
    in_flight: HashMap<String, InFlight>,
    poison: HashMap<String, Poison>,
    /// Claims whose release failed, to retry: `id -> whether the id is in the
    /// in-flight set and leaves it once its claim is released`.
    unreleased: HashMap<String, bool>,
}

/// The in-flight set as a claim saw it (what its doomed check needs).
pub(crate) struct Snapshot {
    /// Per group, the oldest generation among its in-flight jobs.
    min_generation: HashMap<String, u64>,
}

/// What the poller and the lanes share. See the module docs.
pub(crate) struct Pipeline {
    capacity: usize,
    generation: AtomicU64,
    state: Mutex<State>,
    permits: Semaphore,
    failed: AtomicBool,
    /// Test only: the longest pause [`Self::race_point`] takes, in us.
    #[cfg(test)]
    race_pause_us: AtomicU32,
    #[cfg(test)]
    race_seq: AtomicU64,
}

impl Pipeline {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            generation: AtomicU64::new(0),
            state: Mutex::new(State::default()),
            permits: Semaphore::new(capacity),
            failed: AtomicBool::new(false),
            #[cfg(test)]
            race_pause_us: AtomicU32::new(0),
            #[cfg(test)]
            race_seq: AtomicU64::new(0),
        }
    }

    /// A point where two threads can interleave in the steps the ordering
    /// rule depends on (between the generation and the snapshot; around the
    /// removal of a batch's ids). Does nothing outside tests; the stress test
    /// widens these windows so a wrong order is reachable.
    #[inline]
    pub fn race_point(&self) {
        #[cfg(test)]
        {
            let max = self.race_pause_us.load(Ordering::Relaxed);
            if max > 0 {
                let mut z = self
                    .race_seq
                    .fetch_add(1, Ordering::Relaxed)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15);
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                let us = (z >> 33) % u64::from(max + 1);
                thread::sleep(Duration::from_micros(us));
            }
        }
    }

    #[cfg(test)]
    pub fn set_race_pause(&self, max_us: u32) {
        self.race_pause_us.store(max_us, Ordering::Relaxed);
    }

    /// Take the next claim generation. Called before [`Self::snapshot_in_flight`].
    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn snapshot_in_flight(&self) -> Snapshot {
        let state = locked(&self.state);
        let mut min_generation: HashMap<String, u64> = HashMap::new();
        for entry in state.in_flight.values() {
            if let Some(group) = &entry.group {
                let slot = min_generation.entry(group.clone()).or_insert(u64::MAX);
                *slot = (*slot).min(entry.generation);
            }
        }
        Snapshot { min_generation }
    }

    /// `false` when the id was already in flight.
    pub fn add_in_flight(&self, job: &ClaimedJob, generation: u64) -> bool {
        let mut state = locked(&self.state);
        let entry = InFlight {
            group: job.group().map(str::to_string),
            generation,
        };
        let added = state
            .in_flight
            .insert(job.id().to_string(), entry)
            .is_none();
        metrics::gauge!("scheduler.in_flight.size").set(state.in_flight.len() as f64);
        added
    }

    pub fn remove_in_flight<'a>(&self, ids: impl IntoIterator<Item = &'a str>) {
        let mut state = locked(&self.state);
        for id in ids {
            state.in_flight.remove(id);
        }
        metrics::gauge!("scheduler.in_flight.size").set(state.in_flight.len() as f64);
    }

    /// Every id this process holds a claim on that it knows of: the in-flight
    /// set and the claims whose release is being retried. A stale-claim
    /// release must leave exactly these alone.
    pub fn held_ids(&self) -> Vec<String> {
        let state = locked(&self.state);
        let mut ids: Vec<String> = state.in_flight.keys().cloned().collect();
        ids.extend(
            state
                .unreleased
                .keys()
                .filter(|id| !state.in_flight.contains_key(*id))
                .cloned(),
        );
        ids
    }

    /// Remember claims whose release failed (`true`: the id is in the
    /// in-flight set and is removed from it once released).
    pub fn defer_release(&self, ids: impl IntoIterator<Item = (String, bool)>) {
        let mut state = locked(&self.state);
        for (id, owned) in ids {
            state.unreleased.insert(id, owned);
        }
    }

    /// The claims to retry releasing, removed from the list (a failed retry
    /// puts them back with [`Self::defer_release`]).
    pub fn take_unreleased(&self) -> Vec<(String, bool)> {
        locked(&self.state).unreleased.drain().collect()
    }

    #[cfg(test)]
    pub fn unreleased_len(&self) -> usize {
        locked(&self.state).unreleased.len()
    }

    #[cfg(test)]
    pub fn in_flight_len(&self) -> usize {
        locked(&self.state).in_flight.len()
    }

    /// Whether a claim that took `snapshot` must not submit its jobs of
    /// `group`: the snapshot held a job of the group that is doomed, so the
    /// claim skipped it.
    pub fn skipped_a_doomed_job(&self, snapshot: &Snapshot, group: &str) -> bool {
        let state = locked(&self.state);
        match (state.poison.get(group), snapshot.min_generation.get(group)) {
            (Some(p), Some(min)) => *min <= p.generation,
            _ => false,
        }
    }

    /// A job of `group` with `generation` reaches the lane: `false` when it is
    /// poisoned and must be dropped. The first job newer than the poison
    /// clears it.
    pub fn admit(&self, group: &str, generation: u64) -> bool {
        let mut state = locked(&self.state);
        match state.poison.get(group) {
            Some(p) if generation <= p.generation => false,
            Some(_) => {
                state.poison.remove(group);
                true
            }
            None => true,
        }
    }

    pub fn poison(&self, groups: impl IntoIterator<Item = String>, generation: u64) {
        let mut state = locked(&self.state);
        let set_at = Instant::now();
        for group in groups {
            state.poison.insert(group, Poison { generation, set_at });
        }
    }

    pub fn evict_expired_poison(&self) {
        locked(&self.state)
            .poison
            .retain(|_, p| p.set_at.elapsed() < POISON_TTL);
    }

    /// Block for one permit, then take as many more as are free, up to `max`
    /// in all. Only the poller acquires, so permits seen free stay free.
    pub async fn acquire_up_to(&self, max: usize) -> usize {
        match self.permits.acquire().await {
            Ok(p) => p.forget(),
            // The semaphore is never closed.
            Err(_) => return 0,
        }
        let extra = self.permits.available_permits().min(max.saturating_sub(1));
        if extra > 0 {
            if let Ok(p) = self.permits.try_acquire_many(extra as u32) {
                p.forget();
                return extra + 1;
            }
        }
        1
    }

    pub fn release(&self, n: usize) {
        if n > 0 {
            self.permits.add_permits(n);
        }
        metrics::gauge!("scheduler.buffer.in_use").set(self.in_use() as f64);
    }

    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }

    pub fn in_use(&self) -> usize {
        self.capacity.saturating_sub(self.available())
    }

    /// A lane could not publish or mark; the poller backs off once.
    pub fn note_failure(&self) {
        self.failed.store(true, Ordering::SeqCst);
    }

    pub fn take_failure(&self) -> bool {
        self.failed.swap(false, Ordering::SeqCst)
    }
}

/// What one lane batch did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LaneReport {
    pub published: usize,
    pub unpublished: usize,
    pub dropped: usize,
}

/// One dispatcher: publishes the jobs it is handed and marks them QUEUED.
pub(crate) struct Lane {
    index: usize,
    pipeline: Arc<Pipeline>,
    store: Arc<dyn JobStore>,
    dispatcher: Arc<MessageGroupDispatcher>,
    /// How many jobs a batch takes beyond its first.
    batch_extra: usize,
}

impl Lane {
    pub fn new(
        index: usize,
        pipeline: Arc<Pipeline>,
        store: Arc<dyn JobStore>,
        dispatcher: Arc<MessageGroupDispatcher>,
        batch_extra: usize,
    ) -> Self {
        Self {
            index,
            pipeline,
            store,
            dispatcher,
            batch_extra,
        }
    }

    /// Receive, batch and process until `cancel` fires or the channel closes.
    /// A batch already taken is finished (its publish is bounded by the
    /// dispatcher's timeout, its update by [`MARK_TIMEOUT`]); anything still
    /// buffered is left PENDING.
    pub async fn run(mut self, mut rx: mpsc::Receiver<LaneJob>, cancel: CancellationToken) {
        loop {
            let first = tokio::select! {
                biased;
                () = cancel.cancelled() => break,
                job = rx.recv() => match job {
                    Some(job) => job,
                    None => break,
                },
            };
            let mut batch = vec![first];
            while batch.len() <= self.batch_extra {
                match rx.try_recv() {
                    Ok(job) => batch.push(job),
                    Err(_) => break,
                }
            }
            self.process(batch).await;
        }
    }

    /// Publish one batch and finish it: drop what is poisoned, publish the
    /// rest, mark the published ids QUEUED, release the claims of every job
    /// that is not QUEUED, then remove every id from the in-flight set,
    /// record the poison, and release the permits, in that order (see the
    /// module docs for why the order matters).
    pub async fn process(&mut self, batch: Vec<LaneJob>) -> LaneReport {
        if batch.is_empty() {
            return LaneReport::default();
        }
        self.pipeline.evict_expired_poison();
        let ids: Vec<String> = batch.iter().map(|j| j.job.id().to_string()).collect();

        // Groups with a job dropped in this batch: a later job of the group
        // must not overtake it. Groups whose publish failed are poisoned below.
        let mut dropped_groups: HashSet<String> = HashSet::new();
        let mut failed_groups: HashSet<String> = HashSet::new();
        let mut to_publish: Vec<ClaimedJob> = Vec::with_capacity(batch.len());
        let mut dropped = 0usize;
        let mut group_of: HashMap<String, String> = HashMap::new();
        for LaneJob { job, generation } in batch {
            if let Some(group) = job.group() {
                group_of.insert(job.id().to_string(), group.to_string());
                if dropped_groups.contains(group) {
                    dropped += 1;
                    continue;
                }
                if !self.pipeline.admit(group, generation) {
                    // Later jobs of the group in this batch follow it. The
                    // poison already stands; a drop does not renew it.
                    dropped_groups.insert(group.to_string());
                    dropped += 1;
                    continue;
                }
            }
            to_publish.push(job);
        }
        if dropped > 0 {
            metrics::counter!("scheduler.jobs.dropped_poisoned_total").increment(dropped as u64);
        }

        let mut failed = false;
        let mut to_mark: Vec<MarkKey> = Vec::with_capacity(to_publish.len());
        if !to_publish.is_empty() {
            let tokens: Vec<DispatchJobToken> =
                to_publish.iter().map(|j| j.token.clone()).collect();
            let started = Instant::now();
            let outcome = self.dispatcher.publish_claim(&tokens).await;
            metrics::histogram!("scheduler.lane.publish.duration_seconds",
                "lane" => self.index.to_string())
            .record(started.elapsed());
            let unpublished: HashSet<&str> =
                outcome.unpublished.iter().map(String::as_str).collect();
            for job in &to_publish {
                if unpublished.contains(job.id()) {
                    failed = true;
                    if let Some(group) = job.group() {
                        failed_groups.insert(group.to_string());
                    }
                } else {
                    to_mark.push((job.id().to_string(), job.created_at, job.updated_at));
                }
            }
        }
        let published = to_mark.len();
        let unpublished = to_publish.len() - published;

        let mut marked = false;
        if !to_mark.is_empty() {
            // A fresh deadline, not the lane's cancellation: a shutdown must
            // not abandon the update of jobs the broker already has.
            match time::timeout(MARK_TIMEOUT, self.store.mark_queued(&to_mark)).await {
                Ok(Ok(updated)) => {
                    marked = true;
                    let skipped = published.saturating_sub(updated as usize);
                    if skipped > 0 {
                        // The router delivered and the callback moved the job
                        // on first; it is not regressed to QUEUED.
                        metrics::counter!("scheduler.queued_mark.skipped_total")
                            .increment(skipped as u64);
                    }
                }
                Ok(Err(e)) => {
                    warn!(lane = self.index, published, error = %e,
                        "marking published dispatch jobs QUEUED failed; they will be published again");
                    failed = true;
                }
                Err(_) => {
                    warn!(lane = self.index, published,
                        "marking published dispatch jobs QUEUED timed out; they will be published again");
                    failed = true;
                }
            }
        }

        // Every job of the batch that is not now QUEUED (not published,
        // dropped as poisoned, published but not marked) gets its claim
        // released, so that it is claimed again, in order. Then the ids leave
        // the in-flight set, THEN the generation is read, THEN the permits go
        // back: see the module docs for why the order matters.
        let marked_ids: HashSet<&str> = if marked {
            to_mark.iter().map(|(id, _, _)| id.as_str()).collect()
        } else {
            HashSet::new()
        };
        let to_release: Vec<String> = ids
            .iter()
            .filter(|id| !marked_ids.contains(id.as_str()))
            .cloned()
            .collect();
        let mut release_failed = false;
        if !to_release.is_empty() {
            match time::timeout(MARK_TIMEOUT, self.store.release_claims(&to_release)).await {
                Ok(Ok(released)) => {
                    metrics::counter!("scheduler.claims.released_total").increment(released);
                }
                Ok(Err(e)) => {
                    warn!(lane = self.index, jobs = to_release.len(), error = %e,
                        "releasing dispatch claims failed; retrying");
                    release_failed = true;
                }
                Err(_) => {
                    warn!(
                        lane = self.index,
                        jobs = to_release.len(),
                        "releasing dispatch claims timed out; retrying"
                    );
                    release_failed = true;
                }
            }
        }
        self.pipeline.race_point();
        if release_failed {
            // Still claimed in the database: they stay in the in-flight set
            // (so the group stays held back), poisoned, until the poller's
            // retry releases them.
            let kept: HashSet<&str> = to_release.iter().map(String::as_str).collect();
            self.pipeline.remove_in_flight(
                ids.iter()
                    .map(String::as_str)
                    .filter(|id| !kept.contains(id)),
            );
            self.pipeline
                .defer_release(to_release.iter().map(|id| (id.clone(), true)));
            failed_groups.extend(to_release.iter().filter_map(|id| group_of.get(id).cloned()));
        } else {
            self.pipeline
                .remove_in_flight(ids.iter().map(String::as_str));
        }
        self.pipeline.race_point();
        if !failed_groups.is_empty() {
            let generation = self.pipeline.current_generation();
            self.pipeline.poison(failed_groups, generation);
        }
        if failed || release_failed {
            self.pipeline.note_failure();
        }
        self.pipeline.release(ids.len());
        LaneReport {
            published,
            unpublished,
            dropped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::poller::JobStore;
    use crate::scheduler::testkit::{dispatcher, lane_job, lock, FakePublisher, FakeStore, Status};

    const CAP: usize = 10;

    struct Harness {
        pipeline: Arc<Pipeline>,
        store: Arc<FakeStore>,
        publisher: Arc<FakePublisher>,
        lane: Lane,
    }

    fn harness(ids: &[(&str, Option<&str>)]) -> Harness {
        let store = FakeStore::with_jobs(
            ids.iter()
                .map(|(id, g)| (id.to_string(), g.map(str::to_string)))
                .collect(),
        );
        let publisher = Arc::new(FakePublisher::default());
        let pipeline = Arc::new(Pipeline::new(CAP));
        let lane = Lane::new(
            0,
            pipeline.clone(),
            store.clone(),
            dispatcher(publisher.clone()),
            100,
        );
        Harness {
            pipeline,
            store,
            publisher,
            lane,
        }
    }

    impl Harness {
        /// What the poller does for a claim: permits, in-flight, stamp.
        async fn submit(&self, jobs: &[(&str, Option<&str>)], generation: u64) -> Vec<LaneJob> {
            let mut got = 0;
            while got < jobs.len() {
                got += self.pipeline.acquire_up_to(jobs.len() - got).await;
            }
            jobs.iter()
                .map(|(id, g)| {
                    let job = lane_job(id, *g, generation);
                    assert!(
                        self.pipeline.add_in_flight(&job.job, generation),
                        "{id} already in flight"
                    );
                    job
                })
                .collect()
        }

        fn idle(&self) -> bool {
            self.pipeline.in_flight_len() == 0 && self.pipeline.available() == CAP
        }
    }

    /// A claim taken before a failure is handled cannot overtake it: its jobs
    /// of the failed group are dropped when they reach the lane. A claim
    /// taken after the failure was handled passes, and clears the poison.
    #[tokio::test]
    async fn a_claim_racing_a_failure_is_dropped_and_a_later_one_passes() {
        let mut h = harness(&[("a1", Some("g")), ("a2", Some("g")), ("a3", Some("g"))]);
        lock(&h.publisher.fail_once).insert("a1".into());

        let g1 = h.pipeline.next_generation();
        let first = h.submit(&[("a1", Some("g"))], g1).await;
        // A second claim takes its generation and snapshots BEFORE the lane
        // handles a1's failure.
        let g2 = h.pipeline.next_generation();
        let racing = h.submit(&[("a2", Some("g"))], g2).await;

        let r = h.lane.process(first).await;
        assert_eq!((r.published, r.unpublished), (0, 1));
        let r = h.lane.process(racing).await;
        assert_eq!(
            (r.published, r.dropped),
            (0, 1),
            "the racing claim is dropped"
        );
        assert!(h.publisher.published_ids().is_empty());
        assert!(h.idle());

        // A claim taken after the failure was handled passes, in order.
        let g3 = h.pipeline.next_generation();
        let after = h.submit(&[("a1", Some("g")), ("a2", Some("g"))], g3).await;
        let r = h.lane.process(after).await;
        assert_eq!((r.published, r.dropped), (2, 0));
        assert_eq!(h.publisher.published_ids(), vec!["a1", "a2"]);
        // ...and cleared the poison: the next job passes too.
        let g4 = h.pipeline.next_generation();
        let next = h.submit(&[("a3", Some("g"))], g4).await;
        assert_eq!(h.lane.process(next).await.published, 1);
        assert!(h.idle());
    }

    /// A drop does not renew the poison (that livelocks: see the module docs).
    /// The lane alone therefore passes a newer claim's job behind a dropped
    /// one; what keeps such a job from being submitted is the poller's check
    /// against its in-flight snapshot (`poller::tests::a_claim_that_skipped_a_doomed_job_...`).
    #[tokio::test]
    async fn a_drop_does_not_renew_the_poison() {
        let mut h = harness(&[("j1", Some("g")), ("j2", Some("g")), ("j3", Some("g"))]);
        lock(&h.publisher.fail_once).insert("j1".into());
        let ga = h.pipeline.next_generation();
        let x = h.submit(&[("j1", Some("g"))], ga).await;
        let y = h.submit(&[("j2", Some("g"))], ga).await;
        assert_eq!(h.lane.process(x).await.unpublished, 1);
        assert_eq!(h.lane.process(y).await.dropped, 1);
        let gb = h.pipeline.next_generation();
        let z = h.submit(&[("j3", Some("g"))], gb).await;
        assert_eq!(h.lane.process(z).await.published, 1);
    }

    /// Ids leave the in-flight set and permits return on every path:
    /// published, unpublished and dropped.
    #[tokio::test]
    async fn ids_leave_the_set_and_permits_return_on_success_failure_and_drop() {
        let mut h = harness(&[
            ("ok", None),
            ("bad", Some("g")),
            ("behind", Some("g")),
            ("other", Some("h")),
        ]);
        lock(&h.publisher.fail_once).insert("bad".into());
        let g1 = h.pipeline.next_generation();
        let failing = h.submit(&[("bad", Some("g")), ("ok", None)], g1).await;
        let g2 = h.pipeline.next_generation();
        let behind = h.submit(&[("behind", Some("g"))], g2).await;
        assert_eq!(h.pipeline.in_flight_len(), 3);
        assert_eq!(h.pipeline.available(), CAP - 3);

        let r = h.lane.process(failing).await;
        assert_eq!((r.published, r.unpublished), (1, 1));
        assert_eq!(h.pipeline.in_flight_len(), 1, "failure and success are out");
        assert_eq!(h.pipeline.available(), CAP - 1);

        let r = h.lane.process(behind).await;
        assert_eq!(r.dropped, 1);
        assert!(h.idle(), "a drop is out too");
        assert_eq!(h.store.status_of("ok"), Status::Queued);
        assert_eq!(h.store.status_of("bad"), Status::Pending);
        assert_eq!(h.store.status_of("behind"), Status::Pending);
    }

    /// Every job of a batch that is not QUEUED afterwards has its claim
    /// released (failed publish, dropped as poisoned), and the ones marked
    /// QUEUED do not: their queue row is gone.
    #[tokio::test]
    async fn the_claim_of_every_job_that_is_not_queued_is_released() {
        let mut h = harness(&[
            ("ok", None),
            ("bad", Some("g")),
            ("behind", Some("g")),
            ("other", Some("h")),
        ]);
        lock(&h.publisher.fail_once).insert("bad".into());
        // The store holds all four as claimed, as a claim would have left them.
        let claimed = h.store.claim(10).await.unwrap();
        assert_eq!(claimed.len(), 4);
        let g1 = h.pipeline.next_generation();
        let failing = h.submit(&[("bad", Some("g")), ("ok", None)], g1).await;
        let g2 = h.pipeline.next_generation();
        let behind = h
            .submit(&[("behind", Some("g")), ("other", Some("h"))], g2)
            .await;

        let r = h.lane.process(failing).await;
        assert_eq!((r.published, r.unpublished), (1, 1));
        assert!(!h.store.is_claimed("bad"), "the failed publish is released");
        assert_eq!(h.store.status_of("ok"), Status::Queued);
        assert_eq!(
            lock(&h.store.releases).clone(),
            vec![vec!["bad".to_string()]],
            "the marked job is not released"
        );
        let r = h.lane.process(behind).await;
        assert_eq!(
            (r.published, r.dropped),
            (1, 1),
            "behind is dropped, other passes"
        );
        assert!(!h.store.is_claimed("behind"), "the dropped job is released");
        assert_eq!(h.store.status_of("other"), Status::Queued);
        assert!(h.idle());
    }

    /// A release that fails keeps the ids in flight (so the group stays held
    /// back), poisons the group, flags the poller, and leaves the retry to it.
    #[tokio::test]
    async fn a_failed_release_keeps_the_ids_in_flight_and_poisons_the_group() {
        let mut h = harness(&[("a", Some("g")), ("b", None)]);
        lock(&h.publisher.fail_once).insert("a".into());
        h.store.fail_release.store(true, Ordering::SeqCst);
        h.store.claim(10).await.unwrap();
        let g1 = h.pipeline.next_generation();
        let jobs = h.submit(&[("a", Some("g")), ("b", None)], g1).await;
        let r = h.lane.process(jobs).await;
        assert_eq!((r.published, r.unpublished), (1, 1));
        assert!(h.store.is_claimed("a"), "still claimed in the database");
        assert_eq!(h.pipeline.in_flight_len(), 1, "a stays in flight, b is out");
        assert_eq!(h.pipeline.unreleased_len(), 1);
        assert_eq!(h.pipeline.available(), CAP, "permits still go back");
        assert!(h.pipeline.take_failure());
        // The group is poisoned: an older claim's job is dropped.
        let older = h.submit(&[("z", Some("g"))], g1).await;
        assert_eq!(h.lane.process(older).await.dropped, 1);
    }

    /// Ungrouped jobs are never poisoned: nothing orders them.
    #[tokio::test]
    async fn an_ungrouped_failure_poisons_nothing() {
        let mut h = harness(&[("u1", None), ("u2", None)]);
        lock(&h.publisher.fail_once).insert("u1".into());
        let g1 = h.pipeline.next_generation();
        let first = h.submit(&[("u1", None)], g1).await;
        let older = h.submit(&[("u2", None)], g1).await;
        h.lane.process(first).await;
        let r = h.lane.process(older).await;
        assert_eq!((r.published, r.dropped), (1, 0));
    }

    /// A poison entry for a group that is never seen again is forgotten.
    #[tokio::test(start_paused = true)]
    async fn poison_expires_after_ten_minutes() {
        let mut h = harness(&[("a", Some("g")), ("b", Some("g"))]);
        lock(&h.publisher.fail_once).insert("a".into());
        let g1 = h.pipeline.next_generation();
        let first = h.submit(&[("a", Some("g"))], g1).await;
        let stale = h.submit(&[("b", Some("g"))], g1).await;
        h.lane.process(first).await;
        time::advance(POISON_TTL + Duration::from_secs(1)).await;
        let r = h.lane.process(stale).await;
        assert_eq!((r.published, r.dropped), (1, 0), "expired: not dropped");
    }

    /// A failed status update leaves the jobs PENDING and flags the poller,
    /// but poisons nothing: the broker has them.
    #[tokio::test]
    async fn a_failed_mark_flags_the_poller_and_releases_everything() {
        let mut h = harness(&[("a", Some("g")), ("b", Some("g"))]);
        h.store.fail_mark.store(true, Ordering::SeqCst);
        let g1 = h.pipeline.next_generation();
        let jobs = h.submit(&[("a", Some("g")), ("b", Some("g"))], g1).await;
        h.store.claim(10).await.unwrap();
        let r = h.lane.process(jobs).await;
        assert_eq!(r.published, 2);
        assert_eq!(h.store.count(Status::Pending), 2);
        assert_eq!(
            h.store.claimed_count(),
            0,
            "published but not marked: released"
        );
        assert!(h.pipeline.take_failure());
        assert!(h.idle());
        let g2 = h.pipeline.next_generation();
        let again = h.submit(&[("a", Some("g"))], g2).await;
        assert_eq!(h.lane.process(again).await.dropped, 0, "no poison");
    }

    /// A job the callback rescheduled to PENDING between publish and mark is
    /// not set QUEUED (its row version moved on); it stays PENDING.
    #[tokio::test]
    async fn a_job_rescheduled_before_the_mark_is_not_queued() {
        let mut h = harness(&[("a", None), ("b", None)]);
        let claimed = h.store.claim(10).await.unwrap();
        let g = h.pipeline.next_generation();
        let mut jobs = Vec::new();
        for c in claimed {
            let mut got = 0;
            while got < 1 {
                got += h.pipeline.acquire_up_to(1).await;
            }
            assert!(h.pipeline.add_in_flight(&c, g));
            jobs.push(LaneJob {
                job: c,
                generation: g,
            });
        }
        h.store.touch("a");
        let r = h.lane.process(jobs).await;
        assert_eq!(r.published, 2, "both reached the broker");
        assert_eq!(h.store.status_of("a"), Status::Pending);
        assert_eq!(h.store.status_of("b"), Status::Queued);
        assert!(h.idle());
    }
}
