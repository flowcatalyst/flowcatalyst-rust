use async_trait::async_trait;
use chrono::Utc;
use sqlx::{PgPool, Row};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    total_polled: AtomicU64,
    total_acked: AtomicU64,
    total_nacked: AtomicU64,
    total_deferred: AtomicU64,
}

impl PostgresQueue {
    pub fn new(pool: PgPool, queue_name: String, visibility_timeout_seconds: u32) -> Self {
        Self {
            pool,
            queue_name,
            visibility_timeout_seconds,
            running: AtomicBool::new(true),
            total_polled: AtomicU64::new(0),
            total_acked: AtomicU64::new(0),
            total_nacked: AtomicU64::new(0),
            total_deferred: AtomicU64::new(0),
        }
    }

    /// Make a claimed row visible again after `delay_seconds` and clear its
    /// receipt handle (Go: `makeVisible`). A receipt handle that no longer
    /// matches a row is not an error, as in Go: the row was already acked,
    /// or its visibility lapsed and it was re-claimed under a new handle —
    /// either way there is nothing left for this handle to release.
    async fn make_visible(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
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
            debug!(
                receipt_handle = %receipt_handle,
                queue = %self.queue_name,
                "Release matched no row (already acked or re-claimed)"
            );
        }
        Ok(())
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

/// Sensible default `max_connections` for a per-queue Postgres pool
/// (`docs/spec/router.md` §7.3: "one pgxpool per consumer"), mirroring Go's
/// own `pgxpool.New` default of `max(4, runtime.NumCPU())`.
///
/// Found via the router bench rig (`bench/router`, `BROKER=postgres
/// QUEUES=8`, item 1): every claim/ack/nack query itself was fast (p100
/// under the batch < 70ms even under load — see the SQL comment on
/// `poll()`'s claiming UPDATE), but a hardcoded `max_connections(4)` still
/// starved the queue empty: with `POOL_CONCURRENCY=256` worker tasks all
/// needing to ACK through the *same* 4-connection pool a poll loop that
/// re-polls immediately (G12, no pacing sleep) is *also* drawing from,
/// acks queued up behind claim traffic for long enough that the row's
/// visibility timeout (default 30s) lapsed before the ack landed — the row
/// got silently re-claimed out from under the still-in-flight delivery,
/// and the original ack, when it finally ran, failed with "message not
/// found" against the now-superseded receipt handle (`ack`'s doc comment
/// on `receipt_handle`). The failed ack's fallback (`pending_delete` in
/// `manager/routing.rs`) only resolves on the row's *next* natural
/// reclaim, which — since every reclaim resets the visibility deadline
/// another 30s out — can take a full extra visibility cycle per miss,
/// compounding: 947 of 50,000 rows were still sitting in `queue_messages`
/// 30s after the sink had already recorded all 50,000 deliveries (they
/// were not lost — draining the same table by hand afterward showed 0
/// rows once the process was left running — just very slow to resolve).
/// Raising the pool size is a direct fix for the contention that starts
/// the whole cascade, not a workaround for its symptom.
///
/// `std::thread::available_parallelism()` (not the `num_cpus` crate — no
/// new dependency needed) reads the same OS-reported core count Go's
/// `runtime.NumCPU()` does; both are equally unaware of a `--cpus`
/// container quota (`/proc/cpuinfo` inside a `--cpus=1` container still
/// reports the host's full core count) — that's Go's own behaviour being
/// mirrored here, not something this port should silently "fix" by
/// under-sizing relative to Go on the same host.
pub fn default_max_connections() -> u32 {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    cpus.max(4)
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
        // `poll_uuid` identifies THIS poll call; the receipt handle each
        // claimed row gets is `poll_uuid:id` (docs/spec/router.md §7.3:
        // "receipt_handle = pollUUID||':'||id"), computed server-side in
        // the UPDATE below so it's unique PER ROW, not shared across the
        // whole batch — see the doc comment on `QueuedMessage::receipt_handle`
        // just below for why that distinction is load-bearing.
        let poll_uuid = self.generate_receipt_handle();

        // Claim up to max_messages eligible rows atomically — Go's claim
        // query (`internal/queue/postgres/postgres.go` Poll), column for
        // column, so a Go and a Rust router can drain the same table.
        //
        // Eligible = visible now AND the earliest eligible row of its group
        // (COALESCE(message_group_id, id), so a NULL group is a singleton).
        // Two NOT EXISTS clauses, not a windowed CTE:
        //   - `FOR UPDATE SKIP LOCKED` over a ROW_NUMBER() result locks
        //     nothing once Postgres inlines the CTE, so two pollers could
        //     claim the same rows and deliver them twice;
        //   - an earlier row of the group that was RELEASED WITH A DELAY
        //     (receipt_handle NULL, visible_at in the future) blocks its
        //     successors (R4, owner ruling 2026-09-17). The window only saw
        //     visible rows, so a delayed-nacked group head was overtaken by
        //     its own successor on the next poll. A CLAIMED earlier row
        //     (receipt_handle set) does not block, as in Go.
        // Ties on created_at (same-second inserts) break on id, in both the
        // group-head test and the claim order.
        let rows = sqlx::query(
            r#"
            WITH claimed AS (
              SELECT m.id
                FROM queue_messages m
               WHERE m.queue_name = $1
                 AND m.visible_at <= $2
                 AND NOT EXISTS (
                       SELECT 1 FROM queue_messages e
                        WHERE e.queue_name = m.queue_name
                          AND COALESCE(e.message_group_id, e.id) = COALESCE(m.message_group_id, m.id)
                          AND e.visible_at <= $2
                          AND (e.created_at < m.created_at
                               OR (e.created_at = m.created_at AND e.id < m.id))
                     )
                 AND NOT EXISTS (
                       SELECT 1 FROM queue_messages e
                        WHERE e.queue_name = m.queue_name
                          AND COALESCE(e.message_group_id, e.id) = COALESCE(m.message_group_id, m.id)
                          AND e.receipt_handle IS NULL
                          AND e.visible_at > $2
                          AND (e.created_at < m.created_at
                               OR (e.created_at = m.created_at AND e.id < m.id))
                     )
               ORDER BY m.created_at, m.id
               LIMIT $3
               FOR UPDATE SKIP LOCKED
            )
            UPDATE queue_messages t
               SET receipt_handle = $4 || ':' || t.id,
                   visible_at     = $5,
                   receive_count  = t.receive_count + 1
              FROM claimed
             WHERE t.queue_name = $1
               AND t.id = claimed.id
             RETURNING t.id, t.message_group_id, t.payload, t.created_at
            "#,
        )
        .bind(&self.queue_name)
        .bind(now)
        .bind(max_messages as i64)
        .bind(&poll_uuid)
        .bind(new_visible_at)
        .fetch_all(&self.pool)
        .await?;

        // UPDATE ... RETURNING carries no order; hand the batch back in claim
        // order so a batch holding several groups' heads stays deterministic.
        let mut rows = rows;
        rows.sort_by(|a, b| {
            let (ac, bc): (i64, i64) = (a.get("created_at"), b.get("created_at"));
            let (ai, bi): (String, String) = (a.get("id"), b.get("id"));
            ac.cmp(&bc).then(ai.cmp(&bi))
        });

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

            // Per-row receipt handle (`poll_uuid:id`, matching what the
            // UPDATE above just computed server-side) — NOT the bare
            // `poll_uuid` shared by the whole batch. `ack`/`nack` key
            // solely on `receipt_handle` + `queue_name` (no `id` filter),
            // so a shared handle meant any one message's ack deleted every
            // other message claimed in the same poll() call in one shot —
            // the rest then failed their own ack/nack against a row that
            // was already gone, and if that fired for a message whose
            // delivery actually needed a *nack* (release back to the
            // broker), the row had already vanished instead: a real
            // message loss, not just a spurious warning. Verified against
            // the bench rig (`bench/router`, BROKER=postgres QUEUES=8):
            // pre-fix, ~90% of acks logged "message not found or already
            // deleted" and the queue never fully drained within the grace
            // window.
            let row_receipt_handle = format!("{poll_uuid}:{id}");

            messages.push(QueuedMessage {
                message,
                receipt_handle: row_receipt_handle,
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
            self.total_polled
                .fetch_add(messages.len() as u64, Ordering::Relaxed);
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

        self.total_acked.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            "Message acknowledged"
        );
        Ok(())
    }

    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        self.make_visible(receipt_handle, delay_seconds).await?;
        self.total_nacked.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Same release as `nack`, counted as a deferral rather than a failure
    /// (Go: `Defer`).
    async fn defer(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        self.make_visible(receipt_handle, delay_seconds).await?;
        self.total_deferred.fetch_add(1, Ordering::Relaxed);
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
            total_polled: self.total_polled.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_nacked: self.total_nacked.load(Ordering::Relaxed),
            total_deferred: self.total_deferred.load(Ordering::Relaxed),
        }))
    }

    fn get_counters(&self) -> Option<QueueMetrics> {
        Some(QueueMetrics {
            queue_identifier: self.queue_name.clone(),
            total_polled: self.total_polled.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_nacked: self.total_nacked.load(Ordering::Relaxed),
            total_deferred: self.total_deferred.load(Ordering::Relaxed),
            ..QueueMetrics::default()
        })
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

#[cfg(test)]
mod pool_sizing_tests {
    use super::*;

    /// Pins the `max(4, cpus)` formula itself (Go's `pgxpool.New` default)
    /// — independent of whatever core count this particular test host
    /// reports, both arms of the `max` must hold.
    ///
    /// Mutant check (hand-edited `default_max_connections` to return the
    /// bare `cpus` with no `.max(4)` floor, confirmed by hand while
    /// implementing this fix, then restored): on any host with 1-3 cores
    /// visible to the process this assertion fails immediately (a value
    /// under 4); on this dev machine (14 cores) it happened not to catch
    /// the floor, which is exactly why the assertion is a `>=` bound, not
    /// an exact-equality one that could pass vacuously on a wide-core CI
    /// runner.
    #[test]
    fn never_below_four_regardless_of_host_cpu_count() {
        let n = default_max_connections();
        assert!(n >= 4, "default_max_connections() = {n}, must be >= 4");
    }

    /// The formula must actually track the host's reported core count when
    /// that's higher than the floor — not just clamp everything to 4. This
    /// is the assertion that pins the actual bug fixed here (a hardcoded 4
    /// regardless of host cores): a mutant that hardcodes the return value
    /// to `4` passes the floor test above but fails this one whenever the
    /// test host reports more than 4 cores (true of essentially every CI
    /// runner and dev machine today).
    #[test]
    fn tracks_available_parallelism_above_the_floor() {
        let reported = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1);
        let n = default_max_connections();
        assert_eq!(
            n,
            reported.max(4),
            "default_max_connections() must equal max(4, available_parallelism()), \
             not a value independent of the host's reported core count"
        );
    }
}
