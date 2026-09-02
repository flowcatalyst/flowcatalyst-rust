//! QueueManager - Central orchestrator for message routing
//!
//! Mirrors the Java QueueManager with:
//! - In-pipeline message tracking for deduplication
//! - Batch message routing with policies
//! - Pool management and lifecycle
//! - Consumer health monitoring

use dashmap::DashMap;
use futures::future;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use chrono::Utc;
use fc_common::{
    BatchMessage, InFlightMessage, MessageCallback, PoolConfig, PoolStats, QueuedMessage,
    RouterConfig, StallConfig, StalledMessageInfo, WarningCategory, WarningSeverity,
};
use fc_queue::{QueueConsumer, QueueMetrics};
use utoipa::ToSchema;

use crate::circuit_breaker_registry::CircuitBreakerRegistry;
use crate::error::RouterError;
use crate::mediator::{HttpMediator, HttpMediatorConfig, Mediator};
use crate::pool::ProcessPool;
use crate::warning::WarningService;
use crate::Result;

/// Builds a mediator for each new pool, given the manager's current warning
/// service (read at pool-creation time, *after* wiring — so the real service
/// reaches the mediator, not the noop default).
///
/// Production path captures an `HttpMediatorConfig` and builds a **fresh**
/// `HttpMediator` per call, so each pool gets its own reqwest `Client` /
/// connection pool — transport isolation that sidesteps AWS's 128-stream cap
/// on a single HTTP/2 connection. Test path captures a shared mock and returns
/// the same instance every call. This replaces the old `MediatorSource` enum
/// with the boxed-factory idiom already used for the per-host client builder
/// in `http_pool.rs`.
type MediatorFactory =
    Arc<dyn Fn(&Arc<WarningService>) -> Arc<dyn Mediator + 'static> + Send + Sync>;

/// `(queue_id, consumer)` pair — used by `sync_queue_consumers` to shuttle
/// consumers created/removed outside the `consumers` map's lock.
type ConsumerEntry = (String, Arc<dyn QueueConsumer + Send + Sync>);

/// `(queue_id, consumer, queue_config)` triple — the "just created, not yet
/// inserted" shape `sync_queue_consumers` collects before its brief insert
/// write-lock.
type NewConsumerEntry = (
    String,
    Arc<dyn QueueConsumer + Send + Sync>,
    fc_common::QueueConfig,
);

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
        self.app_message_to_pipeline_key
            .remove(&self.app_message_id);
    }
}

#[async_trait::async_trait]
impl MessageCallback for QueueMessageCallback {
    async fn ack(&self) {
        // Mark resolved BEFORE doing any await so the Drop impl knows we
        // owned the resolution even if a panic happens mid-await.
        self.completed
            .store(true, std::sync::atomic::Ordering::Release);

        // Read latest receipt handle (may have been updated by redelivery)
        let (handle, broker_id) = self
            .in_pipeline
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
                    self.pending_delete
                        .lock()
                        .insert(bid.clone(), Instant::now());
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
        self.completed
            .store(true, std::sync::atomic::Ordering::Release);

        let handle = self
            .in_pipeline
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
        let handle = self
            .in_pipeline
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

    /// Active process pools by code. Routing reads this map directly; a pool
    /// removed during a config reload is *moved* out of here into
    /// `draining_pools`, so this never contains a draining pool and the hot
    /// routing path needs no status filter.
    pools: DashMap<String, Arc<ProcessPool>>,

    /// Pools removed from config, kept alive until their in-flight work
    /// finishes. Read by `cleanup_draining_pools`, which drops them once
    /// `is_fully_drained()`. (Consumers don't need an equivalent map: a phased-
    /// out consumer is stopped via `stop()` and its still-running poll task
    /// owns the only `Arc` it needs to finish in flight — there's nothing for
    /// the manager to hold or periodically clean up.)
    draining_pools: DashMap<String, Arc<ProcessPool>>,

    /// Queue consumers (RwLock for async-safe access)
    consumers: RwLock<HashMap<String, Arc<dyn QueueConsumer + Send + Sync>>>,

    /// Current pool configurations (for detecting changes).
    ///
    /// Beyond storing per-pool config for diffing, this lock doubles as the
    /// reload-serialisation lock: `apply_config`/`reload_config` hold
    /// `pool_configs.write()` for the whole of their body, including the
    /// up-to-60s wait inside `ProcessPool::update_concurrency` when a
    /// pool's concurrency is decreased. That single write-held-for-the-
    /// whole-call is what prevents two concurrent reloads from
    /// interleaving — no hot-path reader (routing, monitoring, health
    /// checks) ever takes this lock, only the two config-mutation entry
    /// points do, so a slow in-flight reload blocks only a second
    /// concurrent reload, never message routing or stats reads.
    pool_configs: RwLock<HashMap<String, PoolConfig>>,

    /// Current queue configurations (for detecting changes during sync)
    queue_configs: RwLock<HashMap<String, fc_common::QueueConfig>>,

    /// Consumer factory for creating new queue consumers during config sync
    /// If None, new queues in config will be logged but not auto-created
    consumer_factory: Option<Arc<dyn ConsumerFactory + Send + Sync>>,

    /// How to build a mediator for each new pool. See [`MediatorFactory`].
    mediator_factory: MediatorFactory,

    /// Default pool code for messages without explicit pool
    default_pool_code: String,

    /// Running state
    running: AtomicBool,

    /// Shutdown signal. Level-triggered, unlike the `broadcast` channel this
    /// replaced: `shutdown()` calls `self.shutdown.cancel()`, which
    /// immediately marks every child token — existing or future — as
    /// cancelled. A consumer poll task hot-added (via `sync_queue_consumers`)
    /// *after* shutdown began still observes the cancellation instantly on
    /// its very first `token.cancelled()` poll, instead of missing a signal
    /// it subscribed too late to see.
    shutdown: CancellationToken,

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

    /// Shared per-endpoint circuit breaker registry.
    ///
    /// One instance is shared across every pool this manager creates, so a
    /// breaker that trips for an endpoint protects *all* pools targeting it
    /// (mirrors Java's single `circuitBreakers` passed to every `ProcessPool`).
    /// The same instance is what the monitoring API reads (`get_all_stats`)
    /// and what operator `reset`/`reset_all` act on, and what the lifecycle
    /// idle-eviction task prunes — expose it via [`Self::circuit_breaker_registry`]
    /// so binaries wire one registry everywhere instead of three disconnected
    /// `CircuitBreakerRegistry::default()` instances.
    circuit_breaker_registry: Arc<CircuitBreakerRegistry>,

    /// Health service for recording consumer poll times
    health_service: Option<Arc<crate::health::HealthService>>,

    /// R-13/R-16: `FC_ROUTER_STRICT_ROUTING`. When `true`, `route_batch`
    /// ACKs (never delivers, never NACKs) a message with an empty
    /// `pool_code`, an unspecified `dispatch_mode`, or an ordered mode with
    /// no `message_group_id`, instead of silently falling back
    /// (`DEFAULT-POOL` / the A-09 dispatch-mode default / a shared ordered
    /// group). Off by default — see [`Self::set_strict_routing`].
    strict_routing: AtomicBool,

    /// R-26/R-33/R-34: whether this instance currently holds leadership.
    /// Always `true` when standby is disabled (see the builder default).
    /// When standby is enabled, [`crate::standby::spawn_leadership_monitor`]
    /// keeps this in sync with the election result every tick; the consumer
    /// poll loop (`spawn_consumer_poll_task`) reads it to pause/resume
    /// intake, and the config-reload handler reads it to refuse a reload on
    /// a non-leader instance (R-33). Losing leadership never cancels
    /// in-flight or buffered work — it only stops *new* polling — so no
    /// other state needs to change on a transition.
    is_leader: AtomicBool,
}

/// Builder for [`QueueManager`]. Produces a fully-wired, immutable manager —
/// preferred over `new` + a sequence of `set_*` calls (two-phase mutation).
///
/// Because the warning service and circuit breaker registry are fixed before
/// `build`, every pool the manager later creates is guaranteed to share them:
/// there is no window in which a pool could be created against the noop warning
/// service or a private breaker registry. All knobs default to the same values
/// the legacy constructors used, so `QueueManager::builder(cfg).build()` is
/// byte-for-byte equivalent to the old `QueueManager::new(cfg)`.
pub struct QueueManagerBuilder {
    mediator_factory: MediatorFactory,
    warning_service: Arc<WarningService>,
    circuit_breaker_registry: Arc<CircuitBreakerRegistry>,
    health_service: Option<Arc<crate::health::HealthService>>,
    consumer_factory: Option<Arc<dyn ConsumerFactory + Send + Sync>>,
    max_pools: usize,
    pool_warning_threshold: usize,
    stall_config: StallConfig,
}

impl QueueManagerBuilder {
    fn from_factory(mediator_factory: MediatorFactory) -> Self {
        Self {
            mediator_factory,
            warning_service: Arc::new(WarningService::noop()),
            circuit_breaker_registry: Arc::new(CircuitBreakerRegistry::default()),
            health_service: None,
            consumer_factory: None,
            // Java defaults: max-pools = 10000, pool-warning-threshold = 5000
            max_pools: 10000,
            pool_warning_threshold: 5000,
            stall_config: StallConfig::default(),
        }
    }

    /// Warning service shared by the manager, its pools, and the per-pool
    /// mediators. Defaults to a noop sink.
    pub fn warning_service(mut self, warning_service: Arc<WarningService>) -> Self {
        self.warning_service = warning_service;
        self
    }

    /// Shared per-endpoint circuit breaker registry (see
    /// [`QueueManager::circuit_breaker_registry`]). Defaults to a fresh one.
    pub fn circuit_breaker_registry(mut self, registry: Arc<CircuitBreakerRegistry>) -> Self {
        self.circuit_breaker_registry = registry;
        self
    }

    /// Health service for recording consumer poll times.
    pub fn health_service(mut self, health_service: Arc<crate::health::HealthService>) -> Self {
        self.health_service = Some(health_service);
        self
    }

    /// Consumer factory for hot-creating queues during config sync.
    pub fn consumer_factory(mut self, factory: Arc<dyn ConsumerFactory + Send + Sync>) -> Self {
        self.consumer_factory = Some(factory);
        self
    }

    /// Maximum number of pools allowed (Java default: 10000).
    pub fn max_pools(mut self, max_pools: usize) -> Self {
        self.max_pools = max_pools;
        self
    }

    /// Pool-count warning threshold (Java default: 5000).
    pub fn pool_warning_threshold(mut self, threshold: usize) -> Self {
        self.pool_warning_threshold = threshold;
        self
    }

    /// Stall-detection configuration.
    pub fn stall_config(mut self, stall_config: StallConfig) -> Self {
        self.stall_config = stall_config;
        self
    }

    /// Finalise into an immutable, fully-wired [`QueueManager`]. This is the
    /// single struct-literal that all constructors funnel through.
    pub fn build(self) -> QueueManager {
        let shutdown = CancellationToken::new();

        QueueManager {
            in_pipeline: Arc::new(DashMap::new()),
            app_message_to_pipeline_key: Arc::new(DashMap::new()),
            pools: DashMap::new(),
            draining_pools: DashMap::new(),
            consumers: RwLock::new(HashMap::new()),
            pool_configs: RwLock::new(HashMap::new()),
            queue_configs: RwLock::new(HashMap::new()),
            consumer_factory: self.consumer_factory,
            mediator_factory: self.mediator_factory,
            default_pool_code: "DEFAULT-POOL".to_string(), // Java: DEFAULT_POOL_CODE
            running: AtomicBool::new(true),
            shutdown,
            batch_counter: std::sync::atomic::AtomicU64::new(0),
            pending_delete_broker_ids: Arc::new(Mutex::new(HashMap::new())),
            max_pools: self.max_pools,
            pool_warning_threshold: self.pool_warning_threshold,
            stall_config: self.stall_config,
            warning_service: self.warning_service,
            circuit_breaker_registry: self.circuit_breaker_registry,
            health_service: self.health_service,
            strict_routing: AtomicBool::new(false),
            is_leader: AtomicBool::new(true),
        }
    }
}

/// Sleep for `d`, but race it against `token`. Returns `true` if the token
/// was cancelled before `d` elapsed (caller should stop looping), `false`
/// if the sleep completed normally. Used for the pacing sleeps in the
/// consumer poll loop (backpressure, empty-poll, partial-batch, and error
/// pauses) so a shutdown that lands mid-pause exits promptly instead of
/// waiting out the rest of the sleep — across many consumers those pauses
/// would otherwise add real seconds to shutdown latency.
async fn sleep_or_cancel(token: &CancellationToken, d: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => false,
        _ = token.cancelled() => true,
    }
}

/// Reports why `msg` is malformed under strict routing
/// (`FC_ROUTER_STRICT_ROUTING`; see [`QueueManager::set_strict_routing`]),
/// or `None` if well-formed. Checked once per message at route time, before
/// pool resolution — every condition here is exactly what non-strict routing
/// papers over with a fallback (`DEFAULT-POOL`, the A-09 dispatch-mode
/// default, or a shared ordered group), so under strict routing none of them
/// may be silently repaired.
fn malformed_routing_reason(msg: &fc_common::Message) -> Option<&'static str> {
    if msg.pool_code.is_empty() {
        return Some("empty pool_code");
    }
    if !msg.dispatch_mode_specified {
        return Some("empty dispatch_mode");
    }
    if msg.dispatch_mode.requires_ordering()
        && msg.message_group_id.as_deref().unwrap_or("").is_empty()
    {
        return Some("ordered dispatch_mode with no message_group_id");
    }
    None
}

impl QueueManager {
    /// Start building a manager that creates a **fresh** `HttpMediator` per
    /// pool (production path). Prefer this builder over `new` + `set_*`.
    pub fn builder(mediator_config: HttpMediatorConfig) -> QueueManagerBuilder {
        let factory: MediatorFactory = Arc::new(move |ws: &Arc<WarningService>| {
            Arc::new(
                HttpMediator::with_config(mediator_config.clone()).with_warning_service(ws.clone()),
            ) as Arc<dyn Mediator + 'static>
        });
        QueueManagerBuilder::from_factory(factory)
    }

    /// Start building a manager where every pool shares one mediator instance
    /// (test seam for injecting mocks / instrumenting mediator calls).
    pub fn builder_with_shared_mediator(
        mediator: Arc<dyn Mediator + 'static>,
    ) -> QueueManagerBuilder {
        let factory: MediatorFactory = Arc::new(move |_ws: &Arc<WarningService>| mediator.clone());
        QueueManagerBuilder::from_factory(factory)
    }

    pub fn new(mediator_config: HttpMediatorConfig) -> Self {
        Self::builder(mediator_config).build()
    }

    pub fn with_limits(
        mediator_config: HttpMediatorConfig,
        max_pools: usize,
        pool_warning_threshold: usize,
    ) -> Self {
        Self::builder(mediator_config)
            .max_pools(max_pools)
            .pool_warning_threshold(pool_warning_threshold)
            .build()
    }

    pub fn with_config(
        mediator_config: HttpMediatorConfig,
        max_pools: usize,
        pool_warning_threshold: usize,
        stall_config: StallConfig,
    ) -> Self {
        Self::builder(mediator_config)
            .max_pools(max_pools)
            .pool_warning_threshold(pool_warning_threshold)
            .stall_config(stall_config)
            .build()
    }

    /// Get the shared circuit breaker registry. This is the instance every
    /// pool records into; wire it into the monitoring API and lifecycle
    /// eviction so they observe/act on the real breaker state.
    pub fn circuit_breaker_registry(&self) -> &Arc<CircuitBreakerRegistry> {
        &self.circuit_breaker_registry
    }

    /// Get warning service reference
    pub fn warning_service(&self) -> &Arc<WarningService> {
        &self.warning_service
    }

    /// Get a child cancellation token that resolves when this manager's
    /// `shutdown()` is called. Binaries wanting to tie their own tasks to
    /// the manager's shutdown lifecycle (rather than build a separate
    /// signal) should hold onto one of these instead of polling `running`.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.child_token()
    }

    /// Toggle `FC_ROUTER_STRICT_ROUTING` (R-13/R-16): when `true`,
    /// `route_batch` ACKs a malformed message (empty `pool_code`, an
    /// unspecified `dispatch_mode`, or an ordered mode with no
    /// `message_group_id`) instead of routing it through a fallback. Off by
    /// default.
    pub fn set_strict_routing(&self, enabled: bool) {
        self.strict_routing.store(enabled, Ordering::SeqCst);
        info!(strict_routing = enabled, "Strict routing gate set");
    }

    /// Current value of the strict-routing gate (see [`Self::set_strict_routing`]).
    pub fn strict_routing(&self) -> bool {
        self.strict_routing.load(Ordering::SeqCst)
    }

    /// Whether this instance currently holds leadership (always `true` when
    /// standby is disabled). See [`Self::set_leader`].
    pub fn is_leader(&self) -> bool {
        self.is_leader.load(Ordering::SeqCst)
    }

    /// R-26/R-34: record this instance's current leadership status. Called
    /// every tick by [`crate::standby::spawn_leadership_monitor`] so the
    /// consumer poll loop and the config-reload handler (R-33) always see a
    /// fresh value. Losing leadership pauses new polling only — in-flight
    /// deliveries and buffered group work are never touched (R-26).
    pub fn set_leader(&self, leader: bool) {
        let was_leader = self.is_leader.swap(leader, Ordering::SeqCst);
        if leader && !was_leader {
            info!("This instance became the LEADER — resuming message consumption");
        } else if !leader && was_leader {
            warn!("This instance lost leadership — pausing message consumption (in-flight work continues)");
        }
    }

    /// Build a mediator instance for a pool via the configured factory,
    /// passing the manager's current warning service so per-pool mediators
    /// emit through the real sink (see [`MediatorFactory`]).
    fn build_mediator(&self) -> Arc<dyn Mediator + 'static> {
        (self.mediator_factory)(&self.warning_service)
    }

    /// Test-only constructor: every pool shares the supplied mediator. Use
    /// this when you need to inject a mock or instrument mediator calls.
    /// Production code should use [`QueueManager::builder`] (or [`new`]) and
    /// let the manager build a mediator per pool.
    #[doc(hidden)]
    pub fn with_shared_mediator_for_testing(mediator: Arc<dyn Mediator + 'static>) -> Self {
        Self::builder_with_shared_mediator(mediator).build()
    }

    /// Add a queue consumer
    pub async fn add_consumer(&self, consumer: Arc<dyn QueueConsumer + Send + Sync>) {
        let id = consumer.identifier().to_string();
        self.consumers.write().await.insert(id, consumer);
    }

    /// Apply router configuration (initial setup).
    ///
    /// Takes `self: &Arc<Self>` so `sync_queue_consumers` can spawn poll
    /// tasks for hot-added consumers. Callers already hold the manager
    /// behind an Arc.
    pub async fn apply_config(self: &Arc<Self>, config: RouterConfig) -> Result<()> {
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
    ///
    /// X-11 (verified, no change needed): a pool present in both the old and
    /// new config with only `concurrency`/`rate_limit_per_minute` changed is
    /// updated in place via `Pool::update_concurrency`/`update_rate_limit`
    /// (below) — it is never removed-and-recreated for a parameter-only
    /// change. A pool is only ever torn down when its code drops out of the
    /// new config entirely (the "removed pools" branch).
    pub async fn reload_config(self: &Arc<Self>, config: RouterConfig) -> Result<bool> {
        if !self.running.load(Ordering::SeqCst) {
            warn!("Cannot reload config - QueueManager is shutting down");
            return Ok(false);
        }

        info!("Hot reloading configuration...");

        // Build map of new pool configs
        let new_pool_configs: HashMap<String, PoolConfig> = config
            .processing_pools
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
                    let rate_limit_changed =
                        old_config.rate_limit_per_minute != new_config.rate_limit_per_minute;

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
                    self.draining_pools.insert(code.clone(), pool.clone());
                    pool_configs.remove(&code);
                    pools_removed += 1;

                    // Watcher: removes the pool from `draining_pools` the
                    // moment its in-flight work finishes, instead of
                    // waiting for the next `cleanup_draining_pools` sweep
                    // (only run periodically by the lifecycle manager's
                    // reaper — see that fn's doc comment, which is now the
                    // backstop rather than the primary path).
                    //
                    // **Owns:** an `Arc<QueueManager>` clone (`self`), the
                    // drained `Arc<ProcessPool>`, its code, and a child
                    // cancellation token.
                    // **Exits:** as soon as `pool.wait_drained()` resolves
                    // (removes itself from `draining_pools` and calls
                    // `pool.shutdown()`), or immediately if the manager's
                    // shutdown token is cancelled first — `shutdown()`
                    // already drains every pool in `draining_pools` itself,
                    // so this task backs off rather than double-acting.
                    // **Joined by:** nobody — self-terminating,
                    // fire-and-forget, matching every other background task
                    // in this file.
                    let manager = self.clone();
                    let watched_pool = pool;
                    let watched_code = code.clone();
                    let token = self.shutdown.child_token();
                    tokio::spawn(async move {
                        tokio::select! {
                            _ = watched_pool.wait_drained() => {
                                watched_pool.shutdown().await;
                                manager.draining_pools.remove(&watched_code);
                                info!(pool_code = %watched_code, "Draining pool finished - removed");
                            }
                            _ = token.cancelled() => {}
                        }
                    });
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
                        format!(
                            "Max pool limit reached ({}/{}) - cannot create pool [{}]",
                            current_count, self.max_pools, pool_config.code
                        ),
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
                        format!(
                            "Pool count {} approaching limit {} (threshold: {})",
                            current_count, self.max_pools, self.pool_warning_threshold
                        ),
                        "QueueManager".to_string(),
                    );
                }

                // Create new pool
                self.get_or_create_pool(&pool_config.code, Some(pool_config.clone()))
                    .await?;
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

    /// Sync queue consumers based on configuration changes.
    /// Mirrors Java's queue consumer sync logic in syncConfig().
    ///
    /// `consumers`/`queue_configs` are only held write-locked for two brief,
    /// synchronous sections (remove-stale, then insert-new) — never across
    /// an `.await`. `consumer.stop().await` and `factory.create_consumer(..)
    /// .await` both run with the locks released. `reload_config` holds
    /// `pool_configs.write()` for its entire duration (see that field's doc
    /// comment), which is what serialises concurrent reloads/syncs; releasing
    /// `consumers`/`queue_configs` mid-sync here cannot let two syncs
    /// interleave — it only stops this sync from stalling monitoring/health
    /// readers (`get_queue_metrics`, `consumer_ids`, `is_consumer_healthy`)
    /// or from blocking on a slow `stop()`/`create_consumer()` call while
    /// holding a lock nobody else needs mid-sync.
    async fn sync_queue_consumers(
        self: &Arc<Self>,
        config: &RouterConfig,
    ) -> Result<(usize, usize)> {
        // Build map of new queue configs
        let new_queue_configs: HashMap<String, fc_common::QueueConfig> = config
            .queues
            .iter()
            .map(|q| {
                // Use name as identifier, fall back to uri if name is empty
                let identifier = if q.name.is_empty() {
                    q.uri.clone()
                } else {
                    q.name.clone()
                };
                (identifier, q.clone())
            })
            .collect();

        // Step (a): brief write lock — remove entries no longer in the new
        // config, collecting the removed consumers so `stop()` can run
        // after the lock is dropped. Also snapshot the resulting key set
        // so step (c) below can tell "genuinely new" queues apart without
        // holding the lock across `create_consumer().await`.
        let (removed_consumers, existing_ids): (
            Vec<ConsumerEntry>,
            std::collections::HashSet<String>,
        ) = {
            let mut consumers = self.consumers.write().await;
            let mut queue_configs = self.queue_configs.write().await;

            let existing_queues: Vec<String> = consumers.keys().cloned().collect();
            let mut removed = Vec::new();
            for queue_id in &existing_queues {
                if !new_queue_configs.contains_key(queue_id) {
                    if let Some(consumer) = consumers.remove(queue_id) {
                        queue_configs.remove(queue_id);
                        removed.push((queue_id.clone(), consumer));
                    }
                }
            }
            let remaining_ids: std::collections::HashSet<String> =
                consumers.keys().cloned().collect();
            (removed, remaining_ids)
        };

        // Step (b): stop phased-out consumers — outside the lock.
        //
        // X-11 (verified, no `draining_consumers` map needed): removing a
        // consumer from `self.consumers` here does not strand any buffered
        // message's ability to ack/nack. Every `BatchMessage`'s callback
        // (`QueueMessageCallback`, built in `route_batch`) captures its own
        // `Arc<dyn QueueConsumer>` clone at route time, independent of this
        // map — so a message already buffered in a pool when its queue is
        // removed here still holds a live, working consumer handle. And
        // `stop()` (every backend: sqs/postgres/sqlite/nats/activemq) only
        // flips a `running` flag that gates *polling*; `ack`/`nack` never
        // check it. So "stays addressable for ack/nack until buffers empty"
        // already holds via ordinary `Arc` ownership; there is nothing left
        // for the manager to track once this map entry is removed.
        let mut queues_removed = 0;
        for (queue_id, consumer) in removed_consumers {
            info!(queue_id = %queue_id, "Phasing out consumer for removed queue");
            // Stop consumer: sets running=false and initiates graceful
            // shutdown. The consumer's own poll task owns the Arc it needs
            // to finish any in-flight poll, so once we drop our reference
            // here there is nothing further for the manager to track — the
            // task drains and exits on its own.
            consumer.stop().await;
            queues_removed += 1;
            info!(queue_id = %queue_id, "Consumer stopped and removed");
        }

        // Step (c): create consumers for genuinely new queues (if a factory
        // is available) — outside the lock.
        let mut queues_created = 0;
        let mut new_consumers: Vec<NewConsumerEntry> = Vec::new();

        if let Some(ref factory) = self.consumer_factory {
            for (queue_id, queue_config) in &new_queue_configs {
                if !existing_ids.contains(queue_id) {
                    info!(queue_id = %queue_id, "Creating new queue consumer");

                    match factory.create_consumer(queue_config).await {
                        Ok(consumer) => {
                            new_consumers.push((queue_id.clone(), consumer, queue_config.clone()));
                            queues_created += 1;
                            info!(queue_id = %queue_id, "Queue consumer created and ready");
                        }
                        Err(e) => {
                            error!(queue_id = %queue_id, error = %e, "Failed to create queue consumer");
                            self.warning_service.add_warning(
                                WarningCategory::ConsumerHealth,
                                WarningSeverity::Critical,
                                format!(
                                    "Failed to create consumer for queue [{}]: {}",
                                    queue_id, e
                                ),
                                "QueueManager".to_string(),
                            );
                        }
                    }
                }
            }
        } else {
            // No factory - just log new queues that couldn't be created
            for queue_id in new_queue_configs.keys() {
                if !existing_ids.contains(queue_id) {
                    warn!(
                        queue_id = %queue_id,
                        "New queue in config but no consumer factory available - consumer will not be auto-created"
                    );
                }
            }
        }

        // Step (d): brief write lock — insert the newly created consumers
        // and their configs.
        {
            let mut consumers = self.consumers.write().await;
            let mut queue_configs = self.queue_configs.write().await;
            for (queue_id, consumer, queue_config) in &new_consumers {
                consumers.insert(queue_id.clone(), consumer.clone());
                queue_configs.insert(queue_id.clone(), queue_config.clone());
            }
        }

        // Step (e): spawn poll tasks for newly created consumers.
        for (_, consumer, _) in new_consumers {
            info!(consumer_id = %consumer.identifier(), "Spawning poll task for hot-added consumer");
            self.spawn_consumer_poll_task(consumer);
        }

        Ok((queues_created, queues_removed))
    }

    /// Cleanup draining pools that have finished.
    ///
    /// The primary path is now the per-pool watcher task spawned in
    /// `reload_config` when a pool is moved into `draining_pools` — it
    /// removes the pool the instant `wait_drained()` resolves. This method
    /// is a belt-and-braces sweep for anything the watcher missed (e.g. a
    /// pool inserted into `draining_pools` before this feature existed, or
    /// a watcher task that never got scheduled). A double `remove` here is
    /// a harmless no-op, so calling this periodically alongside the watcher
    /// is safe.
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
    async fn get_or_create_pool(
        &self,
        code: &str,
        config: Option<PoolConfig>,
    ) -> Result<Arc<ProcessPool>> {
        if let Some(pool) = self.pools.get(code) {
            return Ok(pool.clone());
        }

        let pool_config = config.unwrap_or_else(|| PoolConfig {
            code: code.to_string(),
            concurrency: 20, // Java: DEFAULT_POOL_CONCURRENCY = 20
            rate_limit_per_minute: None,
        });

        // Share the manager's single registry so breaker state is shared
        // across pools and surfaced to monitoring (not a fresh private default).
        // The per-pool mediator already carries the real warning service via
        // `build_mediator`.
        let pool = ProcessPool::with_dependencies(
            pool_config.clone(),
            self.build_mediator(),
            self.circuit_breaker_registry.clone(),
        );

        let pool_arc = Arc::new(pool);
        pool_arc.start().await;

        self.pools.insert(code.to_string(), pool_arc.clone());
        info!(pool_code = %code, concurrency = pool_config.concurrency, "Created process pool");

        Ok(pool_arc)
    }

    /// Route a batch of messages from a consumer poll
    pub async fn route_batch(
        &self,
        messages: Vec<QueuedMessage>,
        consumer: Arc<dyn QueueConsumer>,
    ) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            // NACK all messages concurrently on shutdown
            let nack_futs: Vec<_> = messages
                .iter()
                .map(|msg| {
                    let consumer = consumer.clone();
                    let handle = msg.receipt_handle.clone();
                    async move {
                        let _ = consumer.nack(&handle, None).await;
                    }
                })
                .collect();
            future::join_all(nack_futs).await;
            return Err(RouterError::ShutdownInProgress);
        }

        if messages.is_empty() {
            return Ok(());
        }

        let batch_id: Arc<str> = Arc::from(
            self.batch_counter
                .fetch_add(1, Ordering::Relaxed)
                .to_string()
                .as_str(),
        );

        // Phase 0: Check for messages that need immediate deletion (previously processed but ACK failed)
        // First, identify which messages need deletion (while holding lock)
        let mut messages_to_delete = Vec::new();
        let mut messages_to_process = Vec::with_capacity(messages.len());
        {
            let mut pending_delete = self.pending_delete_broker_ids.lock();
            for msg in messages {
                let should_delete = msg
                    .broker_message_id
                    .as_ref()
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
            let delete_futs: Vec<_> = messages_to_delete
                .iter()
                .map(|msg| {
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
                })
                .collect();
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

        // Phase 1.5: R-13/R-16 strict routing gate. Only active under
        // FC_ROUTER_STRICT_ROUTING (off by default). A malformed message
        // (empty pool_code, unspecified dispatch_mode, or an ordered mode
        // with no message_group_id) is never fixable by the usual fallback
        // (DEFAULT-POOL, the A-09 default, a shared group) — under strict
        // routing that's a producer bug, not something to paper over. ACK
        // only: it must never be delivered, and never NACKed either, since
        // nothing about a retry would fix a malformed message. `unique`
        // messages here were never registered in `in_pipeline` (that
        // happens later, per-group, just before `pool.submit`), so there is
        // no tracker entry to release.
        let well_formed = if self.strict_routing.load(Ordering::SeqCst) {
            let mut well_formed = Vec::with_capacity(filtered.unique.len());
            let mut malformed_futs = Vec::new();
            for msg in filtered.unique {
                if let Some(reason) = malformed_routing_reason(&msg.message) {
                    warn!(
                        message_id = %msg.message.id,
                        queue = %consumer.identifier(),
                        reason = reason,
                        "Strict routing: malformed message; ACKing without delivery"
                    );
                    self.warning_service.add_warning(
                        WarningCategory::Configuration,
                        WarningSeverity::Warn,
                        format!(
                            "Malformed message {} on queue {}: {}",
                            msg.message.id,
                            consumer.identifier(),
                            reason
                        ),
                        "QueueManager".to_string(),
                    );
                    let consumer = consumer.clone();
                    let handle = msg.receipt_handle.clone();
                    let app_id = msg.message.id.clone();
                    let broker_id = msg.broker_message_id.clone();
                    malformed_futs.push(async move {
                        if let Err(e) = consumer.ack(&handle).await {
                            warn!(
                                message_id = %app_id,
                                broker_message_id = ?broker_id,
                                error = %e,
                                "ack (strict routing malformed) failed"
                            );
                        }
                    });
                } else {
                    well_formed.push(msg);
                }
            }
            future::join_all(malformed_futs).await;
            well_formed
        } else {
            filtered.unique
        };

        // Phase 2: Group by pool and route
        let by_pool = self.group_by_pool(well_formed);

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
                    format!(
                        "Pool [{}] queue full, deferring {} messages from batch",
                        pool_code,
                        pool_messages.len()
                    ),
                    "QueueManager".to_string(),
                );
                // Defer concurrently - capacity limits are not errors
                let defer_futs: Vec<_> = pool_messages
                    .iter()
                    .map(|msg| {
                        let consumer = consumer.clone();
                        let handle = msg.receipt_handle.clone();
                        async move {
                            let _ = consumer.defer(&handle, Some(5)).await;
                        }
                    })
                    .collect();
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
                    let pipeline_key = msg.broker_message_id.clone().unwrap_or_else(|| {
                        format!("fallback:{}:{}", msg.queue_identifier, msg.message.id)
                    });

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
                    self.app_message_to_pipeline_key
                        .insert(app_message_id.clone(), pipeline_key.clone());

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
            if let Some(existing_pipeline_key) =
                self.app_message_to_pipeline_key.get(&msg.message.id)
            {
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
    ///
    /// R-13: an empty `pool_code` warns exactly like an unknown one (this
    /// used to fall silently to DEFAULT-POOL with no warning at all — the
    /// same "papered over" anti-pattern strict routing exists to reject; a
    /// missing pool code is exactly as much a producer bug as a misspelled
    /// one).
    fn group_by_pool(
        &self,
        messages: Vec<QueuedMessage>,
    ) -> std::collections::HashMap<String, Vec<QueuedMessage>> {
        let mut by_pool: std::collections::HashMap<String, Vec<QueuedMessage>> =
            std::collections::HashMap::new();

        for msg in messages {
            let code = &msg.message.pool_code;
            let pool_code = if !code.is_empty() && self.pools.contains_key(code) {
                code.clone()
            } else {
                // Empty or unknown pool_code → log warning + route to DEFAULT-POOL.
                warn!(
                    message_id = %msg.message.id,
                    pool_code = %code,
                    default_pool = %self.default_pool_code,
                    "No pool found for pool_code, routing to DEFAULT-POOL"
                );
                self.warning_service.add_warning(
                    WarningCategory::Routing,
                    WarningSeverity::Warn,
                    format!(
                        "No pool found for code [{}] on message [{}] — routed to {}",
                        code, msg.message.id, self.default_pool_code
                    ),
                    "QueueManager".to_string(),
                );
                self.default_pool_code.clone()
            };

            by_pool.entry(pool_code).or_default().push(msg);
        }

        by_pool
    }

    /// Group messages by message_group_id for FIFO ordering enforcement
    /// (the per-batch NACK-cascade below: if one message in a group fails
    /// `pool.submit`, the rest of the group is NACKed for FIFO). Mirrors
    /// Java's messagesByGroup logic in routeMessageBatch.
    ///
    /// R-13: messages with no real ordered group (IMMEDIATE mode, or an
    /// ordered mode with no `message_group_id`) each get their own unique
    /// pseudo-group keyed by message id, rather than sharing one
    /// `"__DEFAULT__"` bucket. The shared bucket was the exact anti-pattern
    /// R-13 exists to delete: unrelated IMMEDIATE/groupless messages that
    /// happened to land in the same poll batch would NACK-cascade off each
    /// other on a submit failure, even though nothing actually links them.
    /// With strict routing off, this is the "ordered mode with no group id
    /// routes down the IMMEDIATE path" behaviour (Go parity, deliberate).
    fn group_by_message_group(
        &self,
        messages: Vec<QueuedMessage>,
    ) -> indexmap::IndexMap<String, Vec<QueuedMessage>> {
        // Use IndexMap to preserve insertion order (like Java's LinkedHashMap)
        let mut by_group: indexmap::IndexMap<String, Vec<QueuedMessage>> =
            indexmap::IndexMap::new();

        for msg in messages {
            let group_id = match &msg.message.message_group_id {
                Some(g) if !g.is_empty() && msg.message.dispatch_mode.requires_ordering() => {
                    g.clone()
                }
                _ => format!("__ungrouped__:{}", msg.message.id),
            };
            by_group.entry(group_id).or_default().push(msg);
        }

        by_group
    }

    /// Spawn a poll task for a single consumer. Returns the JoinHandle.
    /// Called from both `start()` (initial consumers) and `sync_queue_consumers`
    /// (hot-added consumers).
    ///
    /// **Why `self: &Arc<Self>`**: the spawned task captures
    /// `manager = self.clone()` so it can call back into the manager for
    /// the lifetime of the consumer. That clone needs the receiver to be
    /// an `Arc`, not `&Self`.
    ///
    /// **Shutdown signalling.** `token` is a child of `self.shutdown`
    /// (`CancellationToken`), level-triggered: `QueueManager::shutdown()`
    /// cancelling the parent marks this child cancelled immediately, even
    /// if the token was created (i.e. this task was hot-added via
    /// `sync_queue_consumers`) *after* shutdown had already begun — unlike
    /// the old `broadcast` channel, there is no "subscribed too late to see
    /// the signal" window. Every pacing sleep in the loop below
    /// (backpressure, empty-poll, partial-batch, error) races the token via
    /// [`sleep_or_cancel`] so a shutdown mid-pause exits promptly instead of
    /// waiting out the full sleep. `route_batch` itself is deliberately
    /// **not** raced against cancellation — once a batch is accepted for
    /// processing it must run to completion so messages are acked/nacked
    /// rather than abandoned mid-poll.
    fn spawn_consumer_poll_task(
        self: &Arc<Self>,
        consumer: Arc<dyn QueueConsumer + Send + Sync>,
    ) -> tokio::task::JoinHandle<()> {
        let manager = self.clone();
        let token = self.shutdown.child_token();

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

                // R-26/R-34: not the leader (standby losing/regaining
                // leadership) — pause polling. In-flight deliveries and
                // buffered group work are untouched; this only stops *new*
                // messages from being pulled off the broker. Resumes as soon
                // as `manager.is_leader()` flips back via
                // `spawn_leadership_monitor`, no consumer rebuild needed.
                if !manager.is_leader() {
                    debug!(consumer = %consumer.identifier(), "Not leader — pausing poll");

                    if let Some(ref health_service) = manager.health_service {
                        health_service.record_consumer_poll(consumer.identifier());
                    }

                    if sleep_or_cancel(&token, Duration::from_secs(2)).await {
                        info!(consumer = %consumer.identifier(), "Consumer shutting down");
                        break;
                    }
                    continue;
                }

                // Backpressure: if all pools are full, wait instead of polling.
                // Prevents hot poll-defer loop that wastes SQS API calls.
                if !manager.has_pool_capacity() {
                    debug!(consumer = %consumer.identifier(), "All pools at capacity — pausing poll");

                    // A capacity wait is a deliberate pause, not a stall — record
                    // liveness before pausing so the lifecycle health monitor's
                    // "no poll recorded in 60s" check never misreads a run of
                    // full pools as a dead consumer and kills a perfectly good
                    // one (see `restart_consumer`'s doc comment for the history
                    // here). `record_consumer_poll` only stamps a last-seen
                    // `Instant` — it doesn't feed any poll-count metric — so
                    // calling it on a non-poll iteration doesn't inflate
                    // anything downstream.
                    if let Some(ref health_service) = manager.health_service {
                        health_service.record_consumer_poll(consumer.identifier());
                    }

                    if sleep_or_cancel(&token, Duration::from_secs(2)).await {
                        info!(consumer = %consumer.identifier(), "Consumer shutting down");
                        break;
                    }
                    continue;
                }

                tokio::select! {
                    _ = token.cancelled() => {
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
                                if sleep_or_cancel(&token, Duration::from_secs(1)).await {
                                    info!(consumer = %consumer.identifier(), "Consumer shutting down");
                                    break;
                                }
                            }
                            Ok(messages) => {
                                let count = messages.len();
                                if let Err(e) = manager.route_batch(messages, consumer.clone()).await {
                                    error!(error = %e, "Error routing batch");
                                }
                                // Full batch (10) — re-poll immediately, more messages likely waiting.
                                // Partial batch (< 10) — brief pause, queue is draining.
                                if count < 10
                                    && sleep_or_cancel(&token, Duration::from_millis(500)).await
                                {
                                    info!(consumer = %consumer.identifier(), "Consumer shutting down");
                                    break;
                                }
                            }
                            Err(fc_queue::QueueError::Stopped) => {
                                // The consumer was stopped (directly, or as
                                // part of `restart_consumer` swapping in a
                                // replacement) — `poll()` will keep returning
                                // `Stopped` forever, so looping on it would
                                // spin at 1s intervals reporting a dead
                                // consumer as "just erroring". Exit instead;
                                // whoever stopped this consumer is
                                // responsible for spawning any replacement.
                                info!(consumer = %consumer.identifier(), "consumer stopped — poll task exiting");
                                break;
                            }
                            Err(e) => {
                                error!(error = %e, consumer = %consumer.identifier(), "Error polling");
                                if sleep_or_cancel(&token, Duration::from_secs(1)).await {
                                    info!(consumer = %consumer.identifier(), "Consumer shutting down");
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        })
    }

    /// Start the queue manager and all consumers.
    ///
    /// **Why `self: Arc<Self>`** (owned, not borrowed): the body fans out
    /// to `spawn_consumer_poll_task(&self)` and
    /// `self.clone().spawn_in_pipeline_reaper()`, each of which moves an
    /// Arc clone into a spawned task that outlives this function. Taking
    /// owned `Arc<Self>` means the caller's last reference is consumed
    /// at the call site; the spawned tasks become the new owners.
    pub async fn start(self: Arc<Self>) -> Result<()> {
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
    /// **Why `self: Arc<Self>`** (owned): the spawned reaper task closes
    /// over `in_pipeline` and `app_index` (Arc clones extracted from
    /// `self`) and lives until shutdown — the receiver's Arc is consumed
    /// by the call site and the task becomes the new owner of the
    /// captured references.
    /// **Shutdown signalling.** `token` is a child of `self.shutdown`
    /// (`CancellationToken`), level-triggered: cancellation is observed
    /// immediately by `token.cancelled()` even if this task were somehow
    /// spawned after `shutdown()` had already run — there is no
    /// subscribe-before-signal race like the old `broadcast` channel had.
    fn spawn_in_pipeline_reaper(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let token = self.shutdown.child_token();
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
                    _ = token.cancelled() => {
                        info!("In-pipeline reaper shutting down");
                        break;
                    }
                }
            }
        })
    }

    /// Graceful shutdown.
    ///
    /// Cancels [`Self::shutdown_token`]'s parent (level-triggered — every
    /// consumer poll task and background watcher observes it immediately,
    /// even one spawned after this call started), stops consumers, drains
    /// every pool (active and already-draining), and waits — bounded by a
    /// 60s drain budget — for tracked pool work to finish via
    /// [`ProcessPool::wait_drained`], instead of polling a "drained?" flag
    /// on a fixed sleep interval.
    ///
    /// **R-49 (ruled 2026-09-02):** the intended semantic is narrower than
    /// what this currently does. A worker should finish only the message
    /// it's in the middle of (its in-hand delivery), then immediately
    /// release the rest of its group's *buffered* backlog back to the
    /// broker (NACK, undelivered) rather than continuing to drain it —
    /// draining the whole backlog against a slow target could take
    /// arbitrarily long, past any drain budget, right up to the point the
    /// orchestrator's SIGKILL severs everything mid-flight anyway.
    ///
    /// R-49 (ledger): shutdown finishes the message currently in the air
    /// and RELEASES each group's buffered remainder back to the broker —
    /// it never drains a whole backlog against a slow target, and never
    /// abandons buffered work to visibility-timeout limbo. The sequencing:
    /// `pool.drain()` stops admission, then `pool.release_remainder()`
    /// empties every group buffer with explicit NACKs, so a drain task
    /// mid-loop finds its queue empty after the in-hand task and exits.
    /// The bounded `wait_drained` below therefore only ever waits on
    /// in-hand deliveries, not backlogs.
    pub async fn shutdown(&self) {
        info!("QueueManager shutting down...");
        self.running.store(false, Ordering::SeqCst);

        // Signal all consumer loops / background watchers to stop.
        self.shutdown.cancel();

        // Stop all consumers. Clone the Arcs and drop the read guard before
        // awaiting `stop()` on each — never hold `consumers` across an
        // `.await` (see the field's doc comment / item 4 of the manager
        // shutdown convention).
        let consumers: Vec<Arc<dyn QueueConsumer + Send + Sync>> = {
            let guard = self.consumers.read().await;
            guard.values().cloned().collect()
        };
        for consumer in consumers {
            consumer.stop().await;
        }

        // Collect every pool — active and already-draining — before
        // awaiting anything. DashMap `Ref`s must never be held across an
        // `.await`; collecting the `Arc<ProcessPool>` clones into a `Vec`
        // first and dropping the iterator does that.
        let pools: Vec<Arc<ProcessPool>> = self
            .pools
            .iter()
            .map(|e| e.value().clone())
            .chain(self.draining_pools.iter().map(|e| e.value().clone()))
            .collect();

        // Drain all pools (non-blocking: flips `running`, closes the tracker).
        for pool in &pools {
            pool.drain().await;
        }

        // R-49: release each group's buffered remainder back to the broker
        // (explicit NACKs). After this, the only work left is the in-hand
        // message inside each live drain/immediate task — which is what the
        // bounded wait below is for.
        let mut released_total = 0usize;
        for pool in &pools {
            released_total += pool.release_remainder().await;
        }
        if released_total > 0 {
            info!(
                released = released_total,
                "Shutdown released buffered messages back to the broker"
            );
        }

        // Wait for every pool's tracked tasks to finish, bounded by a timeout.
        let drain_timeout = Duration::from_secs(60);
        let drained = tokio::time::timeout(
            drain_timeout,
            future::join_all(pools.iter().map(|p| p.wait_drained())),
        )
        .await;

        if drained.is_err() {
            let still_busy = pools.iter().filter(|p| p.tracked_tasks() > 0).count();
            warn!(
                still_busy_pools = still_busy,
                total_pools = pools.len(),
                timeout_secs = drain_timeout.as_secs(),
                "Shutdown drain timed out — some pools still had in-flight work"
            );
        }

        // Log any remaining in-flight messages (they'll be NACKed when tasks are dropped)
        let remaining = self.in_pipeline.len();
        if remaining > 0 {
            warn!(
                remaining = remaining,
                "Remaining in-flight messages will be NACKed"
            );
            self.in_pipeline.clear();
            self.app_message_to_pipeline_key.clear();
        }

        // Shutdown pools (idempotent alongside the `drain()` above — same
        // non-blocking flip-and-close semantics; kept for call-site clarity).
        for pool in &pools {
            pool.shutdown().await;
        }

        info!("QueueManager shutdown complete");
    }

    /// Check if any pool has capacity to accept messages.
    /// Used to gate SQS polling — avoids a hot poll-defer loop when all pools are full.
    fn has_pool_capacity(&self) -> bool {
        self.pools.is_empty()
            || self
                .pools
                .iter()
                .any(|entry| entry.value().available_capacity() > 0)
    }

    /// Get statistics for all pools
    pub fn get_pool_stats(&self) -> Vec<PoolStats> {
        self.pools
            .iter()
            .map(|entry| entry.value().get_stats())
            .collect()
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
    pub fn reap_stale_entries(
        &self,
        max_age: Duration,
        pending_delete_max_age: Duration,
    ) -> (usize, usize) {
        // Skip iteration when maps are empty (common case — zero cost)
        if self.in_pipeline.is_empty() && self.pending_delete_broker_ids.lock().is_empty() {
            return (0, 0);
        }

        // Reap stale in_pipeline entries
        let mut reaped_pipeline = 0;
        if !self.in_pipeline.is_empty() {
            let stale_keys: Vec<String> = self
                .in_pipeline
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

        // Snapshot consumers before awaiting any nack — holding the read
        // lock across `consumer.nack(...).await` for every stalled message
        // would stall concurrent reloads/health reads for however long
        // this whole loop takes.
        let consumers: HashMap<String, Arc<dyn QueueConsumer + Send + Sync>> = {
            let guard = self.consumers.read().await;
            guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
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
            // Share the manager's single registry (see get_or_create_pool) — a
            // reconfigured pool must keep recording into the shared breaker, not
            // a fresh private default.
            let new_pool = ProcessPool::with_dependencies(
                config.clone(),
                self.build_mediator(),
                self.circuit_breaker_registry.clone(),
            );
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

    /// Number of pools currently draining (removed from config, still
    /// finishing in-flight work). Useful for stats/tests.
    pub fn draining_pool_count(&self) -> usize {
        self.draining_pools.len()
    }

    /// Non-blocking check of whether a specific pool (active or draining)
    /// has finished draining — every worker/drain task it ever spawned has
    /// exited (see [`ProcessPool::is_fully_drained`]). Returns `None` if no
    /// pool with this code exists in either set.
    pub fn is_pool_fully_drained(&self, code: &str) -> Option<bool> {
        if let Some(pool) = self.pools.get(code) {
            return Some(pool.is_fully_drained());
        }
        if let Some(pool) = self.draining_pools.get(code) {
            return Some(pool.is_fully_drained());
        }
        None
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
        // Clone the Arcs and drop the read guard before iterating — keeps
        // this consistent with every other consumers-read site in the file
        // (see item 4 of the manager shutdown/lock convention), even though
        // `is_healthy()` itself is synchronous today.
        let consumers: Vec<Arc<dyn QueueConsumer + Send + Sync>> = {
            let guard = self.consumers.read().await;
            if guard.is_empty() {
                return true; // No consumers configured — nothing to check
            }
            guard.values().cloned().collect()
        };
        for consumer in consumers {
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

    /// Restart a specific consumer by ID — actually replaces it.
    ///
    /// Stops the existing consumer, asks the configured [`ConsumerFactory`]
    /// to build a fresh one from the queue's last-known `QueueConfig`, swaps
    /// the replacement into `consumers`, and spawns a new poll task for it
    /// (see [`Self::spawn_consumer_poll_task`], which now exits promptly on
    /// `QueueError::Stopped` rather than looping on it forever). Returns
    /// `true` only if a live replacement ends up running.
    ///
    /// **Why `self: &Arc<Self>`**: it calls `spawn_consumer_poll_task`, which
    /// needs an `Arc` clone to hand to the spawned task.
    ///
    /// **No factory / no stored config → no-op, not a stop.** Building a
    /// replacement requires both a [`ConsumerFactory`] and a `QueueConfig`
    /// for this id. If either is missing, this deliberately does **not**
    /// stop the existing consumer — stopping it with nothing to replace it
    /// is exactly the bug this method used to have (the old body called
    /// `consumer.stop()` and returned `true` with a comment saying "a new
    /// poll loop will need to be started externally", which nothing ever
    /// did — the consumer just died in place). Instead it logs a warning,
    /// records a `ConsumerHealth` warning, and returns `false`.
    ///
    /// **Factory failure → self-healing via the next reload.** If
    /// `create_consumer` errors, the dead entry is removed from `consumers`
    /// but its `queue_configs` entry is deliberately left in place. The next
    /// `reload_config` → `sync_queue_consumers` pass computes "new" queues
    /// as config entries not already present in `consumers` (see that
    /// method's step (c)) — since this id is now missing from `consumers`
    /// but still present in the caller's config, it gets recreated through
    /// the ordinary hot-add path instead of being permanently stranded by a
    /// single transient factory failure.
    pub async fn restart_consumer(self: &Arc<Self>, consumer_id: &str) -> bool {
        // Serialise against `apply_config` / `reload_config`, which hold
        // `pool_configs.write()` for their whole duration (see that field's
        // doc comment — it doubles as the reload lock). Held across the
        // awaits below on purpose: without it, a health-triggered restart
        // racing a reload that removes this very queue could stop the old
        // consumer, then swap a fresh one into `consumers` *after* the
        // reload removed it — resurrecting a queue the config just dropped.
        // Nothing on the hot path takes this lock, so the only thing this
        // can wait on is an in-flight reload.
        let _reload_guard = self.pool_configs.read().await;

        // Brief read lock — clone the Arc and drop the guard before any
        // `.await` (same discipline as `sync_queue_consumers`).
        let old = {
            let guard = self.consumers.read().await;
            guard.get(consumer_id).cloned()
        };
        let Some(old) = old else {
            warn!(consumer_id = %consumer_id, "Consumer not found for restart");
            return false;
        };

        // Brief read lock — clone the stored QueueConfig, if any.
        let queue_config = {
            let guard = self.queue_configs.read().await;
            guard.get(consumer_id).cloned()
        };

        let (factory, queue_config) = match (self.consumer_factory.as_ref(), queue_config) {
            (Some(factory), Some(cfg)) => (factory, cfg),
            _ => {
                warn!(
                    consumer_id = %consumer_id,
                    "Cannot restart consumer: no consumer factory and/or stored queue \
                     config available to build a replacement — restart is unsupported \
                     without both, leaving the existing consumer running"
                );
                self.warning_service.add_warning(
                    WarningCategory::ConsumerHealth,
                    WarningSeverity::Warn,
                    format!(
                        "Restart requested for consumer [{}] but no consumer factory/config \
                         is available to build a replacement — restart unsupported here",
                        consumer_id
                    ),
                    "QueueManager".to_string(),
                );
                return false;
            }
        };

        info!(consumer_id = %consumer_id, "Restarting consumer: stopping old instance");
        // Stopping this makes its poll task observe `QueueError::Stopped` on
        // its next poll and exit on its own (see (b) in spawn_consumer_poll_task).
        old.stop().await;

        match factory.create_consumer(&queue_config).await {
            Ok(new_consumer) => {
                // Brief write lock — swap in the replacement.
                {
                    let mut guard = self.consumers.write().await;
                    guard.insert(consumer_id.to_string(), new_consumer.clone());
                }
                self.spawn_consumer_poll_task(new_consumer);
                info!(consumer_id = %consumer_id, "Consumer restarted with a fresh instance");
                true
            }
            Err(e) => {
                error!(
                    consumer_id = %consumer_id,
                    error = %e,
                    "Failed to create replacement consumer during restart"
                );
                self.warning_service.add_warning(
                    WarningCategory::ConsumerHealth,
                    WarningSeverity::Critical,
                    format!(
                        "Failed to create replacement consumer for [{}] during restart: {}",
                        consumer_id, e
                    ),
                    "QueueManager".to_string(),
                );
                // Remove the dead entry from `consumers` but leave
                // `queue_configs` alone — see the self-healing note above.
                let mut guard = self.consumers.write().await;
                guard.remove(consumer_id);
                false
            }
        }
    }

    /// Check if a consumer is healthy
    pub async fn is_consumer_healthy(&self, consumer_id: &str) -> bool {
        let consumers = self.consumers.read().await;
        consumers
            .get(consumer_id)
            .map(|c| c.is_healthy())
            .unwrap_or(false)
    }

    /// Get queue metrics from all consumers
    pub async fn get_queue_metrics(&self) -> Vec<QueueMetrics> {
        // Snapshot before awaiting `get_metrics()` per consumer — this can
        // be an SQS API call, and holding the read lock across it would
        // stall reloads / other readers for however long the whole sweep
        // takes.
        let consumers: Vec<(String, Arc<dyn QueueConsumer + Send + Sync>)> = {
            let guard = self.consumers.read().await;
            guard
                .iter()
                .map(|(id, c)| (id.clone(), c.clone()))
                .collect()
        };
        let mut metrics = Vec::with_capacity(consumers.len());

        for (id, consumer) in consumers {
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

        for consumer in consumers.values() {
            if let Some(m) = consumer.get_counters() {
                metrics.push(m);
            }
        }

        metrics
    }

    /// Get in-flight messages (currently being processed)
    /// Returns messages sorted by elapsed time (oldest first)
    /// Cheap presence check for a single application message ID. O(1).
    pub fn is_in_flight_by_app_id(&self, app_message_id: &str) -> bool {
        match self.app_message_to_pipeline_key.get(app_message_id) {
            Some(e) => self.in_pipeline.contains_key(e.value().as_str()),
            None => false,
        }
    }

    /// Look up a single application message ID in the in-pipeline map.
    ///
    /// Designed for external recovery systems that have a backlog of
    /// messages they suspect are stuck and want to check whether the router
    /// already owns each one before re-enqueueing it. Returns `None` if the
    /// router does not currently hold the message (safe to resend), or a
    /// populated `InFlightMessageInfo` if it does (caller should wait or
    /// skip).
    ///
    /// O(1): goes through `app_message_to_pipeline_key` then `in_pipeline`.
    /// Both are `DashMap`, no global lock.
    pub fn lookup_in_flight_by_app_id(&self, app_message_id: &str) -> Option<InFlightMessageInfo> {
        let pipeline_key = self
            .app_message_to_pipeline_key
            .get(app_message_id)
            .map(|e| e.value().clone())?;
        self.in_pipeline.get(&pipeline_key).map(|entry| {
            let msg = entry.value();
            let elapsed = msg.started_at.elapsed();
            InFlightMessageInfo {
                message_id: msg.message_id.clone(),
                broker_message_id: msg.broker_message_id.clone(),
                queue_id: msg.queue_identifier.clone(),
                pool_code: msg.pool_code.clone(),
                elapsed_time_ms: elapsed.as_millis() as u64,
                added_to_in_pipeline_at: chrono::Utc::now()
                    - chrono::Duration::milliseconds(elapsed.as_millis() as i64),
            }
        })
    }

    pub fn get_in_flight_messages(
        &self,
        limit: usize,
        message_id_filter: Option<&str>,
        pool_code_filter: Option<&str>,
    ) -> Vec<InFlightMessageInfo> {
        let mut messages: Vec<InFlightMessageInfo> = self
            .in_pipeline
            .iter()
            .filter(|entry| {
                let msg = entry.value();
                // Message ID filter: substring match, case-insensitive (matches Java)
                if let Some(filter) = message_id_filter {
                    if !msg
                        .message_id
                        .to_lowercase()
                        .contains(&filter.to_lowercase())
                    {
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
                    added_to_in_pipeline_at: chrono::Utc::now()
                        - chrono::Duration::milliseconds(
                            msg.started_at.elapsed().as_millis() as i64
                        ),
                }
            })
            .collect();

        // Sort by elapsed time descending (oldest first)
        messages.sort_by_key(|m| std::cmp::Reverse(m.elapsed_time_ms));

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
        fn identifier(&self) -> &str {
            "recording"
        }
        async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
            Ok(vec![])
        }
        async fn ack(&self, _: &str) -> QueueResult<()> {
            self.acks.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
        async fn nack(&self, _: &str, _: Option<u32>) -> QueueResult<()> {
            self.nacks.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
        async fn extend_visibility(&self, _: &str, _: u32) -> QueueResult<()> {
            Ok(())
        }
        fn is_healthy(&self) -> bool {
            true
        }
        async fn stop(&self) {}
    }

    // Test helper — tuple return is intentionally ad-hoc; a type alias
    // would only obscure intent for a single call site.
    #[allow(clippy::type_complexity)]
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
            dispatch_mode_specified: true,
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
        assert_eq!(
            in_pipeline.len(),
            0,
            "in_pipeline should be cleared on drop"
        );
        assert_eq!(app_index.len(), 0, "app index should be cleared on drop");

        // Fallback nack is fired via tokio::spawn — yield to let it run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            consumer.nacks.load(AtomicOrdering::SeqCst),
            1,
            "fallback nack should have fired"
        );
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
        assert_eq!(
            consumer.nacks.load(AtomicOrdering::SeqCst),
            0,
            "no fallback nack after explicit ack"
        );
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
        assert_eq!(
            consumer.nacks.load(AtomicOrdering::SeqCst),
            1,
            "no double-nack on drop after explicit nack"
        );
    }

    /// Regression: every pool the manager creates must record into the
    /// manager's single shared circuit breaker registry — not a private
    /// `CircuitBreakerRegistry::default()` per pool. Otherwise a breaker
    /// tripping for an endpoint in one pool wouldn't protect other pools
    /// targeting the same endpoint, and the monitoring API (which reads the
    /// manager's registry) would show empty stats. Mirrors Java's single
    /// `circuitBreakers` shared by every `ProcessPool`.
    #[tokio::test]
    async fn pools_share_managers_circuit_breaker_registry() {
        // PerPool mediator path; no network occurs (we only record breaker
        // failures directly and never mediate a message).
        let manager = QueueManager::new(HttpMediatorConfig::production());

        let pool_a = manager
            .get_or_create_pool("POOL-A", None)
            .await
            .expect("create pool A");
        let pool_b = manager
            .get_or_create_pool("POOL-B", None)
            .await
            .expect("create pool B");

        // Pointer identity: both pools and the manager hold the same Arc.
        assert!(
            Arc::ptr_eq(
                pool_a.circuit_breaker_registry(),
                manager.circuit_breaker_registry()
            ),
            "pool A must share the manager's circuit breaker registry"
        );
        assert!(
            Arc::ptr_eq(
                pool_b.circuit_breaker_registry(),
                manager.circuit_breaker_registry()
            ),
            "pool B must share the manager's circuit breaker registry"
        );

        // Behavioural cross-pool protection: failures recorded while pool A
        // mediates an endpoint trip the breaker, and pool B targeting the same
        // endpoint immediately sees it open.
        let endpoint = "http://shared.example/api";
        for _ in 0..20 {
            pool_a.circuit_breaker_registry().record_failure(endpoint);
        }
        assert_eq!(
            manager.circuit_breaker_registry().get_state(endpoint),
            Some(crate::CircuitBreakerState::Open),
            "failures recorded via a pool must be visible through the manager's registry"
        );
        assert!(
            !pool_b.circuit_breaker_registry().allow_request(endpoint),
            "pool B must observe the breaker opened by pool A's failures"
        );
    }
}

/// R-13/R-16: `FC_ROUTER_STRICT_ROUTING` gate — `malformed_routing_reason`
/// and the private grouping helpers it depends on
/// (`group_by_pool`/`group_by_message_group`) are unit-tested directly here
/// since they're not `pub`.
#[cfg(test)]
mod routing_gate_tests {
    use super::*;
    use fc_common::{DispatchMode, MediationType, Message, QueuedMessage};

    fn msg(
        id: &str,
        pool_code: &str,
        mode: DispatchMode,
        mode_specified: bool,
        group: Option<&str>,
    ) -> Message {
        Message {
            id: id.to_string(),
            pool_code: pool_code.to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://localhost/x".to_string(),
            message_group_id: group.map(|s| s.to_string()),
            high_priority: false,
            dispatch_mode: mode,
            dispatch_mode_specified: mode_specified,
        }
    }

    fn queued(
        id: &str,
        pool_code: &str,
        mode: DispatchMode,
        mode_specified: bool,
        group: Option<&str>,
    ) -> QueuedMessage {
        QueuedMessage {
            message: msg(id, pool_code, mode, mode_specified, group),
            receipt_handle: format!("rh-{id}"),
            broker_message_id: Some(format!("bh-{id}")),
            queue_identifier: "q".to_string(),
        }
    }

    #[test]
    fn malformed_reason_flags_empty_pool_code() {
        let m = msg("1", "", DispatchMode::Immediate, true, None);
        assert_eq!(malformed_routing_reason(&m), Some("empty pool_code"));
    }

    #[test]
    fn malformed_reason_flags_unspecified_dispatch_mode() {
        let m = msg("1", "POOL", DispatchMode::NextOnError, false, None);
        assert_eq!(malformed_routing_reason(&m), Some("empty dispatch_mode"));
    }

    #[test]
    fn malformed_reason_flags_ordered_mode_with_no_group() {
        let m = msg("1", "POOL", DispatchMode::NextOnError, true, None);
        assert_eq!(
            malformed_routing_reason(&m),
            Some("ordered dispatch_mode with no message_group_id")
        );
        let m2 = msg("1", "POOL", DispatchMode::BlockOnError, true, Some(""));
        assert_eq!(
            malformed_routing_reason(&m2),
            Some("ordered dispatch_mode with no message_group_id")
        );
    }

    #[test]
    fn malformed_reason_none_for_well_formed_messages() {
        assert_eq!(
            malformed_routing_reason(&msg("1", "POOL", DispatchMode::Immediate, true, None)),
            None
        );
        assert_eq!(
            malformed_routing_reason(&msg(
                "1",
                "POOL",
                DispatchMode::NextOnError,
                true,
                Some("grp")
            )),
            None
        );
        // pool_code empty check runs first, but a fully well-formed ordered
        // message must not trip on the group check either.
        assert_eq!(
            malformed_routing_reason(&msg(
                "1",
                "POOL",
                DispatchMode::BlockOnError,
                true,
                Some("grp")
            )),
            None
        );
    }

    /// R-13: two messages with no real ordered group (IMMEDIATE, or ordered
    /// with no group id) in the same batch must never land in the same
    /// NACK-cascade bucket — the deleted shared `"__DEFAULT__"` group
    /// anti-pattern would have merged them.
    #[test]
    fn group_by_message_group_never_shares_a_bucket_for_groupless_messages() {
        let manager = QueueManager::new(HttpMediatorConfig::dev());
        let a = queued("a", "POOL", DispatchMode::Immediate, true, None);
        let b = queued("b", "POOL", DispatchMode::NextOnError, true, None);
        let c = queued("c", "POOL", DispatchMode::BlockOnError, true, Some(""));

        let grouped = manager.group_by_message_group(vec![a, b, c]);
        assert_eq!(
            grouped.len(),
            3,
            "each groupless message must get its own bucket, not share one"
        );
    }

    /// A real ordered group (dispatch mode requires ordering + non-empty
    /// group id) is unaffected: messages sharing a real group id still land
    /// in the same bucket, preserving legitimate FIFO NACK-cascade behaviour.
    #[test]
    fn group_by_message_group_keeps_real_ordered_groups_together() {
        let manager = QueueManager::new(HttpMediatorConfig::dev());
        let a = queued("a", "POOL", DispatchMode::NextOnError, true, Some("g1"));
        let b = queued("b", "POOL", DispatchMode::NextOnError, true, Some("g1"));

        let grouped = manager.group_by_message_group(vec![a, b]);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped.get("g1").map(|v| v.len()), Some(2));
    }

    /// R-13: an empty pool_code must warn exactly like an unknown one — it
    /// used to fall silently to DEFAULT-POOL with no warning at all.
    #[test]
    fn group_by_pool_warns_identically_for_empty_and_unknown_pool_code() {
        let manager = QueueManager::new(HttpMediatorConfig::dev());
        let empty = queued("a", "", DispatchMode::Immediate, true, None);
        let unknown = queued("b", "NOPE", DispatchMode::Immediate, true, None);

        let by_pool = manager.group_by_pool(vec![empty, unknown]);
        assert_eq!(by_pool.len(), 1, "both fall back to the same default pool");
        assert_eq!(by_pool.values().next().unwrap().len(), 2);
        assert_eq!(
            manager.warning_service().warning_count(),
            2,
            "both the empty and the unknown pool_code should have warned identically"
        );
    }
}
