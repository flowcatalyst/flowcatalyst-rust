//! Stale job recovery.
//!
//! A port of Go's `StaleQueuedJobPoller` (`scheduler/stale_recovery.go`):
//! a job QUEUED for longer than `stale_after` (75 minutes, Go's default)
//! since its `updated_at` goes back to PENDING for the poller to publish
//! again. `updated_at` rather than `queued_at`, as Go does, so a QUEUED row
//! whose `queued_at` is NULL — rows written by Go, or by an older Rust build
//! that inserted QUEUED directly — is recovered too.
//!
//! 75 minutes, not less: it must exceed the router's deferral horizon (1h),
//! or a message the router parked for capacity is republished and the broker
//! holds two copies.
//!
//! One addition to Go: a job left PROCESSING for longer than the same
//! threshold also goes back to PENDING. `/api/dispatch/process` finishes an
//! attempt in minutes (the subscriber call is capped at two), so a row that
//! old lost its outcome write (a database blip after delivery) and would
//! otherwise stay PROCESSING forever, as it does in Go. The price is
//! at-least-once: such a job may be delivered again.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::scheduler::SchedulerError;

/// Recorded on a PROCESSING row this loop reclaims.
pub const STALE_PROCESSING_REASON: &str =
    "stale recovery: PROCESSING with no outcome recorded; returned to PENDING";

#[derive(Clone)]
pub struct StaleQueuedJobPoller {
    pool: PgPool,
    stale_after: Duration,
}

/// What one sweep reclaimed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StaleRecovery {
    pub queued: u64,
    pub processing: u64,
}

impl StaleQueuedJobPoller {
    pub fn new(pool: PgPool, stale_after: Duration) -> Self {
        Self { pool, stale_after }
    }

    pub async fn recover_once(&self) -> Result<StaleRecovery, SchedulerError> {
        let cutoff = Utc::now()
            - chrono::Duration::from_std(self.stale_after)
                .unwrap_or_else(|_| chrono::Duration::minutes(75));

        let queued = sqlx::query(
            "UPDATE msg_dispatch_jobs SET status = 'PENDING', queued_at = NULL, updated_at = NOW() \
             WHERE status = 'QUEUED' AND updated_at < $1",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?
        .rows_affected();

        let processing = sqlx::query(
            "UPDATE msg_dispatch_jobs SET status = 'PENDING', queued_at = NULL, last_error = $2, \
                    updated_at = NOW() \
             WHERE status = 'PROCESSING' AND updated_at < $1",
        )
        .bind(cutoff)
        .bind(STALE_PROCESSING_REASON)
        .execute(&self.pool)
        .await?
        .rows_affected();

        metrics::counter!("scheduler.stale_jobs.recovered_total").increment(queued + processing);
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
        cancel: tokio_util::sync::CancellationToken,
    ) {
        // Go's ticker waits one interval before its first sweep.
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
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
