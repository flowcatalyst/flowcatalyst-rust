//! FlowCatalyst Dispatch Scheduler
//!
//! This crate provides the dispatch scheduler functionality:
//! - PendingJobPoller: Polls for PENDING dispatch jobs
//! - BlockOnErrorChecker: Checks for blocked message groups (batch query)
//! - MessageGroupQueue: Per-group FIFO queue (1 in-flight at a time)
//! - MessageGroupDispatcher: Concurrency coordinator with semaphore
//! - JobDispatcher: Dispatches jobs to the message queue
//! - StaleQueuedJobPoller: Recovers jobs stuck in QUEUED status

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{RwLock, Semaphore};
use tokio::time::interval;
use tracing::{error, info, warn, debug};

pub mod auth;
pub mod dispatcher;
pub mod poller;
pub mod stale_recovery;

pub use auth::{AuthError, DispatchAuthService};
pub use dispatcher::JobDispatcher;
pub use poller::PendingJobPoller;
pub use stale_recovery::StaleQueuedJobPoller;

#[derive(Error, Debug)]
pub enum SchedulerError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),
    #[error("Queue error: {0}")]
    QueueError(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchMode {
    Immediate,
    NextOnError,
    BlockOnError,
}

impl Default for DispatchMode {
    fn default() -> Self { Self::Immediate }
}

impl From<&str> for DispatchMode {
    fn from(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "NEXT_ON_ERROR" => Self::NextOnError,
            "BLOCK_ON_ERROR" => Self::BlockOnError,
            _ => Self::Immediate,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchStatus {
    Pending, Queued, Processing, Completed, Error,
}

/// Dispatch job row from msg_dispatch_jobs table
#[derive(Debug, Clone, Serialize, Deserialize, FromQueryResult)]
pub struct DispatchJob {
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
}

impl DispatchJob {
    pub fn dispatch_mode(&self) -> DispatchMode {
        DispatchMode::from(self.mode.as_str())
    }

    pub fn dispatch_status(&self) -> DispatchStatus {
        match self.status.as_str() {
            "QUEUED" => DispatchStatus::Queued,
            "PROCESSING" | "IN_PROGRESS" => DispatchStatus::Processing,
            "COMPLETED" => DispatchStatus::Completed,
            "ERROR" | "FAILED" => DispatchStatus::Error,
            _ => DispatchStatus::Pending,
        }
    }
}

// ============================================================================
// Block on Error Checker (batch query)
// ============================================================================

/// Helper to query for blocked message groups — single batched query
#[derive(Debug, FromQueryResult)]
struct MessageGroupRow {
    pub message_group: String,
}

pub struct BlockOnErrorChecker {
    db: DatabaseConnection,
}

impl BlockOnErrorChecker {
    pub fn new(db: DatabaseConnection) -> Self { Self { db } }

    /// Get blocked groups from a set of candidate groups using a single batch query
    pub async fn get_blocked_groups(&self, groups: &HashSet<String>) -> Result<HashSet<String>, SchedulerError> {
        if groups.is_empty() { return Ok(HashSet::new()); }

        // Build a single IN(...) query for all groups at once
        let group_list: Vec<String> = groups.iter().cloned().collect();
        let placeholders: Vec<String> = (1..=group_list.len()).map(|i| format!("${}", i)).collect();
        let sql = format!(
            "SELECT DISTINCT message_group FROM msg_dispatch_jobs \
             WHERE message_group IN ({}) AND status IN ('FAILED', 'ERROR')",
            placeholders.join(", ")
        );

        let values: Vec<sea_orm::Value> = group_list.iter()
            .map(|g| sea_orm::Value::from(g.clone()))
            .collect();

        let rows = MessageGroupRow::find_by_statement(
            Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values),
        )
        .all(&self.db)
        .await?;

        Ok(rows.into_iter().map(|r| r.message_group).collect())
    }
}

// ============================================================================
// Message Group Queue (1-in-flight per group)
// ============================================================================

/// In-memory queue for a single message group.
/// Ensures only 1 job is dispatched to the queue at a time per group.
/// Jobs are sorted by sequence number and creation time.
pub struct MessageGroupQueue {
    pending_jobs: VecDeque<DispatchJob>,
    job_in_flight: bool,
}

impl MessageGroupQueue {
    pub fn new() -> Self {
        Self {
            pending_jobs: VecDeque::new(),
            job_in_flight: false,
        }
    }

    /// Add jobs to this queue (sorted by sequence, createdAt).
    pub fn add_jobs(&mut self, jobs: Vec<DispatchJob>) {
        let mut sorted = jobs;
        sorted.sort_by(|a, b| {
            a.sequence.cmp(&b.sequence)
                .then(a.created_at.cmp(&b.created_at))
        });
        self.pending_jobs.extend(sorted);
    }

    /// Try to take the next job for dispatch.
    /// Returns None if a job is already in flight or no jobs are pending.
    pub fn try_take_next(&mut self) -> Option<DispatchJob> {
        if self.job_in_flight {
            return None;
        }
        let job = self.pending_jobs.pop_front()?;
        self.job_in_flight = true;
        Some(job)
    }

    /// Called when the current in-flight job has been dispatched.
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
// Message Group Dispatcher (concurrency coordinator with semaphore)
// ============================================================================

/// Coordinates dispatch across message groups with semaphore-based concurrency control.
/// Each group has at most 1 job in-flight; the semaphore limits how many groups
/// dispatch concurrently.
#[derive(Clone)]
pub struct MessageGroupDispatcher {
    inner: Arc<Mutex<HashMap<String, MessageGroupQueue>>>,
    db: DatabaseConnection,
    queue_publisher: Arc<dyn QueuePublisher>,
    auth_service: DispatchAuthService,
    config: SchedulerConfig,
    semaphore: Arc<Semaphore>,
}

impl MessageGroupDispatcher {
    pub fn new(
        config: SchedulerConfig,
        db: DatabaseConnection,
        queue_publisher: Arc<dyn QueuePublisher>,
        auth_service: DispatchAuthService,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_groups));
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            db,
            queue_publisher,
            auth_service,
            config,
            semaphore,
        }
    }

    /// Submit jobs for a message group.
    pub fn submit_jobs(&self, message_group: &str, jobs: Vec<DispatchJob>) {
        if jobs.is_empty() {
            return;
        }

        let next_job = {
            let mut queues = self.inner.lock().unwrap();
            let queue = queues.entry(message_group.to_string())
                .or_insert_with(MessageGroupQueue::new);
            queue.add_jobs(jobs);
            queue.try_take_next()
        };

        if let Some(job) = next_job {
            self.spawn_dispatch(message_group.to_string(), job);
        }
    }

    /// Spawn an async task to dispatch a job with semaphore-based concurrency limiting.
    fn spawn_dispatch(&self, message_group: String, job: DispatchJob) {
        let this = self.clone();

        tokio::spawn(async move {
            let _permit = this.semaphore.acquire().await.unwrap();

            let success = this.dispatch_single_job(&job).await;

            if success {
                debug!(job_id = %job.id, message_group = %message_group, "Successfully dispatched job");
            } else {
                warn!(job_id = %job.id, message_group = %message_group, "Failed to dispatch job");
            }

            // Release permit before triggering next
            drop(_permit);

            // Trigger next job in this group
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

    /// Dispatch a single job to the queue. Returns true on success.
    async fn dispatch_single_job(&self, job: &DispatchJob) -> bool {
        // Generate HMAC auth token
        let auth_token = match self.auth_service.generate_auth_token(&job.id) {
            Ok(token) => token,
            Err(e) => {
                warn!(job_id = %job.id, error = %e, "Failed to generate auth token, using fallback");
                format!("dev_{}", job.id)
            }
        };

        let pointer = MessagePointer {
            job_id: job.id.clone(),
            dispatch_pool_id: job.dispatch_pool_id.clone()
                .unwrap_or_else(|| self.config.default_pool_code.clone()),
            auth_token,
            mediation_type: "HTTP".to_string(),
            processing_endpoint: self.config.processing_endpoint.clone(),
            message_group: job.message_group.clone(),
            batch_id: None,
        };

        let message = QueueMessage {
            id: job.id.clone(),
            message_group: job.message_group.clone(),
            deduplication_id: job.id.clone(),
            body: match serde_json::to_string(&pointer) {
                Ok(b) => b,
                Err(e) => {
                    error!(job_id = %job.id, error = %e, "Failed to serialize message pointer");
                    return false;
                }
            },
        };

        metrics::counter!("scheduler.jobs.dispatched_total").increment(1);

        match self.queue_publisher.publish(message).await {
            Ok(_) => {
                let sql = "UPDATE msg_dispatch_jobs SET status = 'QUEUED', queued_at = NOW(), updated_at = NOW() WHERE id = $1";
                if let Err(e) = self.db.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    sql,
                    vec![sea_orm::Value::from(job.id.clone())],
                )).await {
                    error!(job_id = %job.id, error = %e, "Failed to update job to QUEUED");
                    return false;
                }
                metrics::counter!("scheduler.jobs.queued_total").increment(1);
                true
            }
            Err(e) => {
                let error_msg = format!("{}", e);

                // Handle deduplication — still mark as QUEUED
                if error_msg.contains("Deduplicated") || error_msg.contains("deduplicated") {
                    let sql = "UPDATE msg_dispatch_jobs SET status = 'QUEUED', queued_at = NOW(), updated_at = NOW() WHERE id = $1";
                    if let Err(e) = self.db.execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        sql,
                        vec![sea_orm::Value::from(job.id.clone())],
                    )).await {
                        error!(job_id = %job.id, error = %e, "Failed to update deduplicated job to QUEUED");
                        return false;
                    }
                    debug!(job_id = %job.id, "Job was deduplicated (already dispatched)");
                    return true;
                }

                warn!(job_id = %job.id, error = %error_msg, "Failed to dispatch job");
                metrics::counter!("scheduler.jobs.dispatch_errors_total").increment(1);
                false
            }
        }
    }

    /// Clean up empty queues.
    pub fn cleanup_empty_queues(&self) {
        let mut queues = self.inner.lock().unwrap();
        queues.retain(|_, queue| {
            queue.has_pending_jobs() || queue.has_job_in_flight()
        });
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
    /// App key for HMAC auth token generation
    pub app_key: Option<String>,
    /// Maximum concurrent message group dispatches (semaphore size)
    pub max_concurrent_groups: usize,
    /// Whether to filter out jobs for paused connections
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
            default_pool_code: "default".to_string(),
            processing_endpoint: "http://localhost:8080/api/router/process".to_string(),
            app_key: None,
            max_concurrent_groups: 50,
            connection_filter_enabled: true,
        }
    }
}

// ============================================================================
// Queue types
// ============================================================================

#[async_trait]
pub trait QueuePublisher: Send + Sync {
    async fn publish(&self, message: QueueMessage) -> Result<(), SchedulerError>;
    fn is_healthy(&self) -> bool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMessage {
    pub id: String,
    pub message_group: Option<String>,
    pub deduplication_id: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePointer {
    pub job_id: String,
    pub dispatch_pool_id: String,
    pub auth_token: String,
    pub mediation_type: String,
    pub processing_endpoint: String,
    pub message_group: Option<String>,
    pub batch_id: Option<String>,
}

// ============================================================================
// Dispatch Scheduler (Orchestrator)
// ============================================================================

pub struct DispatchScheduler {
    config: SchedulerConfig,
    db: DatabaseConnection,
    queue_publisher: Arc<dyn QueuePublisher>,
    running: Arc<RwLock<bool>>,
}

impl DispatchScheduler {
    pub fn new(config: SchedulerConfig, db: DatabaseConnection, queue_publisher: Arc<dyn QueuePublisher>) -> Self {
        Self { config, db, queue_publisher, running: Arc::new(RwLock::new(false)) }
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

        let auth_service = DispatchAuthService::new(self.config.app_key.clone());
        let group_dispatcher = Arc::new(MessageGroupDispatcher::new(
            self.config.clone(),
            self.db.clone(),
            self.queue_publisher.clone(),
            auth_service,
        ));

        let poller = PendingJobPoller::new(self.config.clone(), self.db.clone(), group_dispatcher.clone());
        let poll_interval = self.config.poll_interval;
        let running_clone = self.running.clone();

        tokio::spawn(async move {
            let mut interval = interval(poll_interval);
            loop {
                interval.tick().await;
                if !*running_clone.read().await { break; }
                if let Err(e) = poller.poll().await {
                    error!(error = %e, "Error in pending job poller");
                }
                group_dispatcher.cleanup_empty_queues();
            }
        });

        let stale_poller = StaleQueuedJobPoller::new(self.config.clone(), self.db.clone());
        let running_clone2 = self.running.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                if !*running_clone2.read().await { break; }
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
        assert_eq!(DispatchMode::from("IMMEDIATE"), DispatchMode::Immediate);
        assert_eq!(DispatchMode::from("NEXT_ON_ERROR"), DispatchMode::NextOnError);
        assert_eq!(DispatchMode::from("BLOCK_ON_ERROR"), DispatchMode::BlockOnError);
        assert_eq!(DispatchMode::from("unknown"), DispatchMode::Immediate);
    }

    #[test]
    fn test_dispatch_status() {
        let job = DispatchJob {
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
        };
        assert_eq!(job.dispatch_status(), DispatchStatus::Queued);
    }

    #[test]
    fn test_message_group_queue_ordering() {
        let mut queue = MessageGroupQueue::new();
        assert!(!queue.has_pending_jobs());
        assert!(!queue.has_job_in_flight());

        let now = Utc::now();
        let job1 = DispatchJob {
            id: "job1".to_string(), message_group: Some("g1".to_string()),
            dispatch_pool_id: None, status: "PENDING".to_string(),
            mode: "IMMEDIATE".to_string(), target_url: "http://a".to_string(),
            payload: None, sequence: 2, created_at: now, updated_at: now,
            queued_at: None, last_error: None,
        };
        let job2 = DispatchJob {
            id: "job2".to_string(), message_group: Some("g1".to_string()),
            dispatch_pool_id: None, status: "PENDING".to_string(),
            mode: "IMMEDIATE".to_string(), target_url: "http://b".to_string(),
            payload: None, sequence: 1, created_at: now, updated_at: now,
            queued_at: None, last_error: None,
        };

        queue.add_jobs(vec![job1, job2]);
        assert!(queue.has_pending_jobs());

        // First job should be sequence 1 (job2)
        let first = queue.try_take_next().unwrap();
        assert_eq!(first.id, "job2");
        assert!(queue.has_job_in_flight());

        // Can't take another while in-flight
        assert!(queue.try_take_next().is_none());

        // After dispatch completes
        queue.on_current_job_dispatched();
        assert!(!queue.has_job_in_flight());

        // Now can take sequence 2 (job1)
        let second = queue.try_take_next().unwrap();
        assert_eq!(second.id, "job1");
    }

    #[test]
    fn test_default_config_matches_ts() {
        let config = SchedulerConfig::default();
        assert_eq!(config.poll_interval, Duration::from_millis(5000));
        assert_eq!(config.batch_size, 200);
        assert_eq!(config.max_concurrent_groups, 50);
        assert!(config.connection_filter_enabled);
    }
}
