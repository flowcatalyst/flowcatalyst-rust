//! Dispatcher lanes, and the state they share with the poller.
//!
//! The poller claims rows and hands each to a lane; a lane publishes what it
//! is given and marks it QUEUED, in bulk, with no transaction and no row lock
//! held anywhere. The poller never waits for a publish. Three things bound
//! and order the work:
//!
//! - **Permits** ([`Pipeline`]): one per job from claim to the end of its lane
//!   batch. The poller blocks when none are free, so a slow broker holds back
//!   the claim instead of growing a buffer.
//! - **The in-flight set**: ids claimed and not yet marked. The next claim
//!   excludes them (`id <> ALL(...)`), so a row that is still PENDING in the
//!   database while its publish is under way is not claimed again.
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
//! - a lane that leaves jobs of `g` unpublished removes the batch's ids from
//!   the in-flight set and **then** reads the counter, `P`, and records
//!   `poison[g] = P`;
//! - a job of `g` with `generation <= poison[g]` is dropped when it reaches
//!   the lane (it stays PENDING and is claimed again later);
//! - a claim whose generation is `> P` incremented the counter after `P` was
//!   read, so it snapshotted the in-flight set after `j` was removed: it
//!   claims `j` again, in order. Its jobs pass, and the first one clears the
//!   poison.
//!
//! **A dropped job poisons its group too**, again at the generation read
//! after its removal. Without that, a job of `g` still waiting in the lane
//! (so still in the in-flight set, so excluded from a later claim) is dropped,
//! and the later claim, which could not see it, takes the job *behind* it and
//! publishes it ahead of the dropped one.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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

/// What the poller and the lanes share. See the module docs.
pub(crate) struct Pipeline {
    capacity: usize,
    generation: AtomicU64,
    in_flight: Mutex<HashSet<String>>,
    permits: Semaphore,
    failed: AtomicBool,
}

impl Pipeline {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            generation: AtomicU64::new(0),
            in_flight: Mutex::new(HashSet::new()),
            permits: Semaphore::new(capacity),
            failed: AtomicBool::new(false),
        }
    }

    /// Take the next claim generation. Called before [`Self::snapshot_in_flight`].
    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn snapshot_in_flight(&self) -> Vec<String> {
        locked(&self.in_flight).iter().cloned().collect()
    }

    /// `false` when the id was already in flight.
    pub fn add_in_flight(&self, id: &str) -> bool {
        let mut set = locked(&self.in_flight);
        let added = set.insert(id.to_string());
        metrics::gauge!("scheduler.in_flight.size").set(set.len() as f64);
        added
    }

    pub fn remove_in_flight<'a>(&self, ids: impl IntoIterator<Item = &'a str>) {
        let mut set = locked(&self.in_flight);
        for id in ids {
            set.remove(id);
        }
        metrics::gauge!("scheduler.in_flight.size").set(set.len() as f64);
    }

    #[cfg(test)]
    pub fn in_flight_len(&self) -> usize {
        locked(&self.in_flight).len()
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

struct Poison {
    generation: u64,
    set_at: Instant,
}

/// One dispatcher: publishes the jobs it is handed and marks them QUEUED.
pub(crate) struct Lane {
    index: usize,
    pipeline: Arc<Pipeline>,
    store: Arc<dyn JobStore>,
    dispatcher: Arc<MessageGroupDispatcher>,
    /// How many jobs a batch takes beyond its first.
    batch_extra: usize,
    poison: HashMap<String, Poison>,
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
            poison: HashMap::new(),
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

    fn evict_expired_poison(&mut self) {
        self.poison.retain(|_, p| p.set_at.elapsed() < POISON_TTL);
    }

    /// Publish one batch and finish it: drop what is poisoned, publish the
    /// rest, mark the published ids QUEUED, then remove every id from the
    /// in-flight set, record the poison, and release the permits, in that
    /// order (see the module docs for why the order matters).
    pub async fn process(&mut self, batch: Vec<LaneJob>) -> LaneReport {
        if batch.is_empty() {
            return LaneReport::default();
        }
        self.evict_expired_poison();
        let ids: Vec<String> = batch.iter().map(|j| j.job.id().to_string()).collect();

        // Groups that must not publish anything more in this batch: one with
        // a job dropped here (a later job of it would overtake the dropped
        // one), later extended by the groups the publish fails.
        let mut stopped_groups: HashSet<String> = HashSet::new();
        let mut to_publish: Vec<ClaimedJob> = Vec::with_capacity(batch.len());
        let mut dropped = 0usize;
        for LaneJob { job, generation } in batch {
            if let Some(group) = job.group() {
                if stopped_groups.contains(group) {
                    dropped += 1;
                    continue;
                }
                match self.poison.get(group) {
                    Some(p) if generation <= p.generation => {
                        stopped_groups.insert(group.to_string());
                        dropped += 1;
                        continue;
                    }
                    Some(_) => {
                        self.poison.remove(group);
                    }
                    None => {}
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
                        stopped_groups.insert(group.to_string());
                    }
                } else {
                    to_mark.push((job.id().to_string(), job.created_at, job.updated_at));
                }
            }
        }
        let published = to_mark.len();
        let unpublished = to_publish.len() - published;

        if !to_mark.is_empty() {
            // A fresh deadline, not the lane's cancellation: a shutdown must
            // not abandon the update of jobs the broker already has.
            match time::timeout(MARK_TIMEOUT, self.store.mark_queued(&to_mark)).await {
                Ok(Ok(updated)) => {
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

        // Ids out of the set, THEN the generation read, THEN the permits.
        self.pipeline
            .remove_in_flight(ids.iter().map(String::as_str));
        if !stopped_groups.is_empty() {
            let generation = self.pipeline.current_generation();
            let set_at = Instant::now();
            for group in stopped_groups {
                self.poison.insert(group, Poison { generation, set_at });
            }
        }
        if failed {
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
                    assert!(self.pipeline.add_in_flight(id), "{id} already in flight");
                    lane_job(id, *g, generation)
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

    /// A dropped job poisons its group at the generation read after its own
    /// removal. Otherwise a claim that could not see it (it was still in the
    /// in-flight set) takes the job behind it, and that job is published
    /// ahead of the dropped one.
    #[tokio::test]
    async fn a_dropped_job_poisons_its_group_too() {
        let mut h = harness(&[("j1", Some("g")), ("j2", Some("g")), ("j3", Some("g"))]);
        lock(&h.publisher.fail_once).insert("j1".into());

        // Claim A (generation 1) took j1 and j2; the lane received them in
        // two batches.
        let ga = h.pipeline.next_generation();
        let batch_x = h.submit(&[("j1", Some("g"))], ga).await;
        let batch_y = h.submit(&[("j2", Some("g"))], ga).await;

        // j1 fails; poison[g] = 1.
        assert_eq!(h.lane.process(batch_x).await.unpublished, 1);

        // Claim B starts now: generation 2, and its snapshot still holds j2
        // (batch Y is unprocessed), so it returns j1 and j3 but not j2.
        let gb = h.pipeline.next_generation();
        let batch_z = h.submit(&[("j1", Some("g")), ("j3", Some("g"))], gb).await;

        // The lane drops j2 (generation 1 <= 1) and must poison g again.
        assert_eq!(h.lane.process(batch_y).await.dropped, 1);
        let r = h.lane.process(batch_z).await;
        assert_eq!((r.published, r.dropped), (0, 2), "j3 must not pass j2");
        assert!(h.publisher.published_ids().is_empty());

        // Claim C sees all three again, in order.
        let gc = h.pipeline.next_generation();
        let all = h
            .submit(
                &[("j1", Some("g")), ("j2", Some("g")), ("j3", Some("g"))],
                gc,
            )
            .await;
        assert_eq!(h.lane.process(all).await.published, 3);
        assert_eq!(h.publisher.published_ids(), vec!["j1", "j2", "j3"]);
        assert!(h.idle());
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
        let r = h.lane.process(jobs).await;
        assert_eq!(r.published, 2);
        assert_eq!(h.store.count(Status::Pending), 2);
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
        let claimed = h.store.claim(10, Vec::new()).await.unwrap();
        let g = h.pipeline.next_generation();
        let mut jobs = Vec::new();
        for c in claimed {
            let mut got = 0;
            while got < 1 {
                got += h.pipeline.acquire_up_to(1).await;
            }
            assert!(h.pipeline.add_in_flight(c.id()));
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
