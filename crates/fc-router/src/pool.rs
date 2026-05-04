//! ProcessPool - Worker pool with FIFO ordering, rate limiting, and concurrency control
//!
//! Uses lightweight per-message-group handlers (VecDeque + processing flag) instead of
//! dedicated tokio tasks with channels. A task is spawned only when there's work to do
//! and exits when the group's queue is empty. This matches the TS MessageGroupHandler
//! pattern and uses ~200 bytes per idle group vs ~100KB with the old design.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use std::num::NonZeroU32;
use dashmap::{DashMap, DashSet};
use tokio::sync::Semaphore;
use governor::{Quota, RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use tracing::{info, warn, error, debug};

use fc_common::{
    Message, BatchMessage, MessageCallback, PoolConfig, PoolStats,
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
    pub callback: Box<dyn MessageCallback>,
    pub batch_id: Option<Arc<str>>,
    /// Pre-computed batch+group key for FIFO tracking
    pub batch_group_key: Option<BatchGroupKey>,
}

/// Lightweight per-message-group handler.
/// Just a queue of pending tasks and a flag — no tokio task, no channels.
/// A drain task is spawned only when work arrives for an idle group.
struct MessageGroupHandler {
    high_priority: VecDeque<PoolTask>,
    regular: VecDeque<PoolTask>,
    processing: bool,
}

impl MessageGroupHandler {
    fn new() -> Self {
        Self {
            high_priority: VecDeque::new(),
            regular: VecDeque::new(),
            processing: false,
        }
    }

    fn enqueue(&mut self, task: PoolTask, high_priority: bool) {
        if high_priority {
            self.high_priority.push_back(task);
        } else {
            self.regular.push_back(task);
        }
    }

    /// Dequeue next task, high priority first.
    fn dequeue(&mut self) -> Option<PoolTask> {
        self.high_priority.pop_front().or_else(|| self.regular.pop_front())
    }

    fn is_empty(&self) -> bool {
        self.high_priority.is_empty() && self.regular.is_empty()
    }

    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.high_priority.len() + self.regular.len()
    }
}

/// Process pool with FIFO ordering and rate limiting
pub struct ProcessPool {
    config: PoolConfig,
    mediator: Arc<dyn Mediator>,

    /// Current concurrency level (may differ from config after updates)
    concurrency: AtomicU32,

    /// Pool-level concurrency semaphore
    semaphore: Arc<Semaphore>,

    /// Per-message-group handlers (lightweight: VecDeque + processing flag)
    group_handlers: Arc<DashMap<Arc<str>, parking_lot::Mutex<MessageGroupHandler>>>,

    /// Track in-flight message groups
    in_flight_groups: DashSet<Arc<str>>,

    /// Batch+group failure tracking for cascading NACKs
    failed_batch_groups: Arc<DashSet<BatchGroupKey>>,

    /// Track remaining messages per batch+group for cleanup
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

    /// Per-endpoint circuit breaker registry — shared across pools, keyed by mediation target URL.
    circuit_breaker_registry: Arc<crate::circuit_breaker_registry::CircuitBreakerRegistry>,

    /// Warning service for generating warnings
    warning_service: Arc<crate::warning::WarningService>,
}

impl ProcessPool {
    pub fn new(config: PoolConfig, mediator: Arc<dyn Mediator>) -> Self {
        // Java: effectiveConcurrency() — if concurrency is 0, fall back to max(rateLimitPerMinute/60, 1)
        let concurrency_val = if config.concurrency == 0 {
            config.rate_limit_per_minute
                .map(|rpm| (rpm / 60).max(1))
                .unwrap_or(1)
        } else {
            config.concurrency
        };

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
            group_handlers: Arc::new(DashMap::new()),
            in_flight_groups: DashSet::new(),
            failed_batch_groups: Arc::new(DashSet::new()),
            batch_group_message_count: Arc::new(DashMap::new()),
            rate_limiter: Arc::new(parking_lot::RwLock::new(rate_limiter)),
            rate_limit_per_minute: Arc::new(parking_lot::RwLock::new(config.rate_limit_per_minute)),
            running: AtomicBool::new(false),
            queue_size: Arc::new(AtomicU32::new(0)),
            active_workers: Arc::new(AtomicU32::new(0)),
            metrics_collector: Arc::new(PoolMetricsCollector::new()),
            circuit_breaker_registry: Arc::new(crate::circuit_breaker_registry::CircuitBreakerRegistry::default()),
            warning_service: Arc::new(crate::warning::WarningService::noop()),
        }
    }

    /// Set the shared circuit breaker registry
    pub fn set_circuit_breaker_registry(&mut self, registry: Arc<crate::circuit_breaker_registry::CircuitBreakerRegistry>) {
        self.circuit_breaker_registry = registry;
    }

    /// Set the warning service for generating warnings
    pub fn with_warning_service(mut self, warning_service: Arc<crate::warning::WarningService>) -> Self {
        self.warning_service = warning_service;
        self
    }

    /// Set warning service after construction
    pub fn set_warning_service(&mut self, warning_service: Arc<crate::warning::WarningService>) {
        self.warning_service = warning_service;
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
            batch_msg.callback.nack(Some(10)).await;
            return Ok(());
        }

        // Check capacity
        let current_size = self.queue_size.load(Ordering::Relaxed);
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
            batch_msg.callback.nack(Some(10)).await;
            return Ok(());
        }

        // Increment queue size
        self.queue_size.fetch_add(1, Ordering::Relaxed);

        // Get message group
        let group_id: Arc<str> = batch_msg.message.message_group_id
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| Arc::from(s.as_str()))
            .unwrap_or_else(|| Arc::from(DEFAULT_GROUP));

        // Track batch+group message count for cleanup
        let batch_group_key = batch_msg.batch_id.as_ref()
            .map(|batch_id| BatchGroupKey::new(batch_id, &group_id));

        if let Some(ref key) = batch_group_key {
            self.batch_group_message_count
                .entry(key.clone())
                .or_insert_with(|| AtomicU32::new(0))
                .fetch_add(1, Ordering::Relaxed);
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
                self.queue_size.fetch_sub(1, Ordering::Relaxed);
                self.decrement_and_cleanup_batch_group(key);
                batch_msg.callback.nack(Some(10)).await;
                return Ok(());
            }
        }

        // IMMEDIATE mode: no ordering needed — spawn a standalone task per message.
        // This avoids the sequential drain bottleneck where a slow HTTP call blocks
        // all other messages in the group.
        if !batch_msg.message.dispatch_mode.requires_ordering() {
            let task = PoolTask {
                message: batch_msg.message,
                receipt_handle: String::new(), // not used in standalone path
                callback: batch_msg.callback,
                batch_id: batch_msg.batch_id,
                batch_group_key,
            };
            self.spawn_immediate_task(task);
            return Ok(());
        }

        let is_high_priority = batch_msg.message.high_priority;

        let task = PoolTask {
            message: batch_msg.message,
            receipt_handle: batch_msg.receipt_handle,
            callback: batch_msg.callback,
            batch_id: batch_msg.batch_id,
            batch_group_key,
        };

        // Ordered mode: enqueue to group handler and spawn drain task if idle
        let should_spawn = {
            let entry = self.group_handlers
                .entry(Arc::clone(&group_id))
                .or_insert_with(|| parking_lot::Mutex::new(MessageGroupHandler::new()));
            let mut handler = entry.lock();

            handler.enqueue(task, is_high_priority);

            if !handler.processing {
                handler.processing = true;
                true
            } else {
                false
            }
        };

        if should_spawn {
            self.spawn_drain_task(group_id);
        }

        Ok(())
    }

    /// Spawn a standalone task for an IMMEDIATE mode message.
    /// No group ordering — acquires semaphore, rate-limits, mediates, callbacks directly.
    fn spawn_immediate_task(&self, task: PoolTask) {
        let semaphore = self.semaphore.clone();
        let mediator = self.mediator.clone();
        let queue_size = self.queue_size.clone();
        let active_workers = self.active_workers.clone();
        let rate_limiter = self.rate_limiter.clone();
        let metrics_collector = self.metrics_collector.clone();
        let cb_registry = self.circuit_breaker_registry.clone();
        let failed_batch_groups = self.failed_batch_groups.clone();
        let batch_group_message_count = self.batch_group_message_count.clone();

        tokio::spawn(async move {
            // Wait for rate limit
            if !Self::wait_for_rate_limit_permit(&rate_limiter, &metrics_collector).await {
                queue_size.fetch_sub(1, Ordering::Relaxed);
                if let Some(ref key) = task.batch_group_key {
                    Self::decrement_and_cleanup_batch_group_static(key, &batch_group_message_count, &failed_batch_groups);
                }
                task.callback.nack(Some(10)).await;
                return;
            }

            // Acquire semaphore
            let permit = match semaphore.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    queue_size.fetch_sub(1, Ordering::Relaxed);
                    if let Some(ref key) = task.batch_group_key {
                        Self::decrement_and_cleanup_batch_group_static(key, &batch_group_message_count, &failed_batch_groups);
                    }
                    task.callback.nack(Some(10)).await;
                    return;
                }
            };

            active_workers.fetch_add(1, Ordering::Relaxed);
            queue_size.fetch_sub(1, Ordering::Relaxed);

            // Check per-endpoint circuit breaker
            let endpoint = &task.message.mediation_target;
            if !cb_registry.allow_request(endpoint) {
                debug!(message_id = %task.message.id, endpoint = %endpoint, "Endpoint circuit breaker open");
                metrics_collector.record_failure(0);
                task.callback.nack(Some(5)).await;
            } else {
                let start = std::time::Instant::now();
                let outcome = mediator.mediate(&task.message).await;
                let duration_ms = start.elapsed().as_millis() as u64;

                match outcome.result {
                    MediationResult::Success | MediationResult::ErrorConfig => cb_registry.record_success(endpoint),
                    MediationResult::ErrorProcess | MediationResult::ErrorConnection => cb_registry.record_failure(endpoint),
                    // RateLimited is destination throttling, not a real failure —
                    // do not affect the circuit breaker either way.
                    MediationResult::RateLimited => {}
                }

                match outcome.result {
                    MediationResult::Success => {
                        metrics_collector.record_success(duration_ms);
                        task.callback.ack().await;
                    }
                    MediationResult::ErrorConfig => {
                        metrics_collector.record_failure(duration_ms);
                        task.callback.ack().await;
                    }
                    MediationResult::ErrorProcess => {
                        metrics_collector.record_transient(duration_ms);
                        task.callback.nack(outcome.delay_seconds).await;
                    }
                    MediationResult::ErrorConnection => {
                        metrics_collector.record_failure(duration_ms);
                        task.callback.nack(Some(30)).await;
                    }
                    MediationResult::RateLimited => {
                        // Nack with Retry-After so SQS redelivers after the
                        // destination's requested delay. Not counted as a
                        // delivery attempt or a failure.
                        metrics_collector.record_rate_limited();
                        task.callback.nack(outcome.delay_seconds.or(Some(30))).await;
                    }
                }
            }

            if let Some(ref key) = task.batch_group_key {
                Self::decrement_and_cleanup_batch_group_static(key, &batch_group_message_count, &failed_batch_groups);
            }

            active_workers.fetch_sub(1, Ordering::Relaxed);
            drop(permit);
        });
    }

    /// Spawn a task that drains all queued messages for a group, then exits.
    fn spawn_drain_task(&self, group_id: Arc<str>) {
        let pool_code: Arc<str> = Arc::from(self.config.code.as_str());
        let semaphore = self.semaphore.clone();
        let mediator = self.mediator.clone();
        let queue_size = self.queue_size.clone();
        let active_workers = self.active_workers.clone();
        let in_flight_groups = self.in_flight_groups.clone();
        let failed_batch_groups = self.failed_batch_groups.clone();
        let batch_group_message_count = self.batch_group_message_count.clone();
        let rate_limiter = self.rate_limiter.clone();
        let group_handlers = self.group_handlers.clone();
        let metrics_collector = self.metrics_collector.clone();
        let cb_registry = self.circuit_breaker_registry.clone();

        tokio::spawn(async move {
            debug!(group_id = %group_id, pool_code = %pool_code, "Group drain task started");

            // Safety guard: if this task panics or exits via an early break,
            // (a) reset the `processing` flag so a future submit() can spawn
            //     a fresh drain task,
            // (b) drain remaining tasks from the VecDeque — dropping them is
            //     the trigger for `QueueMessageCallback::drop` to clear
            //     `in_pipeline` and fire fallback nacks, releasing SQS
            //     redelivery for those messages,
            // (c) decrement active_workers if a permit was held.
            //
            // Without (b), abandoned tasks would sit in the VecDeque
            // indefinitely (the handler is only freed when its queue is
            // empty AND `processing == false`), and SQS redeliveries would
            // be silently swallowed by the manager's duplicate filter.
            struct PanicGuard {
                group_handlers: Arc<DashMap<Arc<str>, parking_lot::Mutex<MessageGroupHandler>>>,
                group_id: Arc<str>,
                in_flight_groups: DashSet<Arc<str>>,
                active_workers: Arc<AtomicU32>,
                /// Whether a semaphore permit was held when panic occurred
                holding_permit: bool,
                active: bool,
            }
            impl Drop for PanicGuard {
                fn drop(&mut self) {
                    if !self.active {
                        return;
                    }
                    let mut abandoned = 0usize;
                    if let Some(entry) = self.group_handlers.get(&self.group_id) {
                        let mut handler = entry.lock();
                        // Drain queued tasks; their callbacks' Drop impl
                        // does the cleanup. We can't await here so we just
                        // drop them and let the callback's Drop spawn the
                        // fallback nack on the runtime.
                        while handler.dequeue().is_some() {
                            abandoned += 1;
                        }
                        if handler.processing {
                            handler.processing = false;
                        }
                    }
                    if abandoned > 0 {
                        error!(
                            group_id = %self.group_id,
                            abandoned = abandoned,
                            "Drain task exited abnormally — drained queued tasks; their callbacks' Drop will release SQS redelivery"
                        );
                    } else {
                        error!(group_id = %self.group_id, "Drain task exited abnormally — reset processing flag");
                    }
                    if self.holding_permit {
                        self.in_flight_groups.remove(&self.group_id);
                        self.active_workers.fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
            let mut panic_guard = PanicGuard {
                group_handlers: group_handlers.clone(),
                group_id: group_id.clone(),
                in_flight_groups: in_flight_groups.clone(),
                active_workers: active_workers.clone(),
                holding_permit: false,
                active: true,
            };

            loop {
                // Dequeue next task (lock is held only for the dequeue, dropped before any await)
                let dequeue_result = {
                    let handler_entry = group_handlers.get(&group_id);
                    match handler_entry {
                        Some(entry) => {
                            let mut handler = entry.lock();
                            match handler.dequeue() {
                                Some(task) => Some(task),
                                None => {
                                    handler.processing = false;
                                    None
                                }
                            }
                        }
                        None => None,
                    }
                }; // Lock dropped here

                // Check for failed batch+group OUTSIDE the lock
                let task = match dequeue_result {
                    Some(task) => {
                        if let Some(ref key) = task.batch_group_key {
                            if failed_batch_groups.contains(key) {
                                queue_size.fetch_sub(1, Ordering::Relaxed);
                                Self::decrement_and_cleanup_batch_group_static(
                                    key,
                                    &batch_group_message_count,
                                    &failed_batch_groups,
                                );
                                task.callback.nack(Some(10)).await;
                                continue;
                            }
                        }
                        Some(task)
                    }
                    None => None,
                };

                let task = match task {
                    Some(t) => t,
                    None => {
                        // Clean up empty handler from map
                        // Only remove if still empty (another submit might have raced)
                        if let Some(entry) = group_handlers.get(&group_id) {
                            let handler = entry.lock();
                            if handler.is_empty() && !handler.processing {
                                drop(handler);
                                drop(entry);
                                group_handlers.remove(&group_id);
                            }
                        }
                        panic_guard.active = false; // Normal exit, don't trigger guard
                        debug!(group_id = %group_id, pool_code = %pool_code, "Group drain task exited");
                        break;
                    }
                };

                // Decrement queue size
                queue_size.fetch_sub(1, Ordering::Relaxed);

                // Wait for rate limit permit (with timeout to prevent stall)
                let got_permit = Self::wait_for_rate_limit_permit(&rate_limiter, &metrics_collector).await;
                if !got_permit {
                    if let Some(ref key) = task.batch_group_key {
                        Self::decrement_and_cleanup_batch_group_static(
                            key,
                            &batch_group_message_count,
                            &failed_batch_groups,
                        );
                    }
                    task.callback.nack(Some(10)).await;
                    continue;
                }

                // Acquire semaphore permit
                let permit = match semaphore.acquire().await {
                    Ok(p) => p,
                    Err(_) => {
                        error!("Semaphore closed");
                        if let Some(ref key) = task.batch_group_key {
                            Self::decrement_and_cleanup_batch_group_static(
                                key,
                                &batch_group_message_count,
                                &failed_batch_groups,
                            );
                        }
                        task.callback.nack(Some(10)).await;
                        // Leave panic_guard.active = true: the queue may
                        // still hold tasks, and the guard will drain them
                        // (their callbacks' Drop fires the fallback nack)
                        // and reset the processing flag so a future submit
                        // can spawn a new drain task.
                        break;
                    }
                };

                active_workers.fetch_add(1, Ordering::Relaxed);
                in_flight_groups.insert(group_id.clone());
                panic_guard.holding_permit = true;

                // Check per-endpoint circuit breaker before attempting mediation
                let endpoint = &task.message.mediation_target;
                if !cb_registry.allow_request(endpoint) {
                    debug!(
                        message_id = %task.message.id,
                        endpoint = %endpoint,
                        "Endpoint circuit breaker open — NACKing for retry"
                    );
                    metrics_collector.record_failure(0);

                    if let Some(ref key) = task.batch_group_key {
                        failed_batch_groups.insert(key.clone());
                    }

                    task.callback.nack(Some(5)).await;
                } else {
                    // Process the message
                    let start = std::time::Instant::now();
                    let outcome = mediator.mediate(&task.message).await;
                    let duration_ms = start.elapsed().as_millis() as u64;

                    // Record outcome on per-endpoint circuit breaker
                    match outcome.result {
                        MediationResult::Success | MediationResult::ErrorConfig => {
                            cb_registry.record_success(endpoint);
                        }
                        MediationResult::ErrorProcess | MediationResult::ErrorConnection => {
                            cb_registry.record_failure(endpoint);
                        }
                        // RateLimited is destination throttling, not a real failure —
                        // do not affect the circuit breaker either way.
                        MediationResult::RateLimited => {}
                    }

                    // Handle outcome: record metrics, call callback directly
                    match outcome.result {
                        MediationResult::Success => {
                            debug!(
                                message_id = %task.message.id,
                                duration_ms = duration_ms,
                                "Message processed successfully"
                            );
                            metrics_collector.record_success(duration_ms);
                            task.callback.ack().await;
                        }
                        MediationResult::ErrorConfig => {
                            warn!(
                                message_id = %task.message.id,
                                error = ?outcome.error_message,
                                "Configuration error, ACKing to prevent retry"
                            );
                            metrics_collector.record_failure(duration_ms);
                            task.callback.ack().await;
                        }
                        MediationResult::ErrorProcess => {
                            warn!(
                                message_id = %task.message.id,
                                error = ?outcome.error_message,
                                "Transient error, NACKing for retry"
                            );
                            metrics_collector.record_transient(duration_ms);

                            if let Some(ref key) = task.batch_group_key {
                                let was_new = failed_batch_groups.insert(key.clone());
                                if was_new {
                                    warn!(
                                        batch_group = %key,
                                        "Batch+group marked as failed - remaining messages will be NACKed"
                                    );
                                }
                            }

                            task.callback.nack(outcome.delay_seconds).await;
                        }
                        MediationResult::ErrorConnection => {
                            warn!(
                                message_id = %task.message.id,
                                error = ?outcome.error_message,
                                "Connection error, NACKing for retry"
                            );
                            metrics_collector.record_failure(duration_ms);

                            if let Some(ref key) = task.batch_group_key {
                                let was_new = failed_batch_groups.insert(key.clone());
                                if was_new {
                                    warn!(
                                        batch_group = %key,
                                        "Batch+group marked as failed - remaining messages will be NACKed"
                                    );
                                }
                            }

                            task.callback.nack(Some(30)).await;
                        }
                        MediationResult::RateLimited => {
                            // Destination throttled us — nack with Retry-After.
                            // NOT counted as a delivery attempt or failure, and
                            // we deliberately do NOT mark the batch+group as
                            // failed: a 429 means "try again later", not "this
                            // group is broken".
                            warn!(
                                message_id = %task.message.id,
                                retry_after = ?outcome.delay_seconds,
                                "Rate limited by destination, NACKing for retry"
                            );
                            metrics_collector.record_rate_limited();
                            task.callback.nack(outcome.delay_seconds.or(Some(30))).await;
                        }
                    };
                }

                // Decrement batch+group count and cleanup if done
                if let Some(ref key) = task.batch_group_key {
                    Self::decrement_and_cleanup_batch_group_static(
                        key,
                        &batch_group_message_count,
                        &failed_batch_groups,
                    );
                }

                // Cleanup
                in_flight_groups.remove(&group_id);
                active_workers.fetch_sub(1, Ordering::Relaxed);
                panic_guard.holding_permit = false;
                drop(permit);
            }
        });
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
    /// Static version for use in drain tasks.
    fn decrement_and_cleanup_batch_group_static(
        batch_group_key: &BatchGroupKey,
        batch_group_message_count: &DashMap<BatchGroupKey, AtomicU32>,
        failed_batch_groups: &DashSet<BatchGroupKey>,
    ) {
        let should_cleanup = if let Some(counter) = batch_group_message_count.get(batch_group_key) {
            let remaining = counter.fetch_sub(1, Ordering::Relaxed).saturating_sub(1);
            debug!(batch_group = %batch_group_key, remaining = remaining, "Batch+group count decremented");
            remaining == 0
        } else {
            false
        };

        if should_cleanup {
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
        let used = self.queue_size.load(Ordering::Relaxed) as usize;
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

    /// Maximum time to wait for a rate limit permit before giving up.
    const RATE_LIMIT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

    /// Waits for a rate limit permit using governor's async API (zero CPU while waiting).
    /// Returns true if permit was acquired, false if timed out (caller should NACK).
    async fn wait_for_rate_limit_permit(
        rate_limiter: &Arc<parking_lot::RwLock<Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>>>,
        metrics_collector: &Arc<PoolMetricsCollector>,
    ) -> bool {
        let limiter = rate_limiter.read().clone();

        let rl = match limiter {
            None => return true,
            Some(rl) => rl,
        };

        // Fast path: permit available immediately
        if rl.check().is_ok() {
            return true;
        }

        // Slow path: wait for permit with timeout
        metrics_collector.record_rate_limited();
        debug!("Rate limited - waiting for permit");

        match tokio::time::timeout(Self::RATE_LIMIT_WAIT_TIMEOUT, rl.until_ready()).await {
            Ok(_) => true,
            Err(_) => {
                warn!("Rate limit wait timeout after {}s — NACKing message",
                    Self::RATE_LIMIT_WAIT_TIMEOUT.as_secs());
                false
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
        self.queue_size.load(Ordering::Relaxed) == 0 &&
        self.active_workers.load(Ordering::Relaxed) == 0
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
            active_workers: self.active_workers.load(Ordering::Relaxed),
            queue_size: self.queue_size.load(Ordering::Relaxed),
            queue_capacity: std::cmp::max(
                current_concurrency * QUEUE_CAPACITY_MULTIPLIER,
                MIN_QUEUE_CAPACITY,
            ),
            message_group_count: self.group_handlers.len() as u32,
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

    /// Get the circuit breaker registry (for monitoring APIs)
    pub fn circuit_breaker_registry(&self) -> &Arc<crate::circuit_breaker_registry::CircuitBreakerRegistry> {
        &self.circuit_breaker_registry
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
        self.queue_size.load(Ordering::Relaxed)
    }

    /// Get current active worker count
    pub fn active_workers(&self) -> u32 {
        self.active_workers.load(Ordering::Relaxed)
    }

    /// Update concurrency at runtime
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
            let permits_to_acquire = (-diff) as usize;
            let timeout = Duration::from_secs(60);

            match tokio::time::timeout(timeout, self.acquire_permits(permits_to_acquire)).await {
                Ok(permits) => {
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
                    warn!(
                        pool_code = %self.config.code,
                        old = old_concurrency,
                        new = new_concurrency,
                        timeout_secs = 60,
                        active_workers = self.active_workers.load(Ordering::Relaxed),
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
    pub fn update_rate_limit(&self, new_rate_limit: Option<u32>) {
        let old_rate_limit = *self.rate_limit_per_minute.read();

        if old_rate_limit == new_rate_limit {
            return;
        }

        let new_limiter = new_rate_limit.and_then(|rpm| {
            if rpm == 0 {
                None
            } else {
                NonZeroU32::new(rpm).map(|nz| {
                    Arc::new(RateLimiter::direct(Quota::per_minute(nz)))
                })
            }
        });

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
