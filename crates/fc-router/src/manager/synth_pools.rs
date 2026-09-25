//! R-59: per-client fallback pool ("synth pool") machinery —
//! `{identifier}-DEFAULT-POOL` codes the scheduler routes to without ever
//! appearing in `processing_pools` config. This module owns the idle
//! tracker, the code-shape predicates, on-demand synthesis, and idle
//! eviction. See [`QueueManager::ensure_fallback_pool`] and
//! [`QueueManager::evict_idle_synth_pools`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tracing::info;

use fc_common::PoolConfig;

use crate::pool::ProcessPool;
use crate::Result;

use super::{PoolState, QueueManager};

/// R-59: idle tracker for one synthesised per-client fallback pool. Only
/// `last_routed` is mutated after creation — bumped on every route to the
/// pool, including the pool-already-exists fast path (`touch_synth_pool`) —
/// and read by the periodic `evict_idle_synth_pools` sweep. Mirrors the
/// `parking_lot::RwLock<Instant>` `last_activity` pattern
/// `CircuitBreakerRegistry` already uses for its own idle eviction
/// (`evict_idle`), rather than Go's atomic-nanos `synthPoolState`: this
/// crate's convention for "idle clock behind a lock, never held across an
/// `.await`" is a `parking_lot` primitive, not a hand-rolled atomic.
pub(super) struct SynthPoolState {
    last_routed: Mutex<Instant>,
}

impl SynthPoolState {
    fn new() -> Self {
        Self {
            last_routed: Mutex::new(Instant::now()),
        }
    }

    /// Reset the idle clock — a message was just routed to this pool.
    fn touch(&self) {
        *self.last_routed.lock() = Instant::now();
    }

    /// How long since this pool last had a message routed to it.
    fn idle_for(&self) -> Duration {
        self.last_routed.lock().elapsed()
    }
}

impl QueueManager {
    /// R-59: does `code` name a fallback pool — the global `DEFAULT-POOL`
    /// or a per-client `{identifier}-DEFAULT-POOL`?
    ///
    /// A suffix test is the only safe structural read of a composed pool
    /// code: the scheduler composes `{clientIdentifier}-{poolCode}` and
    /// either half may itself contain hyphens, so the string can never be
    /// split back into its parts. Mirrors Go's `isDefaultPoolCode`.
    pub(super) fn is_default_pool_code(&self, code: &str) -> bool {
        code == self.default_pool_code || code.ends_with(Self::SYNTH_POOL_SUFFIX)
    }

    /// R-59: does `code` name a *synthesisable* per-client fallback pool —
    /// eligible for on-demand creation (`ensure_fallback_pool`) and, if it
    /// is one, for idle eviction (`evict_idle_synth_pools`)? Excludes the
    /// global `DEFAULT-POOL` itself, which is always config-defined and
    /// therefore never synthesised or evicted. Mirrors Go's
    /// `isSynthPoolCode`.
    pub(super) fn is_synth_pool_code(&self, code: &str) -> bool {
        code != self.default_pool_code && self.is_default_pool_code(code)
    }

    /// R-59: registers `code` as eviction-eligible (see
    /// `evict_idle_synth_pools`), with its idle clock starting now. Called
    /// only from `ensure_fallback_pool` at creation.
    pub(super) fn track_synth_pool(&self, code: &str) {
        self.synth_pools
            .insert(code.to_string(), SynthPoolState::new());
    }

    /// R-59: resets `code`'s idle clock — a message was just routed to it.
    /// A no-op if `code` isn't currently tracked as synthesised: a
    /// config-defined pool of the same code has had its entry removed by
    /// `forget_synth_pool`, and traffic to it must not re-arm an eviction
    /// that no longer applies.
    pub(super) fn touch_synth_pool(&self, code: &str) {
        if let Some(state) = self.synth_pools.get(code) {
            state.touch();
        }
    }

    /// R-59: removes `code` from eviction tracking — called whenever config
    /// takes ownership of the code (`apply_config`/`reload_config`) or the
    /// pool is torn down outright (`shutdown`), so a stale entry never
    /// points at a pool that is gone or no longer synthesised.
    pub(super) fn forget_synth_pool(&self, code: &str) {
        self.synth_pools.remove(code);
    }

    /// R-59: returns the per-client fallback pool for `code`, synthesising
    /// it on demand with the same default settings `get_or_create_pool`
    /// uses when creating any other pool with no explicit config
    /// (concurrency 20, no rate limit) — the router polls an external
    /// config service and nothing emits `processing_pools` entries for
    /// `{identifier}-DEFAULT-POOL` codes, so these only ever arrive from
    /// the scheduler at routing time.
    ///
    /// Creation is serialised by the manager's pool-creation lock and
    /// re-checked under it, so two first messages for a brand-new client
    /// land in the same pool (Go: `ensureFallbackPool`'s double-check under
    /// `poolMu`).
    pub(super) async fn ensure_fallback_pool(&self, code: &str) -> Result<Arc<ProcessPool>> {
        if let Some(pool) = self.active_pool(code) {
            self.touch_synth_pool(code);
            return Ok(pool);
        }
        let _create = self.pool_create_lock.lock().await;
        if let Some(pool) = self.active_pool(code) {
            self.touch_synth_pool(code);
            return Ok(pool);
        }

        let pool_config = PoolConfig {
            code: code.to_string(),
            concurrency: 20, // Go: defaultPoolConcurrency; Java: DEFAULT_POOL_CONCURRENCY
            rate_limit_per_minute: None,
        };
        let pool = ProcessPool::new(pool_config.clone(), self.build_mediator())
            .with_capacity_notify(self.capacity_notify().clone());
        let pool_arc = Arc::new(pool);
        pool_arc.start().await;

        self.insert_active_pool(code.to_string(), pool_arc.clone())
            .await;
        self.track_synth_pool(code);
        // A new pool is a capacity event (Go: ensureFallbackPool signals).
        self.capacity_notify().notify_waiters();
        info!(
            pool_code = %code,
            concurrency = pool_config.concurrency,
            "Synthesised per-client fallback pool"
        );

        Ok(pool_arc)
    }

    /// R-59: stops and removes every synthesised per-client fallback pool
    /// (`ensure_fallback_pool`) idle for at least `ttl` — no message routed
    /// to it, tracked by `synth_pools`/`touch_synth_pool`. `ttl ==
    /// Duration::ZERO` disables the sweep entirely (see the call site in
    /// `bin/fc-router/src/main.rs` for how `FC_ROUTER_SYNTH_POOL_IDLE_SECS`
    /// maps onto this — `Duration` has no negative representation, so the
    /// "negative disables" half of Go's `EvictIdleSynthPools`/`ttl <= 0`
    /// check collapses to "zero disables" here).
    ///
    /// Configured pools are never candidates: only codes present in
    /// `synth_pools` are, and `apply_config`/`reload_config` remove a code
    /// from that map the instant config defines it (`forget_synth_pool`) —
    /// see those methods' "config always wins" call sites. Uses the same
    /// drain path a config-removed pool takes (`begin_pool_drain`), so
    /// buffered work is flushed rather than dropped (R-26/R-49); the pool
    /// is re-synthesised on demand by the next message naming its code.
    /// Wired onto the lifecycle manager's reaper tick (mirrors Go, which
    /// hangs this off the router's in-flight reaper tick). Returns the
    /// number of pools evicted.
    pub async fn evict_idle_synth_pools(self: &Arc<Self>, ttl: Duration) -> usize {
        if ttl.is_zero() {
            return 0;
        }

        let idle_codes: Vec<String> = self
            .synth_pools
            .iter()
            .filter(|entry| entry.value().idle_for() >= ttl)
            .map(|entry| entry.key().clone())
            .collect();
        if idle_codes.is_empty() {
            return 0;
        }

        let mut evicted = 0usize;
        for code in idle_codes {
            // Re-check: a message may have routed to it (bumping the idle
            // clock) since the scan above, or reload_config may have taken
            // ownership of the code (forget_synth_pool) in the meantime.
            let still_idle = self
                .synth_pools
                .get(&code)
                .map(|state| state.idle_for() >= ttl)
                .unwrap_or(false);
            if !still_idle {
                continue;
            }
            // Remove from tracking before the pool itself, so a message
            // racing in right now sees "not tracked" (a harmless
            // touch_synth_pool no-op) rather than resetting a clock about
            // to be discarded anyway.
            self.synth_pools.remove(&code);

            if let Some((removed_code, entry)) = self
                .pools
                .remove_if(&code, |_, e| e.state == PoolState::Active)
            {
                let pool = entry.pool;
                info!(
                    pool_code = %removed_code,
                    queue_size = pool.queue_size(),
                    active_workers = pool.active_workers(),
                    "Synthesised fallback pool idle past TTL - draining and removing"
                );
                self.begin_pool_drain(removed_code, pool).await;
                evicted += 1;
            }
        }
        evicted
    }

    /// R-59: number of pool codes currently tracked as synthesised (see
    /// `synth_pools`). Exposed for tests — mirrors Go's ability to inspect
    /// `Manager.synthPools` indirectly via `EvictIdleSynthPools`.
    pub fn synth_pool_count(&self) -> usize {
        self.synth_pools.len()
    }
}
