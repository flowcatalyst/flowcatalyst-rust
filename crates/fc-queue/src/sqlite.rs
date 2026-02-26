use async_trait::async_trait;
use chrono::Utc;
use sqlx::{Pool, Sqlite, Row};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn, info};

use fc_common::{Message, QueuedMessage};
use crate::{QueueConsumer, QueuePublisher, EmbeddedQueue, QueueMetrics, Result, QueueError};

/// SQLite-based queue that mimics SQS FIFO semantics for local development
pub struct SqliteQueue {
    pool: Pool<Sqlite>,
    queue_name: String,
    visibility_timeout_seconds: u32,
    running: AtomicBool,
    // Mutex for message group ordering - ensures only one message per group is in-flight
    #[allow(dead_code)]
    group_locks: Arc<Mutex<std::collections::HashMap<String, bool>>>,
}

impl SqliteQueue {
    pub fn new(pool: Pool<Sqlite>, queue_name: String, visibility_timeout_seconds: u32) -> Self {
        Self {
            pool,
            queue_name,
            visibility_timeout_seconds,
            running: AtomicBool::new(true),
            group_locks: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Create the queue schema
    async fn create_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS queue_messages (
                id TEXT PRIMARY KEY,
                queue_name TEXT NOT NULL,
                message_group_id TEXT,
                receipt_handle TEXT,
                visible_at INTEGER NOT NULL,
                payload TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                receive_count INTEGER DEFAULT 0,
                UNIQUE(queue_name, id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Index for efficient polling
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_queue_visible
            ON queue_messages (queue_name, visible_at, message_group_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Index for deduplication
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_queue_id
            ON queue_messages (queue_name, id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!(queue = %self.queue_name, "SQLite queue schema initialized");
        Ok(())
    }

    fn generate_receipt_handle(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[async_trait]
impl QueueConsumer for SqliteQueue {
    fn identifier(&self) -> &str {
        &self.queue_name
    }

    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(QueueError::Stopped);
        }

        let now = Utc::now().timestamp();
        let new_visible_at = now + self.visibility_timeout_seconds as i64;

        // Fetch visible messages, respecting message group ordering
        // For FIFO: only take the first message from each message group
        let rows = sqlx::query(
            r#"
            WITH eligible AS (
                SELECT id, message_group_id, payload,
                       ROW_NUMBER() OVER (PARTITION BY COALESCE(message_group_id, id) ORDER BY created_at) as rn
                FROM queue_messages
                WHERE queue_name = ? AND visible_at <= ?
            )
            SELECT id, message_group_id, payload
            FROM eligible
            WHERE rn = 1
            LIMIT ?
            "#,
        )
        .bind(&self.queue_name)
        .bind(now)
        .bind(max_messages as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut messages = Vec::with_capacity(rows.len());

        for row in rows {
            let id: String = row.get("id");
            let _message_group_id: Option<String> = row.get("message_group_id");
            let payload: String = row.get("payload");

            // Generate receipt handle and update visibility
            let receipt_handle = self.generate_receipt_handle();

            let updated = sqlx::query(
                r#"
                UPDATE queue_messages
                SET receipt_handle = ?, visible_at = ?, receive_count = receive_count + 1
                WHERE id = ? AND queue_name = ? AND visible_at <= ?
                "#,
            )
            .bind(&receipt_handle)
            .bind(new_visible_at)
            .bind(&id)
            .bind(&self.queue_name)
            .bind(now)
            .execute(&self.pool)
            .await?;

            if updated.rows_affected() == 0 {
                // Another consumer grabbed this message
                continue;
            }

            // Parse the message
            let message: Message = serde_json::from_str(&payload)?;

            messages.push(QueuedMessage {
                message,
                receipt_handle,
                broker_message_id: Some(id),
                queue_identifier: self.queue_name.clone(),
            });
        }

        if !messages.is_empty() {
            debug!(
                queue = %self.queue_name,
                count = messages.len(),
                "Polled messages from SQLite queue"
            );
        }

        Ok(messages)
    }

    async fn ack(&self, receipt_handle: &str) -> Result<()> {
        let result = sqlx::query(
            "DELETE FROM queue_messages WHERE receipt_handle = ? AND queue_name = ?",
        )
        .bind(receipt_handle)
        .bind(&self.queue_name)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            warn!(
                receipt_handle = %receipt_handle,
                queue = %self.queue_name,
                "ACK failed - message not found or already deleted"
            );
            return Err(QueueError::NotFound(receipt_handle.to_string()));
        }

        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            "Message acknowledged"
        );
        Ok(())
    }

    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        let delay = delay_seconds.unwrap_or(0) as i64;
        let new_visible_at = Utc::now().timestamp() + delay;

        let result = sqlx::query(
            r#"
            UPDATE queue_messages
            SET visible_at = ?, receipt_handle = NULL
            WHERE receipt_handle = ? AND queue_name = ?
            "#,
        )
        .bind(new_visible_at)
        .bind(receipt_handle)
        .bind(&self.queue_name)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            warn!(
                receipt_handle = %receipt_handle,
                queue = %self.queue_name,
                "NACK failed - message not found"
            );
            return Err(QueueError::NotFound(receipt_handle.to_string()));
        }

        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            delay_seconds = delay,
            "Message negative acknowledged"
        );
        Ok(())
    }

    async fn extend_visibility(&self, receipt_handle: &str, seconds: u32) -> Result<()> {
        let new_visible_at = Utc::now().timestamp() + seconds as i64;

        let result = sqlx::query(
            r#"
            UPDATE queue_messages
            SET visible_at = ?
            WHERE receipt_handle = ? AND queue_name = ?
            "#,
        )
        .bind(new_visible_at)
        .bind(receipt_handle)
        .bind(&self.queue_name)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            warn!(
                receipt_handle = %receipt_handle,
                queue = %self.queue_name,
                "Extend visibility failed - message not found"
            );
            return Err(QueueError::NotFound(receipt_handle.to_string()));
        }

        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            seconds = seconds,
            "Visibility extended"
        );
        Ok(())
    }

    fn is_healthy(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        info!(queue = %self.queue_name, "SQLite queue consumer stopped");
    }

    async fn get_metrics(&self) -> Result<Option<QueueMetrics>> {
        let now = Utc::now().timestamp();

        // Count pending messages (visible, not being processed)
        let pending_row = sqlx::query(
            "SELECT COUNT(*) as count FROM queue_messages WHERE queue_name = ? AND visible_at <= ? AND receipt_handle IS NULL"
        )
        .bind(&self.queue_name)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        let pending_messages: i64 = pending_row.get("count");

        // Count in-flight messages (have receipt_handle, currently being processed)
        let in_flight_row = sqlx::query(
            "SELECT COUNT(*) as count FROM queue_messages WHERE queue_name = ? AND receipt_handle IS NOT NULL"
        )
        .bind(&self.queue_name)
        .fetch_one(&self.pool)
        .await?;
        let in_flight_messages: i64 = in_flight_row.get("count");

        debug!(
            queue = %self.queue_name,
            pending = pending_messages,
            in_flight = in_flight_messages,
            "Retrieved SQLite queue metrics"
        );

        Ok(Some(QueueMetrics {
            pending_messages: pending_messages as u64,
            in_flight_messages: in_flight_messages as u64,
            queue_identifier: self.queue_name.clone(),
            // SQLite queue doesn't track these metrics yet
            total_polled: 0,
            total_acked: 0,
            total_nacked: 0,
            total_deferred: 0,
        }))
    }
}

#[async_trait]
impl QueuePublisher for SqliteQueue {
    fn identifier(&self) -> &str {
        &self.queue_name
    }

    async fn publish(&self, message: Message) -> Result<String> {
        let now = Utc::now();
        let payload = serde_json::to_string(&message)?;

        // Check for duplicate (idempotency)
        let existing = sqlx::query(
            "SELECT id FROM queue_messages WHERE id = ? AND queue_name = ?",
        )
        .bind(&message.id)
        .bind(&self.queue_name)
        .fetch_optional(&self.pool)
        .await?;

        if existing.is_some() {
            debug!(
                message_id = %message.id,
                queue = %self.queue_name,
                "Duplicate message detected, skipping"
            );
            return Ok(message.id);
        }

        sqlx::query(
            r#"
            INSERT INTO queue_messages (id, queue_name, message_group_id, visible_at, payload, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&message.id)
        .bind(&self.queue_name)
        .bind(&message.message_group_id)
        .bind(now.timestamp())
        .bind(&payload)
        .bind(now.timestamp())
        .execute(&self.pool)
        .await?;

        debug!(
            message_id = %message.id,
            queue = %self.queue_name,
            message_group = ?message.message_group_id,
            "Message published to SQLite queue"
        );

        Ok(message.id)
    }

    async fn publish_batch(&self, messages: Vec<Message>) -> Result<Vec<String>> {
        let mut ids = Vec::with_capacity(messages.len());
        for message in messages {
            let id = self.publish(message).await?;
            ids.push(id);
        }
        Ok(ids)
    }
}

#[async_trait]
impl EmbeddedQueue for SqliteQueue {
    async fn init_schema(&self) -> Result<()> {
        self.create_schema().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use fc_common::MediationType;

    async fn create_test_queue() -> SqliteQueue {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let queue = SqliteQueue::new(pool, "test-queue".to_string(), 30);
        queue.init_schema().await.unwrap();
        queue
    }

    #[tokio::test]
    async fn test_publish_and_poll() {
        let queue = create_test_queue().await;

        let message = Message {
            id: "msg-1".to_string(),
            pool_code: "TEST".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://localhost:8080".to_string(),
            message_group_id: None,
            high_priority: false,
        };

        // Publish
        let id = queue.publish(message).await.unwrap();
        assert_eq!(id, "msg-1");

        // Poll
        let messages = queue.poll(10).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message.id, "msg-1");

        // ACK
        queue.ack(&messages[0].receipt_handle).await.unwrap();

        // Poll again - should be empty
        let messages = queue.poll(10).await.unwrap();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_nack_with_delay() {
        let queue = create_test_queue().await;

        let message = Message {
            id: "msg-2".to_string(),
            pool_code: "TEST".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://localhost:8080".to_string(),
            message_group_id: None,
            high_priority: false,
        };

        queue.publish(message).await.unwrap();
        let messages = queue.poll(10).await.unwrap();

        // NACK with 60 second delay
        queue.nack(&messages[0].receipt_handle, Some(60)).await.unwrap();

        // Poll again - should be empty (message is delayed)
        let messages = queue.poll(10).await.unwrap();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_message_group_fifo() {
        let queue = create_test_queue().await;

        // Publish two messages in the same group
        for i in 1..=2 {
            let message = Message {
                id: format!("msg-{}", i),
                pool_code: "TEST".to_string(),
                auth_token: None,
                signing_secret: None,
                mediation_type: MediationType::HTTP,
                mediation_target: "http://localhost:8080".to_string(),
                message_group_id: Some("group-1".to_string()),
                high_priority: false,
            };
            queue.publish(message).await.unwrap();
        }

        // Poll - should only get the first message (FIFO within group)
        let messages = queue.poll(10).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message.id, "msg-1");

        // ACK first message
        queue.ack(&messages[0].receipt_handle).await.unwrap();

        // Poll again - now should get the second message
        let messages = queue.poll(10).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message.id, "msg-2");
    }

    #[tokio::test]
    async fn test_deduplication() {
        let queue = create_test_queue().await;

        let message = Message {
            id: "dup-msg".to_string(),
            pool_code: "TEST".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://localhost:8080".to_string(),
            message_group_id: None,
            high_priority: false,
        };

        // Publish same message twice
        queue.publish(message.clone()).await.unwrap();
        queue.publish(message).await.unwrap();

        // Should only have one message
        let messages = queue.poll(10).await.unwrap();
        assert_eq!(messages.len(), 1);
    }
}
