//! Scheduler configuration.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ScheduledJobSchedulerConfig {
    /// How often the poller wakes up to scan ACTIVE jobs and create instances
    /// for due cron slots. Default 30s.
    pub poll_interval: Duration,
    /// How often the dispatcher drains QUEUED instances. Default 5s.
    pub dispatch_interval: Duration,
    /// Maximum number of instances dispatched per dispatcher tick. Bounds
    /// memory and avoids monopolising the HTTP client. Default 32.
    pub dispatch_batch_size: i64,
    /// HTTP request timeout for webhook delivery. Default 10s.
    pub http_timeout: Duration,
}

impl Default for ScheduledJobSchedulerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(30),
            dispatch_interval: Duration::from_secs(5),
            dispatch_batch_size: 32,
            http_timeout: Duration::from_secs(10),
        }
    }
}

impl ScheduledJobSchedulerConfig {
    /// Build from env vars, falling back to defaults. Env keys:
    ///   FC_SCHEDULED_JOB_POLL_SECONDS
    ///   FC_SCHEDULED_JOB_DISPATCH_SECONDS
    ///   FC_SCHEDULED_JOB_DISPATCH_BATCH
    ///   FC_SCHEDULED_JOB_HTTP_TIMEOUT_SECONDS
    pub fn from_env() -> Self {
        let mut c = Self::default();
        if let Ok(v) = std::env::var("FC_SCHEDULED_JOB_POLL_SECONDS") {
            if let Ok(n) = v.parse::<u64>() {
                c.poll_interval = Duration::from_secs(n);
            }
        }
        if let Ok(v) = std::env::var("FC_SCHEDULED_JOB_DISPATCH_SECONDS") {
            if let Ok(n) = v.parse::<u64>() {
                c.dispatch_interval = Duration::from_secs(n);
            }
        }
        if let Ok(v) = std::env::var("FC_SCHEDULED_JOB_DISPATCH_BATCH") {
            if let Ok(n) = v.parse::<i64>() {
                c.dispatch_batch_size = n;
            }
        }
        if let Ok(v) = std::env::var("FC_SCHEDULED_JOB_HTTP_TIMEOUT_SECONDS") {
            if let Ok(n) = v.parse::<u64>() {
                c.http_timeout = Duration::from_secs(n);
            }
        }
        c
    }
}
