//! Dispatch Scheduler Service
//!
//! Polls PENDING dispatch jobs and publishes them to the message queue,
//! respecting DispatchMode (IMMEDIATE, NEXT_ON_ERROR, BLOCK_ON_ERROR).
//!
//! Architecture mirrors the TypeScript implementation:
//! - PendingJobPoller: queries PENDING jobs, groups by messageGroup, filters by mode
//! - BlockOnErrorChecker: checks for FAILED jobs in message groups
//! - MessageGroupDispatcher: concurrency coordinator with semaphore
//! - MessageGroupQueue: per-group FIFO queue (1 in-flight at a time)
//! - JobDispatcher: publishes to queue, updates status to QUEUED
//! - StaleQueuedJobPoller: safety net for stuck QUEUED jobs
//!
//! Additionally contains EventDispatcher for creating dispatch jobs from events.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use chrono::Utc;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tracing::{info, warn, error, debug};

use fc_common::{Message, MediationType};
use fc_queue::QueuePublisher;

use crate::{DispatchJob, DispatchJobRepository, DispatchMode, DispatchStatus};
use crate::shared::error::Result;

const DEFAULT_MESSAGE_GROUP: &str = "default";

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the dispatch scheduler.
/// Matches TypeScript DispatchSchedulerConfig.
#[derive(Debug, Clone)]
pub struct DispatchSchedulerConfig {
    /// Interval between polling for pending jobs (milliseconds)
    pub poll_interval_ms: u64,

    /// Maximum number of jobs to fetch per poll
    pub batch_size: u32,

    /// Maximum concurrent message group dispatches
    pub max_concurrent_groups: usize,

    /// Endpoint that processes dispatched jobs
    pub processing_endpoint: String,

    /// Default dispatch pool code when job has no pool assigned
    pub default_dispatch_pool_code: String,

    /// Minutes before a QUEUED job is considered stale
    pub stale_queued_threshold_minutes: u64,

    /// Interval between stale QUEUED job polls (milliseconds)
    pub stale_queued_poll_interval_ms: u64,
}

impl Default for DispatchSchedulerConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 5000,
            batch_size: 20,
            max_concurrent_groups: 10,
            processing_endpoint: "http://localhost:8080/api/dispatch/process".to_string(),
            default_dispatch_pool_code: "DISPATCH-POOL".to_string(),
            stale_queued_threshold_minutes: 15,
            stale_queued_poll_interval_ms: 60000,
        }
    }
}

// ============================================================================
// Block on Error Checker
// ============================================================================

/// Checks if message groups are blocked due to FAILED dispatch jobs.
/// For BLOCK_ON_ERROR mode, any failure in a message group blocks further
/// dispatching for that group until resolved.
struct BlockOnErrorChecker {
    job_repo: Arc<DispatchJobRepository>,
}

impl BlockOnErrorChecker {
    fn new(job_repo: Arc<DispatchJobRepository>) -> Self {
        Self { job_repo }
    }

    /// Get the set of blocked groups from a list of groups.
    async fn get_blocked_groups(&self, message_groups: &[String]) -> HashSet<String> {
        if message_groups.is_empty() {
            return HashSet::new();
        }

        let group_set: HashSet<&str> = message_groups.iter().map(|s| s.as_str()).collect();

        match self.job_repo.find_by_status(DispatchStatus::Failed, 1000).await {
            Ok(jobs) => {
                jobs.iter()
                    .filter_map(|j| j.message_group.as_deref())
                    .filter(|g| group_set.contains(g))
                    .map(|g| g.to_string())
                    .collect()
            }
            Err(e) => {
                error!("Error checking blocked groups: {:?}", e);
                HashSet::new()
            }
        }
    }
}

// ============================================================================
// Job Dispatcher
// ============================================================================

/// Dispatches individual dispatch jobs to the external queue.
/// Builds a Message (MessagePointer), publishes via QueuePublisher,
/// and updates the job status to QUEUED on success.
struct JobDispatcher {
    config: DispatchSchedulerConfig,
    job_repo: Arc<DispatchJobRepository>,
    publisher: Arc<dyn QueuePublisher>,
}

impl JobDispatcher {
    fn new(
        config: DispatchSchedulerConfig,
        job_repo: Arc<DispatchJobRepository>,
        publisher: Arc<dyn QueuePublisher>,
    ) -> Self {
        Self { config, job_repo, publisher }
    }

    /// Dispatch a single job to the queue. Returns true on success.
    async fn dispatch(&self, job: &DispatchJob) -> bool {
        // Build MessagePointer
        let message = Message {
            id: job.id.clone(),
            pool_code: job.dispatch_pool_id.clone()
                .unwrap_or_else(|| self.config.default_dispatch_pool_code.clone()),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: self.config.processing_endpoint.clone(),
            message_group_id: Some(
                job.message_group.clone()
                    .unwrap_or_else(|| DEFAULT_MESSAGE_GROUP.to_string())
            ),
            high_priority: false,
        };

        match self.publisher.publish(message).await {
            Ok(_) => {
                // Update status to QUEUED
                if let Err(e) = self.job_repo.update_status(&job.id, DispatchStatus::Queued).await {
                    error!(job_id = %job.id, "Failed to update job to QUEUED: {:?}", e);
                    return false;
                }
                debug!(job_id = %job.id, "Dispatched job to queue, status updated to QUEUED");
                true
            }
            Err(e) => {
                let error_msg = format!("{}", e);

                // Check for deduplication (still mark as QUEUED)
                if error_msg.contains("Deduplicated") || error_msg.contains("deduplicated") {
                    if let Err(e) = self.job_repo.update_status(&job.id, DispatchStatus::Queued).await {
                        error!(job_id = %job.id, "Failed to update deduplicated job to QUEUED: {:?}", e);
                        return false;
                    }
                    debug!(job_id = %job.id, "Job was deduplicated (already dispatched)");
                    return true;
                }

                warn!(job_id = %job.id, error = %error_msg, "Failed to dispatch job");
                false
            }
        }
    }
}

// ============================================================================
// Message Group Queue
// ============================================================================

/// In-memory queue for a single message group.
/// Ensures only 1 job is dispatched to the external queue at a time per group.
/// Jobs are sorted by sequence number and creation time for consistent ordering.
struct MessageGroupQueue {
    pending_jobs: VecDeque<DispatchJob>,
    job_in_flight: bool,
}

impl MessageGroupQueue {
    fn new() -> Self {
        Self {
            pending_jobs: VecDeque::new(),
            job_in_flight: false,
        }
    }

    /// Add jobs to this queue (sorted by sequence, createdAt).
    fn add_jobs(&mut self, jobs: Vec<DispatchJob>) {
        let mut sorted = jobs;
        sorted.sort_by(|a, b| {
            a.sequence.cmp(&b.sequence)
                .then(a.created_at.cmp(&b.created_at))
        });
        self.pending_jobs.extend(sorted);
    }

    /// Try to take the next job for dispatch.
    /// Returns None if a job is already in flight or no jobs are pending.
    fn try_take_next(&mut self) -> Option<DispatchJob> {
        if self.job_in_flight {
            return None;
        }

        let job = self.pending_jobs.pop_front()?;
        self.job_in_flight = true;
        Some(job)
    }

    /// Called when the current in-flight job has been dispatched.
    fn on_current_job_dispatched(&mut self) {
        self.job_in_flight = false;
    }

    fn has_pending_jobs(&self) -> bool {
        !self.pending_jobs.is_empty()
    }

    fn has_job_in_flight(&self) -> bool {
        self.job_in_flight
    }
}

// ============================================================================
// Message Group Dispatcher
// ============================================================================

/// Coordinates dispatch across message groups. Maintains in-memory queues
/// per message group and ensures only 1 job per group is dispatched to
/// the external queue at a time. Uses a semaphore to limit concurrent dispatches.
#[derive(Clone)]
struct MessageGroupDispatcher {
    inner: Arc<Mutex<HashMap<String, MessageGroupQueue>>>,
    job_dispatcher: Arc<JobDispatcher>,
    semaphore: Arc<Semaphore>,
}

impl MessageGroupDispatcher {
    fn new(
        config: &DispatchSchedulerConfig,
        job_dispatcher: Arc<JobDispatcher>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            job_dispatcher,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_groups)),
        }
    }

    /// Submit jobs for a message group.
    fn submit_jobs(&self, message_group: &str, jobs: Vec<DispatchJob>) {
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

            let success = this.job_dispatcher.dispatch(&job).await;

            if success {
                debug!(
                    job_id = %job.id,
                    message_group = %message_group,
                    "Successfully dispatched job"
                );
            } else {
                warn!(
                    job_id = %job.id,
                    message_group = %message_group,
                    "Failed to dispatch job"
                );
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

    /// Clean up empty queues.
    fn cleanup_empty_queues(&self) {
        let mut queues = self.inner.lock().unwrap();
        queues.retain(|_, queue| {
            queue.has_pending_jobs() || queue.has_job_in_flight()
        });
    }
}

// ============================================================================
// Dispatch Scheduler (Orchestrator)
// ============================================================================

/// Dispatch Scheduler - polls PENDING dispatch jobs and publishes them
/// to the message queue with message group ordering.
///
/// Matches the TypeScript dispatch-scheduler architecture:
/// - PendingJobPoller: periodic poll → group → filter → dispatch
/// - StaleQueuedJobPoller: resets stuck QUEUED jobs to PENDING
pub struct DispatchScheduler {
    config: DispatchSchedulerConfig,
    job_repo: Arc<DispatchJobRepository>,
    publisher: Arc<dyn QueuePublisher>,
    running: Arc<std::sync::atomic::AtomicBool>,
    handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl DispatchScheduler {
    pub fn new(
        config: DispatchSchedulerConfig,
        job_repo: Arc<DispatchJobRepository>,
        publisher: Arc<dyn QueuePublisher>,
    ) -> Self {
        Self {
            config,
            job_repo,
            publisher,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            handles: Arc::new(Mutex::new(vec![])),
        }
    }

    /// Start the dispatch scheduler.
    /// Creates components and starts polling loops.
    pub fn start(&self) {
        if self.running.load(std::sync::atomic::Ordering::SeqCst) {
            warn!("Dispatch scheduler already running");
            return;
        }
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);

        // Create components
        let block_on_error_checker = Arc::new(BlockOnErrorChecker::new(self.job_repo.clone()));
        let job_dispatcher = Arc::new(JobDispatcher::new(
            self.config.clone(),
            self.job_repo.clone(),
            self.publisher.clone(),
        ));
        let group_dispatcher = Arc::new(MessageGroupDispatcher::new(
            &self.config,
            job_dispatcher,
        ));

        // Start pollers
        let pending_handle = self.start_pending_job_poller(
            block_on_error_checker,
            group_dispatcher,
        );
        let stale_handle = self.start_stale_queued_job_poller();

        let mut handles = self.handles.lock().unwrap();
        handles.push(pending_handle);
        handles.push(stale_handle);

        info!("Dispatch Scheduler started");
    }

    /// Stop the dispatch scheduler.
    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);

        let mut handles = self.handles.lock().unwrap();
        for handle in handles.drain(..) {
            handle.abort();
        }

        info!("Dispatch Scheduler stopped");
    }

    /// Start the pending job poller task.
    fn start_pending_job_poller(
        &self,
        block_on_error_checker: Arc<BlockOnErrorChecker>,
        group_dispatcher: Arc<MessageGroupDispatcher>,
    ) -> JoinHandle<()> {
        let running = self.running.clone();
        let job_repo = self.job_repo.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            info!(
                poll_interval_ms = config.poll_interval_ms,
                "PendingJobPoller started"
            );

            let interval = Duration::from_millis(config.poll_interval_ms);

            loop {
                if !running.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                Self::do_pending_poll(
                    &job_repo,
                    &config,
                    &block_on_error_checker,
                    &group_dispatcher,
                ).await;

                tokio::time::sleep(interval).await;
            }

            info!("PendingJobPoller stopped");
        })
    }

    /// Execute a single pending job poll cycle.
    async fn do_pending_poll(
        job_repo: &DispatchJobRepository,
        config: &DispatchSchedulerConfig,
        block_on_error_checker: &BlockOnErrorChecker,
        group_dispatcher: &MessageGroupDispatcher,
    ) {
        // 1. Query PENDING jobs
        let mut pending_jobs = match job_repo.find_pending_for_dispatch(config.batch_size as i64).await {
            Ok(jobs) => jobs,
            Err(e) => {
                error!("Error polling for pending jobs: {:?}", e);
                return;
            }
        };

        if pending_jobs.is_empty() {
            return;
        }

        // Sort by messageGroup, sequence, createdAt (matches TypeScript ORDER BY)
        pending_jobs.sort_by(|a, b| {
            let group_a = a.message_group.as_deref().unwrap_or(DEFAULT_MESSAGE_GROUP);
            let group_b = b.message_group.as_deref().unwrap_or(DEFAULT_MESSAGE_GROUP);
            group_a.cmp(group_b)
                .then(a.sequence.cmp(&b.sequence))
                .then(a.created_at.cmp(&b.created_at))
        });
        pending_jobs.truncate(config.batch_size as usize);

        debug!(count = pending_jobs.len(), "Found pending jobs to process");

        // 2. Group by messageGroup
        let mut jobs_by_group: HashMap<String, Vec<DispatchJob>> = HashMap::new();
        for job in pending_jobs {
            let group = job.message_group.clone()
                .unwrap_or_else(|| DEFAULT_MESSAGE_GROUP.to_string());
            jobs_by_group.entry(group).or_default().push(job);
        }

        // 3. Check for blocked groups
        let group_keys: Vec<String> = jobs_by_group.keys().cloned().collect();
        let blocked_groups = block_on_error_checker.get_blocked_groups(&group_keys).await;

        // 4. Process each group
        for (message_group, group_jobs) in jobs_by_group {
            if blocked_groups.contains(&message_group) {
                debug!(
                    message_group = %message_group,
                    count = group_jobs.len(),
                    "Message group is blocked due to FAILED jobs, skipping"
                );
                continue;
            }

            // Filter by DispatchMode
            let dispatchable_jobs = Self::filter_by_dispatch_mode(group_jobs, &blocked_groups);

            if !dispatchable_jobs.is_empty() {
                debug!(
                    message_group = %message_group,
                    count = dispatchable_jobs.len(),
                    "Submitting jobs for message group"
                );
                group_dispatcher.submit_jobs(&message_group, dispatchable_jobs);
            }
        }

        // 5. Cleanup empty queues
        group_dispatcher.cleanup_empty_queues();
    }

    /// Filter jobs by DispatchMode.
    fn filter_by_dispatch_mode(
        jobs: Vec<DispatchJob>,
        blocked_groups: &HashSet<String>,
    ) -> Vec<DispatchJob> {
        jobs.into_iter().filter(|job| {
            let group = job.message_group.as_deref()
                .unwrap_or(DEFAULT_MESSAGE_GROUP);

            match job.mode {
                DispatchMode::Immediate => true,
                DispatchMode::NextOnError | DispatchMode::BlockOnError => {
                    !blocked_groups.contains(group)
                }
            }
        }).collect()
    }

    /// Start the stale queued job poller task.
    fn start_stale_queued_job_poller(&self) -> JoinHandle<()> {
        let running = self.running.clone();
        let job_repo = self.job_repo.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            info!(
                stale_queued_poll_interval_ms = config.stale_queued_poll_interval_ms,
                stale_queued_threshold_minutes = config.stale_queued_threshold_minutes,
                "StaleQueuedJobPoller started"
            );

            let interval = Duration::from_millis(config.stale_queued_poll_interval_ms);

            loop {
                if !running.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                Self::do_stale_queued_poll(&job_repo, &config).await;

                tokio::time::sleep(interval).await;
            }

            info!("StaleQueuedJobPoller stopped");
        })
    }

    /// Execute a single stale queued job poll cycle.
    /// Resets QUEUED jobs older than threshold back to PENDING.
    async fn do_stale_queued_poll(
        job_repo: &DispatchJobRepository,
        config: &DispatchSchedulerConfig,
    ) {
        let threshold_ms = config.stale_queued_threshold_minutes as i64 * 60 * 1000;
        let cutoff = Utc::now() - chrono::Duration::milliseconds(threshold_ms);

        match job_repo.find_by_status(DispatchStatus::Queued, 1000).await {
            Ok(queued_jobs) => {
                let stale_jobs: Vec<_> = queued_jobs
                    .into_iter()
                    .filter(|j| j.updated_at < cutoff)
                    .collect();

                if stale_jobs.is_empty() {
                    return;
                }

                let mut reset_count = 0;
                for job in &stale_jobs {
                    // Reset to PENDING (matches TypeScript behavior - simple reset, no retry counting)
                    if let Err(e) = job_repo.update_status(&job.id, DispatchStatus::Pending).await {
                        error!(job_id = %job.id, "Failed to reset stale queued job: {:?}", e);
                    } else {
                        reset_count += 1;
                    }
                }

                if reset_count > 0 {
                    info!(
                        count = reset_count,
                        threshold_minutes = config.stale_queued_threshold_minutes,
                        "Reset stale QUEUED jobs to PENDING"
                    );
                }
            }
            Err(e) => {
                error!("Error polling for stale QUEUED jobs: {:?}", e);
            }
        }
    }
}

// ============================================================================
// Event Dispatcher (unchanged - creates dispatch jobs from events)
// ============================================================================

/// Event dispatcher - creates dispatch jobs for subscriptions
pub struct EventDispatcher {
    job_repo: Arc<DispatchJobRepository>,
}

impl EventDispatcher {
    pub fn new(job_repo: Arc<DispatchJobRepository>) -> Self {
        Self { job_repo }
    }

    /// Dispatch an event to matching subscriptions
    /// Returns the created dispatch job IDs
    pub async fn dispatch(
        &self,
        event_id: &str,
        event_type: &str,
        source: &str,
        subject: Option<&str>,
        data: serde_json::Value,
        correlation_id: Option<&str>,
        message_group: Option<&str>,
        client_id: Option<&str>,
        subscriptions: Vec<crate::Subscription>,
    ) -> Result<Vec<String>> {
        let mut job_ids = Vec::new();

        // Serialize payload once
        let payload = serde_json::to_string(&data).unwrap_or_default();

        for subscription in subscriptions {
            // Skip if subscription doesn't match the event
            if !subscription.matches_event_type(event_type) {
                continue;
            }

            // Skip if subscription doesn't match the client
            if !subscription.matches_client(client_id) {
                continue;
            }

            // Create dispatch job using for_event constructor
            let mut job = DispatchJob::for_event(
                event_id,
                event_type,
                source,
                &subscription.target,
                &payload,
            );

            // Set subject
            if let Some(sub) = subject {
                job.subject = Some(sub.to_string());
            }

            // Set correlation ID
            if let Some(corr) = correlation_id {
                job = job.with_correlation_id(corr);
            }

            // Set message group
            if let Some(group) = message_group {
                job = job.with_message_group(group);
            }

            // Set client ID
            if let Some(cid) = client_id {
                job = job.with_client_id(cid);
            }

            // Set subscription details
            job = job
                .with_subscription_id(&subscription.id)
                .with_mode(subscription.mode.clone())
                .with_data_only(subscription.data_only);

            // Set dispatch pool if configured
            if let Some(ref pool_id) = subscription.dispatch_pool_id {
                job = job.with_dispatch_pool_id(pool_id);
            }

            // Set service account if configured
            if let Some(ref sa_id) = subscription.service_account_id {
                job = job.with_service_account_id(sa_id);
            }

            job.max_retries = subscription.max_retries;
            job.timeout_seconds = subscription.timeout_seconds;

            job_ids.push(job.id.clone());
            self.job_repo.insert(&job).await?;

            debug!(
                "Created dispatch job {} for event {} to subscription {}",
                job.id, event_id, subscription.id
            );
        }

        info!(
            "Created {} dispatch jobs for event {}",
            job_ids.len(),
            event_id
        );

        Ok(job_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_scheduler_config_default() {
        let config = DispatchSchedulerConfig::default();
        assert_eq!(config.poll_interval_ms, 5000);
        assert_eq!(config.batch_size, 20);
        assert_eq!(config.max_concurrent_groups, 10);
        assert_eq!(config.stale_queued_threshold_minutes, 15);
        assert_eq!(config.stale_queued_poll_interval_ms, 60000);
        assert_eq!(config.default_dispatch_pool_code, "DISPATCH-POOL");
    }

    #[test]
    fn test_message_group_queue_ordering() {
        let mut queue = MessageGroupQueue::new();
        assert!(!queue.has_pending_jobs());
        assert!(!queue.has_job_in_flight());

        // Create jobs with different sequences
        let job1 = {
            let mut j = DispatchJob::for_event("e1", "test", "src", "http://a", "{}");
            j.sequence = 2;
            j
        };
        let job2 = {
            let mut j = DispatchJob::for_event("e2", "test", "src", "http://b", "{}");
            j.sequence = 1;
            j
        };

        queue.add_jobs(vec![job1, job2]);
        assert!(queue.has_pending_jobs());

        // First job should be sequence 1
        let first = queue.try_take_next().unwrap();
        assert_eq!(first.sequence, 1);
        assert!(queue.has_job_in_flight());

        // Can't take another while in-flight
        assert!(queue.try_take_next().is_none());

        // After dispatch completes
        queue.on_current_job_dispatched();
        assert!(!queue.has_job_in_flight());

        // Now can take sequence 2
        let second = queue.try_take_next().unwrap();
        assert_eq!(second.sequence, 2);
    }

    #[test]
    fn test_filter_by_dispatch_mode() {
        let mut blocked = HashSet::new();
        blocked.insert("blocked-group".to_string());

        // IMMEDIATE jobs pass through even for blocked groups
        let immediate_job = {
            let mut j = DispatchJob::for_event("e1", "test", "src", "http://a", "{}");
            j.mode = DispatchMode::Immediate;
            j.message_group = Some("blocked-group".to_string());
            j
        };

        // BLOCK_ON_ERROR jobs are filtered for blocked groups
        let blocked_job = {
            let mut j = DispatchJob::for_event("e2", "test", "src", "http://b", "{}");
            j.mode = DispatchMode::BlockOnError;
            j.message_group = Some("blocked-group".to_string());
            j
        };

        // NEXT_ON_ERROR jobs for non-blocked groups pass
        let next_on_error_job = {
            let mut j = DispatchJob::for_event("e3", "test", "src", "http://c", "{}");
            j.mode = DispatchMode::NextOnError;
            j.message_group = Some("ok-group".to_string());
            j
        };

        let result = DispatchScheduler::filter_by_dispatch_mode(
            vec![immediate_job, blocked_job, next_on_error_job],
            &blocked,
        );

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].event_id.as_deref(), Some("e1")); // IMMEDIATE passes
        assert_eq!(result[1].event_id.as_deref(), Some("e3")); // NEXT_ON_ERROR on ok-group passes
    }
}
