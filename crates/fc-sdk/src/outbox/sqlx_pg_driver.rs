//! Built-in PostgreSQL OutboxDriver using sqlx.

use async_trait::async_trait;
use sqlx::PgPool;

use super::driver::{OutboxDriver, OutboxMessage};
use super::error::OutboxError;

/// PostgreSQL outbox driver backed by sqlx.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::outbox::{SqlxPgDriver, OutboxManager};
///
/// let pool = sqlx::PgPool::connect("postgresql://localhost/myapp").await?;
/// let driver = SqlxPgDriver::new(pool);
/// let outbox = OutboxManager::new(Box::new(driver), "clt_0HZXEQ5Y8JY5Z");
/// ```
#[derive(Clone)]
pub struct SqlxPgDriver {
    pool: PgPool,
    table: String,
}

impl SqlxPgDriver {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            table: "outbox_messages".to_string(),
        }
    }

    pub fn with_table(pool: PgPool, table: impl Into<String>) -> Self {
        Self {
            pool,
            table: table.into(),
        }
    }
}

#[async_trait]
impl OutboxDriver for SqlxPgDriver {
    async fn insert(&self, message: OutboxMessage) -> Result<(), OutboxError> {
        let query = format!(
            "INSERT INTO {} (id, type, message_group, payload, status, retry_count, \
             created_at, updated_at, client_id, payload_size, headers) \
             VALUES ($1, $2, $3, $4::jsonb, $5, 0, NOW(), NOW(), $6, $7, $8::jsonb)",
            self.table
        );

        let headers_json = message
            .headers
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        sqlx::query(&query)
            .bind(&message.id)
            .bind(message.message_type.as_str())
            .bind(&message.message_group)
            .bind(&message.payload)
            .bind(message.status.code())
            .bind(&message.client_id)
            .bind(message.payload_size)
            .bind(&headers_json)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn insert_batch(&self, messages: Vec<OutboxMessage>) -> Result<(), OutboxError> {
        if messages.is_empty() {
            return Ok(());
        }

        let mut txn = self.pool.begin().await?;

        let query = format!(
            "INSERT INTO {} (id, type, message_group, payload, status, retry_count, \
             created_at, updated_at, client_id, payload_size, headers) \
             VALUES ($1, $2, $3, $4::jsonb, $5, 0, NOW(), NOW(), $6, $7, $8::jsonb)",
            self.table
        );

        for message in &messages {
            let headers_json = message
                .headers
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;

            sqlx::query(&query)
                .bind(&message.id)
                .bind(message.message_type.as_str())
                .bind(&message.message_group)
                .bind(&message.payload)
                .bind(message.status.code())
                .bind(&message.client_id)
                .bind(message.payload_size)
                .bind(&headers_json)
                .execute(&mut *txn)
                .await?;
        }

        txn.commit().await?;
        Ok(())
    }
}
