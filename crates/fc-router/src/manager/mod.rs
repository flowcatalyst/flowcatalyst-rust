//! QueueManager - Central orchestrator for message routing
//!
//! Mirrors the Java QueueManager with:
//! - In-pipeline message tracking for deduplication
//! - Batch message routing with policies
//! - Pool management and lifecycle
//! - Consumer health monitoring
//!
//! Split the same way `api/mod.rs` splits the HTTP layer: this file stays
//! the assembly point — the [`QueueManager`] struct, its builder, and
//! construction/misc-getter methods — while behaviour lives in sibling
//! files: [`routing`] (`route_batch`, duplicate filtering, pool/group
//! grouping, the strict-routing gate — plus [`QueueMessageCallback`]),
//! [`synth_pools`] (R-59 per-client fallback pool machinery),
//! [`reconcile`] (`apply_config`/`reload_config`/`sync_queue_consumers` and
//! the pool-drain machinery they share), [`shutdown`] (`start`, the
//! in-pipeline reaper, `shutdown`), [`stall`] (stall detection/reporting
//! and the stale-entry reaper), [`snapshots`] (pool stats, the operator
//! dashboard views, force-ack, in-flight lookups), and [`consumers`]
//! (the consumer poll loop and consumer-facing queries/health/restart).
//! Every submodule is a private child of this one — it can reach
//! [`QueueManager`]'s private fields the same way a method defined right
//! here could — and the crate-visible surface (`QueueManager`,
//! `QueueManagerBuilder`, `ConsumerFactory`, and the DTOs returned by the
//! snapshot/monitoring methods) is re-exported from here unchanged.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use dashmap::{DashMap, DashSet};
use fc_common::{PoolConfig, StallConfig};
use fc_queue::QueueConsumer;

use crate::circuit_breaker_registry::CircuitBreakerRegistry;
use crate::mediator::{HttpMediator, HttpMediatorConfig, Mediator};
use crate::pool::ProcessPool;
use crate::warning::WarningService;
use crate::Result;

mod consumers;
mod reconcile;
mod routing;
mod shutdown;
mod snapshots;
mod stall;
mod synth_pools;

pub use snapshots::{ForceAckResult, GroupFlushSnapshot, InFlightMessageInfo};

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
///
/// Also receives the manager's shared `CircuitBreakerRegistry` (ledger:
/// breaker admission/recording is centralised inside the mediator now, not
/// at the pool call site — see `mediator.rs`'s `Mediator::mediate` impl),
/// so the production factory can wire it into every pool's fresh
/// `HttpMediator` via `with_circuit_breakers`.
type MediatorFactory = Arc<
    dyn Fn(&Arc<WarningService>, &Arc<CircuitBreakerRegistry>) -> Arc<dyn Mediator + 'static>
        + Send
        + Sync,
>;

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

/// Which lifecycle phase a tracked pool is in — see [`PoolEntry`] and the
/// `pools` field's doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PoolState {
    /// Serving routed traffic. The hot routing path, capacity checks, and
    /// stats/code listings only ever consider `Active` entries.
    Active,
    /// Removed from config (or evicted as an idle synth pool) but still
    /// finishing buffered/in-flight work. Never routed to; never counted
    /// as "the" pool for its code by `get_pool`/`pool_codes`/routing.
    Draining,
}

/// One tracked pool plus its lifecycle state — the value half of the
/// `pools` map (concurrency-audit consolidation #2; replaces the old
/// `pools`/`draining_pools` DashMap pair with one map + a tagged value, so
/// "is this pool draining?" is a value check instead of an
/// implicit-via-which-map check).
pub(super) struct PoolEntry {
    pub(super) pool: Arc<ProcessPool>,
    pub(super) state: PoolState,
}

/// Central orchestrator for message routing
pub struct QueueManager {
    /// In-pipeline message tracking for deduplication
    /// Wrapped in Arc so spawned tasks can share the same map
    in_pipeline: Arc<DashMap<String, fc_common::InFlightMessage>>,

    /// App message ID to pipeline key mapping for deduplication
    /// Wrapped in Arc so spawned tasks can share the same map
    app_message_to_pipeline_key: Arc<DashMap<String, String>>,

    /// Every pool this manager is tracking, keyed by code, tagged with its
    /// lifecycle state (concurrency-audit consolidation #2: replaces the
    /// old `pools`/`draining_pools` DashMap pair — see
    /// `docs/developers/router-concurrency-audit.md`). Routing
    /// (`route_batch`/`group_by_pool`/`has_pool_capacity`/`get_pool`/
    /// `ensure_fallback_pool`/`get_or_create_pool`) only ever matches an
    /// [`PoolState::Active`] entry — that half of the state machine used to
    /// be "which map is it in", now it's a value check. `all_pools`,
    /// `cleanup_draining_pools`, and `shutdown` traverse every entry
    /// regardless of state (they used to `.chain()` two maps; now it's one
    /// iteration).
    ///
    /// **One entry per code.** A pool code being removed from config and
    /// re-added *before its predecessor finishes draining* can't be
    /// represented as two coexisting entries under one key the way the old
    /// two-map design allowed (draining under the old code in
    /// `draining_pools`, active under the same code in `pools`). Inserting
    /// the new [`PoolState::Active`] entry displaces the map's reference to
    /// the still-draining predecessor — handled, not dropped: every insert
    /// site that can displace a `Draining` entry (`get_or_create_pool`,
    /// `ensure_fallback_pool`) checks `DashMap::insert`'s returned previous
    /// value and, if it was `Draining`, appends the displaced pool to
    /// [`Self::orphaned_draining`] instead of losing the map's only
    /// reference to it. Removal from `pools` itself is identity-checked
    /// (`remove_if` matching both `Draining` state and pool identity) so a
    /// predecessor finishing after its slot was overwritten can never
    /// remove the new occupant.
    pools: DashMap<String, PoolEntry>,

    /// Pools displaced from `pools` while still [`PoolState::Draining`] —
    /// see the `pools` field's "One entry per code" note. A `DashMap` keyed
    /// by code can only ever hold one entry per code, so an `Active` insert
    /// that lands on top of a still-`Draining` predecessor has nowhere to
    /// put both; this list is where the displaced instance goes instead of
    /// being silently dropped from every manager-level view of "what pools
    /// exist". [`Self::all_pools`] chains this in alongside `pools` (so the
    /// dashboard "Mediating"/"blocked groups"/"group flushes" views, which
    /// are built on `all_pools`, keep showing a displaced predecessor's
    /// buffered/in-flight work), and [`Self::shutdown`] chains it into the
    /// pool list it explicitly drains, releases the buffered remainder of
    /// (R-49), waits on, and shuts down — the router specification's
    /// shutdown MUST (release every pool's buffered remainder, never
    /// abandon it — `docs/router-specification.md` §5.3) would otherwise
    /// not hold for an orphaned predecessor. [`Self::cleanup_draining_pools`]
    /// sweeps this the same way it sweeps `Draining` map entries
    /// (`is_fully_drained()` → `shutdown()` → drop), as a belt-and-braces
    /// backstop alongside the predecessor's own watcher task (spawned back
    /// when it first started draining), which independently calls
    /// `pool.shutdown()` the moment `wait_drained()` resolves regardless of
    /// which list currently references it — so a double `shutdown()` call
    /// here is an expected, idempotent, harmless race, not a bug.
    /// `parking_lot::Mutex<Vec<_>>` rather than a `DashMap`: entries are
    /// identified by `Arc` identity, not a key, and appends/sweeps are rare
    /// (only on the coexistence edge case) compared to the hot-path maps
    /// elsewhere in this struct.
    orphaned_draining: Mutex<Vec<Arc<ProcessPool>>>,

    /// G12 (`docs/go-mirror/2026-09-06-go-fix-list.md`): the capacity-freed
    /// gate every consumer poll loop parks on, untimed, when
    /// [`Self::has_pool_capacity`] answers false. Every pool this manager
    /// creates is wired to this same `Notify` via
    /// [`ProcessPool::with_capacity_notify`], so a `notify_waiters()` from
    /// *any* pool's [`crate::pool::QueueSlotReleaser`] wakes every parked
    /// consumer loop to re-check — a reconfigure/eviction that adds,
    /// removes, or drains a pool also signals this directly (see
    /// `reconcile.rs`/`synth_pools.rs`), since either can change what
    /// `has_pool_capacity` answers. Replaces the old fixed-2s-sleep
    /// backpressure pause.
    capacity_notify: Arc<tokio::sync::Notify>,

    /// R-59: idle tracker for every per-client fallback pool
    /// (`{identifier}-DEFAULT-POOL`) this manager synthesised on demand —
    /// see [`Self::ensure_fallback_pool`]. Never holds an entry for a
    /// config-defined pool (including the global `DEFAULT-POOL`):
    /// `apply_config`/`reload_config` call [`Self::forget_synth_pool`] the
    /// moment a code is config-defined, whether it was previously
    /// synthesised or is brand new — config always wins. Read by the
    /// periodic [`Self::evict_idle_synth_pools`] sweep and written on every
    /// route to a fallback pool ([`Self::touch_synth_pool`]), so this is a
    /// `DashMap` (per-entry locking) rather than living behind one shared
    /// lock the hot routing path would contend on. Mirrors Go's
    /// `Manager.synthPools`.
    synth_pools: DashMap<String, synth_pools::SynthPoolState>,

    /// Queue consumers (RwLock for async-safe access), keyed however the
    /// caller identifies a queue for reconfigure-diffing purposes — the
    /// config's queue name (`sync_queue_consumers`) or, for consumers added
    /// directly via [`Self::add_consumer`], the consumer's own
    /// `identifier()`. **Do not** resolve a consumer from a
    /// `QueueIdentifier` read off a polled/tracked message through this
    /// map — see `consumers_by_id` below (G10).
    consumers: RwLock<HashMap<String, Arc<dyn QueueConsumer + Send + Sync>>>,

    /// `Consumer::identifier()`-keyed index mirroring `consumers`,
    /// maintained on every insert/remove ([`Self::add_consumer`],
    /// `sync_queue_consumers`, [`Self::restart_consumer`]) regardless of
    /// what key `consumers` itself used for that same entry.
    ///
    /// Every resolution that starts from a `QueueIdentifier` carried on a
    /// polled/tracked message — the operator force-ack endpoint
    /// (`force_ack_in_flight`), the stall detector's force-NACK path — MUST
    /// go through this index, never through `consumers`. The two key
    /// spaces genuinely differ: a queue's `identifier()` (NATS:
    /// `<stream>/<consumer>`) is a broker-native identity, while the
    /// config's queue name/key is an operator-chosen label; resolving the
    /// wrong one silently drops the ack/nack — the message then redelivers
    /// forever (G10, `docs/go-mirror/2026-09-06-go-fix-list.md`).
    consumers_by_id: RwLock<HashMap<String, Arc<dyn QueueConsumer + Send + Sync>>>,

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
    /// Concurrency-audit consolidation #3 (`docs/developers/router-concurrency-audit.md`):
    /// this used to be the one `Mutex<HashMap>` in the manager, everything
    /// else here is `DashMap`. The `Mutex` was safe as written — every lock
    /// site (`route_batch`'s batch scan, `QueueMessageCallback::ack`'s
    /// insert, `reap_stale_entries`' retain sweep) is a brief, synchronous
    /// section that never holds the lock across an `.await` — but nothing
    /// about those sites actually depends on one lock covering *every* key
    /// at once: each is keyed by a distinct `broker_message_id`, and no
    /// call site relies on a cross-key atomic snapshot (a redelivery
    /// racing a concurrent `ack()` insert has no ordering guarantee to
    /// preserve either way — catching it a poll cycle earlier or later is
    /// equally correct). So the `Mutex<HashMap>`'s single-lock semantics
    /// weren't load-bearing here; converted to `DashMap` for consistency
    /// with the rest of the manager's key/value stores (per-entry locking,
    /// same brief/no-`.await` discipline).
    pending_delete_broker_ids: Arc<DashMap<String, Instant>>,

    /// Maximum number of pools allowed
    max_pools: usize,

    /// Pool count warning threshold
    pool_warning_threshold: usize,

    /// Stall detection configuration
    stall_config: StallConfig,

    /// X-04: message ids already reported as stalled in the current
    /// episode (mirrors Go's `StallDetector.warned`). Without this,
    /// `check_and_handle_stalled_messages` would re-report the same
    /// still-stalled message on every tick — harmless while it only hit
    /// `tracing::warn!`, but now that stall reports go through
    /// `warning_service` (X-04) that would push `active_warnings` past the
    /// Warning/Degraded thresholds purely from a handful of long-running
    /// deliveries still doing their job. Entries are dropped once a message
    /// leaves `in_pipeline` (see `forget_resolved_stalls`), so a later
    /// stall of the same id reports again.
    stall_warned: Mutex<std::collections::HashSet<String>>,

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

    /// Flips to `true` once [`Self::start`] has spawned a poll task for
    /// every consumer configured at startup (item 5, bench rig finding
    /// 2026-09-07: the HTTP listener and `QueueManager::start()` are two
    /// independent tasks spawned back-to-back in `main.rs` with no
    /// ordering between them, so `/health` was answering 200 — and the
    /// bench rig's drain clock was starting — before a single consumer
    /// poll task existed. `api::health::health_handler` reads this via
    /// [`Self::consumers_started`] and answers 503 until it flips, so a
    /// health check genuinely means "consuming has begun", not just "the
    /// HTTP listener is bound". Deliberately does **not** wait for each
    /// task's first actual `poll()` — a slow/long-polling broker (SQS's
    /// 20s) would then hold `/health` unready for the length of one poll
    /// cycle for no correctness reason; "a poll task is running for every
    /// configured consumer" is what "started consuming" means here.
    consumers_started: AtomicBool,

    /// Item 1 (router bench rig, 2026-09-07): identifiers (`consumer.
    /// identifier()`) that currently have a poll task running, guarding
    /// [`Self::spawn_consumer_poll_task`] against being called twice for
    /// the same queue. Found via a hand-rolled single-queue reproduction
    /// after the bench rig's `ACK failed - message not found` warnings
    /// turned out not to be a visibility-timeout race at all: production
    /// mode (`FLOWCATALYST_CONFIG_URL` set) calls `initial_sync()` ->
    /// `reload_config()` -> `sync_queue_consumers()` *before*
    /// `QueueManager::start()` runs, and `sync_queue_consumers` already
    /// spawns a poll task for every queue it creates (the "hot-add" path,
    /// meant for a *later* config reload) — but `start()` then
    /// unconditionally spawns a poll task for every consumer currently in
    /// `self.consumers` too, with no way to know one is already running.
    /// `bin/fc-router/src/main.rs` compounded this by ALSO manually
    /// creating and `add_consumer`-ing a second, fully independent
    /// `PostgresQueue` instance per queue in between those two calls
    /// (`add_consumer` only overwrites the map entry — it neither stops
    /// whatever poll task is already running for that id nor spawns one
    /// for the new instance itself) — see that file's own fix. Two
    /// concurrent pollers hammering the same `queue_name` doesn't produce
    /// a literal double-claim of one row (`FOR UPDATE SKIP LOCKED` still
    /// prevents that), but it does mean two independent, concurrently-
    /// running dedup/redelivery cycles racing each other, which reproduced
    /// as spurious ACK failures and a small number of genuine duplicate
    /// deliveries (5,006 sink hits for 5,000 seeded messages in the
    /// reproduction) within *milliseconds* of a message's very first
    /// claim — nothing to do with the 120s visibility window at all. This
    /// set makes a second `spawn_consumer_poll_task` call for an id that
    /// already has a live task a no-op (logged) instead of a second
    /// poller, regardless of which code path or which consumer instance
    /// triggers it — defence in depth alongside the direct fix in
    /// `main.rs`.
    polling_consumer_ids: Arc<DashSet<String>>,
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
            orphaned_draining: Mutex::new(Vec::new()),
            capacity_notify: Arc::new(tokio::sync::Notify::new()),
            synth_pools: DashMap::new(),
            consumers: RwLock::new(HashMap::new()),
            consumers_by_id: RwLock::new(HashMap::new()),
            pool_configs: RwLock::new(HashMap::new()),
            queue_configs: RwLock::new(HashMap::new()),
            consumer_factory: self.consumer_factory,
            mediator_factory: self.mediator_factory,
            default_pool_code: "DEFAULT-POOL".to_string(), // Java: DEFAULT_POOL_CODE
            running: AtomicBool::new(true),
            shutdown,
            batch_counter: std::sync::atomic::AtomicU64::new(0),
            pending_delete_broker_ids: Arc::new(DashMap::new()),
            max_pools: self.max_pools,
            pool_warning_threshold: self.pool_warning_threshold,
            stall_config: self.stall_config,
            stall_warned: Mutex::new(std::collections::HashSet::new()),
            warning_service: self.warning_service,
            circuit_breaker_registry: self.circuit_breaker_registry,
            health_service: self.health_service,
            strict_routing: AtomicBool::new(false),
            is_leader: AtomicBool::new(true),
            consumers_started: AtomicBool::new(false),
            polling_consumer_ids: Arc::new(DashSet::new()),
        }
    }
}

impl QueueManager {
    /// R-59: suffix identifying a per-client fallback pool code
    /// (`{identifier}-DEFAULT-POOL`). See `is_default_pool_code`.
    const SYNTH_POOL_SUFFIX: &'static str = "-DEFAULT-POOL";

    /// Start building a manager that creates a **fresh** `HttpMediator` per
    /// pool (production path). Prefer this builder over `new` + `set_*`.
    pub fn builder(mediator_config: HttpMediatorConfig) -> QueueManagerBuilder {
        let factory: MediatorFactory =
            Arc::new(move |ws: &Arc<WarningService>, breakers: &Arc<CircuitBreakerRegistry>| {
                Arc::new(
                    HttpMediator::with_config(mediator_config.clone())
                        .with_warning_service(ws.clone())
                        .with_circuit_breakers(breakers.clone()),
                ) as Arc<dyn Mediator + 'static>
            });
        QueueManagerBuilder::from_factory(factory)
    }

    /// Start building a manager where every pool shares one mediator instance
    /// (test seam for injecting mocks / instrumenting mediator calls). The
    /// mock is handed neither the warning service nor the breaker registry —
    /// same as before breaker centralisation — since a shared-mediator test
    /// mock models its own delivery outcomes directly and doesn't consult
    /// either collaborator.
    pub fn builder_with_shared_mediator(
        mediator: Arc<dyn Mediator + 'static>,
    ) -> QueueManagerBuilder {
        let factory: MediatorFactory = Arc::new(
            move |_ws: &Arc<WarningService>, _breakers: &Arc<CircuitBreakerRegistry>| {
                mediator.clone()
            },
        );
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

    /// Whether [`Self::start`] has spawned a poll task for every consumer
    /// configured at startup. See the `consumers_started` field doc for why
    /// `/health` gates on this rather than answering the moment the HTTP
    /// listener is bound.
    pub fn consumers_started(&self) -> bool {
        self.consumers_started.load(Ordering::SeqCst)
    }

    /// Build a mediator instance for a pool via the configured factory,
    /// passing the manager's current warning service so per-pool mediators
    /// emit through the real sink (see [`MediatorFactory`]).
    fn build_mediator(&self) -> Arc<dyn Mediator + 'static> {
        (self.mediator_factory)(&self.warning_service, &self.circuit_breaker_registry)
    }

    /// Test-only constructor: every pool shares the supplied mediator. Use
    /// this when you need to inject a mock or instrument mediator calls.
    /// Production code should use [`QueueManager::builder`] (or [`new`]) and
    /// let the manager build a mediator per pool.
    #[doc(hidden)]
    pub fn with_shared_mediator_for_testing(mediator: Arc<dyn Mediator + 'static>) -> Self {
        Self::builder_with_shared_mediator(mediator).build()
    }

    /// The G12 capacity-freed gate — see the `capacity_notify` field's doc
    /// comment. `Consumer::poll` loops park on this via `notified()` when
    /// [`Self::has_pool_capacity`] answers false.
    pub(super) fn capacity_notify(&self) -> &Arc<tokio::sync::Notify> {
        &self.capacity_notify
    }

    /// Add a queue consumer
    pub async fn add_consumer(&self, consumer: Arc<dyn QueueConsumer + Send + Sync>) {
        let id = consumer.identifier().to_string();
        self.consumers.write().await.insert(id.clone(), consumer.clone());
        // G10: keep the identifier-keyed resolution index in lockstep.
        self.consumers_by_id.write().await.insert(id, consumer);
    }

    /// The currently *active* pool for `code` — `None` if absent, or if
    /// only a [`PoolState::Draining`] entry exists under this code. The
    /// post-consolidation equivalent of "look it up in the (formerly
    /// separate) active-only `pools` map."
    pub(super) fn active_pool(&self, code: &str) -> Option<Arc<ProcessPool>> {
        self.pools
            .get(code)
            .and_then(|e| (e.state == PoolState::Active).then(|| e.pool.clone()))
    }

    /// Count of [`PoolState::Active`] entries — the post-consolidation
    /// equivalent of the old active-only `pools.len()`.
    pub(super) fn active_pool_count(&self) -> usize {
        self.pools
            .iter()
            .filter(|e| e.value().state == PoolState::Active)
            .count()
    }

    /// Insert `pool` as the `Active` entry for `code`, and — if this
    /// displaces a still-`Draining` entry under the same code — hand the
    /// displaced pool to [`Self::orphaned_draining`] rather than losing the
    /// only reference to it. See the `pools`/`orphaned_draining` field doc
    /// comments for why this coexistence case exists and why it isn't
    /// unsafe. Every call site that can create a fresh `Active` pool for a
    /// code that might already have a `Draining` entry (`get_or_create_pool`,
    /// `ensure_fallback_pool`) must insert through this instead of a bare
    /// `self.pools.insert(...)`.
    pub(super) fn insert_active_pool(&self, code: String, pool: Arc<ProcessPool>) {
        let displaced = self.pools.insert(
            code,
            PoolEntry {
                pool,
                state: PoolState::Active,
            },
        );
        if let Some(entry) = displaced {
            if entry.state == PoolState::Draining {
                self.orphaned_draining.lock().push(entry.pool);
            }
        }
    }

    /// Insert `pool` as the `Draining` entry for `code` (called once,
    /// right after `pool.drain()`, by `begin_pool_drain`). Symmetric with
    /// [`Self::insert_active_pool`], because the reverse race is possible
    /// too: `begin_pool_drain` awaits `pool.drain()` before this insert, so
    /// a concurrent creator (`ensure_fallback_pool` racing
    /// `evict_idle_synth_pools`'s drain of the same synth code is the
    /// realistic case — `update_pool_config` racing `reload_config`'s
    /// removal of the same configured code is the other) can claim the code
    /// slot with a fresh `Active` entry in the gap. A bare insert here
    /// would silently clobber that newer pool. If the displaced entry is
    /// `Active`, it wins the slot back — reinserted immediately — and this
    /// (older, now-draining) pool becomes the orphan instead of the other
    /// way around. If the displaced entry is itself `Draining` (a second
    /// drain landing before the first's watcher/cleanup removed it — same
    /// pattern the old `draining_pools` map already tolerated on a
    /// same-key double-insert), it's handed to `orphaned_draining` rather
    /// than lost.
    pub(super) fn insert_draining_pool(&self, code: String, pool: Arc<ProcessPool>) {
        let entry = PoolEntry {
            pool: pool.clone(),
            state: PoolState::Draining,
        };
        match self.pools.insert(code.clone(), entry) {
            None => {}
            Some(displaced) if displaced.state == PoolState::Draining => {
                self.orphaned_draining.lock().push(displaced.pool);
            }
            Some(active_entry) => {
                // active_entry.state == Active: a concurrent creation won
                // the slot while we were draining — put it back, and treat
                // ourselves as the orphan instead of clobbering it.
                self.pools.insert(code, active_entry);
                self.orphaned_draining.lock().push(pool);
            }
        }
    }
}
