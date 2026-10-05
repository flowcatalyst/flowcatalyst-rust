//! Stale job recovery.
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

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::dispatch_job::lifecycle;
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
        }
    }
}

/// Registers the sweeps' metric descriptions (idempotent).
fn describe_metrics() {
    use metrics::describe_counter;
    describe_counter!(
        "scheduler.stale_jobs.queued_recovered_total",
        "Jobs QUEUED for more than 15 minutes returned to PENDING."
    );
    describe_counter!(
        "scheduler.stale_jobs.processing_recovered_total",
        "Jobs PROCESSING for more than 75 minutes returned to PENDING."
    );
}
