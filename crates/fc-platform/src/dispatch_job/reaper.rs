//! The stranded-sibling reaper (Go `dispatchjob/reaper.go`).
//!
//! Under BLOCK_ON_ERROR the router ACKs a group's untried buffered siblings
//! the moment its head fails terminally, and tells the platform through
//! `POST /api/dispatch/settled`. If that call is lost (or the router dies
//! between the ACK and the call) those rows would sit QUEUED/PROCESSING
//! forever. This sweep finds them and returns them to PENDING, where the
//! scheduler's hold keeps them behind the failed head until an operator
//! resolves it.
//!
//! Not leader-gated, as in Go: each sweep is one conditional UPDATE guarded
//! on `status IN ('QUEUED', 'PROCESSING')`, safe to run on every instance.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::repository::DispatchJobRepository;

/// How often the sweep runs (Go `DefaultReaperInterval`).
pub const DEFAULT_REAPER_INTERVAL: Duration = Duration::from_secs(2 * 60);

/// A PROCESSING sibling updated more recently than this is presumed a live
/// delivery (Go `DefaultProcessingLiveAfter`): above the router's 15-minute
/// per-attempt, three-attempt contract.
pub const DEFAULT_PROCESSING_LIVE_AFTER: Duration = Duration::from_secs(45 * 60);

/// One sweep; returns the ids reset.
pub async fn sweep_once(
    repo: &DispatchJobRepository,
    processing_live_after: Duration,
) -> crate::shared::error::Result<Vec<String>> {
    let live_before = Utc::now()
        - chrono::Duration::from_std(processing_live_after)
            .unwrap_or_else(|_| chrono::Duration::minutes(45));
    repo.sweep_stranded_group_siblings(live_before).await
}

/// Sweep every `interval` until cancelled.
pub async fn run_reaper(
    repo: Arc<DispatchJobRepository>,
    interval: Duration,
    processing_live_after: Duration,
    cancel: CancellationToken,
) {
    info!(
        interval_secs = interval.as_secs(),
        processing_live_after_secs = processing_live_after.as_secs(),
        "dispatch-job group reaper started"
    );
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tick.tick() => {}
        }
        match sweep_once(&repo, processing_live_after).await {
            Ok(ids) if !ids.is_empty() => {
                info!(count = ids.len(), ids = ?ids, "dispatch-job group reaper reset stranded siblings")
            }
            Ok(_) => {}
            Err(e) => warn!(error = %e, "dispatch-job group reaper sweep failed"),
        }
    }
    info!("dispatch-job group reaper stopped");
}
