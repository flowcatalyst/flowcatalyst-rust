//! Outbox Processor
//!
//! Moves rows from the application's outbox table to the platform, as Go's
//! `outbox.Processor` (`flowcatalyst-go/internal/outbox/processor.go`):
//!
//! ```text
//! repo.claim_pending → (grouped)   GroupDistributor → one item at a time ─┐
//!                    → (ungrouped) one batch request per item type ───────┤
//!                                                                         ↓
//!                          repo.mark_success (delete) / repo.mark_failed / repo.release
//! ```
//!
//! - **Claim.** Each poll claims up to `poll_batch_size` PENDING rows of every
//!   type and marks them IN_PROGRESS in one atomic statement; a claimed row is
//!   not polled again while it is in flight.
//! - **Outcome first, then the row.** A row is deleted only after the platform
//!   accepted it. A failure bumps `retry_count` and stores the error: a
//!   retryable status (INTERNAL_ERROR, UNAUTHORIZED, GATEWAY_ERROR) goes back
//!   to PENDING while `retry_count + 1 < max_retries`, and otherwise the row
//!   keeps its failure status and is not claimed again. BAD_REQUEST and
//!   FORBIDDEN are terminal at once. There is no backoff between attempts,
//!   as in Go: a re-queued row is re-claimed by the next poll.
//! - **Groups.** Items with a `message_group` are sent one at a time, in order.
//!   With block-on-error a failed item stops its group (the rest are released
//!   to PENDING and re-claimed in order behind it), and a terminal failure
//!   blocks the group until an operator unblocks or skips it.
//! - **Recovery.** Every `recovery_interval`, rows IN_PROGRESS for longer than
//!   `processing_timeout_seconds` (their processor died) go back to PENDING.
//!
//! The database is the only state that matters: a restart loses nothing,
//! because every row in memory is also a claimed row the recovery returns.

use async_trait::async_trait;
use fc_common::config::{env_bool, env_first};
use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::MissedTickBehavior;
use tracing::{debug, error, info, warn};

use crate::group_distributor::{DistributorStats, GroupDistributor, GroupHandler};
use crate::group_state::{BlockedItem, GroupInfo, GroupStateManager};
use crate::http_dispatcher::{
    DispatchOutcome, HttpDispatcher, HttpDispatcherConfig, OutboxDispatcher, MAX_PLATFORM_BATCH,
};
use crate::repository::{InvalidRow, OutboxRepository};
use crate::LeaderElectionConfig;

#[cfg(feature = "standby")]
use fc_standby::{LeaderElection, LeadershipStatus};

/// Outbox processor configuration. Defaults are Go's `DefaultConfig`.
#[derive(Debug, Clone)]
pub struct EnhancedProcessorConfig {
    /// Polling interval (Go: 1s)
    pub poll_interval: Duration,
    /// Rows claimed per poll (Go `BatchSize`: 100)
    pub poll_batch_size: u32,
    /// Most items in one request for ungrouped items of one type (default
    /// 100; never more than the platform's 1000).
    pub api_batch_size: usize,
    /// Groups sending at once (Go `MaxConcurrentGroups`: 10; 0 = unbounded)
    pub max_concurrent_groups: usize,
    /// No poll while this many items are in flight (Go `MaxInFlight`: 1000)
    pub max_in_flight: u64,
    /// Attempts before a retryable failure is final (Go `MaxRetries`: 3)
    pub max_retries: u32,
    /// Seconds a row may stay IN_PROGRESS before recovery returns it to
    /// PENDING (Go `RecoveryThreshold`: 5 minutes)
    pub processing_timeout_seconds: u64,
    /// How often recovery runs (Go `RecoveryInterval`: 60s)
    pub recovery_interval: Duration,
    /// Stop a group at its first failed item (Go `BlockOnError`: true)
    pub block_on_error: bool,
    /// HTTP dispatcher config
    pub http_config: HttpDispatcherConfig,
    /// Leader election config
    pub leader_election: LeaderElectionConfig,
}

impl Default for EnhancedProcessorConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            poll_batch_size: 100,
            api_batch_size: 100,
            max_concurrent_groups: 10,
            max_in_flight: 1000,
            max_retries: 3,
            processing_timeout_seconds: 300,
            recovery_interval: Duration::from_secs(60),
            block_on_error: true,
            http_config: HttpDispatcherConfig::default(),
            // Default to leader election disabled — consumers that want HA
            // should construct a `LeaderElectionConfig` explicitly.
            leader_election: LeaderElectionConfig::default().with_enabled(false),
        }
    }
}

impl EnhancedProcessorConfig {
    /// The configuration from the environment, reading Go's variable names
    /// first and this processor's earlier names as fallbacks, so either
    /// deployment's environment drops in:
    ///
    /// | setting | variables |
    /// |---|---|
    /// | platform URL | `FC_OUTBOX_PLATFORM_URL`, `FC_OUTBOX_API_URL`, `FC_API_BASE_URL`, `FLOWCATALYST_URL` |
    /// | bearer token | `FC_OUTBOX_PLATFORM_AUTH_TOKEN`, `FC_OUTBOX_TOKEN`, `FC_API_TOKEN` |
    /// | rows per poll | `FC_OUTBOX_BATCH_SIZE` |
    /// | items per request | `FC_API_BATCH_SIZE` |
    /// | max in flight | `FC_OUTBOX_MAX_IN_FLIGHT`, `FC_MAX_IN_FLIGHT` |
    /// | poll interval | `FC_OUTBOX_POLL_INTERVAL_MS` |
    /// | concurrent groups | `FC_OUTBOX_MAX_CONCURRENT_GROUPS`, `FC_MAX_CONCURRENT_GROUPS` |
    /// | block on error | `FC_OUTBOX_BLOCK_ON_ERROR` |
    /// | max retries | `FC_OUTBOX_MAX_RETRIES` |
    ///
    /// Unset, empty, unparseable or zero numbers keep the default, as Go's.
    pub fn from_env() -> Self {
        let defaults = Self::default();
        let positive = |keys: &[&str]| -> Option<u64> {
            let raw = env_first(keys, "");
            raw.trim().parse::<u64>().ok().filter(|n| *n > 0)
        };
        let url = env_first(
            &[
                "FC_OUTBOX_PLATFORM_URL",
                "FC_OUTBOX_API_URL",
                "FC_API_BASE_URL",
                "FLOWCATALYST_URL",
            ],
            &defaults.http_config.api_base_url,
        );
        let token = env_first(
            &[
                "FC_OUTBOX_PLATFORM_AUTH_TOKEN",
                "FC_OUTBOX_TOKEN",
                "FC_API_TOKEN",
            ],
            "",
        );
        Self {
            poll_interval: positive(&["FC_OUTBOX_POLL_INTERVAL_MS"])
                .map(Duration::from_millis)
                .unwrap_or(defaults.poll_interval),
            poll_batch_size: positive(&["FC_OUTBOX_BATCH_SIZE"])
                .map(|n| n.min(u32::MAX as u64) as u32)
                .unwrap_or(defaults.poll_batch_size),
            api_batch_size: positive(&["FC_API_BATCH_SIZE"])
                .map(|n| n as usize)
                .unwrap_or(defaults.api_batch_size),
            max_concurrent_groups: positive(&[
                "FC_OUTBOX_MAX_CONCURRENT_GROUPS",
                "FC_MAX_CONCURRENT_GROUPS",
            ])
            .map(|n| n as usize)
            .unwrap_or(defaults.max_concurrent_groups),
            max_in_flight: positive(&["FC_OUTBOX_MAX_IN_FLIGHT", "FC_MAX_IN_FLIGHT"])
                .unwrap_or(defaults.max_in_flight),
            max_retries: positive(&["FC_OUTBOX_MAX_RETRIES"])
                .map(|n| n.min(u32::MAX as u64) as u32)
                .unwrap_or(defaults.max_retries),
            block_on_error: env_bool("FC_OUTBOX_BLOCK_ON_ERROR", defaults.block_on_error),
            http_config: HttpDispatcherConfig {
                api_base_url: url,
                api_token: Some(token).filter(|t| !t.is_empty()),
                ..defaults.http_config.clone()
            },
            ..defaults
        }
    }
}

/// Processor metrics
#[derive(Debug, Clone, Default)]
pub struct ProcessorMetrics {
    pub items_polled: u64,
    pub items_processed: u64,
    pub items_succeeded: u64,
    pub items_failed: u64,
    pub items_released: u64,
    pub items_recovered: u64,
    pub current_in_flight: u64,
    pub active_groups: usize,
    pub blocked_groups: usize,
}

#[derive(Default)]
struct Counters {
    polled: AtomicU64,
    succeeded: AtomicU64,
    failed: AtomicU64,
    released: AtomicU64,
    recovered: AtomicU64,
}

/// What every poll and dispatch shares.
struct Core {
    config: EnhancedProcessorConfig,
    repository: Arc<dyn OutboxRepository>,
    dispatcher: Arc<dyn OutboxDispatcher>,
    distributor: GroupDistributor,
    groups: GroupStateManager,
    /// Ids of rows this process holds (claimed and not yet resolved). The
    /// in-flight count is its size. A row re-claimed while still held (it
    /// sat longer than the recovery threshold) is left to the holder.
    in_flight: Mutex<HashSet<String>>,
    counters: Counters,
}

/// Outbox processor: claim → per-group FIFO / per-type batch → HTTP → row.
pub struct EnhancedOutboxProcessor {
    core: Arc<Core>,
    is_primary: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl EnhancedOutboxProcessor {
    pub fn new(
        config: EnhancedProcessorConfig,
        repository: Arc<dyn OutboxRepository>,
    ) -> anyhow::Result<Self> {
        let dispatcher = Arc::new(HttpDispatcher::new(config.http_config.clone())?);
        Ok(Self::with_dispatcher(config, repository, dispatcher))
    }

    /// A processor sending through `dispatcher` instead of HTTP.
    pub fn with_dispatcher(
        config: EnhancedProcessorConfig,
        repository: Arc<dyn OutboxRepository>,
        dispatcher: Arc<dyn OutboxDispatcher>,
    ) -> Self {
        let is_primary = Arc::new(AtomicBool::new(!config.leader_election.enabled));
        let distributor =
            GroupDistributor::new(config.max_concurrent_groups, config.block_on_error);
        Self {
            core: Arc::new(Core {
                config,
                repository,
                dispatcher,
                distributor,
                groups: GroupStateManager::new(),
                in_flight: Mutex::default(),
                counters: Counters::default(),
            }),
            is_primary,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if this processor is the current leader
    pub fn is_primary(&self) -> bool {
        self.is_primary.load(Ordering::SeqCst)
    }

    /// Set the primary status (called by leader election)
    pub fn set_primary(&self, primary: bool) {
        self.is_primary.store(primary, Ordering::SeqCst);
        if primary {
            info!("Outbox processor became primary");
        } else {
            warn!("Outbox processor lost primary status");
        }
    }

    /// Get the is_primary flag for leader election
    pub fn is_primary_flag(&self) -> Arc<AtomicBool> {
        self.is_primary.clone()
    }

    /// Items claimed by this process and not yet resolved.
    pub fn in_flight_count(&self) -> u64 {
        self.core.in_flight_count()
    }

    /// Get current metrics
    pub async fn metrics(&self) -> ProcessorMetrics {
        let c = &self.core.counters;
        let succeeded = c.succeeded.load(Ordering::Relaxed);
        let failed = c.failed.load(Ordering::Relaxed);
        ProcessorMetrics {
            items_polled: c.polled.load(Ordering::Relaxed),
            items_processed: succeeded + failed,
            items_succeeded: succeeded,
            items_failed: failed,
            items_released: c.released.load(Ordering::Relaxed),
            items_recovered: c.recovered.load(Ordering::Relaxed),
            current_in_flight: self.in_flight_count(),
            active_groups: self.core.distributor.stats().active_groups,
            blocked_groups: self.core.groups.blocked().len(),
        }
    }

    /// Get distributor stats
    pub fn distributor_stats(&self) -> DistributorStats {
        self.core.distributor.stats()
    }

    // ── Operational state machine (Go `PauseGroup` … `SkipGroup`) ──

    /// Stops sending a group; its items are released to PENDING each poll
    /// until resumed. No-op when the group is Blocked.
    pub fn pause_group(&self, group: &str) {
        self.core.groups.pause(group);
    }

    /// Resumes a Paused group.
    pub fn resume_group(&self, group: &str) {
        self.core.groups.resume(group);
    }

    /// Clears a Blocked group and re-queues the item it was blocked on for a
    /// fresh attempt, so the whole group runs again in order. `false` if the
    /// group wasn't Blocked.
    pub async fn unblock_group(&self, group: &str) -> bool {
        let Some(item) = self.core.groups.clear_block(group) else {
            return false;
        };
        if let Err(e) = self
            .core
            .repository
            .requeue(item.item_type, std::slice::from_ref(&item.id))
            .await
        {
            warn!(group, id = %item.id, error = %e, "Outbox unblock re-queue failed");
        }
        info!(group, id = %item.id, "Outbox group unblocked (item re-queued)");
        true
    }

    /// Clears a Blocked group without re-queuing its item, which stays
    /// failed; the group advances past it. `false` if it wasn't Blocked.
    pub fn skip_group(&self, group: &str) -> bool {
        match self.core.groups.clear_block(group) {
            Some(item) => {
                info!(group, id = %item.id, "Outbox group skipped its blocking item");
                true
            }
            None => false,
        }
    }

    /// Every Paused or Blocked group.
    pub fn group_states(&self) -> Vec<GroupInfo> {
        self.core.groups.snapshot()
    }

    /// Only the Blocked groups.
    pub fn blocked_groups(&self) -> Vec<GroupInfo> {
        self.core.groups.blocked()
    }

    /// Runs one poll: claim and hand out. Exposed for tests and callers
    /// driving their own loop; [`Self::start`] calls it every poll interval.
    pub async fn poll_once(&self) -> anyhow::Result<()> {
        Arc::clone(&self.core).poll().await
    }

    /// Runs one recovery pass: returns rows stuck IN_PROGRESS to PENDING.
    pub async fn recover_once(&self) -> anyhow::Result<u64> {
        self.core.recover().await
    }

    /// Start the processor (runs until stopped, or until the future is dropped)
    pub async fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            warn!("Processor already running");
            return;
        }
        let c = &self.core.config;
        info!(
            poll_interval_ms = %c.poll_interval.as_millis(),
            poll_batch_size = %c.poll_batch_size,
            max_in_flight = %c.max_in_flight,
            max_concurrent_groups = %c.max_concurrent_groups,
            max_retries = %c.max_retries,
            block_on_error = %c.block_on_error,
            "Starting outbox processor"
        );
        self.run_loop().await;
        info!("Outbox processor stopped");
    }

    /// Stop the processor
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if processor is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Go's `Run`: two tickers, polling and recovery, both only while this
    /// processor is primary.
    async fn run_loop(&self) {
        let mut poll = tokio::time::interval(self.core.config.poll_interval);
        poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let recovery_every = if self.core.config.recovery_interval.is_zero() {
            Duration::from_secs(60)
        } else {
            self.core.config.recovery_interval
        };
        let mut recovery =
            tokio::time::interval_at(tokio::time::Instant::now() + recovery_every, recovery_every);
        recovery.set_missed_tick_behavior(MissedTickBehavior::Delay);

        while self.running.load(Ordering::SeqCst) {
            tokio::select! {
                _ = poll.tick() => {
                    if !self.is_primary() {
                        continue;
                    }
                    if let Err(e) = Arc::clone(&self.core).poll().await {
                        warn!(error = %e, "Outbox claim failed");
                    }
                }
                _ = recovery.tick() => {
                    if !self.is_primary() {
                        continue;
                    }
                    if let Err(e) = self.core.recover().await {
                        warn!(error = %e, "Outbox recover stuck failed");
                    }
                }
            }
        }
    }

    /// Start the processor with hot standby integration: it polls only while
    /// `leader_election` says this instance leads.
    #[cfg(feature = "standby")]
    pub async fn start_with_standby(self: Arc<Self>, leader_election: Arc<LeaderElection>) {
        let is_primary = Arc::clone(&self.is_primary);
        let mut status_rx = leader_election.subscribe();
        let watcher = tokio::spawn(async move {
            while status_rx.changed().await.is_ok() {
                let is_leader = *status_rx.borrow() == LeadershipStatus::Leader;
                let was_leader = is_primary.swap(is_leader, Ordering::SeqCst);
                if is_leader && !was_leader {
                    info!("Outbox processor became leader - starting active processing");
                } else if !is_leader && was_leader {
                    warn!("Outbox processor lost leadership - entering standby mode");
                }
            }
        });
        self.is_primary
            .store(leader_election.is_leader(), Ordering::SeqCst);
        self.start().await;
        watcher.abort();
    }
}

impl Core {
    fn in_flight_count(&self) -> u64 {
        self.in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len() as u64
    }

    /// No longer held by this process.
    fn done(&self, ids: impl IntoIterator<Item = String>) {
        let mut held = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        for id in ids {
            held.remove(&id);
        }
    }

    fn max_retries(&self) -> i64 {
        if self.config.max_retries == 0 {
            3
        } else {
            self.config.max_retries as i64
        }
    }

    /// Whether a failed attempt goes back to PENDING: a retryable status, and
    /// this attempt (`retry_count + 1`) is not the last one (Go OB6).
    fn requeues(&self, item: &OutboxItem, status: OutboxStatus) -> bool {
        status.is_retryable() && (item.retry_count as i64) + 1 < self.max_retries()
    }

    async fn recover(&self) -> anyhow::Result<u64> {
        let threshold = if self.config.processing_timeout_seconds == 0 {
            Duration::from_secs(300)
        } else {
            Duration::from_secs(self.config.processing_timeout_seconds)
        };
        let count = self.repository.recover_stuck(threshold).await?;
        if count > 0 {
            info!(count, "Outbox recovered stuck items");
            self.counters.recovered.fetch_add(count, Ordering::Relaxed);
        }
        Ok(count)
    }

    /// Go's `tick`: claim, then hand grouped items to their groups and send
    /// ungrouped items as one batch per type.
    async fn poll(self: Arc<Self>) -> anyhow::Result<()> {
        if self.in_flight_count() >= self.config.max_in_flight {
            debug!("Outbox poll skipped: max in flight");
            return Ok(());
        }

        let claimed = self
            .repository
            .claim_pending(self.config.poll_batch_size.max(1))
            .await?;
        if claimed.is_empty() {
            return Ok(());
        }
        self.counters
            .polled
            .fetch_add(claimed.len() as u64, Ordering::Relaxed);

        for invalid in &claimed.invalid {
            self.fail_invalid(invalid).await;
        }

        // UPDATE … RETURNING order isn't the claim's order: restore it, so
        // each group's items reach the distributor oldest first.
        let mut items = claimed.items;
        items.sort_by(|a, b| {
            (&a.message_group, a.created_at, &a.id).cmp(&(&b.message_group, b.created_at, &b.id))
        });

        // Rows this process still holds (recovered while queued, then
        // re-claimed) stay with the holder: not sent twice.
        let items: Vec<OutboxItem> = {
            let mut held = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
            items
                .into_iter()
                .filter(|item| {
                    let fresh = held.insert(item.id.clone());
                    if !fresh {
                        debug!(id = %item.id, "Outbox row re-claimed while held; left to its holder");
                    }
                    fresh
                })
                .collect()
        };

        let mut to_release: Vec<OutboxItem> = Vec::new();
        let mut by_type: HashMap<OutboxItemType, Vec<OutboxItem>> = HashMap::new();
        let handler: Arc<dyn GroupHandler> = self.clone();
        for item in items {
            match item.message_group.clone() {
                Some(group) => {
                    // A Paused or Blocked group's items go back to PENDING
                    // (re-claimed once it runs again), never past a block.
                    if self.groups.is_active(&group) {
                        self.distributor.submit(&group, item, Arc::clone(&handler));
                    } else {
                        to_release.push(item);
                    }
                }
                None => by_type.entry(item.item_type).or_default().push(item),
            }
        }

        if !to_release.is_empty() {
            self.release_items(to_release).await;
        }

        let chunk = self.config.api_batch_size.clamp(1, MAX_PLATFORM_BATCH);
        for (_, batch) in by_type {
            let mut batch = batch;
            while !batch.is_empty() {
                let rest = batch.split_off(batch.len().min(chunk));
                let core = Arc::clone(&self);
                let this = std::mem::replace(&mut batch, rest);
                tokio::spawn(async move { core.dispatch_batch(this).await });
            }
        }
        Ok(())
    }

    /// A row whose payload can't be read fails terminally (BAD_REQUEST), as
    /// Go's marshal failure does, and blocks its group.
    async fn fail_invalid(&self, row: &InvalidRow) {
        warn!(id = %row.id, error = %row.error, "Outbox row has an unreadable payload");
        if let Err(e) = self
            .repository
            .mark_failed(
                row.item_type,
                std::slice::from_ref(&row.id),
                OutboxStatus::BadRequest,
                &row.error,
                false,
            )
            .await
        {
            warn!(id = %row.id, error = %e, "Outbox mark failed");
        }
        self.counters.failed.fetch_add(1, Ordering::Relaxed);
        if let (true, Some(group)) = (self.config.block_on_error, &row.message_group) {
            self.block(group, row.id.clone(), row.item_type, &row.error);
        }
    }

    fn block(&self, group: &str, id: String, item_type: OutboxItemType, error: &str) {
        warn!(group, %id, error, "Outbox message group blocked");
        self.groups
            .block(group, BlockedItem { id, item_type }, error);
    }

    /// Go's `dispatchBatch`: ungrouped items of one type in one request;
    /// successes deleted together, failures recorded together per outcome.
    async fn dispatch_batch(&self, batch: Vec<OutboxItem>) {
        let item_type = batch[0].item_type;
        let outcomes = self.dispatcher.send_batch(&batch).await;

        let mut succeeded: Vec<String> = Vec::new();
        let mut failed: HashMap<(OutboxStatus, String, bool), Vec<String>> = HashMap::new();
        for (index, item) in batch.iter().enumerate() {
            let outcome = outcomes.get(index).cloned().unwrap_or_else(|| {
                DispatchOutcome::failed(OutboxStatus::InternalError, "no per-item result")
            });
            if outcome.is_success() {
                succeeded.push(item.id.clone());
                continue;
            }
            let requeue = self.requeues(item, outcome.status);
            failed
                .entry((outcome.status, outcome.message, requeue))
                .or_default()
                .push(item.id.clone());
        }

        for ((status, message, requeue), ids) in &failed {
            if let Err(e) = self
                .repository
                .mark_failed(item_type, ids, *status, message, *requeue)
                .await
            {
                warn!(count = ids.len(), error = %e, "Outbox mark failed");
            }
            self.counters
                .failed
                .fetch_add(ids.len() as u64, Ordering::Relaxed);
        }
        if !succeeded.is_empty() {
            match self.repository.mark_success(item_type, &succeeded).await {
                Ok(()) => {
                    self.counters
                        .succeeded
                        .fetch_add(succeeded.len() as u64, Ordering::Relaxed);
                }
                // The rows stay IN_PROGRESS and are recovered and re-sent;
                // the platform's supplied-id handling makes that a no-op.
                Err(e) => error!(count = succeeded.len(), error = %e, "Outbox mark success failed"),
            }
        }
        self.done(batch.into_iter().map(|i| i.id));
    }

    async fn release_items(&self, items: Vec<OutboxItem>) {
        let mut by_type: HashMap<OutboxItemType, Vec<String>> = HashMap::new();
        for item in &items {
            by_type
                .entry(item.item_type)
                .or_default()
                .push(item.id.clone());
        }
        for (item_type, ids) in &by_type {
            if let Err(e) = self.repository.release(*item_type, ids).await {
                warn!(count = ids.len(), error = %e, "Outbox release failed");
            }
            self.counters
                .released
                .fetch_add(ids.len() as u64, Ordering::Relaxed);
        }
        self.done(items.into_iter().map(|i| i.id));
    }

    /// Go's `dispatch`: one grouped item. `false` stops its group.
    async fn dispatch_one(&self, item: &OutboxItem) -> bool {
        let outcome = self
            .dispatcher
            .send_batch(std::slice::from_ref(item))
            .await
            .into_iter()
            .next()
            .unwrap_or_else(|| {
                DispatchOutcome::failed(OutboxStatus::InternalError, "no outcome for item")
            });

        if outcome.is_success() {
            return match self
                .repository
                .mark_success(item.item_type, std::slice::from_ref(&item.id))
                .await
            {
                Ok(()) => {
                    self.counters.succeeded.fetch_add(1, Ordering::Relaxed);
                    true
                }
                Err(e) => {
                    error!(id = %item.id, error = %e, "Outbox mark success failed");
                    false
                }
            };
        }

        let requeue = self.requeues(item, outcome.status);
        if let Err(e) = self
            .repository
            .mark_failed(
                item.item_type,
                std::slice::from_ref(&item.id),
                outcome.status,
                &outcome.message,
                requeue,
            )
            .await
        {
            warn!(id = %item.id, error = %e, "Outbox mark failed");
        }
        self.counters.failed.fetch_add(1, Ordering::Relaxed);

        // A final failure of a grouped item blocks its group until an
        // operator unblocks or skips it: the group never silently advances
        // past it.
        if !requeue && self.config.block_on_error {
            if let Some(group) = &item.message_group {
                self.block(group, item.id.clone(), item.item_type, &outcome.message);
            }
        }
        false
    }
}

#[async_trait]
impl GroupHandler for Core {
    async fn dispatch(&self, item: OutboxItem) -> bool {
        let ok = self.dispatch_one(&item).await;
        self.done([item.id]);
        ok
    }

    async fn release(&self, items: Vec<OutboxItem>) {
        self.release_items(items).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_gos() {
        let config = EnhancedProcessorConfig::default();
        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.poll_batch_size, 100);
        assert_eq!(config.max_in_flight, 1000);
        assert_eq!(config.max_concurrent_groups, 10);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.processing_timeout_seconds, 300);
        assert_eq!(config.recovery_interval, Duration::from_secs(60));
        assert!(config.block_on_error);
        assert_eq!(config.http_config.request_timeout, Duration::from_secs(30));
    }

    /// The only test that touches these variables (tests run in parallel).
    #[test]
    fn from_env_reads_gos_names_first_then_the_earlier_ones() {
        let vars = [
            ("FC_OUTBOX_PLATFORM_URL", "http://go-name"),
            ("FC_API_BASE_URL", "http://old-name"),
            ("FC_API_TOKEN", "old-token"),
            ("FC_OUTBOX_BATCH_SIZE", "250"),
            ("FC_MAX_IN_FLIGHT", "77"),
            ("FC_OUTBOX_MAX_CONCURRENT_GROUPS", "0"),
            ("FC_OUTBOX_BLOCK_ON_ERROR", "false"),
            ("FC_OUTBOX_POLL_INTERVAL_MS", "not a number"),
        ];
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let config = EnhancedProcessorConfig::from_env();
        for (k, _) in vars {
            std::env::remove_var(k);
        }
        assert_eq!(config.http_config.api_base_url, "http://go-name");
        assert_eq!(config.http_config.api_token.as_deref(), Some("old-token"));
        assert_eq!(config.poll_batch_size, 250);
        assert_eq!(config.max_in_flight, 77);
        // Zero and unparseable keep the default, as Go's.
        assert_eq!(config.max_concurrent_groups, 10);
        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert!(!config.block_on_error);
    }
}
