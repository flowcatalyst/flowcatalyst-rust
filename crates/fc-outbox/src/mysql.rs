//! MySQL Outbox Repository Implementation
//!
//! The outbox table an application keeps in MySQL 8 (the TypeScript/Go/Java
//! migrations, or Laravel's on MySQL). MySQL has no `UPDATE … RETURNING`, so
//! the claim is a transaction: `SELECT … FOR UPDATE SKIP LOCKED`, then the
//! update to IN_PROGRESS and the read of the claimed rows, committed together.
//! Columns are read as text or integers, so each SDK's column types read the
//! same way.

use crate::repository::{
    checked_table_name, parse_text_timestamp, ClaimedBatch, OutboxRepository, OutboxTableConfig,
};
use anyhow::Result;
use async_trait::async_trait;
use fc_common::{OutboxItemType, OutboxStatus};
use sqlx::{MySqlPool, Row};
use std::time::Duration;
use tracing::info;

/// MySQL implementation of OutboxRepository
pub struct MySqlOutboxRepository {
    pool: MySqlPool,
    table_config: OutboxTableConfig,
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
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

    fn table(&self, item_type: OutboxItemType) -> Result<&str> {
        checked_table_name(self.table_config.table_for_type(item_type))
    }

    async fn update_ids(
        &self,
        item_type: OutboxItemType,
        set: &str,
        and: &str,
        binds: Vec<String>,
        ids: &[String],
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let sql = format!(
            "UPDATE {} SET {set} WHERE id IN ({}) {and}",
            self.table(item_type)?,
            placeholders(ids.len())
        );
        let mut query = sqlx::query(&sql);
        for b in binds {
            query = query.bind(b);
        }
        for id in ids {
            query = query.bind(id);
        }
        query.execute(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl OutboxRepository for MySqlOutboxRepository {
    async fn claim_pending(&self, limit: u32) -> Result<ClaimedBatch> {
        let mut batch = ClaimedBatch::default();
        for (table, types) in self.table_config.tables_with_types() {
            let remaining = (limit as usize).saturating_sub(batch.len());
            if remaining == 0 {
                break;
            }
            let table = checked_table_name(table)?;
            let mut tx = self.pool.begin().await?;

            let select = format!(
                "SELECT id FROM {table} WHERE status = {} AND type IN ({}) \
                 ORDER BY message_group, created_at, id LIMIT ? FOR UPDATE SKIP LOCKED",
                OutboxStatus::Pending.code(),
                placeholders(types.len())
            );
            let mut query = sqlx::query_scalar::<_, String>(&select);
            for t in &types {
                query = query.bind(t.as_str());
            }
            let ids: Vec<String> = query.bind(remaining as i64).fetch_all(&mut *tx).await?;
            if ids.is_empty() {
                tx.commit().await?;
                continue;
            }

            let update = format!(
                "UPDATE {table} SET status = {}, updated_at = CURRENT_TIMESTAMP WHERE id IN ({})",
                OutboxStatus::InProgress.code(),
                placeholders(ids.len())
            );
            let mut query = sqlx::query(&update);
            for id in &ids {
                query = query.bind(id);
            }
            query.execute(&mut *tx).await?;

            let read = format!(
                "SELECT id, type, message_group, CAST(payload AS CHAR) AS payload, \
                 CAST(retry_count AS SIGNED) AS retry_count, error_message, \
                 DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s.%f') AS created_at, \
                 DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s.%f') AS updated_at \
                 FROM {table} WHERE id IN ({})",
                placeholders(ids.len())
            );
            let mut query = sqlx::query(&read);
            for id in &ids {
                query = query.bind(id);
            }
            let rows = query.fetch_all(&mut *tx).await?;
            tx.commit().await?;

            for row in &rows {
                let type_str: String = row.try_get("type")?;
                let payload: Option<String> = row.try_get("payload")?;
                let created: Option<String> = row.try_get("created_at")?;
                let updated: Option<String> = row.try_get("updated_at")?;
                batch.push_row(
                    row.try_get("id")?,
                    type_str.parse()?,
                    row.try_get("message_group")?,
                    payload.as_deref().unwrap_or_default(),
                    row.try_get::<i64, _>("retry_count")? as i32,
                    row.try_get("error_message")?,
                    parse_text_timestamp(created.as_deref().unwrap_or_default()),
                    parse_text_timestamp(updated.as_deref().unwrap_or_default()),
                );
            }
        }
        Ok(batch)
    }

    async fn mark_success(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let sql = format!(
            "DELETE FROM {} WHERE id IN ({})",
            self.table(item_type)?,
            placeholders(ids.len())
        );
        let mut query = sqlx::query(&sql);
        for id in ids {
            query = query.bind(id);
        }
        query.execute(&self.pool).await?;
        Ok(())
    }

    async fn mark_failed(
        &self,
        item_type: OutboxItemType,
        ids: &[String],
        status: OutboxStatus,
        error_message: &str,
        requeue: bool,
    ) -> Result<()> {
        let new_status = if requeue {
            OutboxStatus::Pending
        } else {
            status
        };
        self.update_ids(
            item_type,
            &format!(
                "status = {}, error_message = ?, retry_count = retry_count + 1, \
                 updated_at = CURRENT_TIMESTAMP",
                new_status.code()
            ),
            "",
            vec![error_message.to_string()],
            ids,
        )
        .await
    }

    async fn release(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        self.update_ids(
            item_type,
            &format!(
                "status = {}, updated_at = CURRENT_TIMESTAMP",
                OutboxStatus::Pending.code()
            ),
            &format!("AND status = {}", OutboxStatus::InProgress.code()),
            Vec::new(),
            ids,
        )
        .await
    }

    async fn requeue(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        self.update_ids(
            item_type,
            &format!(
                "status = {}, retry_count = 0, error_message = NULL, updated_at = CURRENT_TIMESTAMP",
                OutboxStatus::Pending.code()
            ),
            "",
            Vec::new(),
            ids,
        )
        .await
    }

    async fn recover_stuck(&self, older_than: Duration) -> Result<u64> {
        let mut total = 0;
        for table in self.table_config.unique_tables() {
            let sql = format!(
                "UPDATE {} SET status = {}, updated_at = CURRENT_TIMESTAMP \
                 WHERE status = {} AND updated_at < (CURRENT_TIMESTAMP - INTERVAL ? SECOND)",
                checked_table_name(table)?,
                OutboxStatus::Pending.code(),
                OutboxStatus::InProgress.code()
            );
            total += sqlx::query(&sql)
                .bind(older_than.as_secs() as i64)
                .execute(&self.pool)
                .await?
                .rows_affected();
        }
        Ok(total)
    }

    async fn init_schema(&self) -> Result<()> {
        for table in self.table_config.unique_tables() {
            let table = checked_table_name(table)?;
            // The SDK migration's shape for MySQL 8 (TS, Go outboxsql, Java).
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
                    payload_size BIGINT,
                    headers JSON,
                    INDEX idx_outbox_messages_pending (status, message_group, created_at),
                    INDEX idx_outbox_messages_stuck (status, created_at),
                    INDEX idx_outbox_client_pending (client_id, status, created_at)
                ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
                "#,
            );
            sqlx::query(&schema).execute(&self.pool).await?;
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
