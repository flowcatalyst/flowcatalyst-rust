//! FlowCatalyst Dispatch Scheduler
//!
//! Polls PENDING dispatch jobs from the database, groups them by message_group,
//! applies ordering/blocking rules, and publishes to the message queue via
//! `fc_queue::QueuePublisher`. The message router then delivers via the
//! `/api/dispatch/process` callback.
//!
//! Components:
//! - `PendingJobPoller`: Polls for PENDING dispatch jobs
//! - `BlockOnErrorChecker`: Checks for blocked message groups (batch query)
//! - `MessageGroupQueue`: Per-group FIFO queue (1 in-flight at a time)
//! - `MessageGroupDispatcher`: Concurrency coordinator with semaphore
//! - `StaleQueuedJobPoller`: Recovers jobs stuck in QUEUED status
//! - `DispatchAuthService`: HMAC-SHA256 auth tokens for dispatch jobs

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;
use tokio::sync::{RwLock, Semaphore};
use tokio::time::interval;
use tracing::{error, info, trace, warn};

pub mod auth;
pub mod dispatcher;
pub mod poller;
pub mod stale_recovery;

pub use auth::{AuthError, DispatchAuthService};
pub use dispatcher::JobDispatcher;
pub use poller::{PausedConnectionCache, PendingJobPoller};
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
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Lightweight dispatch job row for scheduler queries.
/// This is a projection of msg_dispatch_jobs — only the fields the scheduler needs.
/// The full domain entity lives in `crate::dispatch_job::entity::DispatchJob`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SchedulerJobRow {
    pub id: String,
    pub message_group: Option<String>,
    pub dispatch_pool_id: Option<String>,
    pub status: String,
    pub mode: String,
    pub target_url: String,
    pub payload: Option<String>,
    pub sequence: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub queued_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub subscription_id: Option<String>,
}

impl SchedulerJobRow {
    pub fn dispatch_mode(&self) -> DispatchMode {
        DispatchMode::from_str(&self.mode)
    }

    pub fn dispatch_status(&self) -> DispatchStatus {
        DispatchStatus::from_str(&self.status)
    }
}

// ============================================================================
// Block on Error Checker (batch query)
// ============================================================================

pub struct BlockOnErrorChecker {
    pool: PgPool,
}

impl BlockOnErrorChecker {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get blocked groups from a set of candidate groups using a single batch query
    pub async fn get_blocked_groups(
        &self,
        groups: &HashSet<String>,
    ) -> Result<HashSet<String>, SchedulerError> {
        if groups.is_empty() {
            return Ok(HashSet::new());
        }

        let group_list: Vec<String> = groups.iter().cloned().collect();
        let sql = "SELECT DISTINCT message_group FROM msg_dispatch_jobs \
                   WHERE message_group = ANY($1) AND status IN ('FAILED', 'ERROR')";

        let rows: Vec<Option<String>> = sqlx::query_scalar(sql)
            .bind(&group_list)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().flatten().collect())
    }
}

// ============================================================================
// Message Group Queue (1-in-flight per group)
// ============================================================================

pub struct MessageGroupQueue {
    pending_jobs: VecDeque<SchedulerJobRow>,
    job_in_flight: bool,
}

impl Default for MessageGroupQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageGroupQueue {
    pub fn new() -> Self {
        Self {
            pending_jobs: VecDeque::new(),
            job_in_flight: false,
        }
    }

    pub fn add_jobs(&mut self, jobs: Vec<SchedulerJobRow>) {
        let mut sorted = jobs;
        sorted.sort_by(|a, b| {
            a.sequence
                .cmp(&b.sequence)
                .then(a.created_at.cmp(&b.created_at))
        });
        self.pending_jobs.extend(sorted);
    }

    pub fn try_take_next(&mut self) -> Option<SchedulerJobRow> {
        if self.job_in_flight {
            return None;
        }
        let job = self.pending_jobs.pop_front()?;
        self.job_in_flight = true;
        Some(job)
    }

    pub fn on_current_job_dispatched(&mut self) {
        self.job_in_flight = false;
    }

    pub fn has_pending_jobs(&self) -> bool {
        !self.pending_jobs.is_empty()
    }

    pub fn has_job_in_flight(&self) -> bool {
        self.job_in_flight
    }
}

// ============================================================================
// Message Group Dispatcher (concurrency coordinator)
// ============================================================================

#[derive(Clone)]
pub struct MessageGroupDispatcher {
    inner: Arc<Mutex<HashMap<String, MessageGroupQueue>>>,
    pool: PgPool,
    queue_publisher: Arc<dyn fc_queue::QueuePublisher>,
    config: SchedulerConfig,
    semaphore: Arc<Semaphore>,
}

impl MessageGroupDispatcher {
    pub fn new(
        config: SchedulerConfig,
        pool: PgPool,
        queue_publisher: Arc<dyn fc_queue::QueuePublisher>,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_groups));
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            pool,
            queue_publisher,
            config,
            semaphore,
        }
    }

    pub fn submit_jobs(&self, message_group: &str, jobs: Vec<SchedulerJobRow>) {
        if jobs.is_empty() {
            return;
        }

        let next_job = {
            let mut queues = self.inner.lock().unwrap();
            let queue = queues.entry(message_group.to_string()).or_default();
            queue.add_jobs(jobs);
            queue.try_take_next()
        };

        if let Some(job) = next_job {
            self.spawn_dispatch(message_group.to_string(), job);
        }
    }

    fn spawn_dispatch(&self, message_group: String, job: SchedulerJobRow) {
        let this = self.clone();

        tokio::spawn(async move {
            let _permit = this.semaphore.acquire().await.unwrap();

            let success = this.dispatch_single_job(&job).await;

            if success {
                trace!(job_id = %job.id, message_group = %message_group, "Successfully dispatched job");
            } else {
                warn!(job_id = %job.id, message_group = %message_group, "Failed to dispatch job");
            }

            drop(_permit);

            let next_job = {
                let mut queues = this.inner.lock().unwrap();
                if let Some(queue) = queues.get_mut(&message_group) {
                    queue.on_current_job_dispatched();
                    queue.try_take_next()
                } else {
                    None
                }
            };

            if let Some(next) = next_job {
                this.spawn_dispatch(message_group, next);
            }
        });
    }

    /// Build an fc_common::Message directly and publish via fc_queue::QueuePublisher.
    async fn dispatch_single_job(&self, job: &SchedulerJobRow) -> bool {
        let message = fc_common::Message {
            id: job.id.clone(),
            pool_code: job
                .dispatch_pool_id
                .clone()
                .unwrap_or_else(|| self.config.default_pool_code.clone()),
            auth_token: None,
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: self.config.processing_endpoint.clone(),
            message_group_id: job.message_group.clone(),
            high_priority: false,
            dispatch_mode: job.dispatch_mode(),
            dispatch_mode_specified: true,
        };

        metrics::counter!("scheduler.jobs.dispatched_total").increment(1);

        let should_mark_queued = match self.queue_publisher.publish(message).await {
            Ok(_) => true,
            Err(e) => {
                let error_msg = format!("{}", e);
                if error_msg.contains("Deduplicated") || error_msg.contains("deduplicated") {
                    trace!(job_id = %job.id, "Job was deduplicated (already dispatched)");
                    true
                } else {
                    warn!(job_id = %job.id, error = %error_msg, "Failed to dispatch job");
                    metrics::counter!("scheduler.jobs.dispatch_errors_total").increment(1);
                    false
                }
            }
        };

        if should_mark_queued {
            if let Err(e) = self
                .batch_update_status_queued(&[(&job.id, job.created_at)])
                .await
            {
                error!(job_id = %job.id, error = %e, "Failed to update job to QUEUED");
                return false;
            }
            metrics::counter!("scheduler.jobs.queued_total").increment(1);
            true
        } else {
            false
        }
    }

    /// Mark jobs as QUEUED. Uses (id, created_at) so PG can prune to the
    /// owning partition instead of scanning all active partition PK indexes.
    async fn batch_update_status_queued(
        &self,
        jobs: &[(&str, DateTime<Utc>)],
    ) -> Result<(), sqlx::Error> {
        if jobs.is_empty() {
            return Ok(());
        }

        let ids: Vec<String> = jobs.iter().map(|(id, _)| id.to_string()).collect();
        let created_ats: Vec<DateTime<Utc>> = jobs.iter().map(|(_, ts)| *ts).collect();

        sqlx::query(
            "UPDATE msg_dispatch_jobs SET status = 'QUEUED', queued_at = NOW(), updated_at = NOW() \
             FROM UNNEST($1::varchar[], $2::timestamptz[]) AS t(id, created_at) \
             WHERE msg_dispatch_jobs.id = t.id AND msg_dispatch_jobs.created_at = t.created_at",
        )
        .bind(&ids)
        .bind(&created_ats)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub fn cleanup_empty_queues(&self) {
        let mut queues = self.inner.lock().unwrap();
        queues.retain(|_, queue| queue.has_pending_jobs() || queue.has_job_in_flight());
    }
}

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub enabled: bool,
    pub poll_interval: Duration,
    pub batch_size: usize,
    pub stale_threshold: Duration,
    pub default_dispatch_mode: DispatchMode,
    pub default_pool_code: String,
    pub processing_endpoint: String,
    pub app_key: Option<String>,
    pub max_concurrent_groups: usize,
    pub connection_filter_enabled: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval: Duration::from_millis(5000),
            batch_size: 200,
            stale_threshold: Duration::from_secs(15 * 60),
            default_dispatch_mode: DispatchMode::Immediate,
            default_pool_code: "DISPATCH-POOL".to_string(),
            processing_endpoint: "http://localhost:8080/api/dispatch/process".to_string(),
            app_key: None,
            max_concurrent_groups: 10,
            connection_filter_enabled: true,
        }
    }
}

// ============================================================================
// Dispatch Scheduler (Orchestrator)
// ============================================================================

pub struct DispatchScheduler {
    config: SchedulerConfig,
    pool: PgPool,
    queue_publisher: Arc<dyn fc_queue::QueuePublisher>,
    running: Arc<RwLock<bool>>,
}

impl DispatchScheduler {
    pub fn new(
        config: SchedulerConfig,
        pool: PgPool,
        queue_publisher: Arc<dyn fc_queue::QueuePublisher>,
    ) -> Self {
        Self {
            config,
            pool,
            queue_publisher,
            running: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn start(&self) {
        if !self.config.enabled {
            info!("Dispatch scheduler is disabled");
            return;
        }

        let mut running = self.running.write().await;
        if *running {
            warn!("Scheduler already running");
            return;
        }
        *running = true;
        drop(running);

        info!(
            poll_interval_ms = self.config.poll_interval.as_millis(),
            batch_size = self.config.batch_size,
            max_concurrent_groups = self.config.max_concurrent_groups,
            "Starting dispatch scheduler"
        );

        let group_dispatcher = Arc::new(MessageGroupDispatcher::new(
            self.config.clone(),
            self.pool.clone(),
            self.queue_publisher.clone(),
        ));

        let poller = PendingJobPoller::new(
            self.config.clone(),
            self.pool.clone(),
            group_dispatcher.clone(),
        );
        let batch_size = self.config.batch_size;
        let running_clone = self.running.clone();

        let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);
        poller.paused_cache().spawn_refresh_task(shutdown_tx);

        tokio::spawn(async move {
            loop {
                if !*running_clone.read().await {
                    break;
                }

                let job_count = match poller.poll().await {
                    Ok(count) => count,
                    Err(e) => {
                        error!(error = %e, "Error in pending job poller");
                        0
                    }
                };
                group_dispatcher.cleanup_empty_queues();

                if job_count >= batch_size {
                    tokio::task::yield_now().await;
                } else if job_count > 0 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                } else {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        let stale_poller = StaleQueuedJobPoller::new(self.config.clone(), self.pool.clone());
        let running_clone2 = self.running.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                if !*running_clone2.read().await {
                    break;
                }
                if let Err(e) = stale_poller.recover_stale_jobs().await {
                    error!(error = %e, "Error in stale job recovery");
                }
            }
        });
    }

    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
        info!("Dispatch scheduler stopped");
    }

    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_mode_from_str() {
        assert_eq!(DispatchMode::from_str("IMMEDIATE"), DispatchMode::Immediate);
        assert_eq!(
            DispatchMode::from_str("NEXT_ON_ERROR"),
            DispatchMode::NextOnError
        );
        assert_eq!(
            DispatchMode::from_str("BLOCK_ON_ERROR"),
            DispatchMode::BlockOnError
        );
        // Ledger A-09/X-01: unspecified/unrecognised ⇒ NEXT_ON_ERROR.
        assert_eq!(DispatchMode::from_str("unknown"), DispatchMode::NextOnError);
    }

    #[test]
    fn test_dispatch_status() {
        let job = SchedulerJobRow {
            id: "test".to_string(),
            message_group: None,
            dispatch_pool_id: None,
            status: "QUEUED".to_string(),
            mode: "IMMEDIATE".to_string(),
            target_url: "http://test".to_string(),
            payload: None,
            sequence: 99,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            queued_at: None,
            last_error: None,
            subscription_id: None,
        };
        assert_eq!(job.dispatch_status(), DispatchStatus::Queued);
    }

    #[test]
    fn test_message_group_queue_ordering() {
        let mut queue = MessageGroupQueue::new();
        assert!(!queue.has_pending_jobs());
        assert!(!queue.has_job_in_flight());

        let now = Utc::now();
        let job1 = SchedulerJobRow {
            id: "job1".to_string(),
            message_group: Some("g1".to_string()),
            dispatch_pool_id: None,
            status: "PENDING".to_string(),
            mode: "IMMEDIATE".to_string(),
            target_url: "http://a".to_string(),
            payload: None,
            sequence: 2,
            created_at: now,
            updated_at: now,
            queued_at: None,
            last_error: None,
            subscription_id: None,
        };
        let job2 = SchedulerJobRow {
            id: "job2".to_string(),
            message_group: Some("g1".to_string()),
            dispatch_pool_id: None,
            status: "PENDING".to_string(),
            mode: "IMMEDIATE".to_string(),
            target_url: "http://b".to_string(),
            payload: None,
            sequence: 1,
            created_at: now,
            updated_at: now,
            queued_at: None,
            last_error: None,
            subscription_id: None,
        };

        queue.add_jobs(vec![job1, job2]);
        assert!(queue.has_pending_jobs());

        let first = queue.try_take_next().unwrap();
        assert_eq!(first.id, "job2");
        assert!(queue.has_job_in_flight());

        assert!(queue.try_take_next().is_none());

        queue.on_current_job_dispatched();
        assert!(!queue.has_job_in_flight());

        let second = queue.try_take_next().unwrap();
        assert_eq!(second.id, "job1");
    }

    #[test]
    fn test_default_config_matches_ts() {
        let config = SchedulerConfig::default();
        assert_eq!(config.poll_interval, Duration::from_millis(5000));
        assert_eq!(config.batch_size, 200);
        assert_eq!(config.max_concurrent_groups, 10);
        assert_eq!(config.default_pool_code, "DISPATCH-POOL");
        assert_eq!(
            config.processing_endpoint,
            "http://localhost:8080/api/dispatch/process"
        );
        assert!(config.connection_filter_enabled);
    }
}
