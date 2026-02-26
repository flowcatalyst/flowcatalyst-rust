//! Outbox Repository Trait
//!
//! Defines the interface for outbox persistence matching Java's OutboxRepository.
//! Supports type-aware queries (EVENT, DISPATCH_JOB, AUDIT_LOG) and granular status tracking.
//! Uses a single shared table (outbox_messages) with a `type` column, matching Java/TypeScript.

use async_trait::async_trait;
use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
use anyhow::Result;
use std::collections::HashSet;
use std::time::Duration;

/// Configuration for outbox repository tables.
///
/// By default all types share a single `outbox_messages` table (matching Java/TypeScript).
/// Each type can optionally be routed to a separate table.
#[derive(Debug, Clone)]
pub struct OutboxTableConfig {
    /// Table name for EVENT items (default: "outbox_messages")
    pub events_table: String,
    /// Table name for DISPATCH_JOB items (default: "outbox_messages")
    pub dispatch_jobs_table: String,
    /// Table name for AUDIT_LOG items (default: "outbox_messages")
    pub audit_logs_table: String,
}

impl Default for OutboxTableConfig {
    fn default() -> Self {
        Self {
            events_table: "outbox_messages".to_string(),
            dispatch_jobs_table: "outbox_messages".to_string(),
            audit_logs_table: "outbox_messages".to_string(),
        }
    }
}

impl OutboxTableConfig {
    /// Get table name for item type
    pub fn table_for_type(&self, item_type: OutboxItemType) -> &str {
        match item_type {
            OutboxItemType::EVENT => &self.events_table,
            OutboxItemType::DISPATCH_JOB => &self.dispatch_jobs_table,
            OutboxItemType::AUDIT_LOG => &self.audit_logs_table,
        }
    }

    /// Get the set of unique table names (for schema creation).
    /// Deduplicates when multiple types share the same table.
    pub fn unique_tables(&self) -> Vec<&str> {
        let mut seen = HashSet::new();
        let mut tables = Vec::new();
        for table in [&self.events_table, &self.dispatch_jobs_table, &self.audit_logs_table] {
            if seen.insert(table.as_str()) {
                tables.push(table.as_str());
            }
        }
        tables
    }
}

/// Outbox repository trait matching Java's OutboxRepository interface
#[async_trait]
pub trait OutboxRepository: Send + Sync {
    // ========================================================================
    // Core Operations (Java-compatible)
    // ========================================================================

    /// Fetch pending items of the specified type
    ///
    /// Java equivalent: `fetchPending(OutboxItemType type, int limit)`
    /// Orders by message_group, created_at to match Java/TypeScript behavior.
    async fn fetch_pending_by_type(&self, item_type: OutboxItemType, limit: u32) -> Result<Vec<OutboxItem>>;

    /// Mark items as IN_PROGRESS (status = 9)
    ///
    /// Java equivalent: `markAsInProgress(OutboxItemType type, List<String> ids)`
    async fn mark_in_progress(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()>;

    /// Update status for items with optional error message
    ///
    /// Java equivalent: `markWithStatus(OutboxItemType type, List<String> ids, OutboxStatus status)`
    async fn mark_with_status(
        &self,
        item_type: OutboxItemType,
        ids: Vec<String>,
        status: OutboxStatus,
        error_message: Option<String>,
    ) -> Result<()>;

    /// Increment retry count and reset to PENDING for retry
    ///
    /// Java equivalent: `incrementRetryCount(OutboxItemType type, List<String> ids)`
    async fn increment_retry_count(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()>;

    /// Fetch items that are recoverable (stuck in IN_PROGRESS or error states)
    ///
    /// Java equivalent: `fetchRecoverableItems(OutboxItemType type, int timeoutSeconds, int limit)`
    async fn fetch_recoverable_items(
        &self,
        item_type: OutboxItemType,
        timeout: Duration,
        limit: u32,
    ) -> Result<Vec<OutboxItem>>;

    /// Reset recoverable items back to PENDING
    ///
    /// Java equivalent: `resetRecoverableItems(OutboxItemType type, List<String> ids)`
    async fn reset_recoverable_items(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()>;

    /// Fetch items stuck in IN_PROGRESS for longer than timeout
    ///
    /// Java equivalent: `fetchStuckItems(OutboxItemType type, int timeoutSeconds, int limit)`
    async fn fetch_stuck_items(
        &self,
        item_type: OutboxItemType,
        timeout: Duration,
        limit: u32,
    ) -> Result<Vec<OutboxItem>>;

    /// Reset stuck items back to PENDING
    ///
    /// Java equivalent: `resetStuckItems(OutboxItemType type, List<String> ids)`
    async fn reset_stuck_items(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()>;

    // ========================================================================
    // Convenience Methods
    // ========================================================================

    /// Fetch all pending items (all types) - convenience method
    async fn fetch_pending(&self, limit: u32) -> Result<Vec<OutboxItem>> {
        let per_type = (limit / 3).max(1);
        let mut items = Vec::new();
        for item_type in OutboxItemType::ALL {
            let type_items = self.fetch_pending_by_type(item_type, per_type).await?;
            items.extend(type_items);
        }
        Ok(items)
    }

    /// Mark items as processing (legacy method)
    async fn mark_processing(&self, ids: Vec<String>) -> Result<()> {
        // Assume EVENT type for legacy callers
        self.mark_in_progress(OutboxItemType::EVENT, ids).await
    }

    /// Update status for a single item (legacy method)
    async fn update_status(&self, id: &str, status: OutboxStatus, error: Option<String>) -> Result<()> {
        // Assume EVENT type for legacy callers
        self.mark_with_status(
            OutboxItemType::EVENT,
            vec![id.to_string()],
            status,
            error,
        ).await
    }

    /// Recover stuck items across all types.
    /// Returns the number of items recovered.
    async fn recover_stuck_items(&self, timeout: Duration) -> Result<u64> {
        let mut total = 0u64;

        for item_type in OutboxItemType::ALL {
            let stuck = self.fetch_stuck_items(item_type, timeout, 1000).await?;
            if !stuck.is_empty() {
                let ids: Vec<String> = stuck.iter().map(|i| i.id.clone()).collect();
                let count = ids.len() as u64;
                self.reset_stuck_items(item_type, ids).await?;
                total += count;
            }
        }

        Ok(total)
    }

    // ========================================================================
    // Schema Management
    // ========================================================================

    /// Initialize schema (create tables if not exists)
    async fn init_schema(&self) -> Result<()>;

    /// Get the table configuration
    fn table_config(&self) -> &OutboxTableConfig;
}

/// Extension trait for batch operations
#[async_trait]
pub trait OutboxRepositoryExt: OutboxRepository {
    /// Process a batch of items with status update
    async fn process_batch(
        &self,
        item_type: OutboxItemType,
        _items: &[OutboxItem],
        results: Vec<(String, OutboxStatus, Option<String>)>,
    ) -> Result<()> {
        // Group by status
        let mut success_ids = Vec::new();
        let mut error_items: Vec<(String, OutboxStatus, Option<String>)> = Vec::new();

        for (id, status, error) in results {
            if status.is_terminal() && matches!(status, OutboxStatus::SUCCESS) {
                success_ids.push(id);
            } else {
                error_items.push((id, status, error));
            }
        }

        // Mark successful items
        if !success_ids.is_empty() {
            self.mark_with_status(item_type, success_ids, OutboxStatus::SUCCESS, None).await?;
        }

        // Handle error items individually (they may have different statuses)
        for (id, status, error) in error_items {
            self.mark_with_status(item_type, vec![id], status, error).await?;
        }

        Ok(())
    }

    /// Retry failed items that haven't exceeded max retries
    async fn retry_failed_items(
        &self,
        item_type: OutboxItemType,
        max_retries: i32,
        limit: u32,
    ) -> Result<u64> {
        let recoverable = self.fetch_recoverable_items(item_type, Duration::from_secs(0), limit).await?;

        let mut retried = 0u64;
        let mut to_retry = Vec::new();
        let mut exhausted = Vec::new();

        for item in recoverable {
            if item.retry_count < max_retries {
                to_retry.push(item.id);
            } else {
                exhausted.push(item.id);
            }
        }

        if !to_retry.is_empty() {
            retried = to_retry.len() as u64;
            self.increment_retry_count(item_type, to_retry).await?;
        }

        // Mark exhausted items as permanently failed
        if !exhausted.is_empty() {
            self.mark_with_status(
                item_type,
                exhausted,
                OutboxStatus::INTERNAL_ERROR,
                Some("Max retries exceeded".to_string()),
            ).await?;
        }

        Ok(retried)
    }
}

// Blanket implementation
impl<T: OutboxRepository + ?Sized> OutboxRepositoryExt for T {}
