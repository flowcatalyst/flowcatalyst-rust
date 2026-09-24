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

use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::scheduled_job::entity::{
    InstanceStatus, ScheduledJob, ScheduledJobInstance, TriggerKind,
};
use crate::scheduled_job::scheduler::config::ScheduledJobSchedulerConfig;
use crate::scheduled_job::{ScheduledJobInstanceRepository, ScheduledJobRepository};
use crate::shared::error::PlatformError;

/// Why a job's cron schedule could not be evaluated.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("Invalid timezone '{tz}': {source}")]
    InvalidTimezone {
        tz: String,
        source: chrono_tz::ParseError,
    },
    #[error("Invalid cron '{expr}': {source}")]
    InvalidCron {
        expr: String,
        source: cron::error::Error,
    },
}

/// Why one job could not be fired.
#[derive(Debug, thiserror::Error)]
enum FireError {
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    #[error(transparent)]
    Platform(#[from] PlatformError),
}

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

    async fn process_job(&self, job: &ScheduledJob, now: DateTime<Utc>) -> Result<bool, FireError> {
        let last = job.last_fired_at.unwrap_or(job.created_at);
        let Some(slot) = latest_slot_in_window(&job.crons, &job.timezone, last, now)? else {
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

/// Compute the LATEST cron slot in the half-open window `(after, up_to]`
/// across all `crons` evaluated in `tz`. Returns `None` if no slot fits.
///
/// Pure function — no I/O. The cron crate's `after()` iterator yields slots
/// in ascending order; we walk forward and remember the most recent slot
/// that is `<= up_to`.
pub fn latest_slot_in_window(
    crons: &[String],
    tz_name: &str,
    after: DateTime<Utc>,
    up_to: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, ScheduleError> {
    if after >= up_to {
        return Ok(None);
    }
    let tz: Tz = Tz::from_str(tz_name).map_err(|source| ScheduleError::InvalidTimezone {
        tz: tz_name.to_string(),
        source,
    })?;

    let after_tz = tz.from_utc_datetime(&after.naive_utc());
    let up_to_tz = tz.from_utc_datetime(&up_to.naive_utc());

    let mut best: Option<DateTime<Tz>> = None;

    for expr in crons {
        let schedule = Schedule::from_str(expr).map_err(|source| ScheduleError::InvalidCron {
            expr: expr.clone(),
            source,
        })?;
        // Walk forward from `after` and pick the latest slot <= up_to.
        for slot in schedule.after(&after_tz) {
            if slot > up_to_tz {
                break;
            }
            best = Some(match best {
                Some(b) if b >= slot => b,
                _ => slot,
            });
        }
    }

    Ok(best.map(|t| t.with_timezone(&Utc)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn latest_slot_in_window_picks_most_recent() {
        // A daily-at-midnight cron, window spanning two midnights → expect the
        // later one. Cron crate uses 6-7 field syntax (sec min hour dom mon dow).
        let crons = vec!["0 0 0 * * *".to_string()];
        let after = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
        let up_to = Utc.with_ymd_and_hms(2024, 1, 3, 12, 0, 0).unwrap();
        let slot = latest_slot_in_window(&crons, "UTC", after, up_to).unwrap();
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
        assert_eq!(
            latest_slot_in_window(&crons, "UTC", after, up_to).unwrap(),
            None
        );
    }

    #[test]
    fn latest_slot_unions_multiple_crons() {
        // 5am AND 5pm. Window 4am..6pm → expect 5pm.
        let crons = vec!["0 0 5 * * *".to_string(), "0 0 17 * * *".to_string()];
        let after = Utc.with_ymd_and_hms(2024, 6, 1, 4, 0, 0).unwrap();
        let up_to = Utc.with_ymd_and_hms(2024, 6, 1, 18, 0, 0).unwrap();
        let slot = latest_slot_in_window(&crons, "UTC", after, up_to).unwrap();
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
        let slot = latest_slot_in_window(&crons, "America/New_York", after, up_to)
            .unwrap()
            .expect("should fire");
        // 9am EDT = 13:00 UTC
        assert_eq!(slot, Utc.with_ymd_and_hms(2024, 6, 1, 13, 0, 0).unwrap());
    }

    #[test]
    fn invalid_cron_returns_err() {
        let crons = vec!["not a cron".to_string()];
        let after = Utc::now();
        let up_to = after + Duration::hours(1);
        assert!(matches!(
            latest_slot_in_window(&crons, "UTC", after, up_to),
            Err(ScheduleError::InvalidCron { .. })
        ));
    }

    #[test]
    fn invalid_timezone_returns_err() {
        let crons = vec!["0 0 0 * * *".to_string()];
        let after = Utc::now();
        let up_to = after + Duration::hours(1);
        let err = latest_slot_in_window(&crons, "Mars/Olympus", after, up_to).unwrap_err();
        assert!(matches!(err, ScheduleError::InvalidTimezone { .. }));
        assert!(err
            .to_string()
            .starts_with("Invalid timezone 'Mars/Olympus': "));
    }

    #[test]
    fn empty_window_returns_none() {
        let crons = vec!["0 * * * * *".to_string()];
        let now = Utc::now();
        assert_eq!(
            latest_slot_in_window(&crons, "UTC", now, now).unwrap(),
            None
        );
    }
}
