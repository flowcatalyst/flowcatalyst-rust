//! Pending job poller.
//!
//! A port of Go's `PendingJobPoller` (`scheduler/poller.go`). One tick:
//!
//! 1. In one transaction, claim up to `batch_size` PENDING jobs that are due
//!    (`scheduled_for` NULL or past) with `FOR UPDATE SKIP LOCKED`, in the
//!    total order `(message_group NULLS LAST, sequence, created_at, id)`.
//!    Concurrent schedulers therefore never claim the same row.
//! 2. With the rows still locked, publish the claim in one call (see
//!    [`MessageGroupDispatcher`]), mark exactly the published ids QUEUED, and
//!    commit.
//!
//! **Deliberately not Go's order.** Go commits the claim QUEUED first and
//! publishes after; a worker that dies between the two (a SIGKILL, an OOM, a
//! deploy past its stop timeout) leaves the unpublished rows QUEUED with no
//! queue message, and nothing looks at them again until stale recovery's
//! 75 minutes are up (delivery run 3, `worker-restart`: Go stranded the last
//! two jobs of its publish). Here the claim only commits once the publish is
//! done, so a worker that dies mid-publish rolls the whole claim back to
//! PENDING and the next poll (its own after a restart, or a standby's)
//! publishes it again. The price is at-least-once at the queue: the jobs it
//! did publish before dying are published a second time. That costs no
//! second delivery — `/api/dispatch/process` claims a job before delivering
//! it, and a copy that finds the job taken or finished never delivers it —
//! only a second, redundant queue message. A `/process` call that arrives
//! for a job before this commit waits on the row lock for it.
//!
//! Held and paused jobs are excluded **inside the claim query**, not after
//! it (a deliberate improvement on Go, which filters after the `LIMIT`: a
//! full batch of held or paused rows at the head of the order would stall
//! every other group). Excluded:
//! - jobs whose subscription's connection is PAUSED (cached set, refreshed
//!   every `paused_cache_ttl`);
//! - BLOCK_ON_ERROR jobs with an EARLIER job in their group that is holding
//!   it: FAILED/ERROR, or PENDING in a retry backoff
//!   ([`GROUP_HOLDING_STATUS_SQL`]). The comparison is positional, so the
//!   holder itself dispatches once its backoff expires. IMMEDIATE and
//!   NEXT_ON_ERROR (and, per X-01, an absent or unknown mode) never wait.
//!
//! A NULL group never holds (`=` never matches NULL); an ungrouped job is
//! held only by a row whose group is literally `default` — Go's quirk,
//! kept.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use sqlx::PgPool;
use tracing::{debug, trace, warn};

use super::destination::PoolCodeResolver;
use super::dispatcher::{DispatchJobToken, MessageGroupDispatcher};
use super::SchedulerError;
use std::collections::HashMap;
use std::collections::HashSet;
use tokio::time;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::field::Empty;

/// A job that is holding its message group (Go `GroupHoldingStatusSQL`):
/// terminally failed, or waiting out a retry backoff. QUEUED and PROCESSING
/// are the normal flow and hold nothing. Unqualified column names; callers
/// qualify by aliasing the table.
pub const GROUP_HOLDING_STATUS_SQL: &str =
    "status IN ('FAILED', 'ERROR') OR (status = 'PENDING' AND scheduled_for IS NOT NULL AND scheduled_for > NOW())";

/// The claim query: due PENDING jobs, minus paused subscriptions and held
/// BLOCK_ON_ERROR successors, locked and totally ordered.
const CLAIM_SQL: &str = "\
SELECT j.id, j.subscription_id, j.message_group, j.mode, j.dispatch_pool_id, j.client_id, \
       j.created_at, j.queue \
  FROM msg_dispatch_jobs j \
 WHERE j.status = 'PENDING' \
   AND (j.scheduled_for IS NULL OR j.scheduled_for <= NOW()) \
   AND (j.subscription_id IS NULL OR NOT (j.subscription_id = ANY($2::text[]))) \
   AND NOT (j.mode = 'BLOCK_ON_ERROR' AND EXISTS ( \
        SELECT 1 FROM msg_dispatch_jobs h \
         WHERE h.message_group = COALESCE(j.message_group, 'default') \
           AND (h.status IN ('FAILED', 'ERROR') \
                OR (h.status = 'PENDING' AND h.scheduled_for IS NOT NULL AND h.scheduled_for > NOW())) \
           AND (h.sequence, h.created_at, h.id) < (j.sequence, j.created_at, j.id))) \
 ORDER BY j.message_group ASC NULLS LAST, j.sequence ASC, j.created_at ASC, j.id ASC \
 LIMIT $1 \
 FOR UPDATE OF j SKIP LOCKED";

#[derive(Debug, sqlx::FromRow)]
struct ClaimRow {
    id: String,
    subscription_id: Option<String>,
    message_group: Option<String>,
    mode: String,
    dispatch_pool_id: Option<String>,
    client_id: Option<String>,
    created_at: DateTime<Utc>,
    queue: Option<String>,
}

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

pub struct PendingJobPoller {
    pool: PgPool,
    batch_size: usize,
    dispatcher: Arc<MessageGroupDispatcher>,
    paused: PausedConnectionCache,
    pool_codes: Arc<PoolCodeResolver>,
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
        "Polls whose claim filled the batch (a backlog is likely waiting)."
    );
    describe_counter!(
        "scheduler.poll.errors_total",
        "Polls that failed (claim, publish bookkeeping, mark QUEUED or commit)."
    );
    describe_histogram!(
        "scheduler.poll.duration_seconds",
        Unit::Seconds,
        "Wall time of one poll pass, including the publish."
    );
    describe_histogram!(
        "scheduler.publish.duration_seconds",
        Unit::Seconds,
        "Wall time of publishing one claim to the queues."
    );
    describe_counter!(
        "scheduler.publish.timeouts_total",
        "Claims whose publish hit the deadline and was abandoned."
    );
    describe_gauge!(
        "scheduler.poll.last_success_timestamp_seconds",
        Unit::Seconds,
        "Unix time of the last poll that completed without error."
    );
}

/// What one tick did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PollReport {
    pub claimed: usize,
    pub published: usize,
}

impl PendingJobPoller {
    pub fn new(
        pool: PgPool,
        batch_size: usize,
        paused_cache_ttl: Duration,
        dispatcher: Arc<MessageGroupDispatcher>,
        pool_codes: Arc<PoolCodeResolver>,
    ) -> Self {
        describe_metrics();
        Self {
            paused: PausedConnectionCache::new(pool.clone(), paused_cache_ttl),
            pool,
            batch_size,
            dispatcher,
            pool_codes,
        }
    }

    /// One pass: claim, publish, mark the published ids QUEUED, commit. Runs
    /// in a `scheduler.poll` span carrying how many jobs it claimed and
    /// published, and is measured: `scheduler.poll.duration_seconds`
    /// (every pass, including the publish), `scheduler.poll.errors_total`,
    /// and, on success, `scheduler.poll.last_success_timestamp_seconds`.
    #[tracing::instrument(
        name = "scheduler.poll",
        skip_all,
        fields(claimed = Empty, published = Empty)
    )]
    pub async fn poll_once(&self) -> Result<PollReport, SchedulerError> {
        let started = Instant::now();
        let result = self.poll_pass().await;
        metrics::histogram!("scheduler.poll.duration_seconds").record(started.elapsed());
        match &result {
            Ok(_) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0.0, |d| d.as_secs_f64());
                metrics::gauge!("scheduler.poll.last_success_timestamp_seconds").set(now);
            }
            Err(_) => metrics::counter!("scheduler.poll.errors_total").increment(1),
        }
        result
    }

    async fn poll_pass(&self) -> Result<PollReport, SchedulerError> {
        let paused = self.paused.paused_subscription_ids().await?;

        let mut tx = self.pool.begin().await?;
        let claimed: Vec<ClaimRow> = sqlx::query_as(CLAIM_SQL)
            .bind(self.batch_size as i64)
            .bind(&paused)
            .fetch_all(&mut *tx)
            .await?;
        if claimed.is_empty() {
            tx.rollback().await.ok();
            trace!("no pending jobs");
            return Ok(PollReport::default());
        }

        let created: HashMap<String, DateTime<Utc>> = claimed
            .iter()
            .map(|c| (c.id.clone(), c.created_at))
            .collect();
        let mut tokens = Vec::with_capacity(claimed.len());
        for c in claimed {
            let pool_code = self
                .pool_codes
                .resolve(c.dispatch_pool_id.as_deref(), c.client_id.as_deref())
                .await;
            tokens.push(DispatchJobToken {
                job_id: c.id,
                message_group: c.message_group,
                mode: c.mode,
                pool_code,
                client_id: c.client_id,
                subscription_id: c.subscription_id,
                queue: c.queue,
            });
        }
        let claimed = tokens.len();
        tracing::Span::current().record("claimed", claimed);
        // `scheduler.pending_jobs` is the size of THIS claim (at most the
        // batch size), not the PENDING backlog; the name is kept for
        // existing dashboards.
        metrics::gauge!("scheduler.pending_jobs").set(claimed as f64);
        metrics::counter!("scheduler.jobs.claimed_total").increment(claimed as u64);
        if claimed >= self.batch_size {
            metrics::counter!("scheduler.poll.full_batches_total").increment(1);
        }

        // Publish while the claim is still locked and uncommitted: see the
        // module doc. What did not publish simply stays PENDING.
        let outcome = self.dispatcher.publish_claim(&tokens).await;
        let unpublished: HashSet<&str> = outcome.unpublished.iter().map(String::as_str).collect();
        let (ids, created_ats): (Vec<&str>, Vec<DateTime<Utc>>) = tokens
            .iter()
            .map(|t| t.job_id.as_str())
            .filter(|id| !unpublished.contains(id))
            .map(|id| (id, created[id]))
            .unzip();
        let published = ids.len();
        tracing::Span::current().record("published", published);
        if published == 0 {
            tx.rollback().await.ok();
            debug!(claimed, published, "poll tick");
            return Ok(PollReport { claimed, published });
        }
        let marked = async {
            sqlx::query(
                "UPDATE msg_dispatch_jobs SET status = 'QUEUED', queued_at = NOW(), updated_at = NOW() \
                 FROM UNNEST($1::varchar[], $2::timestamptz[]) AS t(id, created_at) \
                 WHERE msg_dispatch_jobs.id = t.id AND msg_dispatch_jobs.created_at = t.created_at",
            )
            .bind(&ids)
            .bind(&created_ats)
            .execute(&mut *tx)
            .await?;
            tx.commit().await
        }
        .await;
        if let Err(e) = marked {
            // Published but still PENDING: the next poll publishes them
            // again, and `/process` delivers each once.
            warn!(published, error = %e,
                "marking published dispatch jobs QUEUED failed; they will be published again");
            return Err(e.into());
        }
        debug!(claimed, published, "poll tick");
        Ok(PollReport { claimed, published })
    }

    /// Drive the poller every `interval` until cancelled, only while
    /// `is_leader` says so (the per-group order needs one active scheduler).
    /// A full batch that published something is followed by another pass at
    /// once rather than by a sleep: see [`drive`].
    pub async fn run(
        &self,
        interval: Duration,
        is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
        cancel: CancellationToken,
    ) {
        drive(self, self.batch_size, interval, is_leader, cancel).await;
    }
}

/// One poll pass, so [`drive`] can be tested without a database.
pub(crate) trait PollSource {
    async fn poll(&self) -> Result<PollReport, SchedulerError>;
}

impl PollSource for PendingJobPoller {
    async fn poll(&self) -> Result<PollReport, SchedulerError> {
        self.poll_once().await
    }
}

/// Whether `report` says there is probably more waiting: the claim filled
/// the batch and at least one job went out. A claim that published nothing
/// (a failing broker) must not become a hot loop, so it falls back to the
/// interval.
fn more_waiting(report: PollReport, batch_size: usize) -> bool {
    report.claimed >= batch_size && report.published > 0
}

/// The poller loop. Each interval tick runs a pass; while passes keep
/// filling the batch and publishing, the next pass follows immediately
/// (a backlog drains at the speed of the broker, not at `batch / interval`).
/// Leadership and cancellation are re-checked before every pass. A short
/// batch, a pass that published nothing, or an error returns to the
/// interval, so a failing mark/commit is retried at most once per tick.
pub(crate) async fn drive<S: PollSource>(
    source: &S,
    batch_size: usize,
    interval: Duration,
    is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
    cancel: CancellationToken,
) {
    let mut tick = time::interval(interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tick.tick() => {}
        }
        loop {
            if cancel.is_cancelled() || !is_leader() {
                break;
            }
            match source.poll().await {
                Ok(report) if more_waiting(report, batch_size) => {}
                Ok(_) => break,
                Err(e) => {
                    warn!(error = %e, "dispatch poll error");
                    break;
                }
            }
        }
        if cancel.is_cancelled() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_sql_locks_only_the_job_rows_and_orders_totally() {
        assert!(CLAIM_SQL.contains("FOR UPDATE OF j SKIP LOCKED"));
        assert!(CLAIM_SQL.contains(
            "ORDER BY j.message_group ASC NULLS LAST, j.sequence ASC, j.created_at ASC, j.id ASC"
        ));
        assert!(CLAIM_SQL.contains("j.scheduled_for IS NULL OR j.scheduled_for <= NOW()"));
    }

    /// The claim inlines the holding condition under an alias; it must be
    /// the condition /process checks at delivery time.
    #[test]
    fn the_claim_hold_matches_the_shared_definition() {
        let normalise = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let claim = normalise(CLAIM_SQL);
        let aliased = GROUP_HOLDING_STATUS_SQL
            .replace("status", "h.status")
            .replace("scheduled_for", "h.scheduled_for");
        let (failed, backoff) = aliased.split_once(" OR ").unwrap();
        assert!(claim.contains(failed), "{claim}");
        assert!(claim.contains(backoff), "{claim}");
    }

    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::time::Instant as TokioInstant;

    /// Plays back scripted results, then empty ones; records when each
    /// pass ran.
    struct Script {
        results: parking_lot::Mutex<VecDeque<Result<PollReport, SchedulerError>>>,
        calls: AtomicUsize,
        at: parking_lot::Mutex<Vec<Duration>>,
        start: TokioInstant,
        /// Cancelled once the script is exhausted.
        done: CancellationToken,
    }

    impl Script {
        fn new(results: Vec<Result<PollReport, SchedulerError>>, done: CancellationToken) -> Self {
            Self {
                results: parking_lot::Mutex::new(results.into()),
                calls: AtomicUsize::new(0),
                at: parking_lot::Mutex::new(Vec::new()),
                start: TokioInstant::now(),
                done,
            }
        }
    }

    impl PollSource for Script {
        async fn poll(&self) -> Result<PollReport, SchedulerError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.at.lock().push(self.start.elapsed());
            let next = self.results.lock().pop_front();
            if self.results.lock().is_empty() {
                self.done.cancel();
            }
            next.unwrap_or(Ok(PollReport::default()))
        }
    }

    fn report(claimed: usize, published: usize) -> Result<PollReport, SchedulerError> {
        Ok(PollReport { claimed, published })
    }

    fn leader(v: bool) -> Arc<dyn Fn() -> bool + Send + Sync> {
        Arc::new(move || v)
    }

    const TICK: Duration = Duration::from_secs(1);

    /// Full batches follow one another with no interval wait; the short
    /// batch ends the drain.
    #[tokio::test(start_paused = true)]
    async fn full_batches_drain_without_waiting_for_the_tick() {
        let cancel = CancellationToken::new();
        let s = Script::new(
            vec![
                report(100, 100),
                report(100, 100),
                report(100, 100),
                report(7, 7),
            ],
            cancel.clone(),
        );
        drive(&s, 100, TICK, leader(true), cancel).await;
        assert_eq!(s.calls.load(Ordering::SeqCst), 4);
        // The first tick of a tokio interval is immediate; all four passes
        // ran within it, with no paused-clock time spent between them.
        assert!(
            s.at.lock().iter().all(|t| *t == Duration::ZERO),
            "{:?}",
            s.at.lock()
        );
    }

    /// A full claim that publishes nothing waits for the next tick.
    #[tokio::test(start_paused = true)]
    async fn a_full_claim_that_publishes_nothing_waits_for_the_tick() {
        let cancel = CancellationToken::new();
        let s = Script::new(vec![report(100, 0), report(100, 0)], cancel.clone());
        drive(&s, 100, TICK, leader(true), cancel).await;
        let at = s.at.lock().clone();
        assert_eq!(at, vec![Duration::ZERO, TICK]);
    }

    /// An error returns to the interval too (no immediate retry of a
    /// failing mark/commit).
    #[tokio::test(start_paused = true)]
    async fn an_error_waits_for_the_tick() {
        let cancel = CancellationToken::new();
        let s = Script::new(
            vec![
                Err(SchedulerError::ConfigError("boom".into())),
                report(100, 100),
            ],
            cancel.clone(),
        );
        drive(&s, 100, TICK, leader(true), cancel).await;
        let at = s.at.lock().clone();
        assert_eq!(at, vec![Duration::ZERO, TICK]);
    }

    /// A non-leader never polls, however many ticks pass.
    #[tokio::test(start_paused = true)]
    async fn a_non_leader_does_not_poll() {
        let cancel = CancellationToken::new();
        let s = Script::new(vec![report(100, 100)], CancellationToken::new());
        let stop = cancel.clone();
        tokio::spawn(async move {
            time::sleep(Duration::from_secs(5)).await;
            stop.cancel();
        });
        drive(&s, 100, TICK, leader(false), cancel).await;
        assert_eq!(s.calls.load(Ordering::SeqCst), 0);
    }

    /// Losing leadership mid-drain stops the drain before the next pass.
    #[tokio::test(start_paused = true)]
    async fn leadership_is_rechecked_before_every_pass() {
        let cancel = CancellationToken::new();
        let s = Script::new((0..5).map(|_| report(100, 100)).collect(), cancel.clone());
        let flag = Arc::new(AtomicBool::new(true));
        let seen = Arc::new(AtomicUsize::new(0));
        let is_leader: Arc<dyn Fn() -> bool + Send + Sync> = {
            let (flag, seen) = (flag.clone(), seen.clone());
            Arc::new(move || {
                // Leader for the first two checks only.
                if seen.fetch_add(1, Ordering::SeqCst) >= 2 {
                    flag.store(false, Ordering::SeqCst);
                }
                flag.load(Ordering::SeqCst)
            })
        };
        let stop = cancel.clone();
        tokio::spawn(async move {
            time::sleep(Duration::from_millis(500)).await;
            stop.cancel();
        });
        drive(&s, 100, TICK, is_leader, cancel).await;
        assert_eq!(s.calls.load(Ordering::SeqCst), 2);
    }

    /// Cancellation mid-drain ends the drain.
    #[tokio::test(start_paused = true)]
    async fn cancellation_stops_a_drain() {
        let cancel = CancellationToken::new();
        let s = Script::new(
            (0..50).map(|_| report(100, 100)).collect(),
            CancellationToken::new(),
        );
        let c2 = cancel.clone();
        // Cancel after the second pass.
        struct CancelAfter<'a>(&'a Script, CancellationToken);
        impl PollSource for CancelAfter<'_> {
            async fn poll(&self) -> Result<PollReport, SchedulerError> {
                let r = self.0.poll().await;
                if self.0.calls.load(Ordering::SeqCst) == 2 {
                    self.1.cancel();
                }
                r
            }
        }
        drive(&CancelAfter(&s, c2), 100, TICK, leader(true), cancel).await;
        assert_eq!(s.calls.load(Ordering::SeqCst), 2);
    }
}
