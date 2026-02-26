//! PostgreSQL Outbox Repository Implementation
//!
//! Implements the OutboxRepository trait for PostgreSQL with a single shared
//! `outbox_messages` table using a `type` column, matching Java/TypeScript.

use async_trait::async_trait;
use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
use crate::repository::{OutboxRepository, OutboxTableConfig};
use anyhow::Result;
use sqlx::{PgPool, Row};
use chrono::{DateTime, Utc};
use std::time::Duration;
use tracing::{info, debug};

/// PostgreSQL implementation of OutboxRepository
pub struct PostgresOutboxRepository {
    pool: PgPool,
    table_config: OutboxTableConfig,
}

impl PostgresOutboxRepository {
    /// Create a new PostgreSQL outbox repository with default table config
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            table_config: OutboxTableConfig::default(),
        }
    }

    /// Create with custom table configuration
    pub fn with_config(pool: PgPool, table_config: OutboxTableConfig) -> Self {
        Self { pool, table_config }
    }

    /// Get the pool reference
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Parse a row into an OutboxItem
    fn parse_row(&self, row: &sqlx::postgres::PgRow, item_type: OutboxItemType) -> Result<OutboxItem> {
        let created_at: DateTime<Utc> = row.get("created_at");
        let updated_at: DateTime<Utc> = row.get("updated_at");

        let status_code: i16 = row.get("status");
        let status = OutboxStatus::from_code(status_code as i32);

        let payload_str: &str = row.get("payload");

        Ok(OutboxItem {
            id: row.get("id"),
            item_type,
            message_group: row.try_get("message_group").ok().flatten(),
            payload: serde_json::from_str(payload_str)?,
            status,
            retry_count: row.get::<i16, _>("retry_count") as i32,
            error_message: row.try_get("error_message").ok().flatten(),
            created_at,
            updated_at,
            client_id: row.try_get("client_id").ok().flatten(),
            payload_size: row.try_get::<Option<i32>, _>("payload_size").ok().flatten(),
            headers: row.try_get::<Option<serde_json::Value>, _>("headers").ok().flatten(),
        })
    }
}

#[async_trait]
impl OutboxRepository for PostgresOutboxRepository {
    async fn fetch_pending_by_type(&self, item_type: OutboxItemType, limit: u32) -> Result<Vec<OutboxItem>> {
        let table = self.table_config.table_for_type(item_type);
        let query = format!(
            "SELECT id, type, message_group, payload, status, retry_count, error_message, \
             created_at, updated_at, client_id, payload_size, headers \
             FROM {} WHERE status = $1 AND type = $2 \
             ORDER BY message_group, created_at ASC LIMIT $3",
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

        debug!(
            table = %table,
            item_type = %item_type,
            count = items.len(),
            "Fetched pending items"
        );

        Ok(items)
    }

    async fn mark_in_progress(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let table = self.table_config.table_for_type(item_type);

        let query = format!(
            "UPDATE {} SET status = $1, updated_at = NOW() WHERE id = ANY($2) AND type = $3",
            table
        );

        sqlx::query(&query)
            .bind(OutboxStatus::IN_PROGRESS.code() as i16)
            .bind(&ids)
            .bind(item_type.type_value())
            .execute(&self.pool)
            .await?;

        debug!(
            table = %table,
            count = ids.len(),
            "Marked items as IN_PROGRESS"
        );

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

        let query = format!(
            "UPDATE {} SET status = $1, error_message = $2, updated_at = NOW() WHERE id = ANY($3) AND type = $4",
            table
        );

        sqlx::query(&query)
            .bind(status.code() as i16)
            .bind(&error_message)
            .bind(&ids)
            .bind(item_type.type_value())
            .execute(&self.pool)
            .await?;

        debug!(
            table = %table,
            status = ?status,
            count = ids.len(),
            "Marked items with status"
        );

        Ok(())
    }

    async fn increment_retry_count(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let table = self.table_config.table_for_type(item_type);

        let query = format!(
            "UPDATE {} SET retry_count = retry_count + 1, status = $1, updated_at = NOW() WHERE id = ANY($2) AND type = $3",
            table
        );

        sqlx::query(&query)
            .bind(OutboxStatus::PENDING.code() as i16)
            .bind(&ids)
            .bind(item_type.type_value())
            .execute(&self.pool)
            .await?;

        debug!(
            table = %table,
            count = ids.len(),
            "Incremented retry count and reset to PENDING"
        );

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
             FROM {} WHERE type = $1 \
             AND (status = $2 OR status = $3 OR status = $4 OR status = $5 OR status = $6 OR status = $7) \
             AND updated_at < $8 ORDER BY created_at ASC LIMIT $9",
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

        let query = format!(
            "UPDATE {} SET status = $1, updated_at = NOW() WHERE id = ANY($2) AND type = $3",
            table
        );

        sqlx::query(&query)
            .bind(OutboxStatus::PENDING.code() as i16)
            .bind(&ids)
            .bind(item_type.type_value())
            .execute(&self.pool)
            .await?;

        info!(
            table = %table,
            count = ids.len(),
            "Reset recoverable items to PENDING"
        );

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
             FROM {} WHERE status = $1 AND type = $2 AND updated_at < $3 \
             ORDER BY created_at ASC LIMIT $4",
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
                    payload TEXT NOT NULL,
                    status SMALLINT NOT NULL DEFAULT 0,
                    retry_count SMALLINT NOT NULL DEFAULT 0,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                    error_message TEXT,
                    client_id VARCHAR(26),
                    payload_size INTEGER,
                    headers JSONB
                );
                CREATE INDEX IF NOT EXISTS idx_{safe_name}_pending
                    ON {table}(status, message_group, created_at) WHERE status = 0;
                CREATE INDEX IF NOT EXISTS idx_{safe_name}_stuck
                    ON {table}(status, created_at) WHERE status = 9;
                CREATE INDEX IF NOT EXISTS idx_{safe_name}_client_pending
                    ON {table}(client_id, status, created_at);
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
            "Initialized PostgreSQL outbox schema"
        );

        Ok(())
    }

    fn table_config(&self) -> &OutboxTableConfig {
        &self.table_config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_config_default() {
        let config = OutboxTableConfig::default();
        assert_eq!(config.events_table, "outbox_messages");
        assert_eq!(config.dispatch_jobs_table, "outbox_messages");
        assert_eq!(config.audit_logs_table, "outbox_messages");
    }

    #[test]
    fn test_table_for_type() {
        let config = OutboxTableConfig::default();
        assert_eq!(config.table_for_type(OutboxItemType::EVENT), "outbox_messages");
        assert_eq!(config.table_for_type(OutboxItemType::DISPATCH_JOB), "outbox_messages");
        assert_eq!(config.table_for_type(OutboxItemType::AUDIT_LOG), "outbox_messages");
    }

    #[test]
    fn test_unique_tables_shared() {
        let config = OutboxTableConfig::default();
        let tables = config.unique_tables();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0], "outbox_messages");
    }

    #[test]
    fn test_unique_tables_separate() {
        let config = OutboxTableConfig {
            events_table: "outbox_events".to_string(),
            dispatch_jobs_table: "outbox_dispatch_jobs".to_string(),
            audit_logs_table: "outbox_audit_logs".to_string(),
        };
        let tables = config.unique_tables();
        assert_eq!(tables.len(), 3);
    }
}
