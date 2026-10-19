//! Config reconciliation: `apply_config` (initial setup) and `reload_config`
//! (hot reload) plus the machinery they share — consumer sync, pool
//! creation/lookup, and the drain-on-removal path (`begin_pool_drain`,
//! `cleanup_draining_pools`).

use std::collections::HashMap;
use std::sync::Arc;

use tracing::{error, info, warn};

use fc_common::{PoolConfig, RouterConfig, WarningCategory, WarningSeverity};

use crate::error::RouterError;
use crate::pool::ProcessPool;
use crate::Result;
use futures::future;
use futures::{stream, StreamExt};

use super::QueueManager;
use std::sync::atomic::Ordering;

/// Upper bound on consumers being built at once during a config sync.
const CONSUMER_CREATE_CONCURRENCY: usize = 8;

/// Every field of a queue's config matters: a changed URI points at a
/// different queue, and connections / visibility timeout are fixed when the
/// consumer is built (Go compares the whole `QueueConfig`).
fn same_queue_config(a: &fc_common::QueueConfig, b: &fc_common::QueueConfig) -> bool {
    a.name == b.name
        && a.uri == b.uri
        && a.connections == b.connections
        && a.visibility_timeout == b.visibility_timeout
}

impl QueueManager {
    /// `config` plus a `DEFAULT-POOL` at the default concurrency (20) when
    /// it defines none (Go: `Reconfigure`'s `wantPools`).
    fn with_default_pool(&self, mut config: RouterConfig) -> RouterConfig {
        if !config
            .processing_pools
            .iter()
            .any(|p| p.code == self.default_pool_code)
        {
            config.processing_pools.push(PoolConfig {
                code: self.default_pool_code.clone(),
                concurrency: 20,
                rate_limit_per_minute: None,
            });
        }
        config
    }

    /// Apply router configuration (initial setup).
    ///
    /// Takes `self: &Arc<Self>` so `sync_queue_consumers` can spawn poll
    /// tasks for hot-added consumers. Callers already hold the manager
    /// behind an Arc.
    pub async fn apply_config(self: &Arc<Self>, config: RouterConfig) -> Result<()> {
        let config = self.with_default_pool(config);
        let mut pool_configs = self.pool_configs.write().await;
        for pool_config in config.processing_pools {
            let code = pool_config.code.clone();
            // R-59: config always wins — a code config now defines is never
            // eviction-eligible, whether it was previously synthesised or is
            // brand new. Normally a no-op here (apply_config only runs at
            // startup, before anything could have been synthesised), but
            // kept for symmetry with reload_config's equivalent call.
            self.forget_synth_pool(&code);
            pool_configs.insert(code.clone(), pool_config.clone());
            self.get_or_create_pool(&code, Some(pool_config)).await?;
        }
        self.capacity_notify().notify_waiters();
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
        if !self.running.load(Ordering::SeqCst) || self.polling_stopped.load(Ordering::SeqCst) {
            warn!("Cannot reload config - QueueManager is shutting down");
            return Ok(false);
        }

        info!("Hot reloading configuration...");

        // Go's Reconfigure always ensures DEFAULT-POOL: a config that does
        // not define it must not remove it. Dropping it used to start a
        // drain while the next message re-created it, and the two pools
        // then ran the same message groups side by side.
        let config = self.with_default_pool(config);

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
        // What could not be applied. Every pool and queue is still
        // reconciled (Go's Reconfigure aborted at the first failure); the
        // reload then reports these so the caller retries the config.
        let mut failures: Vec<String> = Vec::new();
        // Concurrency changes, applied together after the scan: a decrease
        // waits (up to 60 s) for workers to finish, and one pool's wait
        // must not hold up the next pool's.
        let mut concurrency_updates: Vec<(String, Arc<ProcessPool>, PoolConfig)> = Vec::new();

        // Step 1: Handle existing pools - update or remove
        let existing_codes: Vec<String> = self.pools.iter().map(|e| e.key().clone()).collect();
        for pool_code in existing_codes {
            if let Some(new_config) = new_pool_configs.get(&pool_code) {
                // R-59: config always wins — a code config now defines is
                // never eviction-eligible, whether it was previously
                // synthesised (this is exactly the ownership-transfer case:
                // the pool below is updated in place, never replaced) or a
                // pool config has always owned.
                self.forget_synth_pool(&pool_code);
                // Pool exists in new config - check for changes.
                //
                // R-59: `old_config` is `None` exactly when this code has
                // never gone through `pool_configs` before — which, since
                // `apply_config`/this same branch are the only writers of
                // that map, means the pool was synthesised
                // (`ensure_fallback_pool`/`get_or_create_pool`'s
                // default-config fallback) rather than config-defined. That
                // must count as "changed": config just took ownership of a
                // pool running on synthesised defaults, and the new
                // settings have to actually apply — mirrors Go's
                // Reconfigure, which calls `p.SetRateLimit`/
                // `p.UpdateConcurrency` unconditionally for every pool code
                // it owns, not only when a diff is detected against a prior
                // config.
                let old_config = pool_configs.get(&pool_code);
                let concurrency_changed =
                    old_config.map(|c| c.concurrency) != Some(new_config.concurrency);
                let rate_limit_changed = old_config.map(|c| c.rate_limit_per_minute)
                    != Some(new_config.rate_limit_per_minute);

                if concurrency_changed || rate_limit_changed {
                    if let Some(pool) = self.active_pool(&pool_code) {
                        // Update the pool in-place
                        if concurrency_changed {
                            info!(
                                pool_code = %pool_code,
                                old_concurrency = ?old_config.map(|c| c.concurrency),
                                new_concurrency = new_config.concurrency,
                                "Updating pool concurrency"
                            );
                            concurrency_updates.push((
                                pool_code.clone(),
                                pool.clone(),
                                new_config.clone(),
                            ));
                        }

                        if rate_limit_changed {
                            info!(
                                pool_code = %pool_code,
                                old_rate_limit = ?old_config.and_then(|c| c.rate_limit_per_minute),
                                new_rate_limit = ?new_config.rate_limit_per_minute,
                                "Updating pool rate limit"
                            );
                            pool.update_rate_limit(new_config.rate_limit_per_minute);
                        }

                        pools_updated += 1;
                    }
                }
                // Update stored config
                pool_configs.insert(pool_code, new_config.clone());
            } else {
                // Pool removed from config - drain asynchronously. R-59:
                // this also catches a synthesised per-client fallback pool
                // whenever a genuine config change lands (synth pools are
                // never in `new_pool_configs` in steady state) — mirrors
                // Go's Reconfigure exactly, which sweeps every pool not in
                // `wantPools` the same way regardless of origin; idle-TTL
                // eviction (`evict_idle_synth_pools`) is the mechanism R-59
                // actually targets, this is just the pre-existing "any real
                // config change resets pools it doesn't mention" behaviour.
                if let Some((code, pool)) = self.pools.remove(&pool_code) {
                    info!(
                        pool_code = %code,
                        queue_size = pool.queue_size(),
                        active_workers = pool.active_workers(),
                        "Pool removed from config - draining asynchronously"
                    );
                    self.forget_synth_pool(&code);
                    pool_configs.remove(&code);
                    pools_removed += 1;

                    // begin_pool_drain: drains, tracks the pool as draining,
                    // and spawns the watcher that calls `pool.shutdown()`
                    // the moment its in-flight work finishes, instead of
                    // waiting for the next `cleanup_draining_pools` sweep
                    // (the backstop). Shared with `evict_idle_synth_pools`.
                    self.begin_pool_drain(code, pool).await;
                }
            }
        }

        // Step 1b: the concurrency changes, concurrently. One that could
        // not be applied (a decrease that timed out waiting for busy
        // workers, a zero) keeps the pool's live concurrency in the stored
        // config, so the next reload sees the change again and retries it,
        // instead of recording a value the pool is not running at.
        let applied = future::join_all(
            concurrency_updates
                .iter()
                .map(|(_, pool, cfg)| pool.update_concurrency(cfg.concurrency)),
        )
        .await;
        for ((code, pool, cfg), ok) in concurrency_updates.into_iter().zip(applied) {
            if !ok {
                failures.push(format!(
                    "pool {code}: concurrency {} not applied (still {})",
                    cfg.concurrency,
                    pool.concurrency()
                ));
                pool_configs.insert(
                    code,
                    PoolConfig {
                        concurrency: pool.concurrency(),
                        ..cfg
                    },
                );
            }
        }

        // Step 2: Create new pools
        for pool_config in &config.processing_pools {
            let already_active = self.pools.contains_key(&pool_config.code);
            if !already_active {
                // Check pool count limits
                let current_count = self.active_pool_count();
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

                // R-59: config always wins, even for a brand-new code (see
                // the identical call in the update branch above) — usually
                // a no-op here since a genuinely new code was never
                // synthesised, but defensive against the code having been
                // synthesised and evicted in the narrow window between this
                // reload's Step 1 scan and here.
                self.forget_synth_pool(&pool_config.code);
                // Create new pool
                if let Err(e) = self
                    .get_or_create_pool(&pool_config.code, Some(pool_config.clone()))
                    .await
                {
                    error!(pool_code = %pool_config.code, error = %e, "Could not create pool; continuing with the rest of the configuration");
                    failures.push(format!("pool {}: {e}", pool_config.code));
                    continue;
                }
                pool_configs.insert(pool_config.code.clone(), pool_config.clone());
                pools_created += 1;
            }
        }

        // Step 3: Sync queue consumers (Java: Step 4). A consumer that
        // could not be built fails the reload (after every other queue has
        // been reconciled), so the caller does not record this config as
        // applied and retries it (Go: Reconfigure errors, the watcher
        // forgets the config).
        let synced = self.sync_queue_consumers(&config).await;

        // A reconfigure can add, remove, resize or repoint any pool a parked
        // consumer's capacity check depends on — wake every waiter so it
        // re-evaluates against the new topology (Go: Reconfigure signals
        // the capacity gate). Parked consumers used to stay parked.
        self.capacity_notify().notify_waiters();
        if !failures.is_empty() {
            if let Err(e) = &synced {
                failures.push(e.to_string());
            }
            warn!(failures = ?failures, "Reconfigure incomplete; the configuration will be retried");
            return Err(RouterError::Reconfigure(failures.join("; ")));
        }
        let (queues_created, queues_removed) = synced?;

        let total_active_consumers = self.consumers.len();

        info!(
            pools_updated = pools_updated,
            pools_created = pools_created,
            pools_removed = pools_removed,
            queues_created = queues_created,
            queues_removed = queues_removed,
            total_active_pools = self.active_pool_count(),
            total_draining_pools = self.draining_pool_count(),
            total_active_consumers = total_active_consumers,
            "Configuration reload complete"
        );

        Ok(true)
    }

    /// Reconcile consumers against `config.queues` (Go: the consumer half of
    /// `Reconfigure`), keyed by queue name (the URI when the name is empty):
    ///
    /// - a queue that is gone, or whose config changed in any field (URI,
    ///   connections, visibility timeout), has its consumer DETACHED: its
    ///   poll loop stops at once, but it stays resolvable for the ack/nack
    ///   of messages it already delivered until
    ///   `retire_detached_consumers` finds none left (X-11 / R-26/R-49) —
    ///   never stopped out from under in-flight work;
    /// - a queue with no consumer (new, changed, or one whose earlier build
    ///   failed) gets one built and started.
    ///
    /// Building talks to the broker, so no registry lock is held across it.
    /// If any build fails the rest are still reconciled and an error naming
    /// the failures is returned, so the config is not recorded as applied
    /// and the next sync retries exactly the missing queues.
    async fn sync_queue_consumers(
        self: &Arc<Self>,
        config: &RouterConfig,
    ) -> Result<(usize, usize)> {
        let wanted: HashMap<String, fc_common::QueueConfig> = config
            .queues
            .iter()
            .map(|q| {
                let key = if q.name.is_empty() {
                    q.uri.clone()
                } else {
                    q.name.clone()
                };
                (key, q.clone())
            })
            .collect();

        // Detach consumers whose queue is gone or changed. A consumer
        // registered directly (`add_consumer`, no config) is left alone
        // unless config now names its queue differently.
        let mut queues_removed = 0;
        for rc in self.consumers.active() {
            let keep = match (&rc.queue_config, wanted.get(&rc.name)) {
                (None, None) => true,
                (Some(have), Some(want)) => same_queue_config(have, want),
                (None, Some(_)) | (Some(_), None) => false,
            };
            if keep {
                continue;
            }
            if let Some(removed) = self.consumers.remove(&rc.name) {
                if removed.generation != rc.generation {
                    // Replaced concurrently; put the newer one back.
                    self.consumers.insert(removed);
                    continue;
                }
                info!(
                    queue = %rc.name,
                    "Detaching consumer (queue removed or its config changed)"
                );
                self.detach_consumer(removed).await;
                queues_removed += 1;
            }
        }

        // A queue that left the config, or whose config changed, is no longer
        // "missing" as it was; the build below re-judges it against the broker.
        self.missing_queues.lock().retain(|name, have| {
            wanted
                .get(name)
                .is_some_and(|want| same_queue_config(have, want))
        });

        let Some(factory) = self.consumer_factory.clone() else {
            for name in wanted.keys() {
                if self.consumers.get(name).is_none() {
                    warn!(
                        queue_id = %name,
                        "New queue in config but no consumer factory available - consumer will not be auto-created"
                    );
                }
            }
            return Ok((0, queues_removed));
        };

        // Build the missing consumers concurrently (each build is a broker
        // round-trip; 16 queues built one at a time used to take 16x the
        // latency), then register them one at a time in `wanted` order so
        // logs and behaviour stay deterministic.
        let missing: Vec<(String, fc_common::QueueConfig)> = wanted
            .iter()
            .filter(|(name, _)| self.consumers.get(name.as_str()).is_none())
            .map(|(name, cfg)| (name.clone(), cfg.clone()))
            .collect();
        let (queues_created, failures) = self.start_consumers(factory, missing).await;

        if !failures.is_empty() {
            return Err(RouterError::ConsumerBuild(failures.join("; ")));
        }
        Ok((queues_created, queues_removed))
    }

    /// Build and start a consumer for each of `queues` (name, config).
    /// Returns how many started and the failures; the rest are still started
    /// when one fails. A queue the broker has no queue for yet
    /// ([`RouterError::is_queue_missing`]) is not a failure: it goes in
    /// `missing_queues`, is logged once per streak, and is left alone until
    /// [`Self::recheck_missing_queues`] (or the next reload) finds it.
    async fn start_consumers(
        self: &Arc<Self>,
        factory: Arc<dyn super::ConsumerFactory>,
        queues: Vec<(String, fc_common::QueueConfig)>,
    ) -> (usize, Vec<String>) {
        let mut queues_created = 0;
        let mut failures: Vec<String> = Vec::new();
        // Owned inputs (not references) keep the stream's futures `Send` for
        // the spawned reload task. `buffered` yields results in input order.
        let mut built = stream::iter(queues.into_iter().map(|(name, queue_config)| {
            let factory = factory.clone();
            async move {
                info!(queue_id = %name, "Creating queue consumer");
                let result = factory.create_consumer(&queue_config).await;
                (name, queue_config, result)
            }
        }))
        .buffered(CONSUMER_CREATE_CONCURRENCY);

        while let Some((name, queue_config, result)) = built.next().await {
            match result {
                Ok(consumer) => {
                    let rc = self.new_running_consumer(
                        consumer,
                        name.clone(),
                        Some(queue_config.clone()),
                    );
                    if let Some(prev) = self.consumers.insert(rc.clone()) {
                        // The watchdog registered one while we were
                        // building; keep ours (it matches this config) and
                        // detach theirs.
                        self.detach_consumer(prev).await;
                    }
                    let was_missing = self.missing_queues.lock().remove(&name).is_some();
                    self.spawn_consumer_poll_task(rc);
                    queues_created += 1;
                    if was_missing {
                        info!(queue_id = %name, "Queue now exists; consuming it");
                    }
                    info!(queue_id = %name, "Queue consumer created and polling");
                }
                Err(e) if e.is_queue_missing() => self.note_queue_missing(&name, queue_config),
                Err(e) => {
                    error!(queue_id = %name, error = %e, "Failed to create queue consumer");
                    self.warning_service.add_warning(
                        WarningCategory::ConsumerHealth,
                        WarningSeverity::Critical,
                        format!("Failed to create consumer for queue [{}]: {}", name, e),
                        "QueueManager".to_string(),
                    );
                    failures.push(format!("{name}: {e}"));
                }
            }
        }
        (queues_created, failures)
    }

    /// Record `queue_config` as configured-but-absent and log the one INFO
    /// line for the transition into that state; a recheck that still finds
    /// it missing is silent.
    pub(super) fn note_queue_missing(&self, name: &str, queue_config: fc_common::QueueConfig) {
        let already = self
            .missing_queues
            .lock()
            .insert(name.to_string(), queue_config)
            .is_some();
        if !already {
            info!(
                queue_id = %name,
                "Queue does not exist yet; not consuming it (rechecked periodically)"
            );
        }
    }

    /// Queues currently configured but absent from the broker.
    pub fn missing_queue_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.missing_queues.lock().keys().cloned().collect();
        names.sort();
        names
    }

    /// Start a consumer for every configured queue that was missing at the
    /// last look and has appeared since (Go: `RecheckMissingQueues`). The
    /// platform creates a dispatch queue on the first message published to
    /// it, which can happen long after the config naming it was applied, and
    /// an unchanged config is never re-applied, so without this a queue
    /// created later would not be consumed until the next config change.
    /// With nothing missing it does no work at all. Failures other than
    /// "missing" are logged and retried by the next recheck or reload.
    pub async fn recheck_missing_queues(self: &Arc<Self>) {
        if !self.running.load(Ordering::SeqCst) || self.polling_stopped.load(Ordering::SeqCst) {
            return;
        }
        let Some(factory) = self.consumer_factory.clone() else {
            return;
        };
        // One build at a time against a reload (which holds the write lock).
        let _reload_guard = self.pool_configs.read().await;
        let queues: Vec<(String, fc_common::QueueConfig)> = self
            .missing_queues
            .lock()
            .iter()
            .filter(|(name, _)| self.consumers.get(name.as_str()).is_none())
            .map(|(name, cfg)| (name.clone(), cfg.clone()))
            .collect();
        if queues.is_empty() {
            return;
        }
        self.start_consumers(factory, queues).await;
    }

    /// Cleanup draining pools that have finished (see `draining_pools`).
    ///
    /// The primary path is now the per-pool watcher task spawned in
    /// `begin_pool_drain` when a pool starts draining — it removes/shuts
    /// down the pool the instant `wait_drained()` resolves. This method is
    /// a belt-and-braces sweep for anything the watcher missed (e.g. a pool
    /// inserted before this feature existed, a watcher task that never got
    /// scheduled, or the manager's own shutdown cancelling the watcher
    /// before it could act). A double `shutdown()`/remove here is a
    /// harmless no-op, so calling this periodically alongside the watcher
    /// is safe.
    pub async fn cleanup_draining_pools(&self) {
        let finished: Vec<Arc<ProcessPool>> = {
            let mut draining = self.draining_pools.lock();
            let (done, remaining): (Vec<_>, Vec<_>) =
                draining.drain(..).partition(|p| p.is_fully_drained());
            *draining = remaining;
            done
        };
        for pool in finished {
            info!(pool_code = %pool.code(), "Draining pool finished - cleaning up");
            pool.shutdown().await;
        }
    }

    /// Get or create a pool by code.
    ///
    /// Creation is serialised by `pool_create_lock` and re-checked under it
    /// (Go creates pools under `poolMu`): two messages for a code that has
    /// no pool yet used to each build and start one, and the loser — with
    /// worker tasks running — was silently dropped from the map.
    pub(super) async fn get_or_create_pool(
        &self,
        code: &str,
        config: Option<PoolConfig>,
    ) -> Result<Arc<ProcessPool>> {
        if let Some(pool) = self.active_pool(code) {
            return Ok(pool);
        }
        let _create = self.pool_create_lock.lock().await;
        if let Some(pool) = self.active_pool(code) {
            return Ok(pool);
        }

        let pool_config = config.unwrap_or_else(|| PoolConfig {
            code: code.to_string(),
            concurrency: 20, // Java: DEFAULT_POOL_CONCURRENCY = 20
            rate_limit_per_minute: None,
        });

        // `build_mediator` already wires in the manager's single breaker
        // registry (and the real warning service) — see `MediatorFactory`'s
        // doc — so breaker state is shared across pools and surfaced to
        // monitoring without the pool itself touching a registry at all.
        let pool = ProcessPool::new(pool_config.clone(), self.build_mediator())
            .with_capacity_notify(self.capacity_notify().clone())
            .with_settled_reporter(self.settled_reporter.clone())
            .with_flight_recorder(self.flight_recorder.clone());

        let pool_arc = Arc::new(pool);
        pool_arc.start().await;

        self.insert_active_pool(code.to_string(), pool_arc.clone())
            .await;
        info!(pool_code = %code, concurrency = pool_config.concurrency, "Created process pool");
        // A new pool is a capacity event for any consumer parked on "no
        // room" (Go: ensureFallbackPool signals the gate).
        self.capacity_notify().notify_waiters();

        Ok(pool_arc)
    }

    /// Returns the currently *active* pool for `code` (never one still
    /// draining), or `None`. Exposed mainly for tests that need pool
    /// identity (`Arc::ptr_eq`) to distinguish "the same pool, updated in
    /// place" from "a fresh pool" — mirrors Go's `Manager.Pool`.
    pub fn get_pool(&self, code: &str) -> Option<Arc<ProcessPool>> {
        self.active_pool(code)
    }

    /// Start draining `pool` (already removed from `self.pools`, so its code
    /// is free), track it in [`Self::draining_pools`], and spawn a watcher
    /// that calls `pool.shutdown()` and drops it from the list the moment
    /// its buffered work finishes (`wait_drained()`), or backs off if the
    /// manager's own shutdown fires first (`shutdown()` already drains every
    /// tracked pool itself, so this task would otherwise double-act). Shared
    /// by `reload_config`'s removed-pool branch and `evict_idle_synth_pools`
    /// (R-59): both are "this code no longer routes to this pool" events and
    /// must drain identically (R-26/R-49: a removal drains, it never
    /// flushes).
    pub(super) async fn begin_pool_drain(self: &Arc<Self>, code: String, pool: Arc<ProcessPool>) {
        pool.drain().await;
        self.draining_pools.lock().push(pool.clone());

        let manager = self.clone();
        let token = self.shutdown.child_token();
        tokio::spawn(async move {
            tokio::select! {
                _ = pool.wait_drained() => {
                    pool.shutdown().await;
                    manager
                        .draining_pools
                        .lock()
                        .retain(|p| !Arc::ptr_eq(p, &pool));
                    info!(pool_code = %code, "Draining pool finished - removed");
                }
                _ = token.cancelled() => {}
            }
        });
    }

    /// Update a pool's concurrency and rate limit at runtime, IN PLACE (as
    /// `reload_config` does, and Go's `UpdatePool`). `config` is the full
    /// new setting: `rate_limit_per_minute: None` removes the limit. A code
    /// with no active pool gets one created with `config`.
    ///
    /// This used to build a second pool and swap it in without draining the
    /// first: both processed the same ordered groups concurrently at double
    /// the configured rate, and shutdown never saw the displaced one.
    pub async fn update_pool_config(&self, pool_code: &str, config: PoolConfig) -> Result<()> {
        match self.active_pool(pool_code) {
            Some(pool) => {
                if pool.concurrency() != config.concurrency && config.concurrency > 0 {
                    pool.update_concurrency(config.concurrency).await;
                }
                if pool.rate_limit_per_minute() != config.rate_limit_per_minute {
                    pool.update_rate_limit(config.rate_limit_per_minute);
                }
                info!(
                    pool_code = %pool_code,
                    concurrency = config.concurrency,
                    rate_limit = ?config.rate_limit_per_minute,
                    "Pool configuration updated in place"
                );
                self.capacity_notify().notify_waiters();
                Ok(())
            }
            None => {
                self.get_or_create_pool(pool_code, Some(config)).await?;
                Ok(())
            }
        }
    }

    /// The operator pool update behind `PUT /monitoring/pools/{code}` (Go:
    /// `Manager.UpdatePool`): `None` leaves a knob unchanged, and an
    /// unknown code is refused (`false`) rather than created.
    pub async fn update_pool(
        &self,
        pool_code: &str,
        concurrency: Option<u32>,
        rate_limit_per_minute: Option<u32>,
    ) -> bool {
        let Some(pool) = self.active_pool(pool_code) else {
            return false;
        };
        if let Some(n) = concurrency {
            if n == 0 {
                return false;
            }
            if n != pool.concurrency() && !pool.update_concurrency(n).await {
                return false;
            }
        }
        if rate_limit_per_minute.is_some() {
            pool.update_rate_limit(rate_limit_per_minute);
        }
        self.capacity_notify().notify_waiters();
        true
    }

    /// Get list of all pool codes (active only — see the `pools` field's
    /// doc comment).
    pub fn pool_codes(&self) -> Vec<String> {
        self.pools.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Number of pools currently draining (removed from config, still
    /// finishing in-flight work). Useful for stats/tests.
    pub fn draining_pool_count(&self) -> usize {
        self.draining_pools.lock().len()
    }

    /// Non-blocking check of whether a specific pool (active or draining)
    /// has finished draining — every worker/drain task it ever spawned has
    /// exited (see [`ProcessPool::is_fully_drained`]). Returns `None` if no
    /// pool with this code exists.
    pub fn is_pool_fully_drained(&self, code: &str) -> Option<bool> {
        self.active_pool(code)
            .or_else(|| {
                self.draining_pools
                    .lock()
                    .iter()
                    .find(|p| p.code() == code)
                    .cloned()
            })
            .map(|p| p.is_fully_drained())
    }
}

#[cfg(test)]
mod pool_creation_tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;

    /// Concurrent first messages for one code get ONE pool (Go creates
    /// under `poolMu`); the losers used to build and start pools that were
    /// then silently dropped from the map with their workers running.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_creation_yields_one_pool() {
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let m = manager.clone();
            handles.push(tokio::spawn(async move {
                m.get_or_create_pool("RACE", None).await.unwrap()
            }));
        }
        let mut pools = Vec::new();
        for h in handles {
            pools.push(h.await.unwrap());
        }
        assert!(pools.iter().all(|p| Arc::ptr_eq(p, &pools[0])));
        assert!(manager.draining_pools.lock().is_empty());
    }
}

/// Go `manager_missing_queue_test.go`: a queue the platform has not created
/// yet is neither a failure nor a warning, and is picked up when it appears.
#[cfg(test)]
mod missing_queue_tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;
    use crate::warning::WarningService;
    use async_trait::async_trait;
    use fc_common::{QueueConfig, QueuedMessage};
    use fc_queue::{QueueConsumer, QueueError};
    use std::sync::atomic::{AtomicBool, AtomicU32};
    use std::time::{Duration, Instant};
    use tokio::time::sleep;

    /// Polls empty, or fails with `err` when one is set.
    struct Consumer {
        id: String,
        polls: AtomicU32,
        missing: bool,
    }

    #[async_trait]
    impl QueueConsumer for Consumer {
        fn identifier(&self) -> &str {
            &self.id
        }
        async fn poll(&self, _: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(5)).await;
            if self.missing {
                return Err(QueueError::QueueMissing);
            }
            Ok(vec![])
        }
        async fn ack(&self, _: &str) -> fc_queue::Result<()> {
            Ok(())
        }
        async fn nack(&self, _: &str, _: Option<u32>) -> fc_queue::Result<()> {
            Ok(())
        }
        async fn extend_visibility(&self, _: &str, _: u32) -> fc_queue::Result<()> {
            Ok(())
        }
        fn is_healthy(&self) -> bool {
            true
        }
        async fn stop(&self) {}
    }

    /// `q-new` does not exist until `exists` is set; every other queue does.
    struct Factory {
        exists: AtomicBool,
    }

    #[async_trait]
    impl super::super::ConsumerFactory for Factory {
        async fn create_consumer(
            &self,
            config: &QueueConfig,
        ) -> crate::Result<Arc<dyn QueueConsumer>> {
            if config.name == "q-new" && !self.exists.load(Ordering::SeqCst) {
                return Err(RouterError::QueueMissing(config.name.clone()));
            }
            Ok(Arc::new(Consumer {
                id: config.name.clone(),
                polls: AtomicU32::new(0),
                missing: false,
            }))
        }
    }

    fn queue(name: &str) -> QueueConfig {
        QueueConfig {
            name: name.to_string(),
            uri: format!("test://{name}"),
            connections: 1,
            visibility_timeout: 30,
        }
    }

    fn manager(factory: Arc<Factory>, warnings: Arc<WarningService>) -> Arc<QueueManager> {
        Arc::new(
            QueueManager::builder(HttpMediatorConfig::dev())
                .warning_service(warnings)
                .consumer_factory(factory)
                .build(),
        )
    }

    #[tokio::test]
    async fn a_missing_queue_is_neither_a_failure_nor_a_warning_and_is_picked_up_when_it_appears() {
        let factory = Arc::new(Factory {
            exists: AtomicBool::new(false),
        });
        let warnings = Arc::new(WarningService::default());
        let m = manager(factory.clone(), warnings.clone());
        let config = RouterConfig {
            processing_pools: vec![],
            queues: vec![queue("q-live"), queue("q-new")],
        };

        assert!(
            m.reload_config(config.clone()).await.unwrap(),
            "a queue nobody has published to yet must not fail the apply"
        );
        assert!(
            m.consumers.get("q-live").is_some(),
            "the other queues are consumed"
        );
        assert!(
            m.consumers.get("q-new").is_none(),
            "no consumer for a queue that does not exist"
        );
        assert_eq!(m.missing_queue_names(), vec!["q-new".to_string()]);
        assert_eq!(
            warnings.warning_count(),
            0,
            "a missing queue raises no warning"
        );

        // Still absent: a recheck changes nothing, and stays silent.
        m.recheck_missing_queues().await;
        assert!(m.consumers.get("q-new").is_none());
        assert_eq!(warnings.warning_count(), 0);

        // The first publish creates the queue; an unchanged config is never
        // re-applied, so the recheck is what starts the consumer.
        factory.exists.store(true, Ordering::SeqCst);
        m.recheck_missing_queues().await;
        assert!(
            m.consumers.get("q-new").is_some(),
            "the consumer starts once the queue exists"
        );
        assert!(
            m.missing_queue_names().is_empty(),
            "and it is no longer tracked as missing"
        );
        assert_eq!(warnings.warning_count(), 0);
    }

    #[tokio::test]
    async fn a_queue_leaving_the_config_is_forgotten_as_missing() {
        let factory = Arc::new(Factory {
            exists: AtomicBool::new(false),
        });
        let m = manager(factory, Arc::new(WarningService::default()));
        let both = RouterConfig {
            processing_pools: vec![],
            queues: vec![queue("q-new")],
        };
        m.reload_config(both).await.unwrap();
        assert_eq!(m.missing_queue_names().len(), 1);
        m.reload_config(RouterConfig {
            processing_pools: vec![],
            queues: vec![],
        })
        .await
        .unwrap();
        assert!(m.missing_queue_names().is_empty());
    }

    /// A queue that disappears under a running consumer is retired, not
    /// reported as an erroring or stalled consumer, and comes back through
    /// the recheck.
    #[tokio::test]
    async fn a_queue_deleted_under_a_running_consumer_is_retired_quietly() {
        let warnings = Arc::new(WarningService::default());
        let m = manager(
            Arc::new(Factory {
                exists: AtomicBool::new(true),
            }),
            warnings.clone(),
        );
        let gone = Arc::new(Consumer {
            id: "q-gone".to_string(),
            polls: AtomicU32::new(0),
            missing: true,
        });
        let rc = m.new_running_consumer(gone.clone(), "q-gone".to_string(), Some(queue("q-gone")));
        m.consumers.insert(rc.clone());
        m.spawn_consumer_poll_task(rc);

        let deadline = Instant::now() + Duration::from_secs(2);
        while m.consumers.get("q-gone").is_some() {
            assert!(
                Instant::now() < deadline,
                "the consumer of a vanished queue is detached"
            );
            sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            gone.polls.load(Ordering::SeqCst),
            1,
            "and it stops polling at once"
        );
        assert_eq!(warnings.warning_count(), 0, "no warning");
        assert_eq!(m.missing_queue_names(), vec!["q-gone".to_string()]);

        // The recheck brings it back when it exists again.
        m.recheck_missing_queues().await;
        assert!(m.consumers.get("q-gone").is_some());
    }
}
