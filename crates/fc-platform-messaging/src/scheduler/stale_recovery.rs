//! Stale job recovery, and the queue reconcile sweep.
//!
//! A port of Go's `StaleQueuedJobPoller` (`scheduler/stale_recovery.go`):
//! a job QUEUED for longer than `queued_after` (15 minutes) since its
//! `updated_at` goes back to PENDING for the poller to publish again.
//! `updated_at` rather than `queued_at`, as Go does, so a QUEUED row whose
//! `queued_at` is NULL (rows written by Go, or by an older Rust build that
//! inserted QUEUED directly) is recovered too.
//!
//! 15 minutes (owner ruling 2026-10-04, all three implementations): a copy
//! the router parked for capacity longer than that is republished and the
//! broker holds two; the router drops the duplicate while the original is in
//! its pipeline, and the callback skips a job that is already delivered or
//! being delivered.
//!
//! One addition to Go: a job left PROCESSING for longer than
//! `processing_after` (75 minutes) also goes back to PENDING.
//! `/api/dispatch/process` finishes an attempt in minutes (the subscriber call
//! is capped at two), so a row that old lost its outcome write (a database
//! blip after delivery) and would otherwise stay PROCESSING forever, as it
//! does in Go. The price is at-least-once: such a job may be delivered again.
//!
//! The same loop (leader only, once a minute) reconciles
//! `msg_dispatch_queue` against the jobs ([`lifecycle::reconcile_queue`]):
//! the lifecycle keeps the table exact, so anything it repairs is a bug or an
//! older binary writing the jobs table, and is counted and logged at WARN.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::dispatch_job::lifecycle::{self, ReconcileGuards, Reconciled};
use crate::scheduler::SchedulerError;
use tokio::time;
use tokio::time::Instant;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

/// Recorded on a PROCESSING row this loop reclaims.
pub const STALE_PROCESSING_REASON: &str =
    "stale recovery: PROCESSING with no outcome recorded; returned to PENDING";

#[derive(Clone)]
pub struct StaleQueuedJobPoller {
    pool: PgPool,
    queued_after: Duration,
    processing_after: Duration,
}

/// What one sweep reclaimed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StaleRecovery {
    pub queued: u64,
    pub processing: u64,
}

impl StaleQueuedJobPoller {
    pub fn new(pool: PgPool, queued_after: Duration, processing_after: Duration) -> Self {
        describe_metrics();
        Self {
            pool,
            queued_after,
            processing_after,
        }
    }

    #[tracing::instrument(name = "scheduler.stale_recovery", skip_all)]
    pub async fn recover_once(&self) -> Result<StaleRecovery, SchedulerError> {
        let now = Utc::now();
        let queued_cutoff = now
            - chrono::Duration::from_std(self.queued_after)
                .unwrap_or_else(|_| chrono::Duration::minutes(15));
        let processing_cutoff = now
            - chrono::Duration::from_std(self.processing_after)
                .unwrap_or_else(|_| chrono::Duration::minutes(75));

        let queued = lifecycle::recover_stale_queued(&self.pool, queued_cutoff)
            .await?
            .len() as u64;
        let processing = lifecycle::recover_stale_processing(
            &self.pool,
            processing_cutoff,
            STALE_PROCESSING_REASON,
        )
        .await?
        .len() as u64;

        metrics::counter!("scheduler.stale_jobs.recovered_total").increment(queued + processing);
        metrics::counter!("scheduler.stale_jobs.queued_recovered_total").increment(queued);
        metrics::counter!("scheduler.stale_jobs.processing_recovered_total").increment(processing);
        if queued + processing > 0 {
            info!(
                queued,
                processing, "stale dispatch jobs returned to PENDING"
            );
        }
        Ok(StaleRecovery { queued, processing })
    }

    /// One reconcile pass with `guards` (the production sweep uses
    /// [`ReconcileGuards::production`]). Counts each repair and logs at WARN
    /// when there was any: it means a bug, or an older binary writing the
    /// jobs table.
    #[tracing::instrument(name = "scheduler.queue_reconcile", skip_all)]
    pub async fn reconcile_with(
        &self,
        guards: ReconcileGuards,
    ) -> Result<Reconciled, SchedulerError> {
        let started = Instant::now();
        let done = lifecycle::reconcile_queue(&self.pool, guards).await?;
        metrics::histogram!("scheduler.queue_reconcile.duration_seconds").record(started.elapsed());
        metrics::counter!("scheduler.queue_reconcile.inserted_total").increment(done.inserted);
        metrics::counter!("scheduler.queue_reconcile.deleted_total").increment(done.deleted);
        metrics::counter!("scheduler.queue_reconcile.refreshed_total").increment(done.refreshed);
        if done.total() > 0 {
            warn!(
                inserted = done.inserted,
                deleted = done.deleted,
                refreshed = done.refreshed,
                "msg_dispatch_queue was out of step with msg_dispatch_jobs and was repaired \
                 (a bug, or an older binary writing the jobs table)"
            );
        }
        Ok(done)
    }

    /// The production reconcile pass: 60 s / 5 min age guards, 5,000 rows.
    pub async fn reconcile_once(&self) -> Result<Reconciled, SchedulerError> {
        self.reconcile_with(ReconcileGuards::production()).await
    }

    /// Sweep every `interval` until cancelled, only while leader.
    pub async fn run(
        &self,
        interval: Duration,
        is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
        cancel: CancellationToken,
    ) {
        // Go's ticker waits one interval before its first sweep.
        let mut tick = time::interval_at(Instant::now() + interval, interval);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tick.tick() => {}
            }
            if !is_leader() {
                continue;
            }
            if let Err(e) = self.recover_once().await {
                warn!(error = %e, "stale recovery error");
            }
            if let Err(e) = self.reconcile_once().await {
                warn!(error = %e, "queue reconcile error");
            }
        }
    }
}

/// Registers the sweeps' metric descriptions (idempotent).
fn describe_metrics() {
    use metrics::{describe_counter, describe_histogram, Unit};
    describe_counter!(
        "scheduler.stale_jobs.queued_recovered_total",
        "Jobs QUEUED for more than 15 minutes returned to PENDING."
    );
    describe_counter!(
        "scheduler.stale_jobs.processing_recovered_total",
        "Jobs PROCESSING for more than 75 minutes returned to PENDING."
    );
    describe_counter!(
        "scheduler.queue_reconcile.inserted_total",
        "Queue rows the reconcile sweep inserted for PENDING jobs that had none. Non-zero is a bug or an older binary."
    );
    describe_counter!(
        "scheduler.queue_reconcile.deleted_total",
        "Queue rows the reconcile sweep deleted (their job is missing or not PENDING). Non-zero is a bug or an older binary."
    );
    describe_counter!(
        "scheduler.queue_reconcile.refreshed_total",
        "Queue rows the reconcile sweep refreshed (their version differed from the job's updated_at). Non-zero is a bug or an older binary."
    );
    describe_histogram!(
        "scheduler.queue_reconcile.duration_seconds",
        Unit::Seconds,
        "Wall time of one reconcile pass."
    );
}
