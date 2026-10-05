//! Pending job poller.
//!
//! A port of Go's `PendingJobPoller` (`scheduler/poller.go`), with the poller
//! and the publishing decoupled:
//!
//! ```text
//! poller (leader only) --claim--> lanes[hash(group) % N] --SendMessageBatch--> broker
//!    ^  permits (buffer_capacity)        |  bulk UPDATE status = 'QUEUED'
//!    +----------- released --------------+
//! ```
//!
//! The poller claims due PENDING jobs from `msg_dispatch_jobs` in the total
//! order `(message_group NULLS LAST, sequence, created_at, id)` and hands them
//! to the lanes; it never waits for a publish. It blocks only when
//! `buffer_capacity` jobs are already claimed and not yet finished. A lane
//! publishes its jobs through the [`MessageGroupDispatcher`] and marks the
//! published ids QUEUED in one statement (see [`super::lane`]).
//!
//! The claim is one plain `SELECT` ([`lifecycle::claim`]): no lock, no
//! transaction, no write. What keeps a claimed job out of the next claim is
//! the poller's in-memory in-flight set, passed to the claim (`id <> ALL($4)`),
//! not a status or a column: until the lane's update runs the row is still
//! PENDING. Only the leader claims, so nothing else competes for the rows. A
//! crash needs no recovery: whatever was not yet marked is still PENDING and
//! the next leader publishes it. The claim walks `idx_dispatch_jobs_status_group`
//! in order and stops early.
//!
//! The price is that a job can be published twice (a lane that published and
//! then failed to mark; a claim that raced a failure). That is accepted: the
//! router drops a copy whose original is in its pipeline, and
//! `/api/dispatch/process` claims a job before delivering it, so a copy that
//! finds the job taken or finished never delivers it.
//!
//! Paused jobs are excluded **inside the claim**. Jobs held back (a
//! BLOCK_ON_ERROR job with an EARLIER job in its group that is FAILED / ERROR
//! or PENDING in a retry backoff) are found by one batched query after the
//! claim and dropped (they stay PENDING); the poller remembers each group it
//! has just found held for 5 seconds and skips it in the claim, so a
//! batch-full of held rows at the head of the order does not starve the
//! groups behind it. The comparison is positional, so the holder itself
//! dispatches once its backoff expires. IMMEDIATE and NEXT_ON_ERROR (and, per
//! X-01, an absent or unknown mode) never wait.
//!
//! A NULL group never holds; an ungrouped job is held only by a row whose
//! group is literally `default`: Go's quirk, kept.

use crate::dispatch_job::lifecycle;
use std::collections::{HashMap, HashSet};
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::field::Empty;
use tracing::{debug, error, warn};

use super::destination::PoolCodeResolver;
use super::dispatcher::{DispatchJobToken, MessageGroupDispatcher};
use super::lane::{ClaimedJob, Lane, LaneJob, Pipeline};
use super::{SchedulerConfig, SchedulerError};

/// The subscription ids whose connection is PAUSED, cached.
pub struct PausedConnectionCache {
    pool: PgPool,
    ttl: Duration,
    state: RwLock<(Vec<String>, Option<Instant>)>,
}

impl PausedConnectionCache {
    pub fn new(pool: PgPool, ttl: Duration) -> Self {
        Self {
            pool,
            ttl,
            state: RwLock::new((Vec::new(), None)),
        }
    }

    /// The cached set, refreshed when stale. A refresh failure fails the
    /// tick (Go): claiming without knowing what is paused would dispatch
    /// paused work.
    pub async fn paused_subscription_ids(&self) -> Result<Vec<String>, SchedulerError> {
        {
            let s = self.state.read();
            if s.1.is_some_and(|t| t.elapsed() < self.ttl) {
                return Ok(s.0.clone());
            }
        }
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT s.id FROM msg_subscriptions s \
             JOIN msg_connections c ON c.id = s.connection_id \
             WHERE c.status = 'PAUSED'",
        )
        .fetch_all(&self.pool)
        .await?;
        debug!(
            paused_subscriptions = ids.len(),
            "paused connection cache refreshed"
        );
        *self.state.write() = (ids.clone(), Some(Instant::now()));
        Ok(ids)
    }
}

/// (id, `created_at`, `updated_at` as claimed): one row to mark QUEUED.
pub(crate) type MarkKey = (String, DateTime<Utc>, DateTime<Utc>);

/// What one claim returned.
#[derive(Debug, Default)]
pub(crate) struct ClaimOutcome {
    /// The claimed jobs that may be published, in claim order.
    pub jobs: Vec<ClaimedJob>,
    /// Groups in which the claim found a job held back (the held jobs stay
    /// PENDING and are not returned): the poller skips them for a few seconds.
    pub held_groups: Vec<String>,
    /// Rows the claim read in all, held ones included.
    pub taken: usize,
}

/// The database side of the pipeline, so the loop can be tested without one.
#[async_trait]
pub(crate) trait JobStore: Send + Sync {
    /// Up to `limit` due PENDING jobs in claim order, minus paused
    /// subscriptions, the groups in `skip_groups` and the ids in `in_flight`.
    /// Jobs held back by an earlier job of their group are not returned; their
    /// groups are reported.
    async fn claim(
        &self,
        limit: usize,
        skip_groups: &[String],
        in_flight: &[String],
    ) -> Result<ClaimOutcome, SchedulerError>;

    /// Mark the given jobs QUEUED where they are still PENDING at the version
    /// the claim read; returns how many rows changed.
    async fn mark_queued(&self, jobs: &[MarkKey]) -> Result<u64, SchedulerError>;

    /// The PENDING backlog, bounded.
    async fn backlog(&self) -> Result<Option<lifecycle::PendingBacklog>, SchedulerError> {
        Ok(None)
    }
}

struct PgJobStore {
    pool: PgPool,
    paused: PausedConnectionCache,
    pool_codes: Arc<PoolCodeResolver>,
}

#[async_trait]
impl JobStore for PgJobStore {
    async fn claim(
        &self,
        limit: usize,
        skip_groups: &[String],
        in_flight: &[String],
    ) -> Result<ClaimOutcome, SchedulerError> {
        let paused = self.paused.paused_subscription_ids().await?;
        let rows =
            lifecycle::claim(&self.pool, limit as i64, &paused, skip_groups, in_flight).await?;
        let taken = rows.len();
        // The hold-back: one batched query. Held jobs stay PENDING.
        let held_ids: HashSet<String> = lifecycle::held_among(&self.pool, &rows)
            .await?
            .into_iter()
            .collect();
        let mut held_groups: Vec<String> = Vec::new();
        let mut jobs = Vec::with_capacity(rows.len());
        let mut held = 0u64;
        for c in rows {
            if held_ids.contains(&c.job_id) {
                held += 1;
                if let Some(g) = c.message_group.as_deref() {
                    if !held_groups.iter().any(|h| h == g) {
                        held_groups.push(g.to_string());
                    }
                }
                continue;
            }
            let pool_code = self
                .pool_codes
                .resolve(c.dispatch_pool_id.as_deref(), c.client_id.as_deref())
                .await;
            jobs.push(ClaimedJob {
                created_at: c.job_created_at,
                updated_at: c.version,
                token: DispatchJobToken {
                    job_id: c.job_id,
                    message_group: c.message_group,
                    mode: c.mode,
                    pool_code,
                    client_id: c.client_id,
                    subscription_id: c.subscription_id,
                    queue: c.queue,
                },
            });
        }
        if held > 0 {
            metrics::counter!("scheduler.jobs.held_total").increment(held);
        }
        Ok(ClaimOutcome {
            jobs,
            held_groups,
            taken,
        })
    }

    /// Marks exactly the published ids QUEUED, on a pooled connection, and
    /// only the row version the claim read: the router can deliver and the
    /// callback can move the job on before this runs, and the update must
    /// never regress it. The status guard covers a job that is past
    /// PENDING; the `updated_at` version covers one the callback put back
    /// to PENDING (a retry, a deferral, a hold) in the meantime. The
    /// statement is the lifecycle's `mark_queued`.
    async fn mark_queued(&self, jobs: &[MarkKey]) -> Result<u64, SchedulerError> {
        let done = lifecycle::mark_queued(&self.pool, jobs).await?;
        Ok(done.len() as u64)
    }

    async fn backlog(&self) -> Result<Option<lifecycle::PendingBacklog>, SchedulerError> {
        Ok(Some(lifecycle::pending_backlog(&self.pool).await?))
    }
}

/// Registers the scheduler's metric descriptions (idempotent).
fn describe_metrics() {
    use metrics::{describe_counter, describe_gauge, describe_histogram, Unit};
    describe_gauge!(
        "scheduler.pending_jobs",
        "Jobs claimed by the most recent poll (at most the batch size). NOT the PENDING backlog; the name is historical."
    );
    describe_counter!(
        "scheduler.jobs.claimed_total",
        "Jobs claimed from PENDING by the poller."
    );
    describe_counter!(
        "scheduler.poll.full_batches_total",
        "Polls whose claim filled what it asked for (a backlog is likely waiting)."
    );
    describe_counter!(
        "scheduler.poll.errors_total",
        "Polls that failed (the claim errored)."
    );
    describe_histogram!(
        "scheduler.poll.duration_seconds",
        Unit::Seconds,
        "Wall time of one claim pass (excludes waiting for buffer space and the publish, which the lanes do)."
    );
    describe_histogram!(
        "scheduler.claim.duration_seconds",
        Unit::Seconds,
        "Wall time of the claim query."
    );
    describe_histogram!(
        "scheduler.publish.duration_seconds",
        Unit::Seconds,
        "Wall time of publishing one lane batch to the queues."
    );
    describe_histogram!(
        "scheduler.lane.publish.duration_seconds",
        Unit::Seconds,
        "Wall time of publishing one lane batch, per lane."
    );
    describe_counter!(
        "scheduler.publish.timeouts_total",
        "Lane batches whose publish hit the deadline and was abandoned."
    );
    describe_gauge!(
        "scheduler.buffer.in_use",
        "Jobs claimed and not yet finished by a lane (permits held, out of the buffer capacity)."
    );
    describe_gauge!(
        "scheduler.in_flight.size",
        "Size of the in-flight id set the claim excludes."
    );
    describe_counter!(
        "scheduler.jobs.dropped_poisoned_total",
        "Claimed jobs a lane dropped unpublished because an earlier job of their group had failed."
    );
    describe_counter!(
        "scheduler.queued_mark.skipped_total",
        "Published jobs the QUEUED update left alone because they had already moved past PENDING."
    );
    describe_counter!(
        "scheduler.jobs.held_total",
        "Claimed jobs found held back by an earlier job of their group (FAILED / ERROR, or in a retry backoff); they stay PENDING."
    );
    describe_gauge!(
        "scheduler.held_groups",
        "Groups the poller currently remembers as held and skips in its claim (5 seconds each)."
    );
    describe_counter!(
        "scheduler.jobs.withheld_total",
        "Claimed jobs the poller did not submit because they were behind a doomed job of their group; they stay PENDING."
    );
    describe_counter!(
        "scheduler.claim.in_flight_duplicates_total",
        "Claimed rows dropped because their id was already in flight (the claim excludes the in-flight set, so this should stay 0)."
    );
    describe_gauge!(
        "scheduler.pending.backlog",
        "PENDING jobs (counted up to 100,000: the gauge saturates at 100000), sampled by the leader every 30 s."
    );
    describe_gauge!(
        "scheduler.pending.oldest_age_seconds",
        Unit::Seconds,
        "Age of the first PENDING job in claim order, sampled with the backlog."
    );
    describe_gauge!(
        "scheduler.poll.last_success_timestamp_seconds",
        Unit::Seconds,
        "Unix time of the last poll that completed without error."
    );
}

/// What one poll pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PassReport {
    /// Jobs the claim asked for (the permits it held).
    pub wanted: usize,
    /// Jobs it returned.
    pub claimed: usize,
    /// Jobs handed to a lane.
    pub submitted: usize,
    /// The pass should be followed by a wait however full the claim was: a
    /// lane failed since the previous claim, or the pass did not claim at all.
    pub back_off: bool,
}

impl PassReport {
    /// A pass that did not claim (not leader, shutting down).
    fn skipped() -> Self {
        Self {
            back_off: true,
            ..Self::default()
        }
    }

    /// Whether the loop should wait [`SchedulerConfig::poll_interval`] before
    /// the next pass. Without it a failing broker is retried in a hot loop
    /// (the failed rows are still PENDING), as is an all-held or short claim.
    pub fn should_pause(&self) -> bool {
        self.back_off || self.submitted == 0 || self.claimed < self.wanted
    }
}

/// What one tick did, for [`PendingJobPoller::poll_once`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PollReport {
    pub claimed: usize,
    pub published: usize,
}

/// The sizes the pipeline runs with, never below one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PollerSettings {
    pub buffer_capacity: usize,
    pub dispatchers: usize,
    pub batch_size: usize,
    pub lane_batch: usize,
}

impl PollerSettings {
    pub fn from_config(c: &SchedulerConfig) -> Self {
        Self {
            buffer_capacity: c.buffer_capacity.max(1),
            dispatchers: c.dispatchers.max(1),
            batch_size: c.batch_size.max(1),
            lane_batch: c.lane_batch.max(1),
        }
    }
}

/// A stable hash of a message group (FNV-1a): the same group always maps to
/// the same lane, across restarts and Rust versions.
fn lane_for_group(group: &str, lanes: usize) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in group.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    (h % lanes as u64) as usize
}

/// How long a group found held is skipped by the claim.
const HELD_GROUP_TTL: Duration = Duration::from_secs(5);
/// The most groups remembered as held; the oldest are dropped first.
const HELD_GROUP_CAP: usize = 10_000;

/// Groups the poller has just found held, each remembered for
/// [`HELD_GROUP_TTL`]: the claim skips them, so a batch-full of held rows at
/// the head of the order is not claimed and put back on every pass while the
/// groups behind it wait (the starvation a claim that filters after its
/// `LIMIT` would have).
struct HeldGroups {
    until: Mutex<HashMap<String, time::Instant>>,
    ttl: Duration,
    cap: usize,
}

impl HeldGroups {
    fn new(ttl: Duration, cap: usize) -> Self {
        Self {
            until: Mutex::new(HashMap::new()),
            ttl,
            cap,
        }
    }

    /// The groups to skip now; expired ones are forgotten.
    fn active(&self) -> Vec<String> {
        let now = time::Instant::now();
        let mut map = self.until.lock();
        map.retain(|_, until| *until > now);
        metrics::gauge!("scheduler.held_groups").set(map.len() as f64);
        map.keys().cloned().collect()
    }

    fn remember(&self, groups: &[String]) {
        if groups.is_empty() {
            return;
        }
        let until = time::Instant::now() + self.ttl;
        let mut map = self.until.lock();
        for g in groups {
            map.insert(g.clone(), until);
        }
        // Over the cap: drop the ones that expire first.
        while map.len() > self.cap {
            let Some(oldest) = map.iter().min_by_key(|(_, u)| **u).map(|(g, _)| g.clone()) else {
                break;
            };
            map.remove(&oldest);
        }
        metrics::gauge!("scheduler.held_groups").set(map.len() as f64);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.until.lock().len()
    }
}

/// The claim half of a pass: generation, snapshot, query, in-flight set.
struct ClaimStage {
    store: Arc<dyn JobStore>,
    pipeline: Arc<Pipeline>,
    held: HeldGroups,
}

impl ClaimStage {
    fn new(store: Arc<dyn JobStore>, pipeline: Arc<Pipeline>) -> Self {
        Self {
            store,
            pipeline,
            held: HeldGroups::new(HELD_GROUP_TTL, HELD_GROUP_CAP),
        }
    }

    /// Claim up to `want` jobs, holding `want` permits. The jobs come back
    /// stamped and already in the in-flight set; the permits for the rest
    /// are released. A failed claim releases them all. A job the claim read
    /// but this pass does not submit (behind a doomed job) stays PENDING.
    async fn claim(&self, want: usize) -> Result<(Vec<LaneJob>, usize), SchedulerError> {
        // The generation BEFORE the snapshot: the ordering rule in `lane`
        // depends on it.
        let generation = self.pipeline.next_generation();
        self.pipeline.race_point();
        let snapshot = self.pipeline.snapshot_in_flight();
        let skip = self.held.active();
        let started = Instant::now();
        let claimed = self.store.claim(want, &skip, &snapshot.ids).await;
        metrics::histogram!("scheduler.claim.duration_seconds").record(started.elapsed());
        let outcome = match claimed {
            Ok(o) => o,
            Err(e) => {
                self.pipeline.release(want);
                return Err(e);
            }
        };
        self.held.remember(&outcome.held_groups);
        let taken = outcome.taken;
        let mut jobs = Vec::with_capacity(outcome.jobs.len());
        // Groups with a job this pass did not submit: a later job of such a
        // group would overtake it.
        let mut withheld_groups: HashSet<String> = HashSet::new();
        let mut withheld = 0u64;
        for job in outcome.jobs {
            let group = job.group().map(str::to_string);
            // The claim skipped a job of this group that is doomed (it will be
            // dropped): this one is behind it. It stays PENDING and is claimed
            // again, in order, once the doomed job has gone.
            let behind = group.as_deref().is_some_and(|g| {
                withheld_groups.contains(g) || self.pipeline.skipped_a_doomed_job(&snapshot, g)
            });
            if behind {
                withheld += 1;
                withheld_groups.extend(group);
                continue;
            }
            // Cannot happen (the claim excluded the set); never submit twice.
            if !self.pipeline.add_in_flight(&job, generation) {
                metrics::counter!("scheduler.claim.in_flight_duplicates_total").increment(1);
                withheld_groups.extend(group);
                continue;
            }
            jobs.push(LaneJob { job, generation });
        }
        if withheld > 0 {
            metrics::counter!("scheduler.jobs.withheld_total").increment(withheld);
        }
        self.pipeline.release(want.saturating_sub(jobs.len()));
        Ok((jobs, taken))
    }
}

/// The poller proper: one pass per [`PollSource::poll`].
struct Claimer {
    stage: ClaimStage,
    lanes: Vec<mpsc::Sender<LaneJob>>,
    next_ungrouped: AtomicUsize,
    batch_size: usize,
}

impl Claimer {
    fn lane_of(&self, job: &ClaimedJob) -> usize {
        match job.group() {
            Some(g) => lane_for_group(g, self.lanes.len()),
            None => self.next_ungrouped.fetch_add(1, Ordering::Relaxed) % self.lanes.len(),
        }
    }

    /// Claim and route one batch, given the permits already held.
    #[tracing::instrument(
        name = "scheduler.poll",
        skip_all,
        fields(wanted = want, claimed = Empty, submitted = Empty)
    )]
    async fn claim_pass(&self, want: usize) -> Result<PassReport, SchedulerError> {
        let started = Instant::now();
        let result = self.stage.claim(want).await;
        metrics::histogram!("scheduler.poll.duration_seconds").record(started.elapsed());
        let (jobs, claimed) = match result {
            Ok(r) => r,
            Err(e) => {
                metrics::counter!("scheduler.poll.errors_total").increment(1);
                return Err(e);
            }
        };
        let mut submitted = 0;
        for job in jobs {
            let lane = self.lane_of(&job.job);
            // Capacity equals the permit count, so this never waits; a
            // closed lane (shutdown) leaves the job unpublished: its claim is
            // given back.
            match self.lanes[lane].try_send(job) {
                Ok(()) => submitted += 1,
                Err(e) => {
                    let job = e.into_inner();
                    self.stage
                        .pipeline
                        .remove_in_flight([(job.job.id(), job.generation)]);
                    self.stage.pipeline.release(1);
                }
            }
        }
        let span = tracing::Span::current();
        span.record("claimed", claimed);
        span.record("submitted", submitted);
        // `scheduler.pending_jobs` is the size of THIS claim (at most the
        // batch size), not the PENDING backlog; the name is kept for
        // existing dashboards.
        metrics::gauge!("scheduler.pending_jobs").set(claimed as f64);
        metrics::counter!("scheduler.jobs.claimed_total").increment(claimed as u64);
        if claimed >= want && want >= self.batch_size {
            metrics::counter!("scheduler.poll.full_batches_total").increment(1);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        metrics::gauge!("scheduler.poll.last_success_timestamp_seconds").set(now);
        debug!(want, claimed, submitted, "poll tick");
        Ok(PassReport {
            wanted: want,
            claimed,
            submitted,
            back_off: self.stage.pipeline.take_failure(),
        })
    }
}

/// What a pass needs from the loop that runs it.
pub(crate) struct PollCtl {
    pub cancel: CancellationToken,
    pub is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
}

/// One poll pass, so [`drive`] can be tested without a database.
pub(crate) trait PollSource {
    async fn poll(&self, ctl: &PollCtl) -> Result<PassReport, SchedulerError>;
}

impl PollSource for Claimer {
    /// Block for permits (cancellable), re-check leadership, claim, route.
    async fn poll(&self, ctl: &PollCtl) -> Result<PassReport, SchedulerError> {
        let pipeline = &self.stage.pipeline;
        let want = tokio::select! {
            biased;
            () = ctl.cancel.cancelled() => return Ok(PassReport::skipped()),
            n = pipeline.acquire_up_to(self.batch_size) => n,
        };
        // The wait for permits can be long; leadership may have gone with it.
        if ctl.cancel.is_cancelled() || !(ctl.is_leader)() {
            pipeline.release(want);
            return Ok(PassReport::skipped());
        }
        self.claim_pass(want).await
    }
}

/// The poller loop: a pass, then a wait only when [`PassReport::should_pause`]
/// says so (a backlog drains at the speed of the lanes, not at
/// `batch / interval`). Leadership and cancellation are re-checked before
/// every pass; a non-leader just waits.
pub(crate) async fn drive<S: PollSource>(source: &S, interval: Duration, ctl: &PollCtl) {
    loop {
        if ctl.cancel.is_cancelled() {
            break;
        }
        let pause = if (ctl.is_leader)() {
            match source.poll(ctl).await {
                Ok(report) => report.should_pause(),
                Err(e) => {
                    warn!(error = %e, "dispatch poll error");
                    true
                }
            }
        } else {
            true
        };
        if pause {
            tokio::select! {
                () = ctl.cancel.cancelled() => break,
                () = time::sleep(interval) => {}
            }
        }
    }
}

/// How often the leader samples the PENDING backlog.
const BACKLOG_EVERY: Duration = Duration::from_secs(30);

/// The leader's housekeeping, off the claim path: the backlog gauge every 30
/// s. The count is bounded (it saturates at 100,000), so a huge backlog is
/// never scanned.
async fn maintain(
    store: Arc<dyn JobStore>,
    is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
    cancel: CancellationToken,
) {
    let mut backlog = time::interval_at(time::Instant::now() + BACKLOG_EVERY, BACKLOG_EVERY);
    backlog.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = cancel.cancelled() => break,
            _ = backlog.tick() => {}
        }
        if !is_leader() {
            continue;
        }
        match store.backlog().await {
            Ok(Some(b)) => {
                metrics::gauge!("scheduler.pending.backlog").set(b.depth as f64);
                let age = b.oldest_created_at.map_or(0.0, |t| {
                    (Utc::now() - t).num_milliseconds().max(0) as f64 / 1e3
                });
                metrics::gauge!("scheduler.pending.oldest_age_seconds").set(age);
            }
            Ok(None) => {}
            Err(e) => warn!(error = %e, "sampling the dispatch backlog failed"),
        }
    }
}

pub struct PendingJobPoller {
    store: Arc<dyn JobStore>,
    dispatcher: Arc<MessageGroupDispatcher>,
    settings: PollerSettings,
}

impl PendingJobPoller {
    pub fn new(
        pool: PgPool,
        config: &SchedulerConfig,
        dispatcher: Arc<MessageGroupDispatcher>,
        pool_codes: Arc<PoolCodeResolver>,
    ) -> Self {
        describe_metrics();
        let store = Arc::new(PgJobStore {
            paused: PausedConnectionCache::new(pool.clone(), config.paused_cache_ttl),
            pool,
            pool_codes,
        });
        Self::with_store(store, dispatcher, PollerSettings::from_config(config))
    }

    pub(crate) fn with_store(
        store: Arc<dyn JobStore>,
        dispatcher: Arc<MessageGroupDispatcher>,
        settings: PollerSettings,
    ) -> Self {
        Self {
            store,
            dispatcher,
            settings,
        }
    }

    /// One pass, synchronously: claim, publish through one lane, mark the
    /// published jobs QUEUED, return. For tests and tooling; the scheduler
    /// itself runs [`Self::run`]. Each call starts as a new process would:
    /// an empty in-flight set.
    pub async fn poll_once(&self) -> Result<PollReport, SchedulerError> {
        let pipeline = Arc::new(Pipeline::new(self.settings.buffer_capacity));
        let stage = ClaimStage::new(self.store.clone(), pipeline.clone());
        let want = pipeline.acquire_up_to(self.settings.batch_size).await;
        let (jobs, claimed) = stage.claim(want).await.inspect_err(|_| {
            metrics::counter!("scheduler.poll.errors_total").increment(1);
        })?;
        let mut lane = Lane::new(
            0,
            pipeline,
            self.store.clone(),
            self.dispatcher.clone(),
            self.settings.lane_batch,
        );
        let report = lane.process(jobs).await;
        Ok(PollReport {
            claimed,
            published: report.published,
        })
    }

    /// Run the poller and its lanes until cancelled, claiming only while
    /// `is_leader` says so (the per-group order needs one active scheduler).
    /// On cancel the poller stops claiming and each lane finishes the batch
    /// it is sending; what is still buffered stays PENDING.
    pub async fn run(
        &self,
        interval: Duration,
        is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
        cancel: CancellationToken,
    ) {
        // Fresh state per run: a restart after a panic must not inherit
        // permits or in-flight ids from lanes that died holding them.
        let pipeline = Arc::new(Pipeline::new(self.settings.buffer_capacity));
        self.run_with(pipeline, interval, is_leader, cancel).await;
    }

    async fn run_with(
        &self,
        pipeline: Arc<Pipeline>,
        interval: Duration,
        is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
        cancel: CancellationToken,
    ) {
        let mut lane_tasks: JoinSet<()> = JoinSet::new();
        let mut senders = Vec::with_capacity(self.settings.dispatchers);
        for index in 0..self.settings.dispatchers {
            let (tx, rx) = mpsc::channel(self.settings.buffer_capacity);
            let lane = Lane::new(
                index,
                pipeline.clone(),
                self.store.clone(),
                self.dispatcher.clone(),
                self.settings.lane_batch,
            );
            lane_tasks.spawn(lane.run(rx, cancel.clone()));
            senders.push(tx);
        }
        let claimer = Claimer {
            stage: ClaimStage::new(self.store.clone(), pipeline),
            lanes: senders,
            next_ungrouped: AtomicUsize::new(0),
            batch_size: self.settings.batch_size,
        };
        let maintenance = tokio::spawn(maintain(
            self.store.clone(),
            is_leader.clone(),
            cancel.clone(),
        ));
        let ctl = PollCtl {
            cancel: cancel.clone(),
            is_leader,
        };
        // A lane that exits while we are not shutting down is a failure; one
        // that exits because of the shutdown must not cut the poller's
        // current pass short (it would leak that pass's permits).
        {
            let mut driving = pin!(drive(&claimer, interval, &ctl));
            loop {
                tokio::select! {
                    () = &mut driving => break,
                    Some(res) = lane_tasks.join_next() => {
                        if !cancel.is_cancelled() {
                            error!(result = ?res, "a dispatch lane stopped unexpectedly");
                            lane_tasks.abort_all();
                            // Supervised: the loop restarts with fresh lanes.
                            panic!("a dispatch lane stopped unexpectedly");
                        }
                    }
                }
            }
        }
        // No more submissions; the lanes finish what they hold and exit.
        drop(claimer);
        maintenance.abort();
        while let Some(res) = lane_tasks.join_next().await {
            if let Err(e) = res {
                error!(error = %e, "a dispatch lane failed while stopping");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::future::Future;
    use std::mem;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::sync::Mutex;

    use tokio::sync::Semaphore;
    use tokio::time::Instant as TokioInstant;

    use super::*;
    use crate::scheduler::auth::DispatchAuthService;
    use crate::scheduler::dispatcher::MessageGroupDispatcher;
    use crate::scheduler::testkit::{
        claimed, dispatcher, lock, rig, settings, FakePublisher, FakeStore, Rig, Status,
    };

    const TICK: Duration = Duration::from_secs(1);

    #[test]
    fn a_group_always_maps_to_the_same_lane() {
        // Pinned: a changed hash would reshuffle groups across lanes mid-flight
        // on upgrade (harmless) but must be a deliberate change.
        assert_eq!(
            lane_for_group("orders-1", 10),
            lane_for_group("orders-1", 10)
        );
        assert_eq!(lane_for_group("orders-1", 10), 0);
        assert_eq!(lane_for_group("", 7), 0xcbf2_9ce4_8422_2325u64 as usize % 7);
        for g in ["a", "b", "c", "orders-9", "x".repeat(300).as_str()] {
            assert!(lane_for_group(g, 3) < 3);
        }
    }

    // ── drive (the loop, with a scripted source) ────────────────────────

    /// Plays back scripted results, then empty ones; records when each
    /// pass ran.
    struct Script {
        results: Mutex<VecDeque<Result<PassReport, SchedulerError>>>,
        calls: AtomicUsize,
        at: Mutex<Vec<Duration>>,
        start: TokioInstant,
        /// Cancelled once the script is exhausted.
        done: CancellationToken,
    }

    impl Script {
        fn new(results: Vec<Result<PassReport, SchedulerError>>, done: CancellationToken) -> Self {
            Self {
                results: Mutex::new(results.into()),
                calls: AtomicUsize::new(0),
                at: Mutex::new(Vec::new()),
                start: TokioInstant::now(),
                done,
            }
        }
    }

    impl PollSource for Script {
        async fn poll(&self, _ctl: &PollCtl) -> Result<PassReport, SchedulerError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            lock(&self.at).push(self.start.elapsed());
            let next = lock(&self.results).pop_front();
            if lock(&self.results).is_empty() {
                self.done.cancel();
            }
            next.unwrap_or(Ok(PassReport::default()))
        }
    }

    fn pass(wanted: usize, claimed: usize, submitted: usize) -> Result<PassReport, SchedulerError> {
        Ok(PassReport {
            wanted,
            claimed,
            submitted,
            back_off: false,
        })
    }

    fn leader(v: bool) -> Arc<dyn Fn() -> bool + Send + Sync> {
        Arc::new(move || v)
    }

    fn ctl(is_leader: Arc<dyn Fn() -> bool + Send + Sync>, cancel: &CancellationToken) -> PollCtl {
        PollCtl {
            cancel: cancel.clone(),
            is_leader,
        }
    }

    /// Full claims follow one another with no wait; the short claim ends the
    /// drain and waits.
    #[tokio::test(start_paused = true)]
    async fn full_claims_drain_without_waiting() {
        let cancel = CancellationToken::new();
        let s = Script::new(
            vec![
                pass(100, 100, 100),
                pass(100, 100, 100),
                pass(100, 100, 100),
                pass(100, 7, 7),
            ],
            cancel.clone(),
        );
        drive(&s, TICK, &ctl(leader(true), &cancel)).await;
        assert_eq!(s.calls.load(Ordering::SeqCst), 4);
        assert!(
            lock(&s.at).iter().all(|t| *t == Duration::ZERO),
            "{:?}",
            lock(&s.at)
        );
    }

    /// A short claim waits (nothing more is due).
    #[tokio::test(start_paused = true)]
    async fn a_short_claim_waits_for_the_interval() {
        let cancel = CancellationToken::new();
        let s = Script::new(vec![pass(100, 50, 50), pass(100, 50, 50)], cancel.clone());
        drive(&s, TICK, &ctl(leader(true), &cancel)).await;
        assert_eq!(*lock(&s.at), vec![Duration::ZERO, TICK]);
    }

    /// A claim that submitted nothing (everything held) waits.
    #[tokio::test(start_paused = true)]
    async fn a_claim_that_submits_nothing_waits_for_the_interval() {
        let cancel = CancellationToken::new();
        let s = Script::new(vec![pass(100, 100, 0), pass(100, 100, 0)], cancel.clone());
        drive(&s, TICK, &ctl(leader(true), &cancel)).await;
        assert_eq!(*lock(&s.at), vec![Duration::ZERO, TICK]);
    }

    /// A pass that saw a lane fail waits however full its claim was.
    #[tokio::test(start_paused = true)]
    async fn a_lane_failure_waits_for_the_interval() {
        let cancel = CancellationToken::new();
        let failed = Ok(PassReport {
            wanted: 100,
            claimed: 100,
            submitted: 100,
            back_off: true,
        });
        let s = Script::new(vec![failed, pass(100, 100, 100)], cancel.clone());
        drive(&s, TICK, &ctl(leader(true), &cancel)).await;
        assert_eq!(*lock(&s.at), vec![Duration::ZERO, TICK]);
    }

    /// An error waits too (no immediate retry of a failing claim).
    #[tokio::test(start_paused = true)]
    async fn an_error_waits_for_the_interval() {
        let cancel = CancellationToken::new();
        let s = Script::new(
            vec![
                Err(SchedulerError::ConfigError("boom".into())),
                pass(100, 100, 100),
            ],
            cancel.clone(),
        );
        drive(&s, TICK, &ctl(leader(true), &cancel)).await;
        assert_eq!(*lock(&s.at), vec![Duration::ZERO, TICK]);
    }

    /// A non-leader never polls, however many intervals pass.
    #[tokio::test(start_paused = true)]
    async fn a_non_leader_does_not_poll() {
        let cancel = CancellationToken::new();
        let s = Script::new(vec![pass(100, 100, 100)], CancellationToken::new());
        let stop = cancel.clone();
        tokio::spawn(async move {
            time::sleep(Duration::from_secs(5)).await;
            stop.cancel();
        });
        drive(&s, TICK, &ctl(leader(false), &cancel)).await;
        assert_eq!(s.calls.load(Ordering::SeqCst), 0);
    }

    /// Losing leadership mid-drain stops the drain before the next pass.
    #[tokio::test(start_paused = true)]
    async fn leadership_is_rechecked_before_every_pass() {
        let cancel = CancellationToken::new();
        let s = Script::new(
            (0..5).map(|_| pass(100, 100, 100)).collect(),
            cancel.clone(),
        );
        let seen = Arc::new(AtomicUsize::new(0));
        let is_leader: Arc<dyn Fn() -> bool + Send + Sync> = {
            let seen = seen.clone();
            // Leader for the first two checks only.
            Arc::new(move || seen.fetch_add(1, Ordering::SeqCst) < 2)
        };
        let stop = cancel.clone();
        tokio::spawn(async move {
            time::sleep(Duration::from_millis(500)).await;
            stop.cancel();
        });
        drive(&s, TICK, &ctl(is_leader, &cancel)).await;
        assert_eq!(s.calls.load(Ordering::SeqCst), 2);
    }

    /// Cancellation mid-drain ends the drain.
    #[tokio::test(start_paused = true)]
    async fn cancellation_stops_a_drain() {
        let cancel = CancellationToken::new();
        let s = Script::new(
            (0..50).map(|_| pass(100, 100, 100)).collect(),
            CancellationToken::new(),
        );
        struct CancelAfter<'a>(&'a Script, CancellationToken);
        impl PollSource for CancelAfter<'_> {
            async fn poll(&self, ctl: &PollCtl) -> Result<PassReport, SchedulerError> {
                let r = self.0.poll(ctl).await;
                if self.0.calls.load(Ordering::SeqCst) == 2 {
                    self.1.cancel();
                }
                r
            }
        }
        drive(
            &CancelAfter(&s, cancel.clone()),
            TICK,
            &ctl(leader(true), &cancel),
        )
        .await;
        assert_eq!(s.calls.load(Ordering::SeqCst), 2);
    }

    // ── the pipeline, with a fake table and a fake broker ───────────────

    fn jobs(groups: &[&str], per_group: usize) -> Vec<(String, Option<String>)> {
        groups
            .iter()
            .flat_map(|g| {
                (0..per_group).map(move |n| (format!("{g}-{n:02}"), Some((*g).to_string())))
            })
            .collect()
    }

    fn group_of(id: &str) -> String {
        id.split('-').next().unwrap().to_string()
    }

    /// Run the poller with `pipeline` until `done` holds (checked every 10 ms
    /// of virtual time, at most `limit` of it), then stop it and wait for it.
    async fn run_until(
        rig: &Rig,
        pipeline: Arc<Pipeline>,
        is_leader: bool,
        limit: Duration,
        done: impl Fn() -> bool,
    ) -> Duration {
        let cancel = CancellationToken::new();
        let start = TokioInstant::now();
        let run = rig
            .poller
            .run_with(pipeline, TICK, leader(is_leader), cancel.clone());
        let watch = async {
            while start.elapsed() < limit && !done() {
                time::sleep(Duration::from_millis(10)).await;
            }
            cancel.cancel();
        };
        tokio::join!(run, watch);
        start.elapsed()
    }

    fn all_queued(r: &Rig, n: usize) -> impl Fn() -> bool + '_ {
        move || r.store.count(Status::Queued) == n
    }

    /// A group's jobs are published in order across many claims and lanes,
    /// including when one publish fails midway.
    #[tokio::test(start_paused = true)]
    async fn a_groups_jobs_are_published_in_order_across_claims_and_lanes() {
        let groups = ["g0", "g1", "g2", "g3", "g4", "g5"];
        let r = rig(FakeStore::with_jobs(jobs(&groups, 12)), settings(16, 3, 5));
        *lock(&r.publisher.delay) = Duration::from_millis(50);
        lock(&r.publisher.fail_once).insert("g2-05".into());
        let pipeline = Arc::new(Pipeline::new(16));
        run_until(
            &r,
            pipeline.clone(),
            true,
            Duration::from_secs(120),
            all_queued(&r, 72),
        )
        .await;

        assert_eq!(r.store.count(Status::Queued), 72);
        let per_group = r.publisher.per_group(group_of);
        for g in groups {
            let expected: Vec<String> = (0..12).map(|n| format!("{g}-{n:02}")).collect();
            assert_eq!(per_group[g], expected, "group {g} out of order");
        }
        assert_eq!(pipeline.in_flight_len(), 0);
        assert_eq!(pipeline.available(), 16, "permits are all back");
    }

    /// A claim running while a failure is handled cannot overtake it. The
    /// second claim has its generation and snapshot already when the lane
    /// fails a1 and a2; the claim then returns a3 and a4, which are dropped.
    #[tokio::test(start_paused = true)]
    async fn a_claim_running_concurrently_with_a_failure_cannot_overtake() {
        let ids = ["g-1", "g-2", "g-3", "g-4"];
        let r = rig(
            FakeStore::with_jobs(
                ids.iter()
                    .map(|i| (i.to_string(), Some("g".into())))
                    .collect(),
            ),
            settings(10, 1, 2),
        );
        let publish_gate = Arc::new(Semaphore::new(0));
        *lock(&r.publisher.gate) = Some(publish_gate.clone());
        lock(&r.publisher.fail_once).insert("g-1".into());
        let (entered, proceed) = r.store.gate_claim(2);
        let pipeline = Arc::new(Pipeline::new(10));

        let driver = async {
            // Claim 2 is inside the store, past its generation and snapshot.
            entered.notified().await;
            // Let the lane publish (and fail) the first claim's jobs, and
            // finish handling that failure.
            publish_gate.add_permits(1);
            while pipeline.in_flight_len() != 0 {
                time::sleep(Duration::from_millis(10)).await;
            }
            assert!(r.publisher.published_ids().is_empty());
            // Now claim 2 reads the table: it returns g-3 and g-4.
            proceed.add_permits(1);
            // Everything after is a normal publish.
            publish_gate.add_permits(100);
        };
        let run = run_until(
            &r,
            pipeline.clone(),
            true,
            Duration::from_secs(60),
            all_queued(&r, 4),
        );
        tokio::join!(driver, run);

        assert_eq!(
            r.publisher.published_ids(),
            ids,
            "g-3 and g-4 must not be published ahead of g-1 and g-2"
        );
        assert_eq!(pipeline.available(), 10);
    }

    /// Run the poller while `script` plays; then stop it and wait for it.
    async fn run_with_script<F: Future<Output = ()>>(
        rig: &Rig,
        pipeline: Arc<Pipeline>,
        script: F,
    ) {
        let cancel = CancellationToken::new();
        let run = rig
            .poller
            .run_with(pipeline, TICK, leader(true), cancel.clone());
        let script = async {
            script.await;
            cancel.cancel();
        };
        tokio::join!(run, script);
    }

    /// The poller blocks when the buffer is full and resumes when a lane
    /// releases permits.
    #[tokio::test(start_paused = true)]
    async fn the_poller_blocks_when_the_buffer_is_full_and_resumes() {
        let ids: Vec<(String, Option<String>)> =
            (0..20).map(|n| (format!("u-{n:02}"), None)).collect();
        let r = rig(FakeStore::with_jobs(ids), settings(4, 1, 2));
        let publish_gate = Arc::new(Semaphore::new(0));
        *lock(&r.publisher.gate) = Some(publish_gate.clone());
        let pipeline = Arc::new(Pipeline::new(4));

        run_with_script(&r, pipeline.clone(), async {
            // Publishing is stuck: the buffer fills and the poller stops
            // claiming.
            time::sleep(Duration::from_secs(5)).await;
            let claimed: usize = lock(&r.store.claims).iter().map(|c| c.returned.len()).sum();
            assert_eq!(claimed, 4, "exactly the buffer capacity");
            assert_eq!(
                r.store.claim_calls.load(Ordering::SeqCst),
                2,
                "then it blocks"
            );
            assert_eq!(pipeline.available(), 0);
            assert_eq!(r.store.count(Status::Queued), 0);

            // Releasing the broker frees permits and the poller carries on.
            publish_gate.add_permits(1000);
            while r.store.count(Status::Queued) != 20 {
                time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;

        assert_eq!(r.store.count(Status::Queued), 20);
        for c in lock(&r.store.claims).iter() {
            assert!(
                c.exclude.len() + c.returned.len() <= 4,
                "more than the capacity in flight: {c:?}"
            );
        }
        assert_eq!(pipeline.available(), 4);
    }

    /// No id is submitted twice while it is in flight, and everything is
    /// returned when the pipeline is idle.
    #[tokio::test(start_paused = true)]
    async fn no_job_is_submitted_twice_while_in_flight() {
        let ids: Vec<(String, Option<String>)> = (0..9).map(|n| (format!("u-{n}"), None)).collect();
        let r = rig(FakeStore::with_jobs(ids), settings(9, 2, 3));
        let publish_gate = Arc::new(Semaphore::new(0));
        *lock(&r.publisher.gate) = Some(publish_gate.clone());
        let pipeline = Arc::new(Pipeline::new(9));

        run_with_script(&r, pipeline.clone(), async {
            time::sleep(Duration::from_secs(3)).await;
            let returned: Vec<String> = lock(&r.store.claims)
                .iter()
                .flat_map(|c| c.returned.clone())
                .collect();
            let unique: HashSet<&String> = returned.iter().collect();
            assert_eq!(returned.len(), 9);
            assert_eq!(
                unique.len(),
                9,
                "an in-flight id was claimed again: {returned:?}"
            );
            assert_eq!(pipeline.in_flight_len(), 9);

            publish_gate.add_permits(1000);
            while r.store.count(Status::Queued) != 9 {
                time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert_eq!(pipeline.in_flight_len(), 0);
        assert_eq!(pipeline.available(), 9);
    }

    /// Permits return and the set empties across failures, drops and failed
    /// marks, once the pipeline is idle.
    #[tokio::test(start_paused = true)]
    async fn the_pipeline_is_idle_after_failures_drops_and_failed_marks() {
        let r = rig(
            FakeStore::with_jobs(jobs(&["a", "b", "c"], 10)),
            settings(12, 2, 4),
        );
        lock(&r.publisher.fail_once).extend(["a-03".to_string(), "b-07".to_string()]);
        r.store.fail_mark.store(true, Ordering::SeqCst);
        let pipeline = Arc::new(Pipeline::new(12));
        // Marks fail for a while: jobs are published but stay PENDING.
        run_until(&r, pipeline.clone(), true, Duration::from_secs(3), || false).await;
        assert_eq!(r.store.count(Status::Queued), 0);
        r.store.fail_mark.store(false, Ordering::SeqCst);
        run_until(
            &r,
            pipeline.clone(),
            true,
            Duration::from_secs(120),
            all_queued(&r, 30),
        )
        .await;
        assert_eq!(r.store.count(Status::Queued), 30);
        assert_eq!(pipeline.in_flight_len(), 0);
        assert_eq!(pipeline.available(), 12);
    }

    /// A failing broker is retried once per interval, not in a hot loop,
    /// even though every claim is full.
    #[tokio::test(start_paused = true)]
    async fn a_failing_broker_does_not_hot_loop() {
        let ids: Vec<(String, Option<String>)> =
            (0..20).map(|n| (format!("u-{n:02}"), None)).collect();
        let r = rig(FakeStore::with_jobs(ids), settings(20, 2, 5));
        r.publisher.fail_all.store(true, Ordering::SeqCst);
        let pipeline = Arc::new(Pipeline::new(20));
        run_until(&r, pipeline, true, Duration::from_secs(10), || false).await;
        let calls = r.store.claim_calls.load(Ordering::SeqCst);
        assert!((2..=25).contains(&calls), "{calls} claims in 10 s");
        assert_eq!(r.store.count(Status::Queued), 0);
    }

    /// The same for a failing status update.
    #[tokio::test(start_paused = true)]
    async fn a_failing_mark_does_not_hot_loop() {
        let ids: Vec<(String, Option<String>)> =
            (0..20).map(|n| (format!("u-{n:02}"), None)).collect();
        let r = rig(FakeStore::with_jobs(ids), settings(20, 2, 5));
        r.store.fail_mark.store(true, Ordering::SeqCst);
        let pipeline = Arc::new(Pipeline::new(20));
        run_until(&r, pipeline, true, Duration::from_secs(10), || false).await;
        let calls = r.store.claim_calls.load(Ordering::SeqCst);
        assert!((2..=25).contains(&calls), "{calls} claims in 10 s");
    }

    /// A failing claim waits for the interval too.
    #[tokio::test(start_paused = true)]
    async fn a_failing_claim_does_not_hot_loop() {
        let r = rig(FakeStore::with_jobs(Vec::new()), settings(20, 2, 5));
        r.store.fail_claim.store(true, Ordering::SeqCst);
        run_until(
            &r,
            Arc::new(Pipeline::new(20)),
            true,
            Duration::from_secs(10),
            || false,
        )
        .await;
        let calls = r.store.claim_calls.load(Ordering::SeqCst);
        assert!((2..=25).contains(&calls), "{calls} claims in 10 s");
    }

    /// With nothing to claim (everything held or paused is excluded by the
    /// query, so the claim is empty) the poller waits.
    #[tokio::test(start_paused = true)]
    async fn an_empty_claim_does_not_hot_loop() {
        let r = rig(FakeStore::with_jobs(Vec::new()), settings(20, 2, 5));
        run_until(
            &r,
            Arc::new(Pipeline::new(20)),
            true,
            Duration::from_secs(10),
            || false,
        )
        .await;
        let calls = r.store.claim_calls.load(Ordering::SeqCst);
        assert!((2..=25).contains(&calls), "{calls} claims in 10 s");
    }

    /// A non-leader never claims.
    #[tokio::test(start_paused = true)]
    async fn a_non_leader_pipeline_never_claims() {
        let ids: Vec<(String, Option<String>)> = (0..5).map(|n| (format!("u-{n}"), None)).collect();
        let r = rig(FakeStore::with_jobs(ids), settings(20, 2, 5));
        let pipeline = Arc::new(Pipeline::new(20));
        run_until(&r, pipeline.clone(), false, Duration::from_secs(10), || {
            false
        })
        .await;
        assert_eq!(r.store.claim_calls.load(Ordering::SeqCst), 0);
        assert_eq!(pipeline.available(), 20);
    }

    /// Leadership lost while the poller waits for buffer space: no claim
    /// follows, and the permits go back.
    #[tokio::test(start_paused = true)]
    async fn leadership_lost_during_the_wait_for_permits_means_no_claim() {
        let ids: Vec<(String, Option<String>)> = (0..8).map(|n| (format!("u-{n}"), None)).collect();
        let r = rig(FakeStore::with_jobs(ids), settings(4, 1, 4));
        let publish_gate = Arc::new(Semaphore::new(0));
        *lock(&r.publisher.gate) = Some(publish_gate.clone());
        let pipeline = Arc::new(Pipeline::new(4));
        let leading = Arc::new(AtomicBool::new(true));
        let flag = leading.clone();
        let cancel = CancellationToken::new();
        let run = r.poller.run_with(
            pipeline.clone(),
            TICK,
            Arc::new(move || flag.load(Ordering::SeqCst)),
            cancel.clone(),
        );
        let driver = async {
            time::sleep(Duration::from_secs(2)).await;
            // Blocked on permits (4 held by stuck lanes). Lose leadership,
            // then free the lane.
            assert_eq!(r.store.claim_calls.load(Ordering::SeqCst), 1);
            leading.store(false, Ordering::SeqCst);
            publish_gate.add_permits(1000);
            time::sleep(Duration::from_secs(5)).await;
            cancel.cancel();
        };
        tokio::join!(run, driver);
        assert_eq!(
            r.store.claim_calls.load(Ordering::SeqCst),
            1,
            "no claim after leadership was lost"
        );
    }

    /// Shutdown mid-batch: the batch being sent is finished and marked, the
    /// buffered rest stays PENDING, and the run returns when the batch does.
    #[tokio::test(start_paused = true)]
    async fn shutdown_mid_batch_finishes_the_batch_and_leaves_the_rest_pending() {
        let ids: Vec<(String, Option<String>)> =
            (0..20).map(|n| (format!("u-{n:02}"), None)).collect();
        let settings = PollerSettings {
            lane_batch: 4,
            ..settings(20, 1, 20)
        };
        let r = rig(FakeStore::with_jobs(ids), settings);
        *lock(&r.publisher.delay) = Duration::from_secs(5);
        let pipeline = Arc::new(Pipeline::new(20));
        let cancel = CancellationToken::new();
        let run = r
            .poller
            .run_with(pipeline, TICK, leader(true), cancel.clone());
        let start = TokioInstant::now();
        let stopper = async {
            time::sleep(Duration::from_secs(1)).await;
            cancel.cancel();
        };
        tokio::join!(run, stopper);
        let took = start.elapsed();
        assert!(
            took >= Duration::from_secs(5),
            "{took:?}: the batch was finished"
        );
        assert!(took < Duration::from_secs(6), "{took:?}: and nothing more");
        assert_eq!(r.store.count(Status::Queued), 5, "the batch in flight");
        assert_eq!(r.store.count(Status::Pending), 15, "the buffered rest");
        assert_eq!(r.publisher.calls.load(Ordering::SeqCst), 1);
    }

    /// A publish that hangs does not hold a shutdown past its deadline, and
    /// marks nothing.
    #[tokio::test(start_paused = true)]
    async fn shutdown_with_a_hung_publish_returns_at_the_publish_deadline() {
        let ids: Vec<(String, Option<String>)> = (0..6).map(|n| (format!("u-{n}"), None)).collect();
        let publisher = Arc::new(FakePublisher::default());
        *lock(&publisher.gate) = Some(Arc::new(Semaphore::new(0)));
        let hung = Arc::new(
            MessageGroupDispatcher::new(
                publisher.clone(),
                DispatchAuthService::with_secret("s"),
                "http://x/process".into(),
            )
            .with_publish_timeout(Duration::from_secs(2)),
        );
        let store = FakeStore::with_jobs(ids);
        let poller = PendingJobPoller::with_store(store.clone(), hung, settings(10, 1, 10));
        let cancel = CancellationToken::new();
        let run = poller.run_with(
            Arc::new(Pipeline::new(10)),
            TICK,
            leader(true),
            cancel.clone(),
        );
        let start = TokioInstant::now();
        let stopper = async {
            time::sleep(Duration::from_millis(500)).await;
            cancel.cancel();
        };
        tokio::join!(run, stopper);
        assert_eq!(start.elapsed(), Duration::from_secs(2));
        assert_eq!(store.count(Status::Queued), 0);
        assert_eq!(store.count(Status::Pending), 6);
    }

    /// Grouped jobs always go to the same lane; ungrouped ones round-robin.
    #[tokio::test]
    async fn jobs_are_routed_by_group_and_ungrouped_ones_round_robin() {
        let lanes = 4;
        let (senders, mut receivers): (Vec<_>, Vec<_>) =
            (0..lanes).map(|_| mpsc::channel::<LaneJob>(64)).unzip();
        let rows: Vec<ClaimedJob> = (0..8)
            .map(|n| claimed(&format!("u-{n}"), None))
            .chain((0..8).map(|n| claimed(&format!("g-{n}"), Some("orders-1"))))
            .collect();
        let store = Arc::new(FixedStore(Mutex::new(rows)));
        let pipeline = Arc::new(Pipeline::new(64));
        let claimer = Claimer {
            stage: ClaimStage::new(store, pipeline.clone()),
            lanes: senders,
            next_ungrouped: AtomicUsize::new(0),
            batch_size: 64,
        };
        let want = pipeline.acquire_up_to(64).await;
        let report = claimer.claim_pass(want).await.unwrap();
        assert_eq!(report.submitted, 16);
        let mut grouped_lanes = HashSet::new();
        let mut ungrouped_per_lane = vec![0; lanes];
        for (i, rx) in receivers.iter_mut().enumerate() {
            while let Ok(j) = rx.try_recv() {
                if j.job.group().is_some() {
                    grouped_lanes.insert(i);
                } else {
                    ungrouped_per_lane[i] += 1;
                }
                assert_eq!(j.generation, 1);
            }
        }
        assert_eq!(grouped_lanes.len(), 1, "one group, one lane");
        assert_eq!(ungrouped_per_lane, vec![2; lanes]);
        assert_eq!(pipeline.in_flight_len(), 16);
        assert_eq!(pipeline.available(), 64 - 16);
    }

    /// A store that returns its rows once.
    struct FixedStore(Mutex<Vec<ClaimedJob>>);

    #[async_trait]
    impl JobStore for FixedStore {
        async fn claim(
            &self,
            _limit: usize,
            _skip_groups: &[String],
            _in_flight: &[String],
        ) -> Result<ClaimOutcome, SchedulerError> {
            let jobs = mem::take(&mut *lock(&self.0));
            Ok(ClaimOutcome {
                taken: jobs.len(),
                jobs,
                held_groups: Vec::new(),
            })
        }
        async fn mark_queued(&self, jobs: &[MarkKey]) -> Result<u64, SchedulerError> {
            Ok(jobs.len() as u64)
        }
    }

    /// `poll_once`: claim, publish, mark, report.
    #[tokio::test]
    async fn poll_once_publishes_and_marks() {
        let r = rig(FakeStore::with_jobs(jobs(&["a"], 3)), settings(10, 1, 10));
        lock(&r.publisher.fail_ids).insert("a-01".into());
        let report = r.poller.poll_once().await.unwrap();
        assert_eq!(
            report,
            PollReport {
                claimed: 3,
                published: 1
            }
        );
        assert_eq!(r.store.ids_with(Status::Queued), vec!["a-00"]);
    }

    /// The claim must not skip a doomed job. j2 waits in the lane, doomed
    /// (j1 failed and poisoned the group); a claim taken after the poison
    /// excludes j2 (in flight) and would take j3 behind it. Its jobs of the
    /// group are not submitted; once j2 is dropped, the next claim takes
    /// j1, j2, j3 in order.
    #[tokio::test]
    async fn a_claim_that_skipped_a_doomed_job_does_not_submit_the_jobs_behind_it() {
        let ids: Vec<(String, Option<String>)> = ["j1", "j2", "j3"]
            .iter()
            .map(|i| (i.to_string(), Some("g".to_string())))
            .collect();
        let r = rig(FakeStore::with_jobs(ids), settings(10, 1, 2));
        lock(&r.publisher.fail_once).insert("j1".into());
        let pipeline = Arc::new(Pipeline::new(10));
        let stage = ClaimStage::new(r.store.clone(), pipeline.clone());
        let mut lane = Lane::new(
            0,
            pipeline.clone(),
            r.store.clone(),
            dispatcher(r.publisher.clone()),
            0,
        );
        let take = |n: usize| {
            let pipeline = pipeline.clone();
            async move {
                let mut got = 0;
                while got < n {
                    got += pipeline.acquire_up_to(n - got).await;
                }
            }
        };

        // Claim A takes j1 and j2; the lane receives them in two batches.
        take(2).await;
        let (mut a, _) = stage.claim(2).await.unwrap();
        let (a2, a1) = (a.pop().unwrap(), a.pop().unwrap());
        assert_eq!((a1.job.id(), a2.job.id()), ("j1", "j2"));
        // j1 fails: poison at the current generation.
        assert_eq!(lane.process(vec![a1]).await.unpublished, 1);

        // Claim B: j2 is still in flight (and doomed), so the store returns
        // j1 and j3; neither may be submitted.
        take(2).await;
        let (b, claimed) = stage.claim(2).await.unwrap();
        assert_eq!(claimed, 2, "the store did return j1 and j3");
        assert!(b.is_empty(), "submitted behind a doomed job: {b:?}");
        assert_eq!(pipeline.available(), 10 - 1, "only j2 holds a permit");

        // The lane drops j2; claim C sees everything, in order.
        assert_eq!(lane.process(vec![a2]).await.dropped, 1);
        take(3).await;
        let (c, _) = stage.claim(3).await.unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(lane.process(c).await.published, 3);
        assert_eq!(r.publisher.published_ids(), vec!["j1", "j2", "j3"]);
        assert_eq!(pipeline.available(), 10);
    }

    /// The claim excludes the ids this process is publishing (the in-flight
    /// set): a job that is still PENDING in the table while its publish is
    /// under way is not claimed again.
    #[tokio::test]
    async fn the_claim_excludes_the_in_flight_ids() {
        let ids: Vec<(String, Option<String>)> = ["a", "b", "c"]
            .iter()
            .map(|i| (i.to_string(), None))
            .collect();
        let r = rig(FakeStore::with_jobs(ids), settings(10, 1, 5));
        let pipeline = Arc::new(Pipeline::new(10));
        let stage = ClaimStage::new(r.store.clone(), pipeline.clone());
        assert!(pipeline.add_in_flight(&claimed("a", None), 1));
        assert_eq!(pipeline.acquire_up_to(5).await, 5);
        let (jobs, _) = stage.claim(5).await.unwrap();
        let got: Vec<&str> = jobs.iter().map(|j| j.job.id()).collect();
        assert_eq!(got, vec!["b", "c"]);
        assert_eq!(lock(&r.store.claims)[0].exclude, vec!["a".to_string()]);
    }

    /// A row the claim returns although its id is in flight cannot happen (it
    /// was excluded); if it does it is dropped and counted, never submitted
    /// twice, and nothing behind it in its group goes ahead of it.
    #[tokio::test]
    async fn a_row_returned_for_an_id_already_in_flight_is_dropped_with_its_group() {
        struct Echo(Mutex<Vec<ClaimedJob>>);
        #[async_trait]
        impl JobStore for Echo {
            async fn claim(
                &self,
                _: usize,
                _: &[String],
                _: &[String],
            ) -> Result<ClaimOutcome, SchedulerError> {
                let jobs = mem::take(&mut *lock(&self.0));
                Ok(ClaimOutcome {
                    taken: jobs.len(),
                    jobs,
                    held_groups: Vec::new(),
                })
            }
            async fn mark_queued(&self, jobs: &[MarkKey]) -> Result<u64, SchedulerError> {
                Ok(jobs.len() as u64)
            }
        }
        let rows = vec![
            claimed("j1", Some("g")),
            claimed("j2", Some("g")),
            claimed("x1", Some("h")),
        ];
        let pipeline = Arc::new(Pipeline::new(10));
        let stage = ClaimStage::new(Arc::new(Echo(Mutex::new(rows))), pipeline.clone());
        assert!(pipeline.add_in_flight(&claimed("j1", Some("g")), 1));
        assert_eq!(pipeline.acquire_up_to(3).await, 3);
        let (jobs, taken) = stage.claim(3).await.unwrap();
        assert_eq!(taken, 3);
        let got: Vec<&str> = jobs.iter().map(|j| j.job.id()).collect();
        assert_eq!(got, vec!["x1"], "j1 is a duplicate, j2 is behind it");
        assert_eq!(pipeline.available(), 10 - 1);
    }

    /// STARVATION: more than a batch of held rows at the head of the order
    /// must not keep the jobs behind them from being claimed. The poller
    /// remembers the held group and skips it for a few seconds; without that
    /// every claim would take, find held and put back the same head rows.
    #[tokio::test(start_paused = true)]
    async fn a_batch_full_of_held_rows_at_the_head_does_not_starve_the_groups_behind() {
        let mut jobs: Vec<(String, Option<String>)> = (0..100)
            .map(|n| (format!("a-{n:03}"), Some("a".to_string())))
            .collect();
        jobs.push(("z-000".into(), Some("z".into())));
        let r = rig(FakeStore::with_jobs(jobs), settings(50, 2, 10));
        lock(&r.store.hold_groups).insert("a".into());
        run_until(
            &r,
            Arc::new(Pipeline::new(50)),
            true,
            Duration::from_secs(30),
            || r.store.status_of("z-000") == Status::Queued,
        )
        .await;
        assert_eq!(r.store.status_of("z-000"), Status::Queued, "z was starved");
        assert_eq!(
            r.store.count(Status::Pending),
            100,
            "the held rows are back"
        );
        let skips: Vec<usize> = lock(&r.store.claims).iter().map(|c| c.skip.len()).collect();
        assert!(
            skips.contains(&1),
            "a later claim skipped the held group: {skips:?}"
        );
    }

    /// A held group is remembered for 5 seconds and then tried again; the
    /// memory is capped and drops the oldest first.
    #[tokio::test(start_paused = true)]
    async fn the_held_group_memory_expires_after_five_seconds_and_is_capped() {
        let held = HeldGroups::new(HELD_GROUP_TTL, HELD_GROUP_CAP);
        held.remember(&["a".to_string()]);
        time::advance(Duration::from_secs(4)).await;
        assert_eq!(held.active(), vec!["a".to_string()]);
        time::advance(Duration::from_secs(2)).await;
        assert!(held.active().is_empty(), "retried after 5 seconds");

        let capped = HeldGroups::new(Duration::from_secs(60), 3);
        for g in ["g1", "g2", "g3", "g4", "g5"] {
            capped.remember(&[g.to_string()]);
            time::advance(Duration::from_millis(10)).await;
        }
        assert_eq!(capped.len(), 3);
        let mut now: Vec<String> = capped.active();
        now.sort();
        assert_eq!(now, ["g3", "g4", "g5"], "the oldest were dropped");
        assert_eq!(HELD_GROUP_CAP, 10_000);
    }

    /// The claim stage skips what the memory holds, and asks again after 5 s.
    #[tokio::test(start_paused = true)]
    async fn the_claim_skips_remembered_groups_until_they_expire() {
        let ids: Vec<(String, Option<String>)> = vec![
            ("a-0".into(), Some("a".into())),
            ("b-0".into(), Some("b".into())),
        ];
        let r = rig(FakeStore::with_jobs(ids), settings(10, 1, 5));
        lock(&r.store.hold_groups).insert("a".into());
        let pipeline = Arc::new(Pipeline::new(10));
        let stage = ClaimStage::new(r.store.clone(), pipeline.clone());
        let take = || async { assert_eq!(pipeline.acquire_up_to(5).await, 5) };
        take().await;
        let (jobs, taken) = stage.claim(5).await.unwrap();
        assert_eq!((jobs.len(), taken), (1, 2), "b claimed, a found held");
        take().await;
        let (jobs, taken) = stage.claim(5).await.unwrap();
        assert_eq!(
            (jobs.len(), taken),
            (0, 0),
            "a is skipped: nothing else to take"
        );
        time::advance(Duration::from_secs(6)).await;
        take().await;
        let (_, taken) = stage.claim(5).await.unwrap();
        assert_eq!(taken, 1, "a is tried again after 5 s");
        let skips: Vec<usize> = lock(&r.store.claims).iter().map(|c| c.skip.len()).collect();
        assert_eq!(skips, vec![0, 1, 0]);
    }

    /// Real threads, real time, random publish and mark failures, a small
    /// buffer, and the race windows of the ordering rule widened: for every
    /// group the broker's FIRST delivery of each job must be in claim order
    /// (duplicates are fine: a job published and then not marked is
    /// published again).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stress_first_deliveries_stay_in_claim_order() {
        const GROUPS: usize = 30;
        const PER_GROUP: usize = 100;
        let ids: Vec<(String, Option<String>)> = (0..GROUPS)
            .flat_map(|g| {
                (0..PER_GROUP).map(move |n| (format!("g{g:02}-{n:03}"), Some(format!("g{g:02}"))))
            })
            .collect();
        let settings = PollerSettings {
            lane_batch: 4,
            ..settings(24, 4, 8)
        };
        let r = rig(FakeStore::with_jobs(ids), settings);
        r.publisher.fail_per_mille.store(30, Ordering::Relaxed);
        r.publisher.jitter_us.store(300, Ordering::Relaxed);
        r.store.mark_fail_per_mille.store(10, Ordering::Relaxed);
        r.store.claim_jitter_us.store(300, Ordering::Relaxed);
        let pipeline = Arc::new(Pipeline::new(24));
        pipeline.set_race_pause(150);

        let cancel = CancellationToken::new();
        let total = GROUPS * PER_GROUP;
        let run = r.poller.run_with(
            pipeline.clone(),
            Duration::from_millis(1),
            leader(true),
            cancel.clone(),
        );
        let watch = async {
            let finished = time::timeout(Duration::from_secs(30), async {
                while r.store.count(Status::Queued) != total {
                    time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await;
            cancel.cancel();
            finished
        };
        let (_, finished) = tokio::join!(run, watch);
        assert!(
            finished.is_ok(),
            "not all jobs were queued in time: {} pending, {} permits free of 24, {} in flight, {} claims, {} publishes",
            r.store.count(Status::Pending),
            pipeline.available(),
            pipeline.in_flight_len(),
            r.store.claim_calls.load(Ordering::SeqCst),
            r.publisher.calls.load(Ordering::SeqCst),
        );

        let mut last_first: HashMap<String, usize> = HashMap::new();
        let mut seen: HashSet<String> = HashSet::new();
        for id in r.publisher.published_ids() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let (group, n) = id.split_once('-').unwrap();
            let n: usize = n.parse().unwrap();
            if let Some(prev) = last_first.insert(group.to_string(), n) {
                assert!(
                    n > prev,
                    "{id} was first delivered after {group}-{prev:03}: out of claim order"
                );
            }
        }
        assert_eq!(seen.len(), total, "every job was delivered");
        assert_eq!(pipeline.in_flight_len(), 0);
        assert_eq!(pipeline.available(), 24);
    }
}
