//! Enhanced Outbox Processor
//!
//! Matches the Java outbox processor architecture:
//! - Polls database for pending items
//! - Routes through GlobalBuffer and GroupDistributor
//! - Sends to FlowCatalyst HTTP API (not directly to SQS)
//! - Implements maxInFlight backpressure
//! - Supports message group FIFO ordering
//! - Supports hot standby via fc-standby crate

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::RwLock;
use fc_common::OutboxStatus;
use tracing::{info, debug, warn, error};

use crate::repository::OutboxRepository;
use crate::buffer::{GlobalBuffer, GlobalBufferConfig};
use crate::group_distributor::{GroupDistributor, GroupDistributorConfig, DistributorStats};
use crate::message_group_processor::MessageGroupProcessorConfig;
use crate::http_dispatcher::{HttpDispatcher, HttpDispatcherConfig};
use crate::LeaderElectionConfig;

#[cfg(feature = "standby")]
use fc_standby::{LeaderElection, LeadershipStatus};

/// Enhanced outbox processor configuration
#[derive(Debug, Clone)]
pub struct EnhancedProcessorConfig {
    /// Polling interval
    pub poll_interval: Duration,
    /// Items fetched per poll
    pub poll_batch_size: u32,
    /// Items sent per API call
    pub api_batch_size: usize,
    /// Maximum concurrent message groups
    pub max_concurrent_groups: usize,
    /// Global buffer capacity
    pub global_buffer_size: usize,
    /// Maximum items in flight (backpressure)
    pub max_in_flight: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Processing timeout before recovery (seconds)
    pub processing_timeout_seconds: u64,
    /// Recovery check interval
    pub recovery_interval: Duration,
    /// HTTP dispatcher config
    pub http_config: HttpDispatcherConfig,
    /// Leader election config
    pub leader_election: LeaderElectionConfig,
}

impl Default for EnhancedProcessorConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            poll_batch_size: 500,
            api_batch_size: 100,
            max_concurrent_groups: 10,
            global_buffer_size: 1000,
            max_in_flight: 5000,
            max_retries: 3,
            processing_timeout_seconds: 300,
            recovery_interval: Duration::from_secs(60),
            http_config: HttpDispatcherConfig::default(),
            leader_election: LeaderElectionConfig::default(),
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
    pub items_recovered: u64,
    pub current_in_flight: u64,
    pub buffer_size: usize,
    pub active_groups: usize,
    pub blocked_groups: usize,
}

/// Enhanced outbox processor with Java-like architecture
pub struct EnhancedOutboxProcessor {
    config: EnhancedProcessorConfig,
    repository: Arc<dyn OutboxRepository>,
    buffer: Arc<GlobalBuffer>,
    distributor: Arc<GroupDistributor>,
    in_flight: Arc<AtomicU64>,
    is_primary: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    metrics: Arc<RwLock<ProcessorMetrics>>,
}

impl EnhancedOutboxProcessor {
    pub fn new(
        config: EnhancedProcessorConfig,
        repository: Arc<dyn OutboxRepository>,
    ) -> anyhow::Result<Self> {
        // Create HTTP dispatcher
        let http_dispatcher = Arc::new(HttpDispatcher::new(config.http_config.clone())?);

        // Create global buffer
        let buffer_config = GlobalBufferConfig {
            max_size: config.global_buffer_size,
            batch_size: config.api_batch_size,
        };
        let buffer = Arc::new(GlobalBuffer::new(buffer_config));

        // Create group distributor with HTTP dispatcher
        let processor_config = MessageGroupProcessorConfig {
            max_queue_depth: 1000,
            block_on_error: true,
            max_retries: config.max_retries,
            batch_size: config.api_batch_size,
        };
        let distributor_config = GroupDistributorConfig {
            processor_config,
            max_groups: config.max_concurrent_groups * 10,
            group_idle_timeout_secs: 300,
        };
        let distributor = Arc::new(GroupDistributor::new(distributor_config, http_dispatcher));

        let is_primary = Arc::new(AtomicBool::new(!config.leader_election.enabled));

        Ok(Self {
            config,
            repository,
            buffer,
            distributor,
            in_flight: Arc::new(AtomicU64::new(0)),
            is_primary,
            running: Arc::new(AtomicBool::new(false)),
            metrics: Arc::new(RwLock::new(ProcessorMetrics::default())),
        })
    }

    /// Check if this processor is the current leader
    pub fn is_primary(&self) -> bool {
        self.is_primary.load(Ordering::SeqCst)
    }

    /// Set the primary status (called by leader election)
    pub fn set_primary(&self, primary: bool) {
        self.is_primary.store(primary, Ordering::SeqCst);
        if primary {
            info!("Enhanced outbox processor became primary");
        } else {
            warn!("Enhanced outbox processor lost primary status");
        }
    }

    /// Get the is_primary flag for leader election
    pub fn is_primary_flag(&self) -> Arc<AtomicBool> {
        self.is_primary.clone()
    }

    /// Get current in-flight count
    pub fn in_flight_count(&self) -> u64 {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Get current metrics
    pub async fn metrics(&self) -> ProcessorMetrics {
        let mut metrics = self.metrics.read().await.clone();
        metrics.current_in_flight = self.in_flight_count();
        metrics.buffer_size = self.buffer.len().await;

        let stats = self.distributor.stats().await;
        metrics.active_groups = stats.active_groups;
        metrics.blocked_groups = stats.blocked_groups;

        metrics
    }

    /// Get distributor stats
    pub async fn distributor_stats(&self) -> DistributorStats {
        self.distributor.stats().await
    }

    /// Get list of blocked message groups
    pub async fn blocked_groups(&self) -> Vec<(String, String)> {
        self.distributor.get_blocked_groups().await
    }

    /// Unblock a message group
    pub async fn unblock_group(&self, group_id: &str) -> Result<(), String> {
        self.distributor.unblock_group(group_id).await
    }

    /// Start the processor (runs until stopped)
    pub async fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            warn!("Processor already running");
            return;
        }

        info!(
            poll_interval_ms = %self.config.poll_interval.as_millis(),
            poll_batch_size = %self.config.poll_batch_size,
            max_in_flight = %self.config.max_in_flight,
            global_buffer_size = %self.config.global_buffer_size,
            max_concurrent_groups = %self.config.max_concurrent_groups,
            "Starting Enhanced Outbox Processor"
        );

        // Recovery task
        let recovery_handle = {
            let repository = Arc::clone(&self.repository);
            let timeout = Duration::from_secs(self.config.processing_timeout_seconds);
            let interval = self.config.recovery_interval;
            let running = Arc::clone(&self.running);
            let in_flight = Arc::clone(&self.in_flight);
            let metrics = Arc::clone(&self.metrics);

            tokio::spawn(async move {
                let mut interval_timer = tokio::time::interval(interval);
                while running.load(Ordering::SeqCst) {
                    interval_timer.tick().await;
                    match repository.recover_stuck_items(timeout).await {
                        Ok(count) => {
                            if count > 0 {
                                info!("Recovered {} stuck items", count);
                                in_flight.fetch_sub(count, Ordering::SeqCst);
                                let mut m = metrics.write().await;
                                m.items_recovered += count;
                            }
                        }
                        Err(e) => {
                            error!("Recovery task error: {}", e);
                        }
                    }
                }
            })
        };

        // Buffer distributor task
        let distributor_handle = {
            let buffer = Arc::clone(&self.buffer);
            let distributor = Arc::clone(&self.distributor);
            let repository = Arc::clone(&self.repository);
            let in_flight = Arc::clone(&self.in_flight);
            let running = Arc::clone(&self.running);
            let metrics = Arc::clone(&self.metrics);

            tokio::spawn(async move {
                while running.load(Ordering::SeqCst) {
                    let batch = buffer.drain_batch().await;
                    if batch.is_empty() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }

                    for item in batch {
                        let item_id = item.id.clone();
                        let item_type = item.item_type;
                        match distributor.distribute(item).await {
                            Ok(()) => {
                                if let Err(e) = repository.mark_with_status(
                                    item_type,
                                    vec![item_id.clone()],
                                    OutboxStatus::SUCCESS,
                                    None,
                                ).await {
                                    error!("Failed to update status for {}: {}", item_id, e);
                                }
                                in_flight.fetch_sub(1, Ordering::SeqCst);
                                let mut m = metrics.write().await;
                                m.items_processed += 1;
                                m.items_succeeded += 1;
                            }
                            Err(e) => {
                                warn!("Failed to distribute item {}: {}", item_id, e);
                                if let Err(e2) = repository.mark_with_status(
                                    item_type,
                                    vec![item_id.clone()],
                                    OutboxStatus::INTERNAL_ERROR,
                                    Some(e),
                                ).await {
                                    error!("Failed to update status for {}: {}", item_id, e2);
                                }
                                in_flight.fetch_sub(1, Ordering::SeqCst);
                                let mut m = metrics.write().await;
                                m.items_processed += 1;
                                m.items_failed += 1;
                            }
                        }
                    }
                }
            })
        };

        // Main polling loop
        let mut poll_interval = tokio::time::interval(self.config.poll_interval);
        while self.running.load(Ordering::SeqCst) {
            poll_interval.tick().await;

            // Only process if primary
            if !self.is_primary() {
                debug!("Skipping poll - not primary");
                continue;
            }

            if let Err(e) = self.poll_and_buffer().await {
                error!("Poll error: {}", e);
            }
        }

        // Cleanup
        info!("Shutting down enhanced outbox processor...");
        self.distributor.shutdown().await;
        recovery_handle.abort();
        distributor_handle.abort();
        info!("Enhanced outbox processor stopped");
    }

    /// Stop the processor
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if processor is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Start the processor with hot standby integration
    #[cfg(feature = "standby")]
    pub async fn start_with_standby(self: Arc<Self>, leader_election: Arc<LeaderElection>) {
        if self.running.swap(true, Ordering::SeqCst) {
            warn!("Processor already running");
            return;
        }

        info!(
            poll_interval_ms = %self.config.poll_interval.as_millis(),
            poll_batch_size = %self.config.poll_batch_size,
            max_in_flight = %self.config.max_in_flight,
            api_batch_size = %self.config.api_batch_size,
            "Starting Enhanced Outbox Processor with hot standby"
        );

        // Leader status watcher task
        let leader_watcher_handle = {
            let is_primary = Arc::clone(&self.is_primary);
            let mut status_rx = leader_election.subscribe();
            let running = Arc::clone(&self.running);

            tokio::spawn(async move {
                while running.load(Ordering::SeqCst) {
                    match status_rx.changed().await {
                        Ok(()) => {
                            let status = *status_rx.borrow();
                            let is_leader = status == LeadershipStatus::Leader;
                            let was_leader = is_primary.swap(is_leader, Ordering::SeqCst);

                            if is_leader && !was_leader {
                                info!("Outbox processor became leader - starting active processing");
                            } else if !is_leader && was_leader {
                                warn!("Outbox processor lost leadership - entering standby mode");
                            }
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
            })
        };

        // Set initial primary status
        self.is_primary.store(
            leader_election.is_leader(),
            Ordering::SeqCst
        );

        // Recovery task
        let recovery_handle = {
            let repository = Arc::clone(&self.repository);
            let timeout = Duration::from_secs(self.config.processing_timeout_seconds);
            let interval = self.config.recovery_interval;
            let running = Arc::clone(&self.running);
            let in_flight = Arc::clone(&self.in_flight);
            let metrics = Arc::clone(&self.metrics);
            let is_primary = Arc::clone(&self.is_primary);

            tokio::spawn(async move {
                let mut interval_timer = tokio::time::interval(interval);
                while running.load(Ordering::SeqCst) {
                    interval_timer.tick().await;

                    if !is_primary.load(Ordering::SeqCst) {
                        continue;
                    }

                    match repository.recover_stuck_items(timeout).await {
                        Ok(count) => {
                            if count > 0 {
                                info!("Recovered {} stuck items", count);
                                in_flight.fetch_sub(count, Ordering::SeqCst);
                                let mut m = metrics.write().await;
                                m.items_recovered += count;
                            }
                        }
                        Err(e) => {
                            error!("Recovery task error: {}", e);
                        }
                    }
                }
            })
        };

        // Buffer distributor task
        let distributor_handle = {
            let buffer = Arc::clone(&self.buffer);
            let distributor = Arc::clone(&self.distributor);
            let repository = Arc::clone(&self.repository);
            let in_flight = Arc::clone(&self.in_flight);
            let running = Arc::clone(&self.running);
            let metrics = Arc::clone(&self.metrics);

            tokio::spawn(async move {
                while running.load(Ordering::SeqCst) {
                    let batch = buffer.drain_batch().await;
                    if batch.is_empty() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }

                    for item in batch {
                        let item_id = item.id.clone();
                        let item_type = item.item_type;
                        match distributor.distribute(item).await {
                            Ok(()) => {
                                if let Err(e) = repository.mark_with_status(
                                    item_type,
                                    vec![item_id.clone()],
                                    OutboxStatus::SUCCESS,
                                    None,
                                ).await {
                                    error!("Failed to update status for {}: {}", item_id, e);
                                }
                                in_flight.fetch_sub(1, Ordering::SeqCst);
                                let mut m = metrics.write().await;
                                m.items_processed += 1;
                                m.items_succeeded += 1;
                            }
                            Err(e) => {
                                warn!("Failed to distribute item {}: {}", item_id, e);
                                if let Err(e2) = repository.mark_with_status(
                                    item_type,
                                    vec![item_id.clone()],
                                    OutboxStatus::INTERNAL_ERROR,
                                    Some(e),
                                ).await {
                                    error!("Failed to update status for {}: {}", item_id, e2);
                                }
                                in_flight.fetch_sub(1, Ordering::SeqCst);
                                let mut m = metrics.write().await;
                                m.items_processed += 1;
                                m.items_failed += 1;
                            }
                        }
                    }
                }
            })
        };

        // Main polling loop
        let mut poll_interval = tokio::time::interval(self.config.poll_interval);
        while self.running.load(Ordering::SeqCst) {
            poll_interval.tick().await;

            if !self.is_primary() {
                debug!("Skipping poll - not primary (standby mode)");
                continue;
            }

            if let Err(e) = self.poll_and_buffer().await {
                error!("Poll error: {}", e);
            }
        }

        // Cleanup
        info!("Shutting down enhanced outbox processor...");
        self.distributor.shutdown().await;
        recovery_handle.abort();
        distributor_handle.abort();
        leader_watcher_handle.abort();
        info!("Enhanced outbox processor stopped");
    }

    /// Poll for pending items and add to buffer (with backpressure).
    /// Items are added directly as OutboxItem — no conversion to Message.
    async fn poll_and_buffer(&self) -> anyhow::Result<()> {
        // Check backpressure
        let current_in_flight = self.in_flight.load(Ordering::SeqCst);
        let available_slots = self.config.max_in_flight.saturating_sub(current_in_flight);

        if available_slots < self.config.poll_batch_size as u64 {
            debug!(
                "Skipping poll - insufficient capacity (in_flight: {}, max: {})",
                current_in_flight, self.config.max_in_flight
            );
            return Ok(());
        }

        // Fetch pending items
        let items = self.repository.fetch_pending(self.config.poll_batch_size).await?;
        if items.is_empty() {
            return Ok(());
        }

        debug!("Polled {} items from outbox", items.len());

        // Mark as in-progress
        let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        self.repository.mark_processing(ids).await?;

        // Increment in-flight
        self.in_flight.fetch_add(items.len() as u64, Ordering::SeqCst);

        // Update metrics
        {
            let mut m = self.metrics.write().await;
            m.items_polled += items.len() as u64;
        }

        // Add OutboxItems directly to buffer — no conversion needed
        let mut rejected_count = 0;
        for item in items {
            if self.buffer.push(item).await.is_err() {
                rejected_count += 1;
                // Item stays in IN_PROGRESS, will be recovered
            }
        }

        if rejected_count > 0 {
            warn!(
                "Buffer rejected {} items (will be recovered)",
                rejected_count
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EnhancedProcessorConfig::default();
        assert_eq!(config.poll_batch_size, 500);
        assert_eq!(config.max_in_flight, 5000);
        assert_eq!(config.global_buffer_size, 1000);
        assert_eq!(config.max_concurrent_groups, 10);
    }

    #[test]
    fn test_processor_metrics_default() {
        let metrics = ProcessorMetrics::default();
        assert_eq!(metrics.items_polled, 0);
        assert_eq!(metrics.current_in_flight, 0);
    }
}
