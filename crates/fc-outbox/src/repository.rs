//! Outbox Repository Trait
//!
//! Defines the interface for outbox persistence.
//! Supports type-aware queries (EVENT, DISPATCH_JOB, AUDIT_LOG) and granular status tracking.
//! Uses a single shared table (outbox_messages) with a `type` column; the SDKs
//! (Rust, TypeScript, Laravel, Go) write rows into the same layout.

use anyhow::Result;
use async_trait::async_trait;
use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
use std::collections::HashSet;
use std::time::Duration;

/// Configuration for outbox repository tables.
///
/// By default all types share a single `outbox_messages` table.
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
        for table in [
            &self.events_table,
            &self.dispatch_jobs_table,
            &self.audit_logs_table,
        ] {
            if seen.insert(table.as_str()) {
                tables.push(table.as_str());
            }
        }
        tables
    }
}

/// Outbox persistence, implemented once per supported database.
#[async_trait]
pub trait OutboxRepository: Send + Sync {
    // ========================================================================
    // Core Operations
    // ========================================================================

    /// Fetch pending items of the specified type
    /// Orders by message_group, created_at.
    async fn fetch_pending_by_type(
        &self,
        item_type: OutboxItemType,
        limit: u32,
    ) -> Result<Vec<OutboxItem>>;

    /// Mark items as IN_PROGRESS (status = 9)
    async fn mark_in_progress(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()>;

    /// Update status for items with optional error message
    async fn mark_with_status(
        &self,
        item_type: OutboxItemType,
        ids: Vec<String>,
        status: OutboxStatus,
        error_message: Option<String>,
    ) -> Result<()>;

    /// Increment retry count and reset to PENDING for retry
    async fn increment_retry_count(
        &self,
        item_type: OutboxItemType,
        ids: Vec<String>,
    ) -> Result<()>;

    /// Fetch items that are recoverable (stuck in IN_PROGRESS or error states)
    async fn fetch_recoverable_items(
        &self,
        item_type: OutboxItemType,
        timeout: Duration,
        limit: u32,
    ) -> Result<Vec<OutboxItem>>;

    /// Reset recoverable items back to PENDING
    async fn reset_recoverable_items(
        &self,
        item_type: OutboxItemType,
        ids: Vec<String>,
    ) -> Result<()>;

    /// Fetch items stuck in IN_PROGRESS for longer than timeout
    async fn fetch_stuck_items(
        &self,
        item_type: OutboxItemType,
        timeout: Duration,
        limit: u32,
    ) -> Result<Vec<OutboxItem>>;

    /// Reset stuck items back to PENDING
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
    async fn update_status(
        &self,
        id: &str,
        status: OutboxStatus,
        error: Option<String>,
    ) -> Result<()> {
        // Assume EVENT type for legacy callers
        self.mark_with_status(OutboxItemType::EVENT, vec![id.to_string()], status, error)
            .await
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
