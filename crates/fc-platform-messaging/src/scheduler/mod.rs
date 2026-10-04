//! FlowCatalyst Dispatch Scheduler
//!
//! A port of Go's dispatch-job scheduler (`internal/platform/scheduler`),
//! behaving as a drop-in for it:
//!
//! - [`PendingJobPoller`] claims due PENDING jobs (no lock, no transaction;
//!   the in-memory in-flight ids are excluded from the next claim) and hands
//!   them to a few dispatcher lanes, never waiting for a publish
//!   (`poller.rs`). A lane publishes its jobs and marks the published ones
//!   QUEUED in bulk (`lane.rs`). Unpublished jobs stay PENDING.
//! - [`MessageGroupDispatcher`] renders each queue message (signed token,
//!   resolved pool code, dispatch mode, message group), publishes a lane's
//!   batch under a deadline and reports exactly the unpublished jobs
//!   (`dispatcher.rs`).
//! - A [`DispatchPublisher`] sends to the configured queues: per-(tenant,
//!   priority) SQS FIFO queues in production (`publisher.rs`,
//!   `destination.rs`).
//! - [`StaleQueuedJobPoller`] returns jobs QUEUED (or PROCESSING) for too
//!   long to PENDING (`stale_recovery.rs`).
//! - [`DispatchAuthService`] signs job ids with Go's HKDF-derived key
//!   (`auth.rs`).
//!
//! Retries are owned here, not by the queue: `/api/dispatch/process` always
//! ACKs and reschedules a failed job via `scheduled_for`, and the poller is
//! the only component that re-dispatches it.

use std::sync::Arc;
use std::time::Duration;

pub use destination::{
    DispatchQueueKind, DispatchQueueSettings, PoolCodeResolver, SubscriptionPriorityCache,
};
use fc_common::config::env_first_parse;
use fc_queue::sqs_publisher::QueueAddressing;
use fc_queue::sqs_publisher::SqsFifoPublisher;
pub use publisher::{
    DispatchPublisher, PostgresDispatchPublisher, PublishItem, PublishOutcome,
    SingleQueuePublisher, SqsDispatchPublisher,
};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub mod auth;
pub mod destination;
pub mod dispatcher;
pub(crate) mod lane;
pub mod poller;
pub mod publisher;
pub mod stale_recovery;

pub use auth::DispatchAuthService;
pub use dispatcher::MessageGroupDispatcher;
pub use poller::{PausedConnectionCache, PendingJobPoller, GROUP_HOLDING_STATUS_SQL};
pub use stale_recovery::StaleQueuedJobPoller;

#[cfg(test)]
mod testkit;

pub use fc_common::DispatchMode;
pub use fc_common::DispatchStatus;

#[derive(Error, Debug)]
pub enum SchedulerError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Queue error: {0}")]
    QueueError(#[from] fc_queue::QueueError),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Scheduler tuning. See [`SchedulerConfig::default`] for the defaults.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// How long the poller waits after a pass that found nothing to do, a
    /// short claim, a failure, or when it is not the leader.
    pub poll_interval: Duration,
    /// Most jobs one claim asks for.
    pub batch_size: usize,
    /// Most jobs claimed and not yet finished by a lane, in all; the poller
    /// blocks when it is reached.
    pub buffer_capacity: usize,
    /// Dispatcher lanes (publishers). A message group always uses one lane.
    pub dispatchers: usize,
    /// Most jobs a lane takes beyond the first in one publish batch.
    pub lane_batch: usize,
    /// How long the paused-connection, pool-code and priority caches live.
    pub paused_cache_ttl: Duration,
    /// QUEUED (and PROCESSING) longer than this goes back to PENDING.
    pub stale_after: Duration,
    /// How often stale recovery runs.
    pub stale_scan_interval: Duration,
    /// The URL stamped into every message's `mediationTarget`: the
    /// platform's `/api/dispatch/process`.
    pub processing_endpoint: String,
}

impl SchedulerConfig {
    /// Apply the operator overrides: `FC_SCHEDULER_BUFFER_CAPACITY`,
    /// `FC_SCHEDULER_DISPATCHERS` and `FC_SCHEDULER_BATCH_SIZE` (the last
    /// also read as `FLOWCATALYST_SCHEDULER_BATCH_SIZE`, its older name). A
    /// value that is absent, empty, unparseable or zero leaves the setting
    /// as it is.
    pub fn with_env_overrides(mut self) -> Self {
        self.buffer_capacity = positive(
            env_first_parse(&["FC_SCHEDULER_BUFFER_CAPACITY"], self.buffer_capacity),
            self.buffer_capacity,
        );
        self.dispatchers = positive(
            env_first_parse(&["FC_SCHEDULER_DISPATCHERS"], self.dispatchers),
            self.dispatchers,
        );
        self.batch_size = positive(
            env_first_parse(
                &[
                    "FC_SCHEDULER_BATCH_SIZE",
                    "FLOWCATALYST_SCHEDULER_BATCH_SIZE",
                ],
                self.batch_size,
            ),
            self.batch_size,
        );
        self
    }
}

/// The scheduler's own connection pool size: one connection per dispatcher
/// lane (each does a status update) plus one for the poller's claim and one
/// spare, unless `configured` overrides it (zero is ignored).
pub fn db_pool_size(dispatchers: usize, configured: Option<u32>) -> u32 {
    configured
        .filter(|n| *n > 0)
        .unwrap_or_else(|| u32::try_from(dispatchers.max(1) + 2).unwrap_or(u32::MAX))
}

impl SchedulerConfig {
    /// The size of the pool the scheduler opens for itself: see
    /// [`db_pool_size`]; `FC_SCHEDULER_DB_MAX_CONNECTIONS` overrides it.
    pub fn db_max_connections(&self) -> u32 {
        let configured: u32 = env_first_parse(&["FC_SCHEDULER_DB_MAX_CONNECTIONS"], 0);
        db_pool_size(self.dispatchers, (configured > 0).then_some(configured))
    }
}

/// `value`, or `fallback` when it is zero.
fn positive(value: usize, fallback: usize) -> usize {
    if value == 0 {
        fallback
    } else {
        value
    }
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            batch_size: 500,
            buffer_capacity: 1000,
            dispatchers: 10,
            lane_batch: 100,
            paused_cache_ttl: Duration::from_secs(60),
            stale_after: Duration::from_secs(75 * 60),
            stale_scan_interval: Duration::from_secs(60),
            processing_endpoint: "http://localhost:8080/api/dispatch/process".to_string(),
        }
    }
}

/// The poller and stale recovery, wired to one publisher.
pub struct DispatchScheduler {
    config: SchedulerConfig,
    poller: PendingJobPoller,
    stale: StaleQueuedJobPoller,
    publisher_description: String,
}

impl DispatchScheduler {
    /// Wire the scheduler. `pool_codes` is shared with the destination
    /// resolver when the publisher has one (both read `tnt_clients`).
    pub fn new(
        config: SchedulerConfig,
        pool: sqlx::PgPool,
        publisher: Arc<dyn DispatchPublisher>,
        auth: DispatchAuthService,
        pool_codes: Arc<PoolCodeResolver>,
    ) -> Self {
        let publisher_description = publisher.describe();
        let dispatcher = Arc::new(MessageGroupDispatcher::new(
            publisher,
            auth,
            config.processing_endpoint.clone(),
        ));
        let poller = PendingJobPoller::new(pool.clone(), &config, dispatcher, pool_codes);
        let stale = StaleQueuedJobPoller::new(pool, config.stale_after);
        Self {
            config,
            poller,
            stale,
            publisher_description,
        }
    }

    /// Wire the scheduler to the queues `settings` names: per-(tenant,
    /// priority) SQS FIFO queues, or per-(tenant, priority) Postgres queues
    /// in `pool`'s database.
    pub async fn from_settings(
        config: SchedulerConfig,
        pool: sqlx::PgPool,
        settings: &DispatchQueueSettings,
        auth: DispatchAuthService,
    ) -> Result<Self, SchedulerError> {
        let pool_codes = Arc::new(PoolCodeResolver::new(pool.clone(), config.paused_cache_ttl));
        let destinations = Arc::new(destination::DestinationResolver::new(
            pool_codes.clone(),
            SubscriptionPriorityCache::new(pool.clone(), config.paused_cache_ttl),
            settings,
        ));
        let publisher: Arc<dyn DispatchPublisher> = match &settings.kind {
            DispatchQueueKind::Sqs { region, account_id } => {
                let fifo = SqsFifoPublisher::from_default_chain(
                    Some(region.clone()),
                    QueueAddressing::Composed {
                        region: region.clone(),
                        account_id: account_id.clone(),
                    },
                )
                .await;
                Arc::new(SqsDispatchPublisher::new(fifo, destinations))
            }
            DispatchQueueKind::Postgres => {
                Arc::new(PostgresDispatchPublisher::new(pool.clone(), destinations).await?)
            }
        };
        Ok(Self::new(config, pool, publisher, auth, pool_codes))
    }

    pub fn poller(&self) -> &PendingJobPoller {
        &self.poller
    }

    pub fn stale_recovery(&self) -> &StaleQueuedJobPoller {
        &self.stale
    }

    /// Run the poller and stale recovery until `cancel` fires. Both idle
    /// while `is_leader` is false.
    pub async fn run(
        &self,
        is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
        cancel: CancellationToken,
    ) {
        info!(
            poll_interval_ms = self.config.poll_interval.as_millis() as u64,
            batch_size = self.config.batch_size,
            buffer_capacity = self.config.buffer_capacity,
            dispatchers = self.config.dispatchers,
            stale_after_mins = self.config.stale_after.as_secs() / 60,
            processing_endpoint = %self.config.processing_endpoint,
            publisher = %self.publisher_description,
            "dispatch scheduler starting"
        );
        // Each loop supervised: a panic is logged (with its backtrace and
        // span) and counted, and the loop restarted. The two used to share
        // one task, so a panic in either stopped both for good, silently.
        use fc_common::diagnostics::{supervise, OnPanic};
        tokio::join!(
            supervise("scheduler.poller", OnPanic::Restart, || self.poller.run(
                self.config.poll_interval,
                is_leader.clone(),
                cancel.clone()
            )),
            supervise("scheduler.stale_recovery", OnPanic::Restart, || self
                .stale
                .run(
                    self.config.stale_scan_interval,
                    is_leader.clone(),
                    cancel.clone()
                )),
        );
        info!("dispatch scheduler stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let c = SchedulerConfig::default();
        assert_eq!(c.poll_interval, Duration::from_secs(1));
        assert_eq!(c.batch_size, 500);
        assert_eq!(c.buffer_capacity, 1000);
        assert_eq!(c.dispatchers, 10);
        assert_eq!(c.lane_batch, 100);
        assert_eq!(c.paused_cache_ttl, Duration::from_secs(60));
        assert_eq!(c.stale_after, Duration::from_secs(75 * 60));
        assert_eq!(c.stale_scan_interval, Duration::from_secs(60));
    }

    #[test]
    fn the_scheduler_pool_is_dispatchers_plus_two_unless_overridden() {
        assert_eq!(db_pool_size(10, None), 12);
        assert_eq!(db_pool_size(1, None), 3);
        assert_eq!(db_pool_size(0, None), 3, "never fewer than one lane");
        assert_eq!(db_pool_size(10, Some(30)), 30);
        assert_eq!(db_pool_size(10, Some(0)), 12, "zero is ignored");
    }

    #[test]
    fn the_pool_size_override_is_read_from_the_environment() {
        use std::env;
        let key = "FC_SCHEDULER_DB_MAX_CONNECTIONS";
        let config = SchedulerConfig {
            dispatchers: 4,
            ..SchedulerConfig::default()
        };
        env::remove_var(key);
        assert_eq!(config.db_max_connections(), 6);
        env::set_var(key, "25");
        assert_eq!(config.db_max_connections(), 25);
        env::set_var(key, "lots");
        assert_eq!(config.db_max_connections(), 6, "unparseable is ignored");
        env::remove_var(key);
    }

    /// The only test that touches the other scheduler variables.
    #[test]
    fn env_overrides_apply_and_bad_values_are_ignored() {
        use std::env;
        let keys = [
            "FC_SCHEDULER_BUFFER_CAPACITY",
            "FC_SCHEDULER_DISPATCHERS",
            "FC_SCHEDULER_BATCH_SIZE",
            "FLOWCATALYST_SCHEDULER_BATCH_SIZE",
        ];
        for k in keys {
            env::remove_var(k);
        }
        let none = SchedulerConfig::default().with_env_overrides();
        assert_eq!(
            (none.buffer_capacity, none.dispatchers, none.batch_size),
            (1000, 10, 500)
        );

        env::set_var("FC_SCHEDULER_BUFFER_CAPACITY", "250");
        env::set_var("FC_SCHEDULER_DISPATCHERS", "0");
        env::set_var("FLOWCATALYST_SCHEDULER_BATCH_SIZE", "40");
        let c = SchedulerConfig::default().with_env_overrides();
        assert_eq!(c.buffer_capacity, 250);
        assert_eq!(c.dispatchers, 10, "zero is ignored");
        assert_eq!(c.batch_size, 40, "the older name still works");

        env::set_var("FC_SCHEDULER_BATCH_SIZE", "75");
        env::set_var("FC_SCHEDULER_DISPATCHERS", "many");
        let c = SchedulerConfig::default().with_env_overrides();
        assert_eq!(c.batch_size, 75, "the new name wins");
        assert_eq!(c.dispatchers, 10, "unparseable is ignored");
        for k in keys {
            env::remove_var(k);
        }
    }
}
