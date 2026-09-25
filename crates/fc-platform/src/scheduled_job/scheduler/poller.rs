//! Cron-tick poller.
//!
//! On each `poll_interval` tick:
//!   1. Load every ACTIVE ScheduledJob (small set — definitions, not firings).
//!   2. For each, compute the LATEST cron slot in `(last_fired_at, now]`.
//!   3. If a slot exists, insert a QUEUED `ScheduledJobInstance` for it and
//!      bump `last_fired_at` to that slot. The dispatcher picks it up next.
//!
//! "Skip-missed" semantics: if multiple slots fall in the window (e.g. after
//! a long downtime), only the LATEST fires. Older missed slots are silently
//! dropped — the user accepted this trade-off when picking the AWS-style
//! default. `last_fired_at` advances to the latest fire so we don't keep
//! re-scanning the same window.
//!
//! Crons are read in Go's (robfig's) dialect, with Go's zone handling
//! ([`crate::scheduled_job::cron`]): a cron that does not parse is skipped
//! and an unknown zone is UTC, as Go's `LatestSlotInWindow` does.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::scheduled_job::cron::JobSchedule;
use crate::scheduled_job::entity::{
    InstanceStatus, ScheduledJob, ScheduledJobInstance, TriggerKind,
};
use crate::scheduled_job::scheduler::config::ScheduledJobSchedulerConfig;
use crate::scheduled_job::{ScheduledJobInstanceRepository, ScheduledJobRepository};
use crate::shared::error::PlatformError;

pub struct ScheduledJobPoller {
    config: ScheduledJobSchedulerConfig,
    repo: Arc<ScheduledJobRepository>,
    instance_repo: Arc<ScheduledJobInstanceRepository>,
    shutdown: broadcast::Receiver<()>,
}

impl ScheduledJobPoller {
    pub fn new(
        config: ScheduledJobSchedulerConfig,
        repo: Arc<ScheduledJobRepository>,
        instance_repo: Arc<ScheduledJobInstanceRepository>,
        shutdown: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            config,
            repo,
            instance_repo,
            shutdown,
        }
    }

    pub async fn run(mut self) {
        info!(
            interval_seconds = self.config.poll_interval.as_secs(),
            "Scheduled-job poller started"
        );
        let mut ticker = tokio::time::interval(self.config.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(e) = self.tick().await {
                        error!(error = %e, "Scheduled-job poller tick failed");
                    }
                }
                _ = self.shutdown.recv() => {
                    info!("Scheduled-job poller shutting down");
                    return;
                }
            }
        }
    }

    async fn tick(&self) -> Result<(), PlatformError> {
        let now = Utc::now();
        let jobs = self.repo.find_active_for_polling().await?;
        debug!(count = jobs.len(), "Polling active scheduled jobs");

        let mut fired = 0usize;
        let mut errors = 0usize;
        for job in jobs {
            match self.process_job(&job, now).await {
                Ok(true) => fired += 1,
                Ok(false) => {}
                Err(e) => {
                    errors += 1;
                    warn!(job_id = %job.id, error = %e, "Failed to evaluate scheduled job");
                }
            }
        }
        if fired > 0 || errors > 0 {
            info!(fired, errors, "Scheduled-job poll completed");
        }
        Ok(())
    }

    async fn process_job(
        &self,
        job: &ScheduledJob,
        now: DateTime<Utc>,
    ) -> Result<bool, PlatformError> {
        let last = job.last_fired_at.unwrap_or(job.created_at);
        let schedule = JobSchedule::new(&job.crons, &job.timezone);
        for problem in schedule.problems() {
            debug!(job_id = %job.id, problem = %problem, "Scheduled job schedule");
        }
        let Some(slot) = schedule.latest_in_window(last, now) else {
            return Ok(false);
        };

        let instance = ScheduledJobInstance {
            id: crate::shared::tsid::generate(crate::EntityType::ScheduledJobInstance),
            scheduled_job_id: job.id.clone(),
            client_id: job.client_id.clone(),
            job_code: job.code.clone(),
            trigger_kind: TriggerKind::Cron,
            scheduled_for: Some(slot),
            fired_at: now,
            delivered_at: None,
            completed_at: None,
            status: InstanceStatus::Queued,
            delivery_attempts: 0,
            delivery_error: None,
            completion_status: None,
            completion_result: None,
            correlation_id: None,
            created_at: now,
        };

        self.instance_repo.insert(&instance).await?;
        self.repo.mark_fired(&job.id, slot).await?;
        debug!(job_id = %job.id, slot = %slot, instance_id = %instance.id, "Cron-fired scheduled job");
        Ok(true)
    }
}

/// The latest slot of any of `crons` in the half-open window
/// `(after, up_to]`, evaluated in `tz_name` (Go's `LatestSlotInWindow`: a
/// cron that does not parse is skipped, an unknown zone is UTC). `None` if
/// no slot fits.
pub fn latest_slot_in_window(
    crons: &[String],
    tz_name: &str,
    after: DateTime<Utc>,
    up_to: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    JobSchedule::new(crons, tz_name).latest_in_window(after, up_to)
}

/// The first slot of any of `crons` strictly after `after`, evaluated in
/// `tz_name`: the poller's own reading, one step at a time.
pub fn next_slot_after(
    crons: &[String],
    tz_name: &str,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    JobSchedule::new(crons, tz_name).next_after(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn latest_slot_in_window_picks_most_recent() {
        // A daily-at-midnight cron, window spanning two midnights → expect the
        // later one. Six fields: sec min hour dom mon dow.
        let crons = vec!["0 0 0 * * *".to_string()];
        let after = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
        let up_to = Utc.with_ymd_and_hms(2024, 1, 3, 12, 0, 0).unwrap();
        let slot = latest_slot_in_window(&crons, "UTC", after, up_to);
        assert_eq!(
            slot,
            Some(Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn latest_slot_returns_none_when_no_slot_in_window() {
        let crons = vec!["0 0 0 * * *".to_string()]; // daily midnight
        let after = Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap();
        let up_to = Utc.with_ymd_and_hms(2024, 1, 1, 23, 0, 0).unwrap();
        assert_eq!(latest_slot_in_window(&crons, "UTC", after, up_to), None);
    }

    #[test]
    fn latest_slot_unions_multiple_crons() {
        // 5am AND 5pm. Window 4am..6pm → expect 5pm.
        let crons = vec!["0 0 5 * * *".to_string(), "0 0 17 * * *".to_string()];
        let after = Utc.with_ymd_and_hms(2024, 6, 1, 4, 0, 0).unwrap();
        let up_to = Utc.with_ymd_and_hms(2024, 6, 1, 18, 0, 0).unwrap();
        let slot = latest_slot_in_window(&crons, "UTC", after, up_to);
        assert_eq!(
            slot,
            Some(Utc.with_ymd_and_hms(2024, 6, 1, 17, 0, 0).unwrap())
        );
    }

    #[test]
    fn latest_slot_respects_timezone() {
        // Daily 9am New York — should land at 13:00 UTC (EST=-5) or 14:00
        // UTC (EDT=-4). June is EDT.
        let crons = vec!["0 0 9 * * *".to_string()];
        let after = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
        let up_to = Utc.with_ymd_and_hms(2024, 6, 2, 0, 0, 0).unwrap();
        let slot =
            latest_slot_in_window(&crons, "America/New_York", after, up_to).expect("should fire");
        // 9am EDT = 13:00 UTC
        assert_eq!(slot, Utc.with_ymd_and_hms(2024, 6, 1, 13, 0, 0).unwrap());
    }

    #[test]
    fn an_unreadable_cron_never_fires() {
        let crons = vec!["not a cron".to_string()];
        let after = Utc::now();
        let up_to = after + Duration::hours(1);
        assert_eq!(latest_slot_in_window(&crons, "UTC", after, up_to), None);
    }

    #[test]
    fn an_unknown_timezone_is_utc() {
        let crons = vec!["0 0 0 * * *".to_string()];
        let after = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
        let up_to = Utc.with_ymd_and_hms(2024, 1, 2, 12, 0, 0).unwrap();
        assert_eq!(
            latest_slot_in_window(&crons, "Mars/Olympus", after, up_to),
            Some(Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn empty_window_returns_none() {
        let crons = vec!["0 * * * * *".to_string()];
        let now = Utc::now();
        assert_eq!(latest_slot_in_window(&crons, "UTC", now, now), None);
    }
}
