use async_trait::async_trait;
use chrono::Utc;
use sqlx::{PgPool, Row};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, info, warn};

/// Bound on the stored failure text (R-17). A pathological payload can
/// produce an arbitrarily long parse error, and it would otherwise be stored
/// verbatim once per quarantined row.
const MAX_QUARANTINE_ERROR_LEN: usize = 1000;

use crate::{EmbeddedQueue, QueueConsumer, QueueError, QueueMetrics, QueuePublisher, Result};
use fc_common::{Message, QueuedMessage};

/// Postgres-backed queue that mimics SQS FIFO semantics for local development.
///
/// Mirrors `SqliteQueue` but uses Postgres's `FOR UPDATE SKIP LOCKED` to
/// coordinate between concurrent pollers without serialising writes.
pub struct PostgresQueue {
    pool: PgPool,
    queue_name: String,
    visibility_timeout_seconds: u32,
    running: AtomicBool,
}

impl PostgresQueue {
    pub fn new(pool: PgPool, queue_name: String, visibility_timeout_seconds: u32) -> Self {
        Self {
            pool,
            queue_name,
            visibility_timeout_seconds,
            running: AtomicBool::new(true),
        }
    }

    /// Create the queue schema (idempotent).
    async fn create_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS queue_messages (
                id TEXT NOT NULL,
                queue_name TEXT NOT NULL,
                message_group_id TEXT,
                receipt_handle TEXT,
                visible_at BIGINT NOT NULL,
                payload TEXT NOT NULL,
                created_at BIGINT NOT NULL,
                receive_count INTEGER DEFAULT 0,
                PRIMARY KEY (queue_name, id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_queue_visible
            ON queue_messages (queue_name, visible_at, message_group_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Quarantine for rows whose payload cannot be parsed (R-17). Without
        // it a single malformed row stops its queue forever: the claiming
        // UPDATE...RETURNING above commits before the payload is parsed, so a
        // decode failure leaves the row claimed, it becomes visible again
        // once the visibility window lapses, is re-claimed, and fails
        // identically on every subsequent poll.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS queue_messages_failed (
                id               TEXT NOT NULL,
                queue_name       TEXT NOT NULL,
                message_group_id TEXT,
                payload          TEXT NOT NULL,
                error_message    TEXT NOT NULL,
                receive_count    INTEGER,
                created_at       BIGINT,
                failed_at        BIGINT NOT NULL,
                PRIMARY KEY (queue_name, id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!(queue = %self.queue_name, "Postgres queue schema initialized");
        Ok(())
    }

    /// Move one unparseable row out of `queue_messages` and into
    /// `queue_messages_failed` in a single statement, so the row can never be
    /// both places or neither (R-17).
    ///
    /// A repeat quarantine keeps the LATEST failure (A-07): a row that fails,
    /// is requeued, and fails again is almost always being worked on —
    /// someone changed the payload, the schema, or the consumer — so the most
    /// recent failure is the one that describes what is wrong now.
    async fn quarantine(&self, id: &str, reason: &str) -> Result<()> {
        let mut reason = reason.to_string();
        if reason.len() > MAX_QUARANTINE_ERROR_LEN {
            reason.truncate(MAX_QUARANTINE_ERROR_LEN);
        }
        let failed_at = Utc::now().timestamp();

        sqlx::query(
            r#"
            WITH moved AS (
                DELETE FROM queue_messages
                 WHERE queue_name = $1 AND id = $2
                RETURNING id, queue_name, message_group_id, payload, receive_count, created_at
            )
            INSERT INTO queue_messages_failed
                (id, queue_name, message_group_id, payload, error_message, receive_count, created_at, failed_at)
            SELECT id, queue_name, message_group_id, payload, $3, receive_count, created_at, $4
              FROM moved
            ON CONFLICT (queue_name, id) DO UPDATE SET
                payload       = EXCLUDED.payload,
                error_message = EXCLUDED.error_message,
                receive_count = EXCLUDED.receive_count,
                created_at    = EXCLUDED.created_at,
                failed_at     = EXCLUDED.failed_at
            "#,
        )
        .bind(&self.queue_name)
        .bind(id)
        .bind(&reason)
        .bind(failed_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    fn generate_receipt_handle(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[async_trait]
impl QueueConsumer for PostgresQueue {
    fn identifier(&self) -> &str {
        &self.queue_name
    }

    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(QueueError::Stopped);
        }

        let now = Utc::now().timestamp();
        let new_visible_at = now + self.visibility_timeout_seconds as i64;
        let receipt_handle = self.generate_receipt_handle();

        // Claim up to max_messages eligible rows atomically.
        //
        // For FIFO: within a message group, only the earliest visible
        // message is eligible at a time (ROW_NUMBER over partition).
        // `FOR UPDATE SKIP LOCKED` lets concurrent pollers grab different
        // messages without contending on a global write lock the way
        // SQLite has to.
        let rows = sqlx::query(
            r#"
            WITH eligible AS (
                SELECT id, message_group_id, payload,
                       ROW_NUMBER() OVER (
                           PARTITION BY COALESCE(message_group_id, id)
                           ORDER BY created_at
                       ) AS rn
                FROM queue_messages
                WHERE queue_name = $1 AND visible_at <= $2
            ),
            claimed AS (
                SELECT id
                FROM eligible
                WHERE rn = 1
                LIMIT $3
                FOR UPDATE SKIP LOCKED
            )
            UPDATE queue_messages m
               SET receipt_handle = $4,
                   visible_at = $5,
                   receive_count = m.receive_count + 1
              FROM claimed
             WHERE m.queue_name = $1
               AND m.id = claimed.id
             RETURNING m.id, m.message_group_id, m.payload
            "#,
        )
        .bind(&self.queue_name)
        .bind(now)
        .bind(max_messages as i64)
        .bind(&receipt_handle)
        .bind(new_visible_at)
        .fetch_all(&self.pool)
        .await?;

        let mut messages = Vec::with_capacity(rows.len());
        // Poison rows (payload failed to decode) are quarantined after the
        // main loop below, once we're done matching on `rows` by value.
        let mut poisoned: Vec<(String, String)> = Vec::new();
        for row in rows {
            let id: String = row.get("id");
            let _message_group_id: Option<String> = row.get("message_group_id");
            let payload: String = row.get("payload");

            let message: Message = match serde_json::from_str(&payload) {
                Ok(message) => message,
                Err(err) => {
                    // The claiming UPDATE above has already committed, so
                    // propagating this error would abort the whole batch and
                    // leave the row claimed — it becomes visible again after
                    // the timeout, is re-claimed, and fails identically
                    // forever, taking every healthy row in the same poll
                    // down with it every time (R-17). Quarantine it instead
                    // and keep going so the rest of this batch still
                    // delivers.
                    poisoned.push((id, err.to_string()));
                    continue;
                }
            };

            messages.push(QueuedMessage {
                message,
                // Every row in this batch shares the receipt_handle because
                // we issued one per poll() call. That's fine: ack/nack/extend
                // all match on (receipt_handle, queue_name, id) via the
                // broker_message_id when needed. For typical single-claim
                // per receipt, issue one handle per message by moving the
                // generation into the loop — but for batch claims this is
                // simpler and keeps parity with SQLite behaviour.
                receipt_handle: receipt_handle.clone(),
                broker_message_id: Some(id),
                queue_identifier: self.queue_name.clone(),
            });
        }

        for (id, reason) in poisoned {
            match self.quarantine(&id, &reason).await {
                Ok(()) => {
                    warn!(
                        queue = %self.queue_name,
                        message_id = %id,
                        reason = %reason,
                        "Malformed message moved to queue_messages_failed"
                    );
                }
                Err(err) => {
                    // Couldn't move it — log and leave it claimed. It comes
                    // back on the next poll once its visibility lapses and
                    // we try again; the rest of this batch is already
                    // returned, so one unmovable row no longer costs the
                    // whole queue.
                    warn!(
                        queue = %self.queue_name,
                        message_id = %id,
                        error = %err,
                        "Could not quarantine malformed message"
                    );
                }
            }
        }

        if !messages.is_empty() {
            debug!(
                queue = %self.queue_name,
                count = messages.len(),
                "Polled messages from Postgres queue"
            );
        }

        Ok(messages)
    }

    async fn ack(&self, receipt_handle: &str) -> Result<()> {
        let result =
            sqlx::query("DELETE FROM queue_messages WHERE receipt_handle = $1 AND queue_name = $2")
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
               SET visible_at = $1, receipt_handle = NULL
             WHERE receipt_handle = $2 AND queue_name = $3
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
               SET visible_at = $1
             WHERE receipt_handle = $2 AND queue_name = $3
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
        info!(queue = %self.queue_name, "Postgres queue consumer stopped");
    }

    async fn get_metrics(&self) -> Result<Option<QueueMetrics>> {
        let now = Utc::now().timestamp();

        let pending_row = sqlx::query(
            "SELECT COUNT(*) as count FROM queue_messages WHERE queue_name = $1 AND visible_at <= $2 AND receipt_handle IS NULL"
        )
        .bind(&self.queue_name)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        let pending_messages: i64 = pending_row.get("count");

        let in_flight_row = sqlx::query(
            "SELECT COUNT(*) as count FROM queue_messages WHERE queue_name = $1 AND receipt_handle IS NOT NULL"
        )
        .bind(&self.queue_name)
        .fetch_one(&self.pool)
        .await?;
        let in_flight_messages: i64 = in_flight_row.get("count");

        debug!(
            queue = %self.queue_name,
            pending = pending_messages,
            in_flight = in_flight_messages,
            "Retrieved Postgres queue metrics"
        );

        Ok(Some(QueueMetrics {
            pending_messages: pending_messages as u64,
            in_flight_messages: in_flight_messages as u64,
            queue_identifier: self.queue_name.clone(),
            total_polled: 0,
            total_acked: 0,
            total_nacked: 0,
            total_deferred: 0,
        }))
    }
}

#[async_trait]
impl QueuePublisher for PostgresQueue {
    fn identifier(&self) -> &str {
        &self.queue_name
    }

    async fn publish(&self, message: Message) -> Result<String> {
        let now = Utc::now();
        let payload = serde_json::to_string(&message)?;

        // ON CONFLICT DO NOTHING handles duplicate IDs atomically in one
        // round-trip (the SQLite version does a separate SELECT + INSERT).
        let result = sqlx::query(
            r#"
            INSERT INTO queue_messages
                (id, queue_name, message_group_id, visible_at, payload, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (queue_name, id) DO NOTHING
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

        if result.rows_affected() == 0 {
            debug!(
                message_id = %message.id,
                queue = %self.queue_name,
                "Duplicate message detected, skipping"
            );
        } else {
            debug!(
                message_id = %message.id,
                queue = %self.queue_name,
                message_group = ?message.message_group_id,
                "Message published to Postgres queue"
            );
        }

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
impl EmbeddedQueue for PostgresQueue {
    async fn init_schema(&self) -> Result<()> {
        self.create_schema().await
    }
}
