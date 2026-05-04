//! QueueManager - Central orchestrator for message routing
//!
//! Mirrors the Java QueueManager with:
//! - In-pipeline message tracking for deduplication
//! - Batch message routing with policies
//! - Pool management and lifecycle
//! - Consumer health monitoring

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use dashmap::DashMap;
use futures::future;
use parking_lot::Mutex;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn, error, debug};

use fc_common::{
    QueuedMessage, BatchMessage, InFlightMessage, MessageCallback,
    PoolConfig, RouterConfig, PoolStats, StallConfig, StalledMessageInfo,
    WarningCategory, WarningSeverity,
};
use fc_queue::{QueueConsumer, QueueMetrics};
use chrono::Utc;
use utoipa::ToSchema;

use crate::pool::ProcessPool;
use crate::mediator::{HttpMediator, HttpMediatorConfig, Mediator};
use crate::warning::WarningService;
use crate::error::RouterError;
use crate::Result;

/// How `QueueManager` obtains a mediator for each new pool.
enum MediatorSource {
    /// Build a fresh `HttpMediator` per pool (production path).
    PerPool(HttpMediatorConfig),
    /// All pools share this mediator instance (test seam for mocks).
    Shared(Arc<dyn Mediator + 'static>),
}

/// Callback that the pool worker calls directly when processing completes.
/// Reads the latest receipt handle from in_pipeline (may have been swapped by
/// redelivery), performs the SQS operation, then cleans up tracking.
/// No spawned task, no channel — mirrors the TS closure pattern.
///
/// **Drop safety.** If this callback is dropped without `ack()` or `nack()`
/// being called (panic during mediation, runtime cancellation, abandoned
/// queue task on early drain-task exit, …) the `Drop` impl guarantees:
///
/// 1. The entry is removed from `in_pipeline` and
///    `app_message_to_pipeline_key`. Without this cleanup, SQS redeliveries
///    of the same `broker_message_id` would be silently swallowed by
///    `filter_duplicates` Phase 1 (Check 1) and the message would stick
///    until the SQS message retention period expires — observed in
///    production as "thousands of messages stuck".
/// 2. A best-effort `nack` is fired via `tokio::spawn` so SQS releases the
///    visibility timeout sooner than its default. Failures here are
///    swallowed; the natural visibility timeout is the eventual safety net.
struct QueueMessageCallback {
    pipeline_key: String,
    app_message_id: String,
    consumer: Arc<dyn QueueConsumer + Send + Sync>,
    in_pipeline: Arc<DashMap<String, InFlightMessage>>,
    app_message_to_pipeline_key: Arc<DashMap<String, String>>,
    pending_delete: Arc<Mutex<HashMap<String, Instant>>>,
    /// Set to true the moment `ack()` or `nack()` is entered. The `Drop`
    /// impl checks this and only fires fallback cleanup if no resolution
    /// happened. AcqRel ordering: the load in Drop must observe stores from
    /// any thread that called ack/nack.
    completed: std::sync::atomic::AtomicBool,
}

impl QueueMessageCallback {
    /// Common cleanup: drop the in-memory tracking entries so future
    /// redeliveries of this `broker_message_id` flow through Phase 2 again
    /// instead of being silently swallowed as duplicates.
    fn cleanup_tracking(&self) {
        self.in_pipeline.remove(&self.pipeline_key);
        self.app_message_to_pipeline_key.remove(&self.app_message_id);
    }
}

#[async_trait::async_trait]
impl MessageCallback for QueueMessageCallback {
    async fn ack(&self) {
        // Mark resolved BEFORE doing any await so the Drop impl knows we
        // owned the resolution even if a panic happens mid-await.
        self.completed.store(true, std::sync::atomic::Ordering::Release);

        // Read latest receipt handle (may have been updated by redelivery)
        let (handle, broker_id) = self.in_pipeline
            .get(&self.pipeline_key)
            .map(|e| (e.receipt_handle.clone(), e.broker_message_id.clone()))
            .unwrap_or_default();

        if handle.is_empty() {
            error!(
                pipeline_key = %self.pipeline_key,
                app_message_id = %self.app_message_id,
                "ACK skipped — no receipt handle in in_pipeline (entry may have been reaped)"
            );
        } else {
            if let Err(e) = self.consumer.ack(&handle).await {
                // ACK failed — add to pending_delete BEFORE removing from in_pipeline
                if let Some(ref bid) = broker_id {
                    warn!(
                        broker_message_id = %bid,
                        app_message_id = %self.app_message_id,
                        error = %e,
                        "ACK failed (receipt handle likely expired) - adding to pending delete"
                    );
                    self.pending_delete.lock().insert(bid.clone(), Instant::now());
                } else {
                    error!(
                        app_message_id = %self.app_message_id,
                        error = %e,
                        "ACK failed and no broker message ID to track for pending delete"
                    );
                }
            }
        }

        // Clean up tracking AFTER SQS operation
        self.cleanup_tracking();
    }

    async fn nack(&self, delay_seconds: Option<u32>) {
        // Mark resolved BEFORE doing any await; see ack() above.
        self.completed.store(true, std::sync::atomic::Ordering::Release);

        let handle = self.in_pipeline
            .get(&self.pipeline_key)
            .map(|e| e.receipt_handle.clone())
            .unwrap_or_default();

        if handle.is_empty() {
            error!(
                pipeline_key = %self.pipeline_key,
                app_message_id = %self.app_message_id,
                "NACK skipped — no receipt handle in in_pipeline (entry may have been reaped)"
            );
        } else {
            let _ = self.consumer.nack(&handle, delay_seconds).await;
        }

        // Clean up tracking AFTER SQS operation
        self.cleanup_tracking();
    }
}

impl Drop for QueueMessageCallback {
    fn drop(&mut self) {
        // Fast path: ack() or nack() ran, no fallback needed.
        if self.completed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }

        // The callback was dropped without resolution. Most likely causes:
        //   • mediator panicked mid-mediation
        //   • tokio task was cancelled
        //   • drain task exited early leaving queued PoolTasks abandoned
        //
        // Always clear the in-memory tracking so SQS redeliveries are not
        // silently swallowed. Fire a best-effort nack so the message
        // returns to the queue sooner than its full visibility timeout.

        let pipeline_key = self.pipeline_key.clone();
        let app_message_id = self.app_message_id.clone();

        // Snapshot the current receipt handle before we yank the entry.
        let handle = self.in_pipeline
            .get(&pipeline_key)
            .map(|e| e.receipt_handle.clone())
            .unwrap_or_default();

        // Synchronous cleanup of tracking — never deferred.
        self.cleanup_tracking();

        warn!(
            pipeline_key = %pipeline_key,
            app_message_id = %app_message_id,
            "Callback dropped without ack/nack — fallback cleanup ran (likely mediator panic or task cancel)"
        );

        if !handle.is_empty() {
            // Best-effort nack on a detached task. If we can't get a tokio
            // handle (e.g. shutting down), the SQS visibility timeout will
            // eventually redeliver and processing will retry.
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                let consumer = self.consumer.clone();
                rt.spawn(async move {
                    let _ = consumer.nack(&handle, Some(10)).await;
                });
            }
        }
    }
}

/// Factory trait for creating queue consumers
/// Implementations can create SQS, ActiveMQ, or other consumer types
#[async_trait::async_trait]
pub trait ConsumerFactory {
    /// Create a consumer for the given queue configuration
    async fn create_consumer(
        &self,
        config: &fc_common::QueueConfig,
    ) -> Result<Arc<dyn QueueConsumer + Send + Sync>>;
}

/// Central orchestrator for message routing
pub struct QueueManager {
    /// In-pipeline message tracking for deduplication
    /// Wrapped in Arc so spawned tasks can share the same map
    in_pipeline: Arc<DashMap<String, InFlightMessage>>,

    /// App message ID to pipeline key mapping for deduplication
    /// Wrapped in Arc so spawned tasks can share the same map
    app_message_to_pipeline_key: Arc<DashMap<String, String>>,

    /// Process pools by code
    pools: DashMap<String, Arc<ProcessPool>>,

    /// Pools that are draining (removed from config, waiting for in-flight to complete)
    draining_pools: DashMap<String, Arc<ProcessPool>>,

    /// Queue consumers (RwLock for async-safe access)
    consumers: RwLock<HashMap<String, Arc<dyn QueueConsumer + Send + Sync>>>,

    /// Consumers that are draining (removed from config, waiting for in-flight to complete)
    draining_consumers: RwLock<HashMap<String, Arc<dyn QueueConsumer + Send + Sync>>>,

    /// Current pool configurations (for detecting changes)
    pool_configs: RwLock<HashMap<String, PoolConfig>>,

    /// Current queue configurations (for detecting changes during sync)
    queue_configs: RwLock<HashMap<String, fc_common::QueueConfig>>,

    /// Consumer factory for creating new queue consumers during config sync
    /// If None, new queues in config will be logged but not auto-created
    consumer_factory: Option<Arc<dyn ConsumerFactory + Send + Sync>>,

    /// How to build a mediator for each new pool.
    ///
    /// Production path: `MediatorSource::PerPool(config)` — each pool gets
    /// its own `HttpMediator` with its own reqwest `Client` / connection
    /// pool. Transport isolation between pools avoids the AWS 128-stream
    /// cap on a single HTTP/2 connection.
    ///
    /// Test path: `MediatorSource::Shared(mediator)` — all pools share
    /// one mediator instance, used by tests that inject mocks via
    /// `QueueManager::with_shared_mediator_for_testing`.
    mediator_source: MediatorSource,

    /// Default pool code for messages without explicit pool
    default_pool_code: String,

    /// Running state
    running: AtomicBool,

    /// Shutdown signal sender
    shutdown_tx: broadcast::Sender<()>,

    /// Batch ID counter for grouping messages
    batch_counter: std::sync::atomic::AtomicU64,

    /// Track broker message IDs that were successfully processed but failed to delete
    /// (due to expired receipt handle). When these reappear, delete them immediately.
    /// Uses the broker's internal MessageId (not our application message ID) to correctly
    /// distinguish redeliveries from new instructions with the same application ID.
    /// Each entry includes the insertion time for TTL-based eviction.
    ///
    /// Uses parking_lot::Mutex (not tokio) intentionally — all lock sites are brief
    /// (single insert/remove/retain) and never held across .await boundaries.
    pending_delete_broker_ids: Arc<Mutex<HashMap<String, Instant>>>,

    /// Maximum number of pools allowed
    max_pools: usize,

    /// Pool count warning threshold
    pool_warning_threshold: usize,

    /// Stall detection configuration
    stall_config: StallConfig,

    /// Warning service for generating operational warnings
    warning_service: Arc<WarningService>,

    /// Health service for recording consumer poll times
    health_service: Option<Arc<crate::health::HealthService>>,

    /// Weak self-reference for spawning poll tasks from sync_queue_consumers.
    /// Set via `init_self_ref()` after wrapping in Arc.
    self_ref: parking_lot::RwLock<Option<std::sync::Weak<Self>>>,
}

impl QueueManager {
    pub fn new(mediator_config: HttpMediatorConfig) -> Self {
        // Java defaults: max-pools = 10000, pool-warning-threshold = 5000
        Self::with_limits(mediator_config, 10000, 5000)
    }

    pub fn with_limits(mediator_config: HttpMediatorConfig, max_pools: usize, pool_warning_threshold: usize) -> Self {
        Self::with_config(mediator_config, max_pools, pool_warning_threshold, StallConfig::default())
    }

    pub fn with_config(
        mediator_config: HttpMediatorConfig,
        max_pools: usize,
        pool_warning_threshold: usize,
        stall_config: StallConfig,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            in_pipeline: Arc::new(DashMap::new()),
            app_message_to_pipeline_key: Arc::new(DashMap::new()),
            pools: DashMap::new(),
            draining_pools: DashMap::new(),
            consumers: RwLock::new(HashMap::new()),
            draining_consumers: RwLock::new(HashMap::new()),
            pool_configs: RwLock::new(HashMap::new()),
            queue_configs: RwLock::new(HashMap::new()),
            consumer_factory: None,
            mediator_source: MediatorSource::PerPool(mediator_config),
            default_pool_code: "DEFAULT-POOL".to_string(),  // Java: DEFAULT_POOL_CODE
            running: AtomicBool::new(true),
            shutdown_tx,
            batch_counter: std::sync::atomic::AtomicU64::new(0),
            pending_delete_broker_ids: Arc::new(Mutex::new(HashMap::new())),
            max_pools,
            pool_warning_threshold,
            stall_config,
            warning_service: Arc::new(WarningService::noop()),
            health_service: None,
            self_ref: parking_lot::RwLock::new(None),
        }
    }

    /// Initialize the self-reference after wrapping in Arc.
    /// Must be called before `start()` or any config sync.
    pub fn init_self_ref(self: &Arc<Self>) {
        *self.self_ref.write() = Some(Arc::downgrade(self));
    }

    /// Set the consumer factory for creating new queue consumers during config sync
    pub fn set_consumer_factory(&mut self, factory: Arc<dyn ConsumerFactory + Send + Sync>) {
        self.consumer_factory = Some(factory);
    }

    /// Set the warning service
    pub fn set_warning_service(&mut self, warning_service: Arc<WarningService>) {
        self.warning_service = warning_service;
    }

    /// Set the health service (for recording consumer poll times)
    pub fn set_health_service(&mut self, health_service: Arc<crate::health::HealthService>) {
        self.health_service = Some(health_service);
    }

    /// Get warning service reference
    pub fn warning_service(&self) -> &Arc<WarningService> {
        &self.warning_service
    }

    /// Build a mediator instance for a pool. Production path builds a fresh
    /// `HttpMediator` per pool (its own reqwest `Client` / connection pool).
    /// Test path returns a shared mock.
    fn build_mediator(&self) -> Arc<dyn Mediator + 'static> {
        match &self.mediator_source {
            MediatorSource::PerPool(config) => Arc::new(
                HttpMediator::with_config(config.clone())
                    .with_warning_service(self.warning_service.clone())
            ),
            MediatorSource::Shared(m) => m.clone(),
        }
    }

    /// Test-only constructor: every pool shares the supplied mediator. Use
    /// this when you need to inject a mock or instrument mediator calls.
    /// Production code should use [`QueueManager::new`] and let the manager
    /// build a mediator per pool.
    #[doc(hidden)]
    pub fn with_shared_mediator_for_testing(mediator: Arc<dyn Mediator + 'static>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            in_pipeline: Arc::new(DashMap::new()),
            app_message_to_pipeline_key: Arc::new(DashMap::new()),
            pools: DashMap::new(),
            draining_pools: DashMap::new(),
            consumers: RwLock::new(HashMap::new()),
            draining_consumers: RwLock::new(HashMap::new()),
            pool_configs: RwLock::new(HashMap::new()),
            queue_configs: RwLock::new(HashMap::new()),
            consumer_factory: None,
            mediator_source: MediatorSource::Shared(mediator),
            default_pool_code: "DEFAULT-POOL".to_string(),
            running: AtomicBool::new(true),
            shutdown_tx,
            batch_counter: std::sync::atomic::AtomicU64::new(0),
            pending_delete_broker_ids: Arc::new(Mutex::new(HashMap::new())),
            max_pools: 10000,
            pool_warning_threshold: 5000,
            stall_config: StallConfig::default(),
            warning_service: Arc::new(WarningService::noop()),
            health_service: None,
            self_ref: parking_lot::RwLock::new(None),
        }
    }

    /// Add a queue consumer
    pub async fn add_consumer(&self, consumer: Arc<dyn QueueConsumer + Send + Sync>) {
        let id = consumer.identifier().to_string();
        self.consumers.write().await.insert(id, consumer);
    }

    /// Apply router configuration (initial setup)
    pub async fn apply_config(&self, config: RouterConfig) -> Result<()> {
        let mut pool_configs = self.pool_configs.write().await;
        for pool_config in config.processing_pools {
            let code = pool_config.code.clone();
            pool_configs.insert(code.clone(), pool_config.clone());
            self.get_or_create_pool(&code, Some(pool_config)).await?;
        }
        Ok(())
    }

    /// Hot reload configuration - applies changes without restart
    /// Mirrors Java's updatePoolConfiguration behavior:
    /// - Removed pools: drain asynchronously
    /// - Updated pools: update concurrency/rate limit in-place
    /// - New pools: create and start
    pub async fn reload_config(&self, config: RouterConfig) -> Result<bool> {
        if !self.running.load(Ordering::SeqCst) {
            warn!("Cannot reload config - QueueManager is shutting down");
            return Ok(false);
        }

        info!("Hot reloading configuration...");

        // Build map of new pool configs
        let new_pool_configs: HashMap<String, PoolConfig> = config.processing_pools
            .iter()
            .map(|p| (p.code.clone(), p.clone()))
            .collect();

        let mut pool_configs = self.pool_configs.write().await;
        let mut pools_updated = 0;
        let mut pools_created = 0;
        let mut pools_removed = 0;

        // Step 1: Handle existing pools - update or remove
        let existing_codes: Vec<String> = self.pools.iter().map(|e| e.key().clone()).collect();
        for pool_code in existing_codes {
            if let Some(new_config) = new_pool_configs.get(&pool_code) {
                // Pool exists in new config - check for changes
                if let Some(old_config) = pool_configs.get(&pool_code) {
                    let concurrency_changed = old_config.concurrency != new_config.concurrency;
                    let rate_limit_changed = old_config.rate_limit_per_minute != new_config.rate_limit_per_minute;

                    if concurrency_changed || rate_limit_changed {
                        if let Some(pool) = self.pools.get(&pool_code) {
                            // Update the pool in-place
                            if concurrency_changed {
                                info!(
                                    pool_code = %pool_code,
                                    old_concurrency = old_config.concurrency,
                                    new_concurrency = new_config.concurrency,
                                    "Updating pool concurrency"
                                );
                                pool.update_concurrency(new_config.concurrency).await;
                            }

                            if rate_limit_changed {
                                info!(
                                    pool_code = %pool_code,
                                    old_rate_limit = ?old_config.rate_limit_per_minute,
                                    new_rate_limit = ?new_config.rate_limit_per_minute,
                                    "Updating pool rate limit"
                                );
                                pool.update_rate_limit(new_config.rate_limit_per_minute);
                            }

                            pools_updated += 1;
                        }
                    }
                }
                // Update stored config
                pool_configs.insert(pool_code, new_config.clone());
            } else {
                // Pool removed from config - drain asynchronously
                if let Some((code, pool)) = self.pools.remove(&pool_code) {
                    info!(
                        pool_code = %code,
                        queue_size = pool.queue_size(),
                        active_workers = pool.active_workers(),
                        "Pool removed from config - draining asynchronously"
                    );
                    pool.drain().await;
                    self.draining_pools.insert(code.clone(), pool);
                    pool_configs.remove(&code);
                    pools_removed += 1;
                }
            }
        }

        // Step 2: Create new pools
        for pool_config in &config.processing_pools {
            if !self.pools.contains_key(&pool_config.code) {
                // Check pool count limits
                let current_count = self.pools.len();
                if current_count >= self.max_pools {
                    error!(
                        pool_code = %pool_config.code,
                        current_count = current_count,
                        max_pools = self.max_pools,
                        "Cannot create pool: maximum pool limit reached"
                    );
                    self.warning_service.add_warning(
                        WarningCategory::PoolHealth,
                        WarningSeverity::Critical,
                        format!("Max pool limit reached ({}/{}) - cannot create pool [{}]",
                            current_count, self.max_pools, pool_config.code),
                        "QueueManager".to_string(),
                    );
                    continue;
                }

                if current_count >= self.pool_warning_threshold {
                    warn!(
                        pool_code = %pool_config.code,
                        current_count = current_count,
                        max_pools = self.max_pools,
                        threshold = self.pool_warning_threshold,
                        "Pool count approaching limit"
                    );
                    self.warning_service.add_warning(
                        WarningCategory::PoolHealth,
                        WarningSeverity::Warn,
                        format!("Pool count {} approaching limit {} (threshold: {})",
                            current_count, self.max_pools, self.pool_warning_threshold),
                        "QueueManager".to_string(),
                    );
                }

                // Create new pool
                self.get_or_create_pool(&pool_config.code, Some(pool_config.clone())).await?;
                pool_configs.insert(pool_config.code.clone(), pool_config.clone());
                pools_created += 1;
            }
        }

        // Step 3: Sync queue consumers (Java: Step 4)
        let (queues_created, queues_removed) = self.sync_queue_consumers(&config).await?;

        // Get counts before logging (avoid await in info! macro)
        let total_active_consumers = self.consumers.read().await.len();

        info!(
            pools_updated = pools_updated,
            pools_created = pools_created,
            pools_removed = pools_removed,
            queues_created = queues_created,
            queues_removed = queues_removed,
            total_active_pools = self.pools.len(),
            total_draining_pools = self.draining_pools.len(),
            total_active_consumers = total_active_consumers,
            "Configuration reload complete"
        );

        Ok(true)
    }

    /// Sync queue consumers based on configuration changes
    /// Mirrors Java's queue consumer sync logic in syncConfig()
    async fn sync_queue_consumers(&self, config: &RouterConfig) -> Result<(usize, usize)> {
        let mut queues_created = 0;
        let mut queues_removed = 0;

        // Build map of new queue configs
        let new_queue_configs: HashMap<String, fc_common::QueueConfig> = config.queues
            .iter()
            .map(|q| {
                // Use name as identifier, fall back to uri if name is empty
                let identifier = if q.name.is_empty() { q.uri.clone() } else { q.name.clone() };
                (identifier, q.clone())
            })
            .collect();

        let mut queue_configs = self.queue_configs.write().await;
        let mut consumers = self.consumers.write().await;
        let mut draining = self.draining_consumers.write().await;

        // Phase out consumers for queues that no longer exist
        let existing_queues: Vec<String> = consumers.keys().cloned().collect();
        for queue_id in existing_queues {
            if !new_queue_configs.contains_key(&queue_id) {
                info!(queue_id = %queue_id, "Phasing out consumer for removed queue");

                if let Some(consumer) = consumers.remove(&queue_id) {
                    // Stop consumer (sets running=false, initiates graceful shutdown)
                    consumer.stop().await;

                    // Move to draining consumers for async cleanup
                    draining.insert(queue_id.clone(), consumer);
                    queue_configs.remove(&queue_id);
                    queues_removed += 1;

                    info!(queue_id = %queue_id, "Consumer moved to draining state");
                }
            }
        }

        // Start consumers for new queues (if factory is available)
        // Collect new consumers to spawn poll tasks after releasing locks
        let mut new_consumers: Vec<Arc<dyn QueueConsumer + Send + Sync>> = Vec::new();

        if let Some(ref factory) = self.consumer_factory {
            for (queue_id, queue_config) in &new_queue_configs {
                if !consumers.contains_key::<String>(queue_id) {
                    info!(queue_id = %queue_id, "Creating new queue consumer");

                    match factory.create_consumer(queue_config).await {
                        Ok(consumer) => {
                            consumers.insert(queue_id.clone(), consumer.clone());
                            queue_configs.insert(queue_id.clone(), queue_config.clone());
                            new_consumers.push(consumer);
                            queues_created += 1;
                            info!(queue_id = %queue_id, "Queue consumer created and ready");
                        }
                        Err(e) => {
                            error!(queue_id = %queue_id, error = %e, "Failed to create queue consumer");
                            self.warning_service.add_warning(
                                WarningCategory::ConsumerHealth,
                                WarningSeverity::Critical,
                                format!("Failed to create consumer for queue [{}]: {}", queue_id, e),
                                "QueueManager".to_string(),
                            );
                        }
                    }
                }
            }
        } else {
            // No factory - just log new queues that couldn't be created
            for queue_id in new_queue_configs.keys() {
                if !consumers.contains_key::<String>(queue_id) {
                    warn!(
                        queue_id = %queue_id,
                        "New queue in config but no consumer factory available - consumer will not be auto-created"
                    );
                }
            }
        }

        // Release write locks before spawning tasks
        drop(consumers);
        drop(queue_configs);
        drop(draining);

        // Spawn poll tasks for newly created consumers
        if !new_consumers.is_empty() {
            if let Some(arc_self) = self.self_ref.read().as_ref().and_then(|w| w.upgrade()) {
                for consumer in new_consumers {
                    info!(consumer_id = %consumer.identifier(), "Spawning poll task for hot-added consumer");
                    arc_self.spawn_consumer_poll_task(consumer);
                }
            } else {
                warn!("Cannot spawn poll tasks for new consumers — self_ref not initialized (call init_self_ref after Arc::new)");
            }
        }

        Ok((queues_created, queues_removed))
    }

    /// Cleanup draining pools that have finished
    /// Should be called periodically (e.g., every 10 seconds)
    pub async fn cleanup_draining_pools(&self) {
        let mut cleaned = Vec::new();

        for entry in self.draining_pools.iter() {
            let pool = entry.value();
            if pool.is_fully_drained() {
                info!(pool_code = %entry.key(), "Draining pool finished - cleaning up");
                pool.shutdown().await;
                cleaned.push(entry.key().clone());
            }
        }

        for code in cleaned {
            self.draining_pools.remove(&code);
        }
    }

    /// Get or create a pool by code
    async fn get_or_create_pool(&self, code: &str, config: Option<PoolConfig>) -> Result<Arc<ProcessPool>> {
        if let Some(pool) = self.pools.get(code) {
            return Ok(pool.clone());
        }

        let pool_config = config.unwrap_or_else(|| PoolConfig {
            code: code.to_string(),
            concurrency: 20,  // Java: DEFAULT_POOL_CONCURRENCY = 20
            rate_limit_per_minute: None,
        });

        let pool = ProcessPool::new(
            pool_config.clone(),
            self.build_mediator(),
        );

        let pool_arc = Arc::new(pool);
        pool_arc.start().await;

        self.pools.insert(code.to_string(), pool_arc.clone());
        info!(pool_code = %code, concurrency = pool_config.concurrency, "Created process pool");

        Ok(pool_arc)
    }

    /// Route a batch of messages from a consumer poll
    pub async fn route_batch(&self, messages: Vec<QueuedMessage>, consumer: Arc<dyn QueueConsumer>) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            // NACK all messages concurrently on shutdown
            let nack_futs: Vec<_> = messages.iter().map(|msg| {
                let consumer = consumer.clone();
                let handle = msg.receipt_handle.clone();
                async move { let _ = consumer.nack(&handle, None).await; }
            }).collect();
            future::join_all(nack_futs).await;
            return Err(RouterError::ShutdownInProgress);
        }

        if messages.is_empty() {
            return Ok(());
        }

        let batch_id: Arc<str> = Arc::from(self.batch_counter.fetch_add(1, Ordering::Relaxed).to_string().as_str());

        // Phase 0: Check for messages that need immediate deletion (previously processed but ACK failed)
        // First, identify which messages need deletion (while holding lock)
        let mut messages_to_delete = Vec::new();
        let mut messages_to_process = Vec::with_capacity(messages.len());
        {
            let mut pending_delete = self.pending_delete_broker_ids.lock();
            for msg in messages {
                let should_delete = msg.broker_message_id.as_ref()
                    .map(|broker_id| pending_delete.remove(broker_id).is_some())
                    .unwrap_or(false);

                if should_delete {
                    // This message was already processed successfully, mark for deletion
                    messages_to_delete.push(msg);
                } else {
                    messages_to_process.push(msg);
                }
            }
        }
        // Perform the deletions concurrently (independent SQS API calls)
        if !messages_to_delete.is_empty() {
            let delete_futs: Vec<_> = messages_to_delete.iter().map(|msg| {
                let consumer = consumer.clone();
                let handle = msg.receipt_handle.clone();
                let broker_id = msg.broker_message_id.clone();
                let app_id = msg.message.id.clone();
                async move {
                    info!(
                        broker_message_id = ?broker_id,
                        app_message_id = %app_id,
                        "Message was previously processed - deleting from queue now"
                    );
                    let _ = consumer.ack(&handle).await;
                }
            }).collect();
            future::join_all(delete_futs).await;
        }

        if messages_to_process.is_empty() {
            return Ok(());
        }

        // Phase 1: Filter duplicates (takes ownership to avoid cloning payloads)
        let filtered = self.filter_duplicates(messages_to_process);

        // Handle duplicates - no SQS API call needed.
        // filter_duplicates() already updated the receipt handle in in_pipeline,
        // so the eventual ACK will use the latest valid handle from this redelivery.
        // We intentionally do NOT defer/nack here — the message stays in SQS with its
        // natural visibility timeout. When it expires SQS redelivers, we update the
        // handle again, and this repeats until processing completes and we ACK with
        // the latest handle. This matches the Java behavior and avoids a hot
        // poll-defer loop that inflates SQS metrics and wastes API calls.
        if !filtered.duplicates.is_empty() {
            debug!(
                count = filtered.duplicates.len(),
                "Duplicate messages (redelivery) — receipt handles updated, no SQS action needed"
            );
        }

        // Handle requeued - these were already completed, ACK them
        // ACK requeued duplicates concurrently
        if !filtered.requeued.is_empty() {
            let requeue_futs: Vec<_> = filtered.requeued.iter().map(|req| {
                let consumer = consumer.clone();
                let handle = req.message.receipt_handle.clone();
                let msg_id = req.message.message.id.clone();
                let key = req.existing_pipeline_key.clone();
                async move {
                    debug!(message_id = %msg_id, pipeline_key = %key, "Requeued duplicate, ACKing");
                    let _ = consumer.ack(&handle).await;
                }
            }).collect();
            future::join_all(requeue_futs).await;
        }

        // Phase 2: Group by pool and route
        let by_pool = self.group_by_pool(filtered.unique);

        for (pool_code, pool_messages) in by_pool {
            let pool = match self.get_or_create_pool(&pool_code, None).await {
                Ok(p) => p,
                Err(e) => {
                    error!(pool_code = %pool_code, error = %e, "Failed to get/create pool");
                    // NACK all messages for this pool
                    for msg in pool_messages {
                        let _ = consumer.nack(&msg.receipt_handle, Some(5)).await;
                    }
                    continue;
                }
            };

            // Check pool capacity for ALL messages in this pool
            let available = pool.available_capacity();
            if available < pool_messages.len() {
                warn!(
                    pool_code = %pool_code,
                    available = available,
                    requested = pool_messages.len(),
                    "Pool at capacity, deferring all messages for this pool"
                );
                self.warning_service.add_warning(
                    WarningCategory::QueueHealth,
                    WarningSeverity::Warn,
                    format!("Pool [{}] queue full, deferring {} messages from batch", pool_code, pool_messages.len()),
                    "QueueManager".to_string(),
                );
                // Defer concurrently - capacity limits are not errors
                let defer_futs: Vec<_> = pool_messages.iter().map(|msg| {
                    let consumer = consumer.clone();
                    let handle = msg.receipt_handle.clone();
                    async move { let _ = consumer.defer(&handle, Some(5)).await; }
                }).collect();
                future::join_all(defer_futs).await;
                continue;
            }

            // Note: Rate limiting is now handled inside the pool worker (blocking wait)
            // Messages stay in pool queue instead of being deferred back to SQS

            // Phase 3: Group by messageGroupId for FIFO ordering enforcement
            // This mirrors Java's messagesByGroup logic in routeMessageBatch
            let messages_by_group = self.group_by_message_group(pool_messages);

            for (group_id, group_messages) in messages_by_group {
                let mut nack_remaining = false;

                for msg in group_messages {
                    // If previous message in group failed, NACK all remaining in this group
                    // This enforces FIFO ordering - if message A fails, message B (which depends on A) must also fail
                    if nack_remaining {
                        debug!(
                            message_id = %msg.message.id,
                            group_id = %group_id,
                            "NACKing message - previous message in group failed submission"
                        );
                        let _ = consumer.nack(&msg.receipt_handle, Some(5)).await;
                        continue;
                    }

                    let app_message_id = msg.message.id.clone();

                    // Use broker_message_id as pipeline key (mirrors Java's sqsMessageId usage)
                    // Fall back to a composite key if broker_message_id is not available
                    let pipeline_key = msg.broker_message_id.clone()
                        .unwrap_or_else(|| format!("fallback:{}:{}", msg.queue_identifier, msg.message.id));

                    let receipt_handle = msg.receipt_handle.clone();

                    // Track in pipeline with receipt handle
                    let in_flight = InFlightMessage::new(
                        &msg.message,
                        msg.broker_message_id.clone(),
                        msg.queue_identifier.clone(),
                        Some(Arc::clone(&batch_id)),
                        msg.receipt_handle.clone(),
                    );
                    self.in_pipeline.insert(pipeline_key.clone(), in_flight);

                    // Track app message ID -> pipeline key for requeue detection
                    self.app_message_to_pipeline_key.insert(app_message_id.clone(), pipeline_key.clone());

                    // Create callback — pool worker calls this directly, no spawned task
                    let callback = QueueMessageCallback {
                        pipeline_key: pipeline_key.clone(),
                        app_message_id: app_message_id.clone(),
                        consumer: consumer.clone(),
                        in_pipeline: self.in_pipeline.clone(),
                        app_message_to_pipeline_key: self.app_message_to_pipeline_key.clone(),
                        pending_delete: self.pending_delete_broker_ids.clone(),
                        completed: std::sync::atomic::AtomicBool::new(false),
                    };

                    let batch_msg = BatchMessage {
                        message: msg.message,
                        receipt_handle: msg.receipt_handle,
                        broker_message_id: msg.broker_message_id,
                        queue_identifier: msg.queue_identifier,
                        batch_id: Some(Arc::clone(&batch_id)),
                        callback: Box::new(callback),
                    };

                    // Submit to pool — pool worker calls callback.ack()/nack() when done
                    if let Err(e) = pool.submit(batch_msg).await {
                        error!(
                            message_id = %app_message_id,
                            group_id = %group_id,
                            error = %e,
                            "Failed to submit to pool - NACKing this and remaining messages in group"
                        );

                        // Remove from pipeline since we're NACKing
                        self.in_pipeline.remove(&pipeline_key);
                        self.app_message_to_pipeline_key.remove(&app_message_id);

                        // NACK this message
                        let _ = consumer.nack(&receipt_handle, Some(5)).await;

                        // Set flag to NACK all remaining messages in this group (FIFO enforcement)
                        nack_remaining = true;
                    }
                }
            }
        }

        Ok(())
    }

    /// Filter duplicates from a batch.
    ///
    /// Mirrors Java's deduplication logic:
    /// 1. Check broker_message_id first (same SQS message = redelivery due to visibility timeout)
    /// 2. Check app_message_id second (same app ID, different broker ID = external requeue)
    ///
    /// Takes ownership of the messages Vec to avoid cloning payloads.
    fn filter_duplicates(&self, messages: Vec<QueuedMessage>) -> FilteredBatch {
        let mut result = FilteredBatch {
            unique: Vec::with_capacity(messages.len()),
            duplicates: Vec::new(),
            requeued: Vec::new(),
        };

        for msg in messages {
            // Check 1: Same broker message ID (physical redelivery from SQS due to visibility timeout)
            // This MUST be checked FIRST because the same broker ID means it's a visibility timeout redelivery,
            // NOT a requeue by an external process
            if let Some(ref broker_msg_id) = msg.broker_message_id {
                if let Some(mut entry) = self.in_pipeline.get_mut(broker_msg_id) {
                    // Update receipt handle with the new one from the redelivered message
                    // This ensures when processing completes, ACK uses the valid (latest) receipt handle
                    if entry.receipt_handle != msg.receipt_handle {
                        debug!(
                            message_id = %msg.message.id,
                            broker_message_id = %broker_msg_id,
                            "Updating receipt handle for redelivered message (visibility timeout)"
                        );
                        entry.receipt_handle = msg.receipt_handle.clone();
                        // Also update broker_message_id in case it was a fallback key
                        if entry.broker_message_id.is_none() {
                            entry.broker_message_id = Some(broker_msg_id.clone());
                        }
                    }
                    let pipeline_key = broker_msg_id.clone();
                    result.duplicates.push(DuplicateMessage {
                        message: msg,
                        existing_pipeline_key: pipeline_key,
                    });
                    continue;
                }
            }

            // Check 2: Same application message ID but DIFFERENT broker message ID (requeued by external process)
            // This happens when a separate process requeues messages that were stuck in QUEUED status for 20+ min
            // The external process creates a NEW SQS message with the same application message ID
            if let Some(existing_pipeline_key) = self.app_message_to_pipeline_key.get(&msg.message.id) {
                let existing_key = existing_pipeline_key.value().clone();

                // Only treat as requeued duplicate if the broker message IDs are DIFFERENT
                // If they're the same, it would have been caught by the check above
                if let Some(ref new_broker_id) = msg.broker_message_id {
                    if *new_broker_id != existing_key {
                        info!(
                            app_message_id = %msg.message.id,
                            existing_broker_id = %existing_key,
                            new_broker_id = %new_broker_id,
                            "Requeued message detected - app ID already in pipeline, will ACK to remove duplicate"
                        );
                        result.requeued.push(DuplicateMessage {
                            message: msg,
                            existing_pipeline_key: existing_key,
                        });
                        continue;
                    }
                }

                // Same broker ID or no broker ID - check if still in pipeline
                if let Some(mut entry) = self.in_pipeline.get_mut(&existing_key) {
                    // Update receipt handle for redelivery
                    if entry.receipt_handle != msg.receipt_handle {
                        debug!(
                            message_id = %msg.message.id,
                            "Updating receipt handle for redelivered message"
                        );
                        entry.receipt_handle = msg.receipt_handle.clone();
                    }
                    result.duplicates.push(DuplicateMessage {
                        message: msg,
                        existing_pipeline_key: existing_key,
                    });
                    continue;
                }
            }

            result.unique.push(msg);
        }

        result
    }

    /// Group messages by pool code.
    /// Mirrors Java's pool routing logic: if a pool code is not found in processPools,
    /// log a ROUTING warning and fall back to DEFAULT-POOL.
    fn group_by_pool(&self, messages: Vec<QueuedMessage>) -> std::collections::HashMap<String, Vec<QueuedMessage>> {
        let mut by_pool: std::collections::HashMap<String, Vec<QueuedMessage>> = std::collections::HashMap::new();

        for msg in messages {
            let pool_code = if msg.message.pool_code.is_empty() {
                self.default_pool_code.clone()
            } else if self.pools.get(&msg.message.pool_code).is_none() {
                // No pool found → log warning + route to DEFAULT-POOL
                warn!(
                    message_id = %msg.message.id,
                    pool_code = %msg.message.pool_code,
                    default_pool = %self.default_pool_code,
                    "No pool found for pool_code, routing to DEFAULT-POOL"
                );
                self.warning_service.add_warning(
                    WarningCategory::Routing,
                    WarningSeverity::Warn,
                    format!("No pool found for code [{}] on message [{}] — routed to {}",
                        msg.message.pool_code, msg.message.id, self.default_pool_code),
                    "QueueManager".to_string(),
                );
                self.default_pool_code.clone()
            } else {
                msg.message.pool_code.clone()
            };

            by_pool.entry(pool_code).or_default().push(msg);
        }

        by_pool
    }

    /// Group messages by message_group_id for FIFO ordering enforcement
    /// Mirrors Java's messagesByGroup logic in routeMessageBatch
    fn group_by_message_group(&self, messages: Vec<QueuedMessage>) -> indexmap::IndexMap<String, Vec<QueuedMessage>> {
        // Use IndexMap to preserve insertion order (like Java's LinkedHashMap)
        let mut by_group: indexmap::IndexMap<String, Vec<QueuedMessage>> = indexmap::IndexMap::new();

        for msg in messages {
            let group_id = msg.message.message_group_id.clone()
                .unwrap_or_else(|| "__DEFAULT__".to_string());
            by_group.entry(group_id).or_default().push(msg);
        }

        by_group
    }

    /// Spawn a poll task for a single consumer. Returns the JoinHandle.
    /// Called from both `start()` (initial consumers) and `sync_queue_consumers` (hot-added consumers).
    fn spawn_consumer_poll_task(
        self: &Arc<Self>,
        consumer: Arc<dyn QueueConsumer + Send + Sync>,
    ) -> tokio::task::JoinHandle<()> {
        let manager = self.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut last_poll_end = Instant::now();
            const STARVATION_THRESHOLD: Duration = Duration::from_secs(30);

            loop {
                // Detect thread/task starvation: warn if >30s between poll loops (Java: 30s)
                let loop_gap = last_poll_end.elapsed();
                if loop_gap > STARVATION_THRESHOLD {
                    warn!(
                        consumer = %consumer.identifier(),
                        gap_seconds = loop_gap.as_secs(),
                        "Task starvation detected: {}s between poll loops (threshold: {}s)",
                        loop_gap.as_secs(),
                        STARVATION_THRESHOLD.as_secs()
                    );
                }

                // Backpressure: if all pools are full, wait instead of polling.
                // Prevents hot poll-defer loop that wastes SQS API calls.
                if !manager.has_pool_capacity() {
                    debug!(consumer = %consumer.identifier(), "All pools at capacity — pausing poll");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }

                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!(consumer = %consumer.identifier(), "Consumer shutting down");
                        break;
                    }
                    result = consumer.poll(10) => {
                        last_poll_end = Instant::now();

                        // Record consumer poll with health service
                        if let Some(ref health_service) = manager.health_service {
                            health_service.record_consumer_poll(consumer.identifier());
                        }

                        match result {
                            Ok(messages) if messages.is_empty() => {
                                // No messages — SQS long poll already waited up to 20s.
                                // Brief pause before re-polling.
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                            Ok(messages) => {
                                let count = messages.len();
                                if let Err(e) = manager.route_batch(messages, consumer.clone()).await {
                                    error!(error = %e, "Error routing batch");
                                }
                                // Full batch (10) — re-poll immediately, more messages likely waiting.
                                // Partial batch (< 10) — brief pause, queue is draining.
                                if count < 10 {
                                    tokio::time::sleep(Duration::from_millis(500)).await;
                                }
                            }
                            Err(e) => {
                                error!(error = %e, consumer = %consumer.identifier(), "Error polling");
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                        }
                    }
                }
            }
        })
    }

    /// Start the queue manager and all consumers
    pub async fn start(self: Arc<Self>) -> Result<()> {
        // Ensure self_ref is set so sync_queue_consumers can spawn poll tasks
        self.init_self_ref();

        let consumers = self.consumers.read().await;
        info!(consumers = consumers.len(), "Starting QueueManager");

        let mut handles = Vec::new();

        // Clone consumers for spawning tasks
        let consumers_vec: Vec<_> = consumers.values().cloned().collect();
        drop(consumers); // Release the read lock

        for consumer in consumers_vec {
            handles.push(self.spawn_consumer_poll_task(consumer));
        }

        // Defence-in-depth: reaper for stuck `in_pipeline` entries.
        handles.push(self.clone().spawn_in_pipeline_reaper());

        // Wait for all consumer tasks
        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

    /// TTL of an `in_pipeline` entry before the reaper considers it stuck.
    /// Production processing should never take this long; legitimate
    /// long-running work should have its visibility timeout extended.
    const IN_PIPELINE_TTL: Duration = Duration::from_secs(15 * 60);
    const IN_PIPELINE_REAPER_INTERVAL: Duration = Duration::from_secs(60);

    /// Spawn a periodic task that scans `in_pipeline` and removes any entry
    /// older than `IN_PIPELINE_TTL`. This is a safety net for cases where a
    /// callback is dropped without firing AND its `Drop` impl somehow
    /// doesn't run (e.g. forgotten ownership in a future map). Without this,
    /// SQS would keep redelivering and `filter_duplicates` would silently
    /// swallow each redelivery as a duplicate, leaving thousands of
    /// messages stuck on the queue.
    fn spawn_in_pipeline_reaper(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let in_pipeline = self.in_pipeline.clone();
        let app_index = self.app_message_to_pipeline_key.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Self::IN_PIPELINE_REAPER_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Skip the immediate first tick so we don't reap during startup.
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let now = Instant::now();

                        // Snapshot candidates first (don't mutate while iterating).
                        // Each candidate captures the full context we need to log
                        // — once we yank the entry from the map, this is gone.
                        struct Candidate {
                            pipeline_key: String,
                            app_message_id: String,
                            broker_message_id: Option<String>,
                            queue_identifier: String,
                            pool_code: String,
                            message_group_id: Option<String>,
                            age_secs: u64,
                        }
                        let mut candidates: Vec<Candidate> = Vec::new();
                        for entry in in_pipeline.iter() {
                            let age = now.duration_since(entry.value().started_at);
                            if age > Self::IN_PIPELINE_TTL {
                                candidates.push(Candidate {
                                    pipeline_key: entry.key().clone(),
                                    app_message_id: entry.value().message_id.clone(),
                                    broker_message_id: entry.value().broker_message_id.clone(),
                                    queue_identifier: entry.value().queue_identifier.clone(),
                                    pool_code: entry.value().pool_code.clone(),
                                    message_group_id: entry.value().message_group_id.clone(),
                                    age_secs: age.as_secs(),
                                });
                            }
                        }

                        for c in &candidates {
                            in_pipeline.remove(&c.pipeline_key);
                            app_index.remove(&c.app_message_id);
                            warn!(
                                pipeline_key = %c.pipeline_key,
                                app_message_id = %c.app_message_id,
                                broker_message_id = ?c.broker_message_id,
                                queue = %c.queue_identifier,
                                pool_code = %c.pool_code,
                                message_group_id = ?c.message_group_id,
                                age_secs = c.age_secs,
                                ttl_secs = Self::IN_PIPELINE_TTL.as_secs(),
                                "Reaped stuck in_pipeline entry — SQS redelivery will retry"
                            );
                        }

                        if !candidates.is_empty() {
                            warn!(
                                count = candidates.len(),
                                ttl_secs = Self::IN_PIPELINE_TTL.as_secs(),
                                "in_pipeline reaper cycle: {} entries expired",
                                candidates.len()
                            );
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("In-pipeline reaper shutting down");
                        break;
                    }
                }
            }
        })
    }

    /// Graceful shutdown
    pub async fn shutdown(&self) {
        info!("QueueManager shutting down...");
        self.running.store(false, Ordering::SeqCst);

        // Signal all consumer loops to stop
        let _ = self.shutdown_tx.send(());

        // Stop all consumers
        {
            let consumers = self.consumers.read().await;
            for consumer in consumers.values() {
                consumer.stop().await;
            }
        }

        // Drain all pools
        for entry in self.pools.iter() {
            entry.value().drain().await;
        }

        // Wait for pools to drain with timeout
        let drain_timeout = Duration::from_secs(60);
        let start = Instant::now();

        while !self.all_pools_drained() && start.elapsed() < drain_timeout {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Log any remaining in-flight messages (they'll be NACKed when tasks are dropped)
        let remaining = self.in_pipeline.len();
        if remaining > 0 {
            warn!(remaining = remaining, "Remaining in-flight messages will be NACKed");
            self.in_pipeline.clear();
            self.app_message_to_pipeline_key.clear();
        }

        // Shutdown pools
        for entry in self.pools.iter() {
            entry.value().shutdown().await;
        }

        info!("QueueManager shutdown complete");
    }

    fn all_pools_drained(&self) -> bool {
        self.pools.iter().all(|entry| entry.value().is_fully_drained())
    }

    /// Check if any pool has capacity to accept messages.
    /// Used to gate SQS polling — avoids a hot poll-defer loop when all pools are full.
    fn has_pool_capacity(&self) -> bool {
        self.pools.is_empty() || self.pools.iter().any(|entry| entry.value().available_capacity() > 0)
    }

    /// Get statistics for all pools
    pub fn get_pool_stats(&self) -> Vec<PoolStats> {
        self.pools.iter().map(|entry| entry.value().get_stats()).collect()
    }

    /// Extend visibility for long-running messages
    /// Called periodically by LifecycleManager to prevent visibility timeout
    /// for messages that are still being processed.
    pub async fn extend_visibility_for_long_running(&self) {
        // Extend visibility when message has been processing for 50+ seconds
        // This matches SQS visibility timeout minus a safety buffer
        let threshold_seconds = 50;
        let extension_seconds = 120; // Extend by 120 seconds (matches Java)

        // Collect messages that need visibility extension
        let mut extensions = Vec::new();
        for entry in self.in_pipeline.iter() {
            let value = entry.value();
            if value.elapsed_seconds() >= threshold_seconds {
                extensions.push((
                    value.queue_identifier.clone(),
                    value.receipt_handle.clone(),
                    value.message_id.clone(),
                    value.elapsed_seconds(),
                ));
            }
        }

        if extensions.is_empty() {
            return;
        }

        // Get consumers and extend visibility
        let consumers = self.consumers.read().await;
        for (queue_id, receipt_handle, message_id, elapsed) in extensions {
            if let Some(consumer) = consumers.get(&queue_id) {
                match consumer.extend_visibility(&receipt_handle, extension_seconds).await {
                    Ok(()) => {
                        debug!(
                            message_id = %message_id,
                            queue = %queue_id,
                            elapsed = elapsed,
                            extension = extension_seconds,
                            "Extended visibility for long-running message"
                        );
                    }
                    Err(e) => {
                        warn!(
                            message_id = %message_id,
                            queue = %queue_id,
                            error = %e,
                            "Failed to extend visibility for long-running message"
                        );
                    }
                }
            }
        }
    }

    /// Check for potential memory leaks (large in-pipeline maps)
    pub fn check_memory_health(&self) -> bool {
        let in_pipeline_size = self.in_pipeline.len();
        let threshold = 10000;

        if in_pipeline_size > threshold {
            warn!(
                in_pipeline_size = in_pipeline_size,
                threshold = threshold,
                "Potential memory leak detected - in_pipeline map is large"
            );
            return false;
        }

        true
    }

    /// Reap stale entries from in-memory tracking maps.
    ///
    /// Evicts `in_pipeline` and `app_message_to_pipeline_key` entries older than
    /// `max_age`, which indicates the ACK callback task is stuck or was dropped.
    /// Also evicts `pending_delete_broker_ids` entries older than `pending_delete_max_age`
    /// (messages that were processed but never re-polled for deletion).
    pub fn reap_stale_entries(&self, max_age: Duration, pending_delete_max_age: Duration) -> (usize, usize) {
        // Skip iteration when maps are empty (common case — zero cost)
        if self.in_pipeline.is_empty() && self.pending_delete_broker_ids.lock().is_empty() {
            return (0, 0);
        }

        // Reap stale in_pipeline entries
        let mut reaped_pipeline = 0;
        if !self.in_pipeline.is_empty() {
            let stale_keys: Vec<String> = self.in_pipeline
                .iter()
                .filter(|entry| entry.value().started_at.elapsed() > max_age)
                .map(|entry| entry.key().clone())
                .collect();

            for key in &stale_keys {
                if let Some((_, entry)) = self.in_pipeline.remove(key) {
                    self.app_message_to_pipeline_key.remove(&entry.message_id);
                    reaped_pipeline += 1;
                }
            }

            if reaped_pipeline > 0 {
                warn!(
                    reaped = reaped_pipeline,
                    max_age_seconds = max_age.as_secs(),
                    "Reaped stale in_pipeline entries (likely orphaned by dropped ACK tasks)"
                );
            }
        }

        // Reap stale pending_delete_broker_ids entries
        let reaped_pending = {
            let mut pending = self.pending_delete_broker_ids.lock();
            if pending.is_empty() {
                0
            } else {
                let before = pending.len();
                pending.retain(|_, inserted_at| inserted_at.elapsed() < pending_delete_max_age);
                before - pending.len()
            }
        };

        if reaped_pending > 0 {
            info!(
                reaped = reaped_pending,
                max_age_seconds = pending_delete_max_age.as_secs(),
                "Reaped stale pending_delete_broker_ids entries"
            );
        }

        (reaped_pipeline, reaped_pending)
    }

    // ============================================================================
    // Stall Detection
    // ============================================================================

    /// Detect stalled messages that have been processing beyond the threshold.
    ///
    /// Returns a list of stalled message information for monitoring/alerting.
    pub fn detect_stalled_messages(&self) -> Vec<StalledMessageInfo> {
        if !self.stall_config.enabled {
            return Vec::new();
        }

        let threshold = self.stall_config.stall_threshold_seconds;
        let now = Utc::now();

        self.in_pipeline
            .iter()
            .filter(|entry| entry.value().elapsed_seconds() >= threshold)
            .map(|entry| {
                let msg = entry.value();
                StalledMessageInfo {
                    message_id: msg.message_id.clone(),
                    message_group_id: msg.message_group_id.clone(),
                    pool_code: msg.pool_code.clone(),
                    queue_identifier: msg.queue_identifier.clone(),
                    elapsed_seconds: msg.elapsed_seconds(),
                    detected_at: now,
                }
            })
            .collect()
    }

    /// Check for stalled messages and optionally force-NACK them.
    ///
    /// This method should be called periodically (e.g., every 30 seconds).
    /// It will:
    /// 1. Detect messages that have exceeded the stall threshold
    /// 2. Log warnings for stalled messages
    /// 3. If force_nack_stalled is enabled, NACK messages exceeding the force_nack_after_seconds threshold
    ///
    /// Returns the number of messages that were force-NACKed.
    pub async fn check_and_handle_stalled_messages(&self) -> usize {
        if !self.stall_config.enabled {
            return 0;
        }

        let stalled = self.detect_stalled_messages();
        if stalled.is_empty() {
            return 0;
        }

        // Log warnings for all stalled messages
        for msg in &stalled {
            warn!(
                message_id = %msg.message_id,
                message_group_id = ?msg.message_group_id,
                pool_code = %msg.pool_code,
                queue_identifier = %msg.queue_identifier,
                elapsed_seconds = msg.elapsed_seconds,
                "Stalled message detected - processing time exceeds threshold"
            );
        }

        // If force-NACK is not enabled, just return the count of detected stalls
        if !self.stall_config.force_nack_stalled {
            info!(
                stalled_count = stalled.len(),
                threshold_seconds = self.stall_config.stall_threshold_seconds,
                "Stalled messages detected (force-NACK disabled)"
            );
            return 0;
        }

        // Force-NACK messages that have exceeded the force_nack_after_seconds threshold
        let force_threshold = self.stall_config.force_nack_after_seconds;
        let nack_delay = self.stall_config.nack_delay_seconds;
        let consumers = self.consumers.read().await;
        let mut force_nacked = 0;

        for msg in &stalled {
            if msg.elapsed_seconds >= force_threshold {
                // Get the in-flight message to get the receipt handle
                if let Some(in_flight) = self.in_pipeline.get(&msg.message_id) {
                    let receipt_handle = in_flight.receipt_handle.clone();
                    let queue_id = in_flight.queue_identifier.clone();
                    drop(in_flight); // Release the lock before async call

                    if let Some(consumer) = consumers.get(&queue_id) {
                        warn!(
                            message_id = %msg.message_id,
                            elapsed_seconds = msg.elapsed_seconds,
                            force_threshold_seconds = force_threshold,
                            "Force-NACKing stalled message"
                        );

                        if let Err(e) = consumer.nack(&receipt_handle, Some(nack_delay)).await {
                            error!(
                                message_id = %msg.message_id,
                                error = %e,
                                "Failed to force-NACK stalled message"
                            );
                        } else {
                            // Remove from pipeline since we've force-NACKed
                            self.in_pipeline.remove(&msg.message_id);
                            self.app_message_to_pipeline_key.remove(&msg.message_id);
                            force_nacked += 1;
                        }
                    }
                }
            }
        }

        if force_nacked > 0 {
            info!(
                force_nacked = force_nacked,
                total_stalled = stalled.len(),
                "Force-NACKed stalled messages"
            );
        }

        force_nacked
    }

    /// Get stall detection configuration
    pub fn stall_config(&self) -> &StallConfig {
        &self.stall_config
    }

    /// Update stall detection configuration at runtime
    pub fn update_stall_config(&mut self, config: StallConfig) {
        info!(
            enabled = config.enabled,
            stall_threshold_seconds = config.stall_threshold_seconds,
            force_nack_stalled = config.force_nack_stalled,
            force_nack_after_seconds = config.force_nack_after_seconds,
            "Updating stall detection configuration"
        );
        self.stall_config = config;
    }

    /// Update pool configuration at runtime (hot-reload)
    /// Note: Concurrency changes take effect on next message batch
    /// Rate limit changes take effect immediately
    pub async fn update_pool_config(&self, pool_code: &str, config: PoolConfig) -> Result<()> {
        // Check if pool exists and get current settings
        // IMPORTANT: Drop the Ref guard before calling insert() to avoid deadlock
        let pool_exists = if let Some(existing_pool) = self.pools.get(pool_code) {
            let current_concurrency = existing_pool.concurrency();
            let new_concurrency = config.concurrency;

            if current_concurrency != new_concurrency {
                info!(
                    pool_code = %pool_code,
                    old_concurrency = current_concurrency,
                    new_concurrency = new_concurrency,
                    "Pool concurrency update requested - will take effect after pool restart"
                );
            }

            let current_rate_limit = existing_pool.rate_limit_per_minute();
            let new_rate_limit = config.rate_limit_per_minute;

            if current_rate_limit != new_rate_limit {
                info!(
                    pool_code = %pool_code,
                    old_rate_limit = ?current_rate_limit,
                    new_rate_limit = ?new_rate_limit,
                    "Pool rate limit update requested - creating new pool"
                );
            }
            true
        } else {
            false
        };
        // Ref guard is now dropped

        if pool_exists {
            // For now, we recreate the pool with new config
            // In production, you might want to drain first
            let new_pool = ProcessPool::new(config.clone(), self.build_mediator());
            let pool_arc = Arc::new(new_pool);
            pool_arc.start().await;

            // Replace the old pool
            self.pools.insert(pool_code.to_string(), pool_arc);

            info!(
                pool_code = %pool_code,
                concurrency = config.concurrency,
                rate_limit = ?config.rate_limit_per_minute,
                "Pool configuration updated"
            );

            Ok(())
        } else {
            // Pool doesn't exist, create it
            self.get_or_create_pool(pool_code, Some(config)).await?;
            Ok(())
        }
    }

    /// Get list of all pool codes
    pub fn pool_codes(&self) -> Vec<String> {
        self.pools.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Get list of all consumer identifiers
    pub async fn consumer_ids(&self) -> Vec<String> {
        self.consumers.read().await.keys().cloned().collect()
    }

    /// Check broker connectivity by verifying all consumers report healthy.
    /// Java: BrokerHealthService.checkBrokerConnectivity() pings the broker (SQS listQueues,
    /// NATS connection state, ActiveMQ test connection). Returns false if any consumer
    /// reports unhealthy, indicating the broker is unreachable.
    pub async fn check_broker_connectivity(&self) -> bool {
        let consumers = self.consumers.read().await;
        if consumers.is_empty() {
            return true; // No consumers configured — nothing to check
        }
        for consumer in consumers.values() {
            if !consumer.is_healthy() {
                warn!(
                    consumer = %consumer.identifier(),
                    "Broker connectivity check failed: consumer unhealthy"
                );
                return false;
            }
        }
        true
    }

    /// Restart a specific consumer by ID
    /// Returns true if consumer was found and restart was initiated
    pub async fn restart_consumer(&self, consumer_id: &str) -> bool {
        let consumers = self.consumers.read().await;
        if let Some(consumer) = consumers.get(consumer_id) {
            info!(consumer_id = %consumer_id, "Restarting consumer");

            // Stop the consumer first
            consumer.stop().await;

            // The consumer loop will detect the stop and exit
            // A new poll loop will need to be started externally
            // This is a signal that the consumer needs attention
            true
        } else {
            warn!(consumer_id = %consumer_id, "Consumer not found for restart");
            false
        }
    }

    /// Check if a consumer is healthy
    pub async fn is_consumer_healthy(&self, consumer_id: &str) -> bool {
        let consumers = self.consumers.read().await;
        consumers.get(consumer_id)
            .map(|c| c.is_healthy())
            .unwrap_or(false)
    }

    /// Get queue metrics from all consumers
    pub async fn get_queue_metrics(&self) -> Vec<QueueMetrics> {
        let consumers = self.consumers.read().await;
        let mut metrics = Vec::with_capacity(consumers.len());

        for (id, consumer) in consumers.iter() {
            match consumer.get_metrics().await {
                Ok(Some(m)) => metrics.push(m),
                Ok(None) => {
                    debug!(consumer_id = %id, "Consumer does not support metrics");
                }
                Err(e) => {
                    warn!(consumer_id = %id, error = %e, "Failed to get queue metrics");
                }
            }
        }

        metrics
    }

    /// Get counter metrics only (no SQS API call — instant atomic reads)
    pub async fn get_queue_metrics_counters_only(&self) -> Vec<QueueMetrics> {
        let consumers = self.consumers.read().await;
        let mut metrics = Vec::with_capacity(consumers.len());

        for (_id, consumer) in consumers.iter() {
            if let Some(m) = consumer.get_counters() {
                metrics.push(m);
            }
        }

        metrics
    }

    /// Get in-flight messages (currently being processed)
    /// Returns messages sorted by elapsed time (oldest first)
    pub fn get_in_flight_messages(&self, limit: usize, message_id_filter: Option<&str>, pool_code_filter: Option<&str>) -> Vec<InFlightMessageInfo> {
        let mut messages: Vec<InFlightMessageInfo> = self.in_pipeline
            .iter()
            .filter(|entry| {
                let msg = entry.value();
                // Message ID filter: substring match, case-insensitive (matches Java)
                if let Some(filter) = message_id_filter {
                    if !msg.message_id.to_lowercase().contains(&filter.to_lowercase()) {
                        return false;
                    }
                }
                // Pool code filter: exact match, case-insensitive (matches Java)
                if let Some(filter) = pool_code_filter {
                    if !msg.pool_code.eq_ignore_ascii_case(filter) {
                        return false;
                    }
                }
                true
            })
            .map(|entry| {
                let msg = entry.value();
                InFlightMessageInfo {
                    message_id: msg.message_id.clone(),
                    broker_message_id: msg.broker_message_id.clone(),
                    queue_id: msg.queue_identifier.clone(),
                    pool_code: msg.pool_code.clone(),
                    elapsed_time_ms: msg.started_at.elapsed().as_millis() as u64,
                    added_to_in_pipeline_at: chrono::Utc::now() - chrono::Duration::milliseconds(msg.started_at.elapsed().as_millis() as i64),
                }
            })
            .collect();

        // Sort by elapsed time descending (oldest first)
        messages.sort_by(|a, b| b.elapsed_time_ms.cmp(&a.elapsed_time_ms));

        // Apply limit
        messages.truncate(limit);
        messages
    }

    /// Get count of in-flight messages
    pub fn in_flight_count(&self) -> usize {
        self.in_pipeline.len()
    }
}

/// Result of filtering duplicates from a message batch
struct FilteredBatch {
    /// Messages that are new and should be processed
    unique: Vec<QueuedMessage>,
    /// Messages already in pipeline (redelivery due to visibility timeout) - NACK these
    duplicates: Vec<DuplicateMessage>,
    /// Messages requeued externally while original still processing - ACK these
    requeued: Vec<DuplicateMessage>,
}

/// A duplicate message with its existing pipeline key
struct DuplicateMessage {
    message: QueuedMessage,
    /// The pipeline key of the original message being processed
    existing_pipeline_key: String,
}

/// Information about an in-flight message for API response
#[derive(Debug, Clone, serde::Serialize, ToSchema)]
pub struct InFlightMessageInfo {
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(rename = "brokerMessageId")]
    pub broker_message_id: Option<String>,
    #[serde(rename = "queueId")]
    pub queue_id: String,
    #[serde(rename = "poolCode")]
    pub pool_code: String,
    #[serde(rename = "elapsedTimeMs")]
    pub elapsed_time_ms: u64,
    #[serde(rename = "addedToInPipelineAt")]
    pub added_to_in_pipeline_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod callback_drop_tests {
    use super::*;
    use async_trait::async_trait;
    use fc_common::{Message, QueuedMessage};
    use fc_queue::Result as QueueResult;
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

    /// Records ack/nack calls for assertions in unit tests.
    #[derive(Default)]
    struct RecordingConsumer {
        acks: AtomicU32,
        nacks: AtomicU32,
    }

    #[async_trait]
    impl QueueConsumer for RecordingConsumer {
        fn identifier(&self) -> &str { "recording" }
        async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> { Ok(vec![]) }
        async fn ack(&self, _: &str) -> QueueResult<()> {
            self.acks.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
        async fn nack(&self, _: &str, _: Option<u32>) -> QueueResult<()> {
            self.nacks.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
        async fn extend_visibility(&self, _: &str, _: u32) -> QueueResult<()> { Ok(()) }
        fn is_healthy(&self) -> bool { true }
        async fn stop(&self) {}
    }

    fn build_callback(
        consumer: Arc<RecordingConsumer>,
    ) -> (
        QueueMessageCallback,
        Arc<DashMap<String, InFlightMessage>>,
        Arc<DashMap<String, String>>,
    ) {
        let in_pipeline: Arc<DashMap<String, InFlightMessage>> = Arc::new(DashMap::new());
        let app_index: Arc<DashMap<String, String>> = Arc::new(DashMap::new());
        let pending_delete = Arc::new(Mutex::new(HashMap::new()));

        let pipeline_key = "broker-msg-1".to_string();
        let app_message_id = "app-msg-1".to_string();

        // Simulate the manager pre-populating tracking maps before submit().
        let msg = Message {
            id: app_message_id.clone(),
            pool_code: String::new(),
            auth_token: None,
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: "http://localhost".to_string(),
            message_group_id: None,
            high_priority: false,
            dispatch_mode: fc_common::DispatchMode::Immediate,
        };
        let in_flight = InFlightMessage::new(
            &msg,
            Some(pipeline_key.clone()),
            "queue-id".to_string(),
            None,
            "receipt-handle-xyz".to_string(),
        );
        in_pipeline.insert(pipeline_key.clone(), in_flight);
        app_index.insert(app_message_id.clone(), pipeline_key.clone());

        let cb = QueueMessageCallback {
            pipeline_key,
            app_message_id,
            consumer: consumer as Arc<dyn QueueConsumer + Send + Sync>,
            in_pipeline: in_pipeline.clone(),
            app_message_to_pipeline_key: app_index.clone(),
            pending_delete,
            completed: std::sync::atomic::AtomicBool::new(false),
        };
        (cb, in_pipeline, app_index)
    }

    #[tokio::test]
    async fn drop_without_resolution_clears_tracking_and_nacks() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (cb, in_pipeline, app_index) = build_callback(consumer.clone());
        assert_eq!(in_pipeline.len(), 1);
        assert_eq!(app_index.len(), 1);

        // Drop without ack/nack — simulates panic / cancellation /
        // abandoned PoolTask.
        drop(cb);

        // Tracking maps cleared synchronously inside Drop.
        assert_eq!(in_pipeline.len(), 0, "in_pipeline should be cleared on drop");
        assert_eq!(app_index.len(), 0, "app index should be cleared on drop");

        // Fallback nack is fired via tokio::spawn — yield to let it run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(consumer.nacks.load(AtomicOrdering::SeqCst), 1, "fallback nack should have fired");
        assert_eq!(consumer.acks.load(AtomicOrdering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ack_then_drop_does_not_fire_fallback_nack() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (cb, in_pipeline, _app_index) = build_callback(consumer.clone());

        cb.ack().await;
        assert_eq!(in_pipeline.len(), 0);
        assert_eq!(consumer.acks.load(AtomicOrdering::SeqCst), 1);

        // Drop happens implicitly here — should NOT fire a nack.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(consumer.nacks.load(AtomicOrdering::SeqCst), 0, "no fallback nack after explicit ack");
    }

    #[tokio::test]
    async fn nack_then_drop_does_not_fire_fallback_nack() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (cb, in_pipeline, _app_index) = build_callback(consumer.clone());

        cb.nack(Some(15)).await;
        assert_eq!(in_pipeline.len(), 0);
        assert_eq!(consumer.nacks.load(AtomicOrdering::SeqCst), 1);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Total should still be 1 — Drop did not add a second nack.
        assert_eq!(consumer.nacks.load(AtomicOrdering::SeqCst), 1, "no double-nack on drop after explicit nack");
    }
}
