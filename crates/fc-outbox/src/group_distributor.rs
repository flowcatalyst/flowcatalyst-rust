//! Group Distributor
//!
//! Routes outbox items to appropriate MessageGroupProcessor based on message_group.
//! Items without a group are dispatched directly (no ordering guarantee).

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, oneshot};
use fc_common::OutboxItem;
use tracing::{debug, info, warn};

use crate::message_group_processor::{
    MessageGroupProcessor, MessageGroupProcessorConfig, BatchMessageDispatcher,
    ProcessorState, TrackedMessage, DispatchResult,
};

/// Group distributor configuration
#[derive(Debug, Clone)]
pub struct GroupDistributorConfig {
    /// Config for individual message group processors
    pub processor_config: MessageGroupProcessorConfig,
    /// Maximum number of active group processors
    pub max_groups: usize,
    /// Idle timeout before cleaning up a group processor (seconds)
    pub group_idle_timeout_secs: u64,
}

impl Default for GroupDistributorConfig {
    fn default() -> Self {
        Self {
            processor_config: MessageGroupProcessorConfig::default(),
            max_groups: 10000,
            group_idle_timeout_secs: 300, // 5 minutes
        }
    }
}

/// Group processor entry with metadata
struct GroupEntry {
    processor: Arc<MessageGroupProcessor>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    last_activity: std::time::Instant,
}

/// Statistics for the distributor
#[derive(Debug, Clone, Default)]
pub struct DistributorStats {
    pub active_groups: usize,
    pub total_messages_distributed: u64,
    pub messages_without_group: u64,
    pub blocked_groups: usize,
}

/// Group distributor - routes outbox items to per-group processors
pub struct GroupDistributor {
    config: GroupDistributorConfig,
    dispatcher: Arc<dyn BatchMessageDispatcher>,
    groups: Arc<RwLock<HashMap<String, GroupEntry>>>,
    stats: Arc<RwLock<DistributorStats>>,
}

impl GroupDistributor {
    pub fn new(config: GroupDistributorConfig, dispatcher: Arc<dyn BatchMessageDispatcher>) -> Self {
        Self {
            config,
            dispatcher,
            groups: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(DistributorStats::default())),
        }
    }

    /// Distribute an outbox item to the appropriate processor
    pub async fn distribute(&self, item: OutboxItem) -> Result<(), String> {
        let group_id = match &item.message_group {
            Some(gid) => gid.clone(),
            None => {
                // No group - dispatch directly without ordering
                let mut stats = self.stats.write().await;
                stats.total_messages_distributed += 1;
                stats.messages_without_group += 1;
                drop(stats);

                debug!("Item {} has no group, dispatching directly", item.id);
                return self.dispatch_direct(item).await;
            }
        };

        // Get or create processor for this group
        let processor = self.get_or_create_processor(&group_id).await?;

        // Enqueue item
        processor.enqueue(item).await?;

        let mut stats = self.stats.write().await;
        stats.total_messages_distributed += 1;

        Ok(())
    }

    /// Get or create a processor for a message group
    async fn get_or_create_processor(&self, group_id: &str) -> Result<Arc<MessageGroupProcessor>, String> {
        // First try read lock
        {
            let groups = self.groups.read().await;
            if let Some(entry) = groups.get(group_id) {
                return Ok(Arc::clone(&entry.processor));
            }
        }

        // Need to create - acquire write lock
        let mut groups = self.groups.write().await;

        // Double-check after acquiring write lock
        if let Some(entry) = groups.get(group_id) {
            return Ok(Arc::clone(&entry.processor));
        }

        // Check if we're at capacity
        if groups.len() >= self.config.max_groups {
            warn!("Max groups reached ({}), cleaning up idle groups", self.config.max_groups);
            self.cleanup_idle_groups_internal(&mut groups).await;

            if groups.len() >= self.config.max_groups {
                return Err("Maximum group count reached".to_string());
            }
        }

        // Create new processor
        let (processor, shutdown_tx) = MessageGroupProcessor::new(
            group_id.to_string(),
            self.config.processor_config.clone(),
            Arc::clone(&self.dispatcher),
        );

        let processor = Arc::new(processor);

        // Spawn processor task
        let processor_clone = Arc::clone(&processor);
        tokio::spawn(async move {
            processor_clone.run().await;
        });

        groups.insert(group_id.to_string(), GroupEntry {
            processor: Arc::clone(&processor),
            shutdown_tx: Some(shutdown_tx),
            last_activity: std::time::Instant::now(),
        });

        let mut stats = self.stats.write().await;
        stats.active_groups = groups.len();

        info!("Created message group processor for {}", group_id);

        Ok(processor)
    }

    /// Dispatch an item directly (for items without a group)
    async fn dispatch_direct(&self, item: OutboxItem) -> Result<(), String> {
        let batch_result = self.dispatcher.dispatch_batch(&[item]).await;

        match batch_result.results.first() {
            Some(r) => match &r.result {
                DispatchResult::Success => Ok(()),
                DispatchResult::Failure { error, .. } => Err(error.clone()),
                DispatchResult::Blocked { reason } => Err(reason.clone()),
            },
            None => Err("No result from dispatch".to_string()),
        }
    }

    /// Clean up idle group processors
    async fn cleanup_idle_groups_internal(&self, groups: &mut HashMap<String, GroupEntry>) {
        let threshold = std::time::Duration::from_secs(self.config.group_idle_timeout_secs);
        let now = std::time::Instant::now();

        let idle_groups: Vec<String> = groups
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.last_activity) > threshold)
            .map(|(k, _)| k.clone())
            .collect();

        for group_id in idle_groups {
            if let Some(mut entry) = groups.remove(&group_id) {
                // Check if queue is empty before removing
                if entry.processor.queue_depth().await == 0 {
                    if let Some(tx) = entry.shutdown_tx.take() {
                        let _ = tx.send(());
                    }
                    info!("Cleaned up idle message group processor: {}", group_id);
                } else {
                    // Put it back - still has items
                    groups.insert(group_id, entry);
                }
            }
        }
    }

    /// Public cleanup method
    pub async fn cleanup_idle_groups(&self) {
        let mut groups = self.groups.write().await;
        self.cleanup_idle_groups_internal(&mut groups).await;

        let mut stats = self.stats.write().await;
        stats.active_groups = groups.len();
    }

    /// Get statistics
    pub async fn stats(&self) -> DistributorStats {
        let stats = self.stats.read().await;
        let groups = self.groups.read().await;

        let blocked_count = {
            let mut count = 0;
            for entry in groups.values() {
                if matches!(entry.processor.state().await, ProcessorState::Blocked { .. }) {
                    count += 1;
                }
            }
            count
        };

        DistributorStats {
            active_groups: groups.len(),
            blocked_groups: blocked_count,
            ..stats.clone()
        }
    }

    /// Get list of blocked groups
    pub async fn get_blocked_groups(&self) -> Vec<(String, String)> {
        let groups = self.groups.read().await;
        let mut blocked = Vec::new();

        for (group_id, entry) in groups.iter() {
            if let ProcessorState::Blocked { message_id, error } = entry.processor.state().await {
                blocked.push((group_id.clone(), format!("msg={}, error={}", message_id, error)));
            }
        }

        blocked
    }

    /// Unblock a specific group
    pub async fn unblock_group(&self, group_id: &str) -> Result<(), String> {
        let groups = self.groups.read().await;
        if let Some(entry) = groups.get(group_id) {
            entry.processor.unblock().await;
            Ok(())
        } else {
            Err(format!("Group {} not found", group_id))
        }
    }

    /// Skip the blocking item in a group
    pub async fn skip_blocking_message(&self, group_id: &str) -> Result<Option<TrackedMessage>, String> {
        let groups = self.groups.read().await;
        if let Some(entry) = groups.get(group_id) {
            Ok(entry.processor.skip_blocking_message().await)
        } else {
            Err(format!("Group {} not found", group_id))
        }
    }

    /// Pause a specific group
    pub async fn pause_group(&self, group_id: &str) -> Result<(), String> {
        let groups = self.groups.read().await;
        if let Some(entry) = groups.get(group_id) {
            entry.processor.pause().await;
            Ok(())
        } else {
            Err(format!("Group {} not found", group_id))
        }
    }

    /// Resume a specific group
    pub async fn resume_group(&self, group_id: &str) -> Result<(), String> {
        let groups = self.groups.read().await;
        if let Some(entry) = groups.get(group_id) {
            entry.processor.resume().await;
            Ok(())
        } else {
            Err(format!("Group {} not found", group_id))
        }
    }

    /// Get queue depth for a group
    pub async fn group_queue_depth(&self, group_id: &str) -> Option<usize> {
        let groups = self.groups.read().await;
        if let Some(entry) = groups.get(group_id) {
            Some(entry.processor.queue_depth().await)
        } else {
            None
        }
    }

    /// Get all active group IDs
    pub async fn active_groups(&self) -> Vec<String> {
        let groups = self.groups.read().await;
        groups.keys().cloned().collect()
    }

    /// Shutdown all processors
    pub async fn shutdown(&self) {
        let mut groups = self.groups.write().await;

        for (group_id, mut entry) in groups.drain() {
            if let Some(tx) = entry.shutdown_tx.take() {
                let _ = tx.send(());
            }
            info!("Shutdown message group processor: {}", group_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_common::OutboxStatus;
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use crate::message_group_processor::{BatchDispatchResult, BatchItemResult};
    use async_trait::async_trait;

    struct MockBatchDispatcher {
        dispatch_count: AtomicUsize,
    }

    impl MockBatchDispatcher {
        fn new() -> Self {
            Self {
                dispatch_count: AtomicUsize::new(0),
            }
        }

        fn count(&self) -> usize {
            self.dispatch_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl BatchMessageDispatcher for MockBatchDispatcher {
        async fn dispatch_batch(&self, items: &[OutboxItem]) -> BatchDispatchResult {
            self.dispatch_count.fetch_add(items.len(), Ordering::SeqCst);
            BatchDispatchResult {
                results: items.iter().map(|item| BatchItemResult {
                    item_id: item.id.clone(),
                    result: DispatchResult::Success,
                }).collect(),
            }
        }
    }

    fn create_test_item(id: &str, group: Option<&str>) -> OutboxItem {
        OutboxItem {
            id: id.to_string(),
            item_type: fc_common::OutboxItemType::EVENT,
            message_group: group.map(String::from),
            payload: serde_json::json!({"test": true}),
            status: OutboxStatus::IN_PROGRESS,
            retry_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            error_message: None,
            client_id: None,
            payload_size: None,
            headers: None,
        }
    }

    #[tokio::test]
    async fn test_distribute_with_group() {
        let dispatcher = Arc::new(MockBatchDispatcher::new());
        let distributor = GroupDistributor::new(
            GroupDistributorConfig::default(),
            dispatcher.clone(),
        );

        distributor.distribute(create_test_item("msg-1", Some("group-a"))).await.unwrap();
        distributor.distribute(create_test_item("msg-2", Some("group-a"))).await.unwrap();
        distributor.distribute(create_test_item("msg-3", Some("group-b"))).await.unwrap();

        let stats = distributor.stats().await;
        assert_eq!(stats.active_groups, 2);
        assert_eq!(stats.total_messages_distributed, 3);

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        distributor.shutdown().await;
    }

    #[tokio::test]
    async fn test_distribute_without_group() {
        let dispatcher = Arc::new(MockBatchDispatcher::new());
        let distributor = GroupDistributor::new(
            GroupDistributorConfig::default(),
            dispatcher.clone(),
        );

        distributor.distribute(create_test_item("msg-1", None)).await.unwrap();
        distributor.distribute(create_test_item("msg-2", None)).await.unwrap();

        let stats = distributor.stats().await;
        assert_eq!(stats.active_groups, 0);
        assert_eq!(stats.messages_without_group, 2);
        assert_eq!(dispatcher.count(), 2);

        distributor.shutdown().await;
    }

    #[tokio::test]
    async fn test_active_groups() {
        let dispatcher = Arc::new(MockBatchDispatcher::new());
        let distributor = GroupDistributor::new(
            GroupDistributorConfig::default(),
            dispatcher.clone(),
        );

        distributor.distribute(create_test_item("msg-1", Some("group-1"))).await.unwrap();
        distributor.distribute(create_test_item("msg-2", Some("group-2"))).await.unwrap();
        distributor.distribute(create_test_item("msg-3", Some("group-3"))).await.unwrap();

        let groups = distributor.active_groups().await;
        assert_eq!(groups.len(), 3);
        assert!(groups.contains(&"group-1".to_string()));
        assert!(groups.contains(&"group-2".to_string()));
        assert!(groups.contains(&"group-3".to_string()));

        distributor.shutdown().await;
    }
}
