//! Scheduled-job scheduler.
//!
//! Runs two cooperating background tasks:
//!   * `Poller` — every `poll_interval`, scans ACTIVE jobs, computes the
//!     latest cron slot in (last_fired_at, now] per job, and inserts a
//!     QUEUED instance row for it (skip-missed semantics).
//!   * `Dispatcher` — every `dispatch_interval`, drains QUEUED instances,
//!     POSTs each to the job's `target_url`, and marks the instance
//!     DELIVERED / DELIVERY_FAILED.
//!
//! Single-replica assumption for v1 — concurrent pollers would double-fire
//! cron slots, and concurrent dispatchers would double-deliver instances.
//! Add `SELECT ... FOR UPDATE SKIP LOCKED` claims when scaling out.

pub mod config;
pub mod dispatcher;
pub mod poller;

pub use config::ScheduledJobSchedulerConfig;
pub use dispatcher::ScheduledJobDispatcher;
pub use poller::ScheduledJobPoller;

use std::sync::Arc;
use tokio::sync::broadcast;

use crate::scheduled_job::{ScheduledJobInstanceRepository, ScheduledJobRepository};
use crate::service_account::outbound_credentials::OutboundCredentialsResolver;

/// Composes Poller + Dispatcher behind a single start/stop handle.
pub struct ScheduledJobSchedulerService {
    config: ScheduledJobSchedulerConfig,
    repo: Arc<ScheduledJobRepository>,
    instance_repo: Arc<ScheduledJobInstanceRepository>,
    http_client: reqwest::Client,
    credentials: Option<Arc<OutboundCredentialsResolver>>,
    shutdown: broadcast::Sender<()>,
}

impl ScheduledJobSchedulerService {
    pub fn new(
        config: ScheduledJobSchedulerConfig,
        repo: Arc<ScheduledJobRepository>,
        instance_repo: Arc<ScheduledJobInstanceRepository>,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(config.http_timeout)
            .build()
            .expect("Failed to build scheduled-job HTTP client");
        let (shutdown, _) = broadcast::channel(1);
        Self {
            config,
            repo,
            instance_repo,
            http_client,
            credentials: None,
            shutdown,
        }
    }

    /// Sign each firing with its job's application's credentials (Java
    /// `JobDispatcher`); without this every firing goes out unsigned.
    pub fn with_credentials(mut self, credentials: Arc<OutboundCredentialsResolver>) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Spawn poller + dispatcher tasks. Returns join handles caller can `.abort()`
    /// or await; for graceful shutdown call [`Self::shutdown`].
    pub fn start(&self) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>) {
        let poller = ScheduledJobPoller::new(
            self.config.clone(),
            self.repo.clone(),
            self.instance_repo.clone(),
            self.shutdown.subscribe(),
        );
        let mut dispatcher = ScheduledJobDispatcher::new(
            self.config.clone(),
            self.repo.clone(),
            self.instance_repo.clone(),
            self.http_client.clone(),
            self.shutdown.subscribe(),
        );
        if let Some(credentials) = &self.credentials {
            dispatcher = dispatcher.with_credentials(credentials.clone());
        }
        (
            tokio::spawn(async move { poller.run().await }),
            tokio::spawn(async move { dispatcher.run().await }),
        )
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown.send(());
    }
}
