use async_trait::async_trait;
use chrono::Utc;
use sqlx::{Pool, Row, Sqlite};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::{EmbeddedQueue, QueueConsumer, QueueError, QueueMetrics, QueuePublisher, Result};
use fc_common::{Message, QueuedMessage};

/// Bound on the stored failure text (R-17). A pathological payload can
/// produce an arbitrarily long parse error, and it would otherwise be stored
/// verbatim once per quarantined row.
const MAX_QUARANTINE_ERROR_LEN: usize = 1000;

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

        // Quarantine for rows whose payload cannot be parsed (R-17). Without
        // it a single malformed row stops its queue forever: the claiming
        // UPDATE above commits before the payload is parsed, so a decode
        // failure leaves the row claimed, it becomes visible again once the
        // visibility window lapses, is re-claimed, and fails identically on
        // every subsequent poll. Table name matches the Postgres backend's
        // quarantine table on purpose (A-07 / R-17): one place for an
        // operator to look regardless of which backend is deployed.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS queue_messages_failed (
                id TEXT NOT NULL,
                queue_name TEXT NOT NULL,
                message_group_id TEXT,
                payload TEXT NOT NULL,
                error_message TEXT NOT NULL,
                receive_count INTEGER,
                created_at INTEGER,
                failed_at INTEGER NOT NULL,
                UNIQUE(queue_name, id)
            )
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

    /// Move one unparseable row out of `queue_messages` and into
    /// `queue_messages_failed`, so the row can never be both places or
    /// neither (R-17).
    ///
    /// A repeat quarantine keeps the LATEST failure (A-07): a row that
    /// fails, is requeued, and fails again is almost always being worked on
    /// — someone changed the payload, the schema, or the consumer — so the
    /// most recent failure is the one that describes what is wrong now.
    ///
    /// SQLite (unlike the Postgres backend) has no `DELETE ... RETURNING`
    /// combined with an upsert in one statement across two tables, so this
    /// runs as an explicit transaction: DELETE the poisoned row, then
    /// upsert it into `queue_messages_failed` using the payload/group/
    /// receive_count/created_at already read out of the row by the caller.
    /// The transaction keeps the move atomic even though it's two
    /// statements.
    #[allow(clippy::too_many_arguments)]
    async fn quarantine(
        &self,
        id: &str,
        message_group_id: Option<&str>,
        payload: &str,
        reason: &str,
        receive_count: i64,
        created_at: i64,
    ) -> Result<()> {
        let mut reason = reason.to_string();
        if reason.len() > MAX_QUARANTINE_ERROR_LEN {
            reason.truncate(MAX_QUARANTINE_ERROR_LEN);
        }
        let failed_at = Utc::now().timestamp();

        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM queue_messages WHERE queue_name = ? AND id = ?")
            .bind(&self.queue_name)
            .bind(id)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO queue_messages_failed
                (id, queue_name, message_group_id, payload, error_message, receive_count, created_at, failed_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (queue_name, id) DO UPDATE SET
                payload       = excluded.payload,
                error_message = excluded.error_message,
                receive_count = excluded.receive_count,
                created_at    = excluded.created_at,
                failed_at     = excluded.failed_at
            "#,
        )
        .bind(id)
        .bind(&self.queue_name)
        .bind(message_group_id)
        .bind(payload)
        .bind(&reason)
        .bind(receive_count)
        .bind(created_at)
        .bind(failed_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
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
                SELECT id, message_group_id, payload, created_at,
                       ROW_NUMBER() OVER (PARTITION BY COALESCE(message_group_id, id) ORDER BY created_at) as rn
                FROM queue_messages
                WHERE queue_name = ? AND visible_at <= ?
            )
            SELECT id, message_group_id, payload, created_at
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
            let message_group_id: Option<String> = row.get("message_group_id");
            let payload: String = row.get("payload");
            let created_at: i64 = row.get("created_at");

            // Generate receipt handle and update visibility
            let receipt_handle = self.generate_receipt_handle();

            let updated = sqlx::query(
                r#"
                UPDATE queue_messages
                SET receipt_handle = ?, visible_at = ?, receive_count = receive_count + 1
                WHERE id = ? AND queue_name = ? AND visible_at <= ?
                RETURNING receive_count
                "#,
            )
            .bind(&receipt_handle)
            .bind(new_visible_at)
            .bind(&id)
            .bind(&self.queue_name)
            .bind(now)
            .fetch_optional(&self.pool)
            .await?;

            let Some(updated_row) = updated else {
                // Another consumer grabbed this message
                continue;
            };
            let receive_count: i64 = updated_row.get("receive_count");

            // Parse the message. The claiming UPDATE above has already
            // committed, so propagating a decode error here would abort the
            // whole batch (via `?`) and leave the row claimed — it becomes
            // visible again once the timeout lapses, is re-claimed, and
            // fails identically forever, taking every healthy row in the
            // same poll down with it every time (R-17). Quarantine it
            // instead and keep going so the rest of this batch still
            // delivers.
            let message: Message = match serde_json::from_str(&payload) {
                Ok(message) => message,
                Err(err) => {
                    let reason = err.to_string();
                    match self
                        .quarantine(
                            &id,
                            message_group_id.as_deref(),
                            &payload,
                            &reason,
                            receive_count,
                            created_at,
                        )
                        .await
                    {
                        Ok(()) => {
                            warn!(
                                queue = %self.queue_name,
                                message_id = %id,
                                reason = %reason,
                                "Malformed message moved to queue_messages_failed"
                            );
                        }
                        Err(quarantine_err) => {
                            // Couldn't move it — log and leave it claimed. It
                            // comes back on the next poll once its
                            // visibility lapses and we try again; the rest
                            // of this batch is already returned, so one
                            // unmovable row no longer costs the whole queue.
                            warn!(
                                queue = %self.queue_name,
                                message_id = %id,
                                error = %quarantine_err,
                                "Could not quarantine malformed message"
                            );
                        }
                    }
                    continue;
                }
            };

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
        let result =
            sqlx::query("DELETE FROM queue_messages WHERE receipt_handle = ? AND queue_name = ?")
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
        let existing = sqlx::query("SELECT id FROM queue_messages WHERE id = ? AND queue_name = ?")
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
    use fc_common::MediationType;
    use sqlx::sqlite::SqlitePoolOptions;

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
            dispatch_mode: fc_common::DispatchMode::default(),
            dispatch_mode_specified: true,
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
            dispatch_mode: fc_common::DispatchMode::default(),
            dispatch_mode_specified: true,
        };

        queue.publish(message).await.unwrap();
        let messages = queue.poll(10).await.unwrap();

        // NACK with 60 second delay
        queue
            .nack(&messages[0].receipt_handle, Some(60))
            .await
            .unwrap();

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
                dispatch_mode: fc_common::DispatchMode::default(),
                dispatch_mode_specified: true,
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
            dispatch_mode: fc_common::DispatchMode::default(),
            dispatch_mode_specified: true,
        };

        // Publish same message twice
        queue.publish(message.clone()).await.unwrap();
        queue.publish(message).await.unwrap();

        // Should only have one message
        let messages = queue.poll(10).await.unwrap();
        assert_eq!(messages.len(), 1);
    }

    /// Insert a row directly with a payload that will not parse as a
    /// `Message`, bypassing `publish()` (which can only ever produce valid
    /// JSON). This is how a poison row actually gets into the table in
    /// practice: a producer writing a stale/foreign schema, not this queue.
    async fn insert_poison_row(queue: &SqliteQueue, id: &str, payload: &str) {
        let now = Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO queue_messages (id, queue_name, message_group_id, visible_at, payload, created_at) \
             VALUES (?, ?, NULL, ?, ?, ?)",
        )
        .bind(id)
        .bind(&queue.queue_name)
        .bind(now)
        .bind(payload)
        .bind(now)
        .execute(&queue.pool)
        .await
        .unwrap();
    }

    struct FailedRow {
        payload: String,
        error_message: String,
    }

    async fn get_failed_rows(queue: &SqliteQueue, id: &str) -> Vec<FailedRow> {
        sqlx::query("SELECT payload, error_message FROM queue_messages_failed WHERE queue_name = ? AND id = ?")
            .bind(&queue.queue_name)
            .bind(id)
            .fetch_all(&queue.pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| FailedRow {
                payload: row.get("payload"),
                error_message: row.get("error_message"),
            })
            .collect()
    }

    // R-17 / A-07: a malformed payload must not abort the whole poll batch,
    // and must not keep re-claiming forever — it is quarantined to
    // `queue_messages_failed` and the poll continues.
    #[tokio::test]
    async fn test_poison_row_quarantined_healthy_row_still_delivers() {
        let queue = create_test_queue().await;

        // A healthy message alongside the poison row in the same poll batch.
        let healthy = Message {
            id: "healthy-1".to_string(),
            pool_code: "TEST".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://localhost:8080".to_string(),
            message_group_id: None,
            high_priority: false,
            dispatch_mode: fc_common::DispatchMode::default(),
            dispatch_mode_specified: true,
        };
        queue.publish(healthy).await.unwrap();
        insert_poison_row(&queue, "poison-1", "not-valid-json-at-all").await;

        // The healthy row still delivers; the poison row is silently
        // dropped from this batch (it never fails the poll via `?`).
        let messages = queue.poll(10).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message.id, "healthy-1");

        // The poison row landed in queue_messages_failed with its error.
        let failed = get_failed_rows(&queue, "poison-1").await;
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].payload, "not-valid-json-at-all");
        assert!(
            !failed[0].error_message.is_empty(),
            "expected a non-empty decode error to be recorded"
        );

        // It is gone from queue_messages, so it can never re-claim forever.
        let remaining: i64 =
            sqlx::query("SELECT COUNT(*) as count FROM queue_messages WHERE queue_name = ? AND id = ?")
                .bind(&queue.queue_name)
                .bind("poison-1")
                .fetch_one(&queue.pool)
                .await
                .unwrap()
                .get("count");
        assert_eq!(remaining, 0);

        // A second poll never sees the poison row again (it's not just
        // invisible until its visibility timeout lapses — it's gone).
        let messages = queue.poll(10).await.unwrap();
        assert!(messages.is_empty());
    }

    // A-07: a second failure for the same id overwrites the first — the
    // latest failure wins, not the first.
    #[tokio::test]
    async fn test_quarantine_latest_failure_wins() {
        let queue = create_test_queue().await;

        insert_poison_row(&queue, "poison-2", "totally not json").await;
        let messages = queue.poll(10).await.unwrap();
        assert!(messages.is_empty());

        let failed = get_failed_rows(&queue, "poison-2").await;
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].payload, "totally not json");
        let first_error = failed[0].error_message.clone();

        // Requeue the same id with a payload that fails for a *different*
        // reason (valid JSON, but missing required Message fields) so the
        // two failure reasons are distinguishable.
        insert_poison_row(&queue, "poison-2", "{}").await;
        let messages = queue.poll(10).await.unwrap();
        assert!(messages.is_empty());

        let failed = get_failed_rows(&queue, "poison-2").await;
        assert_eq!(
            failed.len(),
            1,
            "a repeat failure must overwrite, not accumulate, rows for the same id"
        );
        assert_eq!(failed[0].payload, "{}");
        assert_ne!(
            failed[0].error_message, first_error,
            "the latest failure's reason must replace the earlier one"
        );
    }
}
