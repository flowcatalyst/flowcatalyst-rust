//! Config reconciliation: `apply_config` (initial setup) and `reload_config`
//! (hot reload) plus the machinery they share — consumer sync, pool
//! creation/lookup, and the drain-on-removal path (`begin_pool_drain`,
//! `cleanup_draining_pools`).

use std::collections::HashMap;
use std::sync::Arc;

use tracing::{error, info, warn};

use fc_common::{PoolConfig, RouterConfig, WarningCategory, WarningSeverity};

use crate::pool::ProcessPool;
use crate::Result;

use super::{ConsumerEntry, NewConsumerEntry, PoolState, QueueManager};

impl QueueManager {
    /// Apply router configuration (initial setup).
    ///
    /// Takes `self: &Arc<Self>` so `sync_queue_consumers` can spawn poll
    /// tasks for hot-added consumers. Callers already hold the manager
    /// behind an Arc.
    pub async fn apply_config(self: &Arc<Self>, config: RouterConfig) -> Result<()> {
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
        if !self.running.load(std::sync::atomic::Ordering::SeqCst) {
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
        let existing_codes: Vec<String> = self
            .pools
            .iter()
            .filter(|e| e.value().state == PoolState::Active)
            .map(|e| e.key().clone())
            .collect();
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
                            pool.update_concurrency(new_config.concurrency).await;
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
                if let Some((code, entry)) = self
                    .pools
                    .remove_if(&pool_code, |_, e| e.state == PoolState::Active)
                {
                    let pool = entry.pool;
                    info!(
                        pool_code = %code,
                        queue_size = pool.queue_size(),
                        active_workers = pool.active_workers(),
                        "Pool removed from config - draining asynchronously"
                    );
                    self.forget_synth_pool(&code);
                    pool_configs.remove(&code);
                    pools_removed += 1;

                    // begin_pool_drain: drains, marks the entry `Draining`
                    // (or orphans it — see `insert_draining_pool`), and
                    // spawns the watcher that calls `pool.shutdown()` the
                    // moment its
                    // in-flight work finishes — instead of waiting for the
                    // next `cleanup_draining_pools` sweep (only run
                    // periodically by the lifecycle manager's reaper — see
                    // that fn's doc comment, which is now the backstop
                    // rather than the primary path). Shared with
                    // `evict_idle_synth_pools` — see that method's doc
                    // comment for the full ownership/lifecycle writeup.
                    self.begin_pool_drain(code, pool).await;
                }
            }
        }

        // Step 2: Create new pools
        for pool_config in &config.processing_pools {
            let already_active = self
                .pools
                .get(&pool_config.code)
                .is_some_and(|e| e.state == PoolState::Active);
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
            total_active_pools = self.active_pool_count(),
            total_draining_pools = self.draining_pool_count(),
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
            let mut consumers_by_id = self.consumers_by_id.write().await;
            let mut queue_configs = self.queue_configs.write().await;

            let existing_queues: Vec<String> = consumers.keys().cloned().collect();
            let mut removed = Vec::new();
            for queue_id in &existing_queues {
                if !new_queue_configs.contains_key(queue_id) {
                    if let Some(consumer) = consumers.remove(queue_id) {
                        queue_configs.remove(queue_id);
                        // G10: keep the identifier-keyed index in lockstep.
                        consumers_by_id.remove(consumer.identifier());
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
            let mut consumers_by_id = self.consumers_by_id.write().await;
            let mut queue_configs = self.queue_configs.write().await;
            for (queue_id, consumer, queue_config) in &new_consumers {
                consumers.insert(queue_id.clone(), consumer.clone());
                // G10: keep the identifier-keyed index in lockstep.
                consumers_by_id.insert(consumer.identifier().to_string(), consumer.clone());
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

    /// Cleanup draining pools that have finished — both `Draining` entries
    /// still in `pools` and any predecessor that landed in
    /// `orphaned_draining` (see that field's doc comment).
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
        let mut cleaned = Vec::new();

        for entry in self.pools.iter() {
            if entry.value().state != PoolState::Draining {
                continue;
            }
            let pool = entry.value().pool.clone();
            if pool.is_fully_drained() {
                info!(pool_code = %entry.key(), "Draining pool finished - cleaning up");
                pool.shutdown().await;
                cleaned.push((entry.key().clone(), pool));
            }
        }

        for (code, pool) in cleaned {
            // Identity-checked: only remove this exact still-Draining
            // instance, never a newer Active entry that has since taken
            // over the same code (see the `pools` field's doc comment).
            self.pools.remove_if(&code, |_, e| {
                e.state == PoolState::Draining && Arc::ptr_eq(&e.pool, &pool)
            });
        }

        // Orphaned predecessors — displaced from `pools` by a later Active
        // insert (or, more rarely, by a second Draining insert) under the
        // same code. Swept by identity rather than by map key.
        let finished: Vec<Arc<ProcessPool>> = {
            let mut guard = self.orphaned_draining.lock();
            let (done, remaining): (Vec<_>, Vec<_>) =
                guard.drain(..).partition(|p| p.is_fully_drained());
            *guard = remaining;
            done
        };
        for pool in finished {
            info!(pool_code = %pool.code(), "Orphaned draining pool finished - cleaning up");
            pool.shutdown().await;
        }
    }

    /// Get or create a pool by code
    pub(super) async fn get_or_create_pool(
        &self,
        code: &str,
        config: Option<PoolConfig>,
    ) -> Result<Arc<ProcessPool>> {
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
            .with_capacity_notify(self.capacity_notify().clone());

        let pool_arc = Arc::new(pool);
        pool_arc.start().await;

        self.insert_active_pool(code.to_string(), pool_arc.clone());
        info!(pool_code = %code, concurrency = pool_config.concurrency, "Created process pool");

        Ok(pool_arc)
    }

    /// Returns the currently *active* pool for `code` (never one still
    /// draining), or `None`. Exposed mainly for tests that need pool
    /// identity (`Arc::ptr_eq`) to distinguish "the same pool, updated in
    /// place" from "a fresh pool" — mirrors Go's `Manager.Pool`.
    pub fn get_pool(&self, code: &str) -> Option<Arc<ProcessPool>> {
        self.active_pool(code)
    }

    /// Move `pool` (already removed from `self.pools`) into `Draining`
    /// state (via [`Self::insert_draining_pool`], which also resolves the
    /// rare race against a concurrent creator claiming the same code — see
    /// that method's doc) and spawn a watcher that calls `pool.shutdown()`
    /// the moment its buffered work finishes (`wait_drained()`), or backs
    /// off if the manager's own shutdown fires first (`shutdown()` already
    /// drains every tracked pool itself, including `orphaned_draining`, so
    /// this task would otherwise double-act). Shared by `reload_config`'s
    /// removed-pool branch and `evict_idle_synth_pools` (R-59) — both are
    /// "this code no longer routes to this pool" events and must drain
    /// identically (R-26/R-49: a removal drains, it never flushes).
    pub(super) async fn begin_pool_drain(self: &Arc<Self>, code: String, pool: Arc<ProcessPool>) {
        pool.drain().await;
        self.insert_draining_pool(code.clone(), pool.clone());

        let manager = self.clone();
        let watched_pool = pool;
        let watched_code = code;
        let token = self.shutdown.child_token();
        tokio::spawn(async move {
            tokio::select! {
                _ = watched_pool.wait_drained() => {
                    watched_pool.shutdown().await;
                    // Identity-checked: never remove a newer Active entry
                    // that has since taken over this code (see the `pools`
                    // field's doc comment).
                    manager.pools.remove_if(&watched_code, |_, e| {
                        e.state == PoolState::Draining && Arc::ptr_eq(&e.pool, &watched_pool)
                    });
                    info!(pool_code = %watched_code, "Draining pool finished - removed");
                }
                _ = token.cancelled() => {}
            }
        });
    }

    /// Update pool configuration at runtime (hot-reload)
    /// Note: Concurrency changes take effect on next message batch
    /// Rate limit changes take effect immediately
    pub async fn update_pool_config(&self, pool_code: &str, config: PoolConfig) -> Result<()> {
        // Check if pool exists and get current settings
        // IMPORTANT: Drop the Ref guard before calling insert() to avoid deadlock
        let pool_exists = if let Some(existing_pool) = self.active_pool(pool_code) {
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
            // `build_mediator` shares the manager's single registry (see
            // `get_or_create_pool`) — a reconfigured pool's fresh mediator
            // keeps recording into it, not a private default.
            let new_pool = ProcessPool::new(config.clone(), self.build_mediator())
                .with_capacity_notify(self.capacity_notify().clone());
            let pool_arc = Arc::new(new_pool);
            pool_arc.start().await;

            // Replace the old pool. `pool_exists` required an `Active`
            // entry above, so this can't displace a `Draining` one — routed
            // through `insert_active_pool` anyway for consistency with
            // every other Active-pool insert site.
            self.insert_active_pool(pool_code.to_string(), pool_arc);

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

    /// Get list of all pool codes (active only — see the `pools` field's
    /// doc comment).
    pub fn pool_codes(&self) -> Vec<String> {
        self.pools
            .iter()
            .filter(|e| e.value().state == PoolState::Active)
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Number of pools currently draining (removed from config, still
    /// finishing in-flight work). Useful for stats/tests.
    pub fn draining_pool_count(&self) -> usize {
        self.pools
            .iter()
            .filter(|e| e.value().state == PoolState::Draining)
            .count()
    }

    /// Non-blocking check of whether a specific pool (active or draining)
    /// has finished draining — every worker/drain task it ever spawned has
    /// exited (see [`ProcessPool::is_fully_drained`]). Returns `None` if no
    /// pool with this code exists.
    pub fn is_pool_fully_drained(&self, code: &str) -> Option<bool> {
        self.pools.get(code).map(|e| e.pool.is_fully_drained())
    }
}
