//! ProcessPool - Worker pool with FIFO ordering, rate limiting, and concurrency control
//!
//! Mirrors the Java ProcessPoolImpl with:
//! - Per-message-group FIFO ordering
//! - Semaphore-based concurrency control
//! - Rate limiting using governor
//! - Dynamic worker tasks per message group

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use std::num::NonZeroU32;
use dashmap::{DashMap, DashSet};
use tokio::sync::{mpsc, Semaphore, oneshot};
use governor::{Quota, RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use tracing::{info, warn, error, debug};

use fc_common::{
    Message, BatchMessage, AckNack, PoolConfig, PoolStats,
    MediationResult, EnhancedPoolMetrics,
};
use crate::mediator::Mediator;
use crate::metrics::PoolMetricsCollector;
use crate::Result;

const DEFAULT_GROUP: &str = "__DEFAULT__";
const QUEUE_CAPACITY_MULTIPLIER: u32 = 20;  // Java: QUEUE_CAPACITY_MULTIPLIER = 20
const MIN_QUEUE_CAPACITY: u32 = 50;          // Java: MIN_QUEUE_CAPACITY = 50

/// Composite key for batch+group tracking - avoids format!() string allocation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BatchGroupKey {
    pub batch_id: Arc<str>,
    pub group_id: Arc<str>,
}

impl BatchGroupKey {
    #[inline]
    pub fn new(batch_id: &str, group_id: &str) -> Self {
        Self {
            batch_id: Arc::from(batch_id),
            group_id: Arc::from(group_id),
        }
    }
}

impl std::fmt::Display for BatchGroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.batch_id, self.group_id)
    }
}

/// Task submitted to a pool worker
pub struct PoolTask {
    pub message: Message,
    pub receipt_handle: String,
    pub ack_tx: oneshot::Sender<AckNack>,
    pub batch_id: Option<Arc<str>>,
    /// Pre-computed batch+group key for FIFO tracking (uses tuple to avoid string formatting)
    pub batch_group_key: Option<BatchGroupKey>,
}

/// Dual-channel pair for high/regular priority routing within a message group.
/// The worker drains the high channel first (biased select) before checking regular.
#[derive(Clone)]
struct PriorityChannels {
    high: mpsc::Sender<PoolTask>,
    regular: mpsc::Sender<PoolTask>,
}

/// Process pool with FIFO ordering and rate limiting
pub struct ProcessPool {
    config: PoolConfig,
    mediator: Arc<dyn Mediator>,

    /// Current concurrency level (may differ from config after updates)
    concurrency: AtomicU32,

    /// Pool-level concurrency semaphore
    semaphore: Arc<Semaphore>,

    /// Per-message-group queues for FIFO ordering with priority support (uses Arc<str> to avoid cloning)
    message_group_queues: DashMap<Arc<str>, PriorityChannels>,

    /// Track active group threads for liveness detection (Java: activeGroupThreads)
    /// When a worker exits (normally or abnormally), it removes itself from this map
    active_group_threads: DashSet<Arc<str>>,

    /// Track in-flight message groups
    in_flight_groups: DashSet<Arc<str>>,

    /// Batch+group failure tracking for cascading NACKs (uses tuple key to avoid format!())
    failed_batch_groups: DashSet<BatchGroupKey>,

    /// Track remaining messages per batch+group for cleanup (Java: batchGroupMessageCount)
    batch_group_message_count: Arc<DashMap<BatchGroupKey, AtomicU32>>,

    /// Rate limiter (optional, behind Arc<RwLock> for sharing with workers and in-place updates)
    rate_limiter: Arc<parking_lot::RwLock<Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>>>,

    /// Current rate limit value for comparison during updates
    rate_limit_per_minute: Arc<parking_lot::RwLock<Option<u32>>>,

    /// Running state
    running: AtomicBool,

    /// Queue size counter (Arc for sharing across tasks)
    queue_size: Arc<AtomicU32>,

    /// Active workers counter (Arc for sharing across tasks)
    active_workers: Arc<AtomicU32>,

    /// Enhanced metrics collector
    metrics_collector: Arc<PoolMetricsCollector>,

    /// Warning service for generating warnings (optional)
    warning_service: Option<Arc<crate::warning::WarningService>>,
}

impl ProcessPool {
    pub fn new(config: PoolConfig, mediator: Arc<dyn Mediator>) -> Self {
        let concurrency_val = config.concurrency;

        let rate_limiter = config.rate_limit_per_minute.and_then(|rpm| {
            NonZeroU32::new(rpm).map(|nz| {
                Arc::new(RateLimiter::direct(Quota::per_minute(nz)))
            })
        });

        Self {
            config: config.clone(),
            mediator,
            concurrency: AtomicU32::new(concurrency_val),
            semaphore: Arc::new(Semaphore::new(concurrency_val as usize)),
            message_group_queues: DashMap::new(),
            active_group_threads: DashSet::new(),
            in_flight_groups: DashSet::new(),
            failed_batch_groups: DashSet::new(),
            batch_group_message_count: Arc::new(DashMap::new()),
            rate_limiter: Arc::new(parking_lot::RwLock::new(rate_limiter)),
            rate_limit_per_minute: Arc::new(parking_lot::RwLock::new(config.rate_limit_per_minute)),
            running: AtomicBool::new(false),
            queue_size: Arc::new(AtomicU32::new(0)),
            active_workers: Arc::new(AtomicU32::new(0)),
            metrics_collector: Arc::new(PoolMetricsCollector::new()),
            warning_service: None,
        }
    }

    /// Set the warning service for generating warnings
    pub fn with_warning_service(mut self, warning_service: Arc<crate::warning::WarningService>) -> Self {
        self.warning_service = Some(warning_service);
        self
    }

    /// Set warning service after construction
    pub fn set_warning_service(&mut self, warning_service: Arc<crate::warning::WarningService>) {
        self.warning_service = Some(warning_service);
    }

    /// Start the pool
    pub async fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // Already running
        }

        info!(
            pool_code = %self.config.code,
            concurrency = self.config.concurrency,
            rate_limit = ?self.config.rate_limit_per_minute,
            "Starting process pool"
        );
    }

    /// Submit a message to the pool
    pub async fn submit(&self, batch_msg: BatchMessage) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            let _ = batch_msg.ack_tx.send(AckNack::Nack { delay_seconds: Some(10) });
            return Ok(());
        }

        // Check capacity
        let current_size = self.queue_size.load(Ordering::SeqCst);
        let capacity = std::cmp::max(
            self.config.concurrency * QUEUE_CAPACITY_MULTIPLIER,
            MIN_QUEUE_CAPACITY,
        );

        if current_size >= capacity {
            debug!(
                pool_code = %self.config.code,
                current = current_size,
                capacity = capacity,
                "Pool at capacity, rejecting"
            );
            let _ = batch_msg.ack_tx.send(AckNack::Nack { delay_seconds: Some(10) });
            return Ok(());
        }

        // Increment queue size
        self.queue_size.fetch_add(1, Ordering::SeqCst);

        // Get message group - use Cow to avoid allocation when group_id exists
        let group_id: Arc<str> = batch_msg.message.message_group_id
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| Arc::from(s.as_str()))
            .unwrap_or_else(|| Arc::from(DEFAULT_GROUP));

        // Track batch+group message count for cleanup (Java: batchGroupMessageCount)
        // Uses BatchGroupKey tuple instead of format!() to avoid string allocation
        let batch_group_key = batch_msg.batch_id.as_ref()
            .map(|batch_id| BatchGroupKey::new(batch_id, &group_id));

        if let Some(ref key) = batch_group_key {
            self.batch_group_message_count
                .entry(key.clone())
                .or_insert_with(|| AtomicU32::new(0))
                .fetch_add(1, Ordering::SeqCst);
            debug!(batch_id = %key.batch_id, group_id = %key.group_id, "Tracking message in batch+group");
        }

        // Check if batch+group has failed (early check before queueing)
        if let Some(ref key) = batch_group_key {
            if self.failed_batch_groups.contains(key) {
                debug!(
                    message_id = %batch_msg.message.id,
                    batch_id = %key.batch_id,
                    group_id = %key.group_id,
                    "Batch+group failed, NACKing for FIFO"
                );
                self.queue_size.fetch_sub(1, Ordering::SeqCst);
                self.decrement_and_cleanup_batch_group(key);
                // Java: setFastFailVisibility = 10 seconds for batch+group failure NACKs
                let _ = batch_msg.ack_tx.send(AckNack::Nack { delay_seconds: Some(10) });
                return Ok(());
            }
        }

        // Get or create group queue and worker
        let channels = self.get_or_create_group_queue(&group_id);

        // Clone batch_group_key before moving into task (for error handling)
        let batch_group_key_for_error = batch_group_key.clone();

        // Route to high or regular channel based on priority
        let is_high_priority = batch_msg.message.high_priority;

        // Send to group queue
        let task = PoolTask {
            message: batch_msg.message,
            receipt_handle: batch_msg.receipt_handle,
            ack_tx: batch_msg.ack_tx,
            batch_id: batch_msg.batch_id.map(|s| Arc::from(s.as_str())),
            batch_group_key,
        };

        let send_result = if is_high_priority {
            channels.high.send(task).await
        } else {
            channels.regular.send(task).await
        };

        if let Err(e) = send_result {
            // Channel closed - worker exited during idle timeout, race condition
            // Remove stale entry and retry with a fresh queue/worker
            debug!(
                error = %e,
                group_id = %group_id,
                "Group channel closed (idle worker exited), retrying with new worker"
            );
            self.message_group_queues.remove(&group_id);

            let new_channels = self.get_or_create_group_queue(&group_id);
            let retry_task = PoolTask {
                message: e.0.message,
                receipt_handle: e.0.receipt_handle,
                ack_tx: e.0.ack_tx,
                batch_id: e.0.batch_id,
                batch_group_key: e.0.batch_group_key,
            };

            let retry_result = if is_high_priority {
                new_channels.high.send(retry_task).await
            } else {
                new_channels.regular.send(retry_task).await
            };

            if let Err(e2) = retry_result {
                error!(error = %e2, group_id = %group_id, "Failed to send to group queue on retry");
                self.queue_size.fetch_sub(1, Ordering::SeqCst);
                if let Some(ref key) = batch_group_key_for_error {
                    self.decrement_and_cleanup_batch_group(key);
                }
            }
        }

        Ok(())
    }

    /// Get or create a message group queue and worker
    /// Mirrors Java's pattern: get queue, check thread liveness, restart if dead
    fn get_or_create_group_queue(&self, group_id: &Arc<str>) -> PriorityChannels {
        // Check if queue exists AND thread is alive
        let mut is_restart = false;
        if let Some(channels) = self.message_group_queues.get(group_id) {
            if self.active_group_threads.contains(group_id) {
                // Queue exists and worker is alive - return existing channels
                return channels.clone();
            }
            // Queue exists but worker is dead - fall through to recreate
            is_restart = true;
            drop(channels); // Release the DashMap guard before removing
        }

        // Either queue doesn't exist, or worker is dead - create fresh channels and start worker
        // Remove any stale entry first
        self.message_group_queues.remove(group_id);

        // Generate warning on worker restart (Java: GROUP_THREAD_RESTART)
        if is_restart {
            warn!(
                group_id = %group_id,
                pool_code = %self.config.code,
                "Virtual thread for message group appears to have died - restarting"
            );
            if let Some(ref ws) = self.warning_service {
                use fc_common::{WarningCategory, WarningSeverity};
                ws.add_warning(
                    WarningCategory::PoolHealth,
                    WarningSeverity::Warn,
                    format!("Virtual thread for group [{}] in pool [{}] died and was restarted",
                        group_id, self.config.code),
                    format!("ProcessPool:{}", self.config.code),
                );
            }
        }

        let (high_tx, high_rx) = mpsc::channel(100);
        let (regular_tx, regular_rx) = mpsc::channel(100);
        let channels = PriorityChannels {
            high: high_tx,
            regular: regular_tx,
        };
        self.message_group_queues.insert(Arc::clone(group_id), channels.clone());

        // Start the worker with both receivers
        self.start_group_worker(group_id, high_rx, regular_rx);

        channels
    }

    /// Start a dedicated worker task for a message group
    /// Called when creating a new group or restarting a dead worker
    fn start_group_worker(
        &self,
        group_id: &Arc<str>,
        high_rx: mpsc::Receiver<PoolTask>,
        regular_rx: mpsc::Receiver<PoolTask>,
    ) {
        // Mark thread as active BEFORE spawning (Java pattern)
        self.active_group_threads.insert(Arc::clone(group_id));

        let group_id_clone = Arc::clone(group_id);
        let pool_code: Arc<str> = Arc::from(self.config.code.as_str());
        let semaphore = self.semaphore.clone();
        let mediator = self.mediator.clone();
        let queue_size = self.queue_size.clone();
        let active_workers = self.active_workers.clone();
        let in_flight_groups = self.in_flight_groups.clone();
        let failed_batch_groups = self.failed_batch_groups.clone();
        let batch_group_message_count = self.batch_group_message_count.clone();
        let rate_limiter = self.rate_limiter.clone(); // Share Arc with worker for config updates
        let message_group_queues = self.message_group_queues.clone();
        let active_group_threads = self.active_group_threads.clone();
        let metrics_collector = self.metrics_collector.clone();
        let warning_service = self.warning_service.clone();

        debug!(group_id = %group_id, pool_code = %self.config.code, "Spawning group worker task");

        tokio::spawn(async move {
            Self::run_group_worker(
                group_id_clone,
                pool_code,
                high_rx,
                regular_rx,
                semaphore,
                mediator,
                queue_size,
                active_workers,
                in_flight_groups,
                failed_batch_groups,
                batch_group_message_count,
                rate_limiter,
                message_group_queues,
                active_group_threads,
                metrics_collector,
                warning_service,
            ).await;
        });
    }

    /// Worker loop for a message group with priority support.
    /// Uses biased select to always drain high-priority messages first.
    async fn run_group_worker(
        group_id: Arc<str>,
        pool_code: Arc<str>,
        mut high_rx: mpsc::Receiver<PoolTask>,
        mut regular_rx: mpsc::Receiver<PoolTask>,
        semaphore: Arc<Semaphore>,
        mediator: Arc<dyn Mediator>,
        queue_size: Arc<AtomicU32>,
        active_workers: Arc<AtomicU32>,
        in_flight_groups: DashSet<Arc<str>>,
        failed_batch_groups: DashSet<BatchGroupKey>,
        batch_group_message_count: Arc<DashMap<BatchGroupKey, AtomicU32>>,
        rate_limiter: Arc<parking_lot::RwLock<Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>>>,
        message_group_queues: DashMap<Arc<str>, PriorityChannels>,
        active_group_threads: DashSet<Arc<str>>,
        metrics_collector: Arc<PoolMetricsCollector>,
        warning_service: Option<Arc<crate::warning::WarningService>>,
    ) {
        info!(group_id = %group_id, pool_code = %pool_code, "Group worker started");

        // Idle timeout for cleanup
        let idle_timeout = Duration::from_secs(300); // 5 minutes

        loop {
            // Biased select: always drain high-priority messages before regular ones.
            // The idle timeout only applies to the regular branch — if both channels
            // are empty after the timeout, the worker cleans up and exits.
            let task = tokio::select! {
                biased;
                result = high_rx.recv() => {
                    match result {
                        Some(t) => t,
                        None => {
                            debug!(group_id = %group_id, "High priority channel closed, exiting");
                            break;
                        }
                    }
                }
                result = tokio::time::timeout(idle_timeout, regular_rx.recv()) => {
                    match result {
                        Ok(Some(t)) => t,
                        Ok(None) => {
                            debug!(group_id = %group_id, "Regular channel closed, exiting");
                            break;
                        }
                        Err(_) => {
                            // Idle timeout - cleanup only if BOTH channels are empty
                            if high_rx.is_empty() && regular_rx.is_empty() {
                                debug!(group_id = %group_id, "Group idle timeout, cleaning up");
                                message_group_queues.remove(&group_id);
                                break;
                            }
                            continue;
                        }
                    }
                }
            };

            // Decrement queue size
            queue_size.fetch_sub(1, Ordering::SeqCst);

            // Check if batch+group has already failed AFTER polling (Java: line 548)
            // This catches messages that were queued before a failure occurred
            if let Some(ref batch_group_key) = task.batch_group_key {
                if failed_batch_groups.contains(batch_group_key) {
                    warn!(
                        message_id = %task.message.id,
                        batch_group = %batch_group_key,
                        "Message from failed batch+group, NACKing to preserve FIFO ordering"
                    );
                    Self::decrement_and_cleanup_batch_group_static(
                        batch_group_key,
                        &batch_group_message_count,
                        &failed_batch_groups,
                    );
                    // Java: setFastFailVisibility = 10 seconds for batch+group failure NACKs
                    let _ = task.ack_tx.send(AckNack::Nack { delay_seconds: Some(10) });
                    continue;
                }
            }

            // Wait for rate limit permit (blocking with config-change awareness)
            // Messages stay in memory instead of being NACKed back to SQS
            Self::wait_for_rate_limit_permit(&rate_limiter, &metrics_collector).await;

            // Acquire semaphore permit
            let permit = match semaphore.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    error!("Semaphore closed");
                    // Decrement batch+group count on semaphore error
                    if let Some(ref key) = task.batch_group_key {
                        Self::decrement_and_cleanup_batch_group_static(
                            key,
                            &batch_group_message_count,
                            &failed_batch_groups,
                        );
                    }
                    let _ = task.ack_tx.send(AckNack::Nack { delay_seconds: Some(10) });
                    break;
                }
            };

            active_workers.fetch_add(1, Ordering::SeqCst);
            in_flight_groups.insert(group_id.clone());

            // Process the message
            let start = std::time::Instant::now();
            let outcome = mediator.mediate(&task.message).await;
            let duration_ms = start.elapsed().as_millis() as u64;

            // Handle outcome and record metrics
            let ack_nack = match outcome.result {
                MediationResult::Success => {
                    debug!(
                        message_id = %task.message.id,
                        duration_ms = duration_ms,
                        "Message processed successfully"
                    );
                    // Record success metric
                    metrics_collector.record_success(duration_ms);
                    AckNack::Ack
                }
                MediationResult::ErrorConfig => {
                    warn!(
                        message_id = %task.message.id,
                        error = ?outcome.error_message,
                        "Configuration error, ACKing to prevent retry"
                    );
                    // Config errors count as failures for metrics
                    metrics_collector.record_failure(duration_ms);
                    AckNack::Ack
                }
                MediationResult::ErrorProcess => {
                    warn!(
                        message_id = %task.message.id,
                        error = ?outcome.error_message,
                        "Transient error, NACKing for retry"
                    );
                    // Java: recordProcessingTransient() — NOT counted as a permanent failure.
                    // Message will be retried, so success rate should not be penalised.
                    metrics_collector.record_transient(duration_ms);

                    // Mark batch+group as failed to trigger cascading NACKs
                    if let Some(ref key) = task.batch_group_key {
                        let was_new = failed_batch_groups.insert(key.clone());
                        if was_new {
                            warn!(
                                batch_group = %key,
                                "Batch+group marked as failed - remaining messages will be NACKed"
                            );
                        }
                    }

                    AckNack::Nack { delay_seconds: outcome.delay_seconds }
                }
                MediationResult::ErrorConnection => {
                    warn!(
                        message_id = %task.message.id,
                        error = ?outcome.error_message,
                        "Connection error, NACKing for retry"
                    );
                    // Record failure metric
                    metrics_collector.record_failure(duration_ms);

                    // Mark batch+group as failed to trigger cascading NACKs
                    if let Some(ref key) = task.batch_group_key {
                        let was_new = failed_batch_groups.insert(key.clone());
                        if was_new {
                            warn!(
                                batch_group = %key,
                                "Batch+group marked as failed - remaining messages will be NACKed"
                            );
                        }
                    }

                    // Java: visibilityControl.resetVisibilityToDefault = 30 seconds
                    AckNack::Nack { delay_seconds: Some(30) }
                }
            };

            // Send ACK/NACK
            let _ = task.ack_tx.send(ack_nack);

            // Decrement batch+group count and cleanup if done (Java: decrementAndCleanupBatchGroup)
            if let Some(ref key) = task.batch_group_key {
                Self::decrement_and_cleanup_batch_group_static(
                    key,
                    &batch_group_message_count,
                    &failed_batch_groups,
                );
            }

            // Cleanup
            in_flight_groups.remove(&group_id);
            active_workers.fetch_sub(1, Ordering::SeqCst);
            drop(permit);
        }

        // Worker exiting - remove from active threads so it can be restarted if needed
        active_group_threads.remove(&group_id);

        // Java: GROUP_THREAD_EXIT_WITH_MESSAGES warning — if the pool is still running
        // and messages remain in the queues, the worker exited abnormally (not via idle timeout).
        // The next submit() for this group will detect orphaned messages and restart the worker.
        let has_orphaned = !high_rx.is_empty() || !regular_rx.is_empty();
        if has_orphaned {
            let orphan_count = high_rx.len() + regular_rx.len();
            warn!(
                group_id = %group_id,
                pool_code = %pool_code,
                orphaned_messages = orphan_count,
                "Group worker exited with messages still in its queue"
            );
            if let Some(ref ws) = warning_service {
                use fc_common::{WarningCategory, WarningSeverity};
                ws.add_warning(
                    WarningCategory::PoolHealth,
                    WarningSeverity::Warn,
                    format!("Group worker [{}] in pool [{}] exited with {} orphaned message(s)",
                        group_id, pool_code, orphan_count),
                    format!("ProcessPool:{}", pool_code),
                );
            }
        }

        info!(group_id = %group_id, pool_code = %pool_code, "Group worker exited");
    }

    /// Decrement batch+group message count and cleanup tracking maps when count reaches zero.
    /// Instance version for use in submit().
    fn decrement_and_cleanup_batch_group(&self, batch_group_key: &BatchGroupKey) {
        Self::decrement_and_cleanup_batch_group_static(
            batch_group_key,
            &self.batch_group_message_count,
            &self.failed_batch_groups,
        );
    }

    /// Decrement batch+group message count and cleanup tracking maps when count reaches zero.
    /// Static version for use in worker tasks.
    fn decrement_and_cleanup_batch_group_static(
        batch_group_key: &BatchGroupKey,
        batch_group_message_count: &DashMap<BatchGroupKey, AtomicU32>,
        failed_batch_groups: &DashSet<BatchGroupKey>,
    ) {
        // Use get() to decrement, then check if we should cleanup
        // IMPORTANT: We must drop the Ref guard before calling remove() to avoid deadlock
        let should_cleanup = if let Some(counter) = batch_group_message_count.get(batch_group_key) {
            let remaining = counter.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
            debug!(batch_group = %batch_group_key, remaining = remaining, "Batch+group count decremented");
            remaining == 0
        } else {
            false
        };
        // Ref guard is now dropped, safe to mutate

        if should_cleanup {
            // All messages in this batch+group have been processed - cleanup
            batch_group_message_count.remove(batch_group_key);
            failed_batch_groups.remove(batch_group_key);
            debug!(batch_group = %batch_group_key, "Batch+group fully processed, cleaned up");
        }
    }

    /// Check available capacity
    pub fn available_capacity(&self) -> usize {
        let capacity = std::cmp::max(
            self.config.concurrency * QUEUE_CAPACITY_MULTIPLIER,
            MIN_QUEUE_CAPACITY,
        ) as usize;
        let used = self.queue_size.load(Ordering::SeqCst) as usize;
        capacity.saturating_sub(used)
    }

    /// Check if rate limited
    pub fn is_rate_limited(&self) -> bool {
        self.rate_limiter
            .read()
            .as_ref()
            .map(|rl| rl.check().is_err())
            .unwrap_or(false)
    }

    /// Waits for a rate limit permit, handling config changes gracefully.
    /// Uses a timed poll loop to detect when rate limiter is replaced or removed.
    ///
    /// This method handles the following scenarios:
    /// - Rate limit removed (100→null): Returns immediately on next check
    /// - Rate limit changed (100→200): Uses new limiter on next poll
    /// - Permits available: check() succeeds immediately
    async fn wait_for_rate_limit_permit(
        rate_limiter: &Arc<parking_lot::RwLock<Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>>>,
        metrics_collector: &Arc<PoolMetricsCollector>,
    ) {
        let mut recorded_rate_limit = false;

        loop {
            // Read current rate limiter (may have changed via config update)
            let limiter = rate_limiter.read().clone();

            match limiter {
                None => return, // No rate limiting configured, proceed immediately
                Some(rl) => {
                    if rl.check().is_ok() {
                        return; // Got permit, proceed with processing
                    }

                    // Record rate limit event once per wait (not every poll)
                    if !recorded_rate_limit {
                        metrics_collector.record_rate_limited();
                        recorded_rate_limit = true;
                        debug!("Rate limited - waiting for permit");
                    }

                    // No permit available - wait briefly then re-check
                    // This handles: rate limit removed, rate limit changed, permits available
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Drain the pool (stop accepting new work)
    pub async fn drain(&self) {
        info!(pool_code = %self.config.code, "Draining pool");
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if fully drained
    pub fn is_fully_drained(&self) -> bool {
        self.queue_size.load(Ordering::SeqCst) == 0 &&
        self.active_workers.load(Ordering::SeqCst) == 0
    }

    /// Shutdown the pool
    pub async fn shutdown(&self) {
        info!(pool_code = %self.config.code, "Shutting down pool");
        self.running.store(false, Ordering::SeqCst);
    }

    /// Get pool statistics
    pub fn get_stats(&self) -> PoolStats {
        let current_concurrency = self.concurrency.load(Ordering::SeqCst);
        PoolStats {
            pool_code: self.config.code.clone(),
            concurrency: current_concurrency,
            active_workers: self.active_workers.load(Ordering::SeqCst),
            queue_size: self.queue_size.load(Ordering::SeqCst),
            queue_capacity: std::cmp::max(
                current_concurrency * QUEUE_CAPACITY_MULTIPLIER,
                MIN_QUEUE_CAPACITY,
            ),
            message_group_count: self.message_group_queues.len() as u32,
            rate_limit_per_minute: *self.rate_limit_per_minute.read(),
            is_rate_limited: self.is_rate_limited(),
            metrics: Some(self.metrics_collector.get_metrics()),
        }
    }

    /// Get enhanced metrics for this pool
    pub fn get_enhanced_metrics(&self) -> EnhancedPoolMetrics {
        self.metrics_collector.get_metrics()
    }

    /// Reset metrics (useful for testing)
    pub fn reset_metrics(&self) {
        self.metrics_collector.reset();
    }

    /// Get the pool code
    pub fn code(&self) -> &str {
        &self.config.code
    }

    /// Get current concurrency setting
    pub fn concurrency(&self) -> u32 {
        self.concurrency.load(Ordering::SeqCst)
    }

    /// Get current rate limit setting
    pub fn rate_limit_per_minute(&self) -> Option<u32> {
        *self.rate_limit_per_minute.read()
    }

    /// Get current queue size
    pub fn queue_size(&self) -> u32 {
        self.queue_size.load(Ordering::SeqCst)
    }

    /// Get current active worker count
    pub fn active_workers(&self) -> u32 {
        self.active_workers.load(Ordering::SeqCst)
    }

    /// Update concurrency at runtime
    /// Mirrors Java's updateConcurrency behavior:
    /// - Increase: Immediately releases permits to semaphore
    /// - Decrease: Tries to acquire excess permits with timeout
    pub async fn update_concurrency(&self, new_concurrency: u32) -> bool {
        let old_concurrency = self.concurrency.load(Ordering::SeqCst);
        if new_concurrency == old_concurrency {
            return true;
        }

        if new_concurrency == 0 {
            warn!(pool_code = %self.config.code, "Rejecting invalid concurrency limit: 0");
            return false;
        }

        let diff = (new_concurrency as i32) - (old_concurrency as i32);

        if diff > 0 {
            // Increasing concurrency - add permits immediately
            self.semaphore.add_permits(diff as usize);
            self.concurrency.store(new_concurrency, Ordering::SeqCst);
            info!(
                pool_code = %self.config.code,
                old = old_concurrency,
                new = new_concurrency,
                added_permits = diff,
                "Increased pool concurrency"
            );
            true
        } else {
            // Decreasing concurrency - try to acquire excess permits with timeout
            // This mirrors Java's semaphore.tryAcquire(permitDifference, timeoutSeconds, TimeUnit.SECONDS)
            let permits_to_acquire = (-diff) as usize;
            let timeout = Duration::from_secs(60); // 60 second timeout like Java

            match tokio::time::timeout(timeout, self.acquire_permits(permits_to_acquire)).await {
                Ok(permits) => {
                    // Successfully acquired permits - now "forget" them to reduce capacity
                    // (Tokio semaphore doesn't have a remove_permits, so we just hold them forever)
                    std::mem::forget(permits);
                    self.concurrency.store(new_concurrency, Ordering::SeqCst);
                    info!(
                        pool_code = %self.config.code,
                        old = old_concurrency,
                        new = new_concurrency,
                        acquired_permits = permits_to_acquire,
                        "Decreased pool concurrency"
                    );
                    true
                }
                Err(_) => {
                    // Timeout - keep current limit
                    warn!(
                        pool_code = %self.config.code,
                        old = old_concurrency,
                        new = new_concurrency,
                        timeout_secs = 60,
                        active_workers = self.active_workers.load(Ordering::SeqCst),
                        "Concurrency decrease timed out waiting for idle slots - retaining current limit"
                    );
                    false
                }
            }
        }
    }

    /// Helper to acquire multiple permits (needed for concurrency decrease)
    async fn acquire_permits(&self, count: usize) -> Vec<tokio::sync::SemaphorePermit<'_>> {
        let mut permits = Vec::with_capacity(count);
        for _ in 0..count {
            permits.push(self.semaphore.acquire().await.expect("semaphore closed"));
        }
        permits
    }

    /// Update rate limit at runtime
    /// Atomically replaces the rate limiter (mirrors Java's updateRateLimit)
    pub fn update_rate_limit(&self, new_rate_limit: Option<u32>) {
        let old_rate_limit = *self.rate_limit_per_minute.read();

        // Check if actually changed
        if old_rate_limit == new_rate_limit {
            return;
        }

        // Create new rate limiter
        let new_limiter = new_rate_limit.and_then(|rpm| {
            if rpm == 0 {
                None // Disable rate limiting
            } else {
                NonZeroU32::new(rpm).map(|nz| {
                    Arc::new(RateLimiter::direct(Quota::per_minute(nz)))
                })
            }
        });

        // Atomically replace
        *self.rate_limiter.write() = new_limiter;
        *self.rate_limit_per_minute.write() = new_rate_limit;

        info!(
            pool_code = %self.config.code,
            old = ?old_rate_limit.map(|r| format!("{}/min", r)).unwrap_or_else(|| "none".to_string()),
            new = ?new_rate_limit.map(|r| format!("{}/min", r)).unwrap_or_else(|| "none".to_string()),
            "Rate limit updated in-place"
        );
    }
}

/// Configuration update that can be applied at runtime
#[derive(Debug, Clone)]
pub struct PoolConfigUpdate {
    /// New concurrency level (if changed)
    pub concurrency: Option<u32>,
    /// New rate limit per minute (None to clear, Some(0) means no limit)
    pub rate_limit_per_minute: Option<Option<u32>>,
}

/// Builder for creating pool configuration updates
impl PoolConfigUpdate {
    pub fn new() -> Self {
        Self {
            concurrency: None,
            rate_limit_per_minute: None,
        }
    }

    pub fn with_concurrency(mut self, concurrency: u32) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    pub fn with_rate_limit(mut self, rate_limit: Option<u32>) -> Self {
        self.rate_limit_per_minute = Some(rate_limit);
        self
    }
}

impl Default for PoolConfigUpdate {
    fn default() -> Self {
        Self::new()
    }
}
