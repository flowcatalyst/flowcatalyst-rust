//! FlowCatalyst Dispatch Scheduler
//!
//! A port of Go's dispatch-job scheduler (`internal/platform/scheduler`),
//! behaving as a drop-in for it:
//!
//! - [`PendingJobPoller`] claims due PENDING jobs with `FOR UPDATE SKIP
//!   LOCKED`, marks them QUEUED in the same transaction, commits, then
//!   publishes the claim in one call (`poller.rs`).
//! - [`MessageGroupDispatcher`] renders each queue message (signed token,
//!   resolved pool code, dispatch mode, message group) and reverts exactly
//!   the unpublished jobs `QUEUED → PENDING` (`dispatcher.rs`).
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

use thiserror::Error;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub mod auth;
pub mod destination;
pub mod dispatcher;
pub mod poller;
pub mod publisher;
pub mod stale_recovery;

pub use auth::DispatchAuthService;
pub use destination::{
    DispatchQueueKind, DispatchQueueSettings, PoolCodeResolver, SubscriptionPriorityCache,
};
pub use dispatcher::MessageGroupDispatcher;
pub use poller::{PausedConnectionCache, PendingJobPoller, GROUP_HOLDING_STATUS_SQL};
pub use publisher::{
    DispatchPublisher, PostgresDispatchPublisher, PublishItem, PublishOutcome,
    SingleQueuePublisher, SqsDispatchPublisher,
};
pub use stale_recovery::StaleQueuedJobPoller;

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

/// Scheduler tuning. The defaults are Go's `DefaultConfig`.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// How often the poller claims.
    pub poll_interval: Duration,
    /// Most jobs claimed (and published) per tick.
    pub batch_size: usize,
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

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            batch_size: 100,
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
        let poller = PendingJobPoller::new(
            pool.clone(),
            config.batch_size,
            config.paused_cache_ttl,
            dispatcher,
            pool_codes,
        );
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
                let fifo = fc_queue::sqs_publisher::SqsFifoPublisher::from_default_chain(
                    Some(region.clone()),
                    fc_queue::sqs_publisher::QueueAddressing::Composed {
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
            stale_after_mins = self.config.stale_after.as_secs() / 60,
            processing_endpoint = %self.config.processing_endpoint,
            publisher = %self.publisher_description,
            "dispatch scheduler starting"
        );
        tokio::join!(
            self.poller
                .run(self.config.poll_interval, is_leader.clone(), cancel.clone()),
            self.stale
                .run(self.config.stale_scan_interval, is_leader, cancel),
        );
        info!("dispatch scheduler stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_gos() {
        let c = SchedulerConfig::default();
        assert_eq!(c.poll_interval, Duration::from_secs(1));
        assert_eq!(c.batch_size, 100);
        assert_eq!(c.paused_cache_ttl, Duration::from_secs(60));
        assert_eq!(c.stale_after, Duration::from_secs(75 * 60));
        assert_eq!(c.stale_scan_interval, Duration::from_secs(60));
    }
}
