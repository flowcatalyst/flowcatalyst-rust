//! MySQL Outbox Repository Implementation
//!
//! Implements the OutboxRepository trait for MySQL with a single shared
//! `outbox_messages` table using a `type` column, matching Java/TypeScript.

use async_trait::async_trait;
use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
use crate::repository::{OutboxRepository, OutboxTableConfig};
use anyhow::Result;
use sqlx::{MySqlPool, Row};
use chrono::{DateTime, Utc};
use std::time::Duration;
use tracing::{info, debug};

/// MySQL implementation of OutboxRepository
pub struct MySqlOutboxRepository {
    pool: MySqlPool,
    table_config: OutboxTableConfig,
}

impl MySqlOutboxRepository {
    /// Create a new MySQL outbox repository with default table config
    pub fn new(pool: MySqlPool) -> Self {
        Self {
            pool,
            table_config: OutboxTableConfig::default(),
        }
    }

    /// Create with custom table configuration
    pub fn with_config(pool: MySqlPool, table_config: OutboxTableConfig) -> Self {
        Self { pool, table_config }
    }

    /// Get the pool reference
    pub fn pool(&self) -> &MySqlPool {
        &self.pool
    }

    /// Build a query with the appropriate number of placeholders for IN clause
    fn build_in_clause(count: usize) -> String {
        let placeholders: Vec<&str> = (0..count).map(|_| "?").collect();
        placeholders.join(", ")
    }

    /// Parse a row into an OutboxItem
    fn parse_row(&self, row: &sqlx::mysql::MySqlRow, item_type: OutboxItemType) -> Result<OutboxItem> {
        let created_at: DateTime<Utc> = row.get("created_at");
        let updated_at: DateTime<Utc> = row.get("updated_at");

        let status_code: i16 = row.get("status");
        let status = OutboxStatus::from_code(status_code as i32);

        let payload_str: String = row.get("payload");

        Ok(OutboxItem {
            id: row.get("id"),
            item_type,
            message_group: row.try_get("message_group").ok().flatten(),
            payload: serde_json::from_str(&payload_str)?,
            status,
            retry_count: row.get::<i16, _>("retry_count") as i32,
            error_message: row.try_get("error_message").ok().flatten(),
            created_at,
            updated_at,
            client_id: row.try_get("client_id").ok().flatten(),
            payload_size: row.try_get::<Option<i32>, _>("payload_size").ok().flatten(),
            headers: {
                let h: Option<String> = row.try_get("headers").ok().flatten();
                h.and_then(|s| serde_json::from_str(&s).ok())
            },
        })
    }
}

#[async_trait]
impl OutboxRepository for MySqlOutboxRepository {
    async fn fetch_pending_by_type(&self, item_type: OutboxItemType, limit: u32) -> Result<Vec<OutboxItem>> {
        let table = self.table_config.table_for_type(item_type);
        let query = format!(
            "SELECT id, type, message_group, payload, status, retry_count, error_message, \
             created_at, updated_at, client_id, payload_size, headers \
             FROM {} WHERE status = ? AND type = ? \
             ORDER BY message_group, created_at ASC LIMIT ?",
            table
        );

        let rows = sqlx::query(&query)
            .bind(OutboxStatus::PENDING.code() as i16)
            .bind(item_type.type_value())
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;

        let mut items = Vec::with_capacity(rows.len());
        for row in &rows {
            items.push(self.parse_row(row, item_type)?);
        }

        debug!(table = %table, item_type = %item_type, count = items.len(), "Fetched pending items");
        Ok(items)
    }

    async fn mark_in_progress(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let table = self.table_config.table_for_type(item_type);
        let in_clause = Self::build_in_clause(ids.len());

        let query = format!(
            "UPDATE {} SET status = ?, updated_at = CURRENT_TIMESTAMP(3) WHERE type = ? AND id IN ({})",
            table, in_clause
        );

        let mut q = sqlx::query(&query)
            .bind(OutboxStatus::IN_PROGRESS.code() as i16)
            .bind(item_type.type_value());
        for id in &ids {
            q = q.bind(id);
        }
        q.execute(&self.pool).await?;

        debug!(table = %table, count = ids.len(), "Marked items as IN_PROGRESS");
        Ok(())
    }

    async fn mark_with_status(
        &self,
        item_type: OutboxItemType,
        ids: Vec<String>,
        status: OutboxStatus,
        error_message: Option<String>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let table = self.table_config.table_for_type(item_type);
        let in_clause = Self::build_in_clause(ids.len());

        let query = format!(
            "UPDATE {} SET status = ?, error_message = ?, updated_at = CURRENT_TIMESTAMP(3) WHERE type = ? AND id IN ({})",
            table, in_clause
        );

        let mut q = sqlx::query(&query)
            .bind(status.code() as i16)
            .bind(&error_message)
            .bind(item_type.type_value());
        for id in &ids {
            q = q.bind(id);
        }
        q.execute(&self.pool).await?;

        debug!(table = %table, status = ?status, count = ids.len(), "Marked items with status");
        Ok(())
    }

    async fn increment_retry_count(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let table = self.table_config.table_for_type(item_type);
        let in_clause = Self::build_in_clause(ids.len());

        let query = format!(
            "UPDATE {} SET retry_count = retry_count + 1, status = ?, updated_at = CURRENT_TIMESTAMP(3) WHERE type = ? AND id IN ({})",
            table, in_clause
        );

        let mut q = sqlx::query(&query)
            .bind(OutboxStatus::PENDING.code() as i16)
            .bind(item_type.type_value());
        for id in &ids {
            q = q.bind(id);
        }
        q.execute(&self.pool).await?;

        debug!(table = %table, count = ids.len(), "Incremented retry count");
        Ok(())
    }

    async fn fetch_recoverable_items(
        &self,
        item_type: OutboxItemType,
        timeout: Duration,
        limit: u32,
    ) -> Result<Vec<OutboxItem>> {
        let table = self.table_config.table_for_type(item_type);
        let cutoff = Utc::now() - chrono::Duration::from_std(timeout).unwrap_or_default();

        let query = format!(
            "SELECT id, type, message_group, payload, status, retry_count, error_message, \
             created_at, updated_at, client_id, payload_size, headers \
             FROM {} WHERE type = ? AND status IN (?, ?, ?, ?, ?, ?) AND updated_at < ? \
             ORDER BY created_at ASC LIMIT ?",
            table
        );

        let rows = sqlx::query(&query)
            .bind(item_type.type_value())
            .bind(OutboxStatus::IN_PROGRESS.code() as i16)
            .bind(OutboxStatus::BAD_REQUEST.code() as i16)
            .bind(OutboxStatus::INTERNAL_ERROR.code() as i16)
            .bind(OutboxStatus::UNAUTHORIZED.code() as i16)
            .bind(OutboxStatus::FORBIDDEN.code() as i16)
            .bind(OutboxStatus::GATEWAY_ERROR.code() as i16)
            .bind(cutoff)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;

        let mut items = Vec::with_capacity(rows.len());
        for row in &rows {
            items.push(self.parse_row(row, item_type)?);
        }
        Ok(items)
    }

    async fn reset_recoverable_items(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let table = self.table_config.table_for_type(item_type);
        let in_clause = Self::build_in_clause(ids.len());

        let query = format!(
            "UPDATE {} SET status = ?, updated_at = CURRENT_TIMESTAMP(3) WHERE type = ? AND id IN ({})",
            table, in_clause
        );

        let mut q = sqlx::query(&query)
            .bind(OutboxStatus::PENDING.code() as i16)
            .bind(item_type.type_value());
        for id in &ids {
            q = q.bind(id);
        }
        q.execute(&self.pool).await?;

        info!(table = %table, count = ids.len(), "Reset recoverable items to PENDING");
        Ok(())
    }

    async fn fetch_stuck_items(
        &self,
        item_type: OutboxItemType,
        timeout: Duration,
        limit: u32,
    ) -> Result<Vec<OutboxItem>> {
        let table = self.table_config.table_for_type(item_type);
        let cutoff = Utc::now() - chrono::Duration::from_std(timeout).unwrap_or_default();

        let query = format!(
            "SELECT id, type, message_group, payload, status, retry_count, error_message, \
             created_at, updated_at, client_id, payload_size, headers \
             FROM {} WHERE status = ? AND type = ? AND updated_at < ? \
             ORDER BY created_at ASC LIMIT ?",
            table
        );

        let rows = sqlx::query(&query)
            .bind(OutboxStatus::IN_PROGRESS.code() as i16)
            .bind(item_type.type_value())
            .bind(cutoff)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;

        let mut items = Vec::with_capacity(rows.len());
        for row in &rows {
            items.push(self.parse_row(row, item_type)?);
        }
        Ok(items)
    }

    async fn reset_stuck_items(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        self.reset_recoverable_items(item_type, ids).await
    }

    async fn init_schema(&self) -> Result<()> {
        for table in self.table_config.unique_tables() {
            let safe_name = table.replace('.', "_");
            let schema = format!(
                r#"
                CREATE TABLE IF NOT EXISTS {table} (
                    id VARCHAR(26) PRIMARY KEY,
                    type VARCHAR(20) NOT NULL,
                    message_group VARCHAR(255),
                    payload LONGTEXT NOT NULL,
                    status SMALLINT NOT NULL DEFAULT 0,
                    retry_count SMALLINT NOT NULL DEFAULT 0,
                    created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
                    updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
                    error_message TEXT,
                    client_id VARCHAR(26),
                    payload_size INT,
                    headers JSON,
                    INDEX idx_{safe_name}_pending (status, message_group, created_at),
                    INDEX idx_{safe_name}_stuck (status, created_at),
                    INDEX idx_{safe_name}_client_pending (client_id, status, created_at)
                ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
                "#,
                table = table,
                safe_name = safe_name,
            );

            sqlx::query(&schema)
                .execute(&self.pool)
                .await?;
        }

        info!(
            tables = ?self.table_config.unique_tables(),
            "Initialized MySQL outbox schema"
        );

        Ok(())
    }

    fn table_config(&self) -> &OutboxTableConfig {
        &self.table_config
    }
}
