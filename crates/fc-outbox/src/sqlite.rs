//! SQLite Outbox Repository Implementation
//!
//! The outbox table an application keeps in SQLite (e.g. a Laravel app on
//! SQLite through the Laravel SDK's migration). SQLite has no column types to
//! speak of, so every column is read as text or integer, and timestamps are
//! accepted in RFC 3339 or SQL form. The claim is one `UPDATE … RETURNING`
//! statement, which SQLite runs atomically.

use crate::repository::{
    checked_table_name, parse_text_timestamp, ClaimedBatch, OutboxRepository, OutboxTableConfig,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use fc_common::{OutboxItemType, OutboxStatus};
use sqlx::{Row, SqlitePool};
use std::time::Duration;
use tracing::info;

/// SQLite implementation of OutboxRepository
pub struct SqliteOutboxRepository {
    pool: SqlitePool,
    table_config: OutboxTableConfig,
}

/// The processor's own timestamps: RFC 3339 UTC with milliseconds, which
/// sort as text (recovery compares them as text).
fn now_text() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

impl SqliteOutboxRepository {
    /// Create a new SQLite outbox repository with default table config
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            table_config: OutboxTableConfig::default(),
        }
    }

    /// Create with custom table configuration
    pub fn with_config(pool: SqlitePool, table_config: OutboxTableConfig) -> Self {
        Self { pool, table_config }
    }

    /// Get the pool reference
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    fn table(&self, item_type: OutboxItemType) -> Result<&str> {
        checked_table_name(self.table_config.table_for_type(item_type))
    }

    /// `UPDATE {table} SET {set} WHERE id IN (…) {and}` with `ids` bound
    /// after `binds`.
    async fn update_ids(
        &self,
        item_type: OutboxItemType,
        set: &str,
        and: &str,
        binds: Vec<String>,
        ids: &[String],
    ) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
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
        Ok(query.execute(&self.pool).await?.rows_affected())
    }
}

#[async_trait]
impl OutboxRepository for SqliteOutboxRepository {
    async fn claim_pending(&self, limit: u32) -> Result<ClaimedBatch> {
        let mut batch = ClaimedBatch::default();
        for (table, types) in self.table_config.tables_with_types() {
            let remaining = (limit as usize).saturating_sub(batch.len());
            if remaining == 0 {
                break;
            }
            let table = checked_table_name(table)?;
            let sql = format!(
                "UPDATE {table} SET status = {in_progress}, updated_at = ? \
                 WHERE id IN (SELECT id FROM {table} \
                     WHERE status = {pending} AND type IN ({types}) \
                     ORDER BY message_group, created_at, id LIMIT ?) \
                 RETURNING id, type, message_group, CAST(payload AS TEXT) AS payload, \
                     CAST(retry_count AS INTEGER) AS retry_count, error_message, \
                     CAST(created_at AS TEXT) AS created_at, CAST(updated_at AS TEXT) AS updated_at",
                in_progress = OutboxStatus::InProgress.code(),
                pending = OutboxStatus::Pending.code(),
                types = placeholders(types.len()),
            );
            let mut query = sqlx::query(&sql).bind(now_text());
            for t in &types {
                query = query.bind(t.as_str());
            }
            let rows = query.bind(remaining as i64).fetch_all(&self.pool).await?;
            for row in &rows {
                let type_str: String = row.try_get("type")?;
                let item_type: OutboxItemType = type_str.parse()?;
                let payload: Option<String> = row.try_get("payload")?;
                let created: Option<String> = row.try_get("created_at")?;
                let updated: Option<String> = row.try_get("updated_at")?;
                batch.push_row(
                    row.try_get("id")?,
                    item_type,
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
                "status = {}, error_message = ?, retry_count = retry_count + 1, updated_at = ?",
                new_status.code()
            ),
            "",
            vec![error_message.to_string(), now_text()],
            ids,
        )
        .await?;
        Ok(())
    }

    async fn release(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        self.update_ids(
            item_type,
            &format!("status = {}, updated_at = ?", OutboxStatus::Pending.code()),
            &format!("AND status = {}", OutboxStatus::InProgress.code()),
            vec![now_text()],
            ids,
        )
        .await?;
        Ok(())
    }

    async fn requeue(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        self.update_ids(
            item_type,
            &format!(
                "status = {}, retry_count = 0, error_message = NULL, updated_at = ?",
                OutboxStatus::Pending.code()
            ),
            "",
            vec![now_text()],
            ids,
        )
        .await?;
        Ok(())
    }

    async fn recover_stuck(&self, older_than: Duration) -> Result<u64> {
        let cutoff = (Utc::now() - chrono::Duration::from_std(older_than)?)
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let mut total = 0;
        for table in self.table_config.unique_tables() {
            let sql = format!(
                "UPDATE {} SET status = {}, updated_at = ? WHERE status = {} AND updated_at < ?",
                checked_table_name(table)?,
                OutboxStatus::Pending.code(),
                OutboxStatus::InProgress.code(),
            );
            total += sqlx::query(&sql)
                .bind(now_text())
                .bind(&cutoff)
                .execute(&self.pool)
                .await?
                .rows_affected();
        }
        Ok(total)
    }

    async fn init_schema(&self) -> Result<()> {
        for table in self.table_config.unique_tables() {
            let table = checked_table_name(table)?;
            let safe_name = table.replace('.', "_");
            // The Go SDK's SQLite shape (outbox/sqlite).
            let schema = format!(
                r#"
                CREATE TABLE IF NOT EXISTS {table} (
                    id TEXT PRIMARY KEY,
                    type TEXT NOT NULL,
                    message_group TEXT,
                    payload TEXT NOT NULL,
                    status INTEGER NOT NULL DEFAULT 0,
                    retry_count INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                    error_message TEXT,
                    client_id TEXT,
                    payload_size INTEGER,
                    headers TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_{safe_name}_pending
                    ON {table}(status, message_group, created_at);
                CREATE INDEX IF NOT EXISTS idx_{safe_name}_stuck
                    ON {table}(status, created_at);
                CREATE INDEX IF NOT EXISTS idx_{safe_name}_client_pending
                    ON {table}(client_id, status, created_at);
                "#,
            );
            sqlx::raw_sql(&schema).execute(&self.pool).await?;
        }

        info!(
            tables = ?self.table_config.unique_tables(),
            "Initialized SQLite outbox schema"
        );
        Ok(())
    }

    fn table_config(&self) -> &OutboxTableConfig {
        &self.table_config
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    pub(crate) async fn repo() -> SqliteOutboxRepository {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let repo = SqliteOutboxRepository::new(pool);
        repo.init_schema().await.unwrap();
        repo
    }

    /// Inserts a PENDING row as an SDK would.
    pub(crate) async fn insert(
        repo: &SqliteOutboxRepository,
        id: &str,
        item_type: &str,
        group: Option<&str>,
        payload: &str,
        created_at: &str,
    ) {
        sqlx::query(
            "INSERT INTO outbox_messages (id, type, message_group, payload, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(id)
        .bind(item_type)
        .bind(group)
        .bind(payload)
        .bind(created_at)
        .bind(created_at)
        .execute(repo.pool())
        .await
        .unwrap();
    }

    /// `(status, retry_count, error_message)` of a row, `None` once deleted.
    pub(crate) async fn row(
        repo: &SqliteOutboxRepository,
        id: &str,
    ) -> Option<(i32, i32, Option<String>)> {
        sqlx::query_as::<_, (i32, i32, Option<String>)>(
            "SELECT status, retry_count, error_message FROM outbox_messages WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(repo.pool())
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn claim_takes_every_type_and_marks_it_in_progress() {
        let repo = repo().await;
        // Laravel's SQL timestamps and RFC 3339 alike.
        insert(&repo, "e1", "EVENT", None, "{}", "2026-01-01 00:00:01").await;
        insert(
            &repo,
            "d1",
            "DISPATCH_JOB",
            None,
            "{}",
            "2026-01-01T00:00:02Z",
        )
        .await;
        insert(&repo, "a1", "AUDIT_LOG", None, "{}", "2026-01-01 00:00:03").await;
        insert(
            &repo,
            "x1",
            "SOMETHING_ELSE",
            None,
            "{}",
            "2026-01-01 00:00:04",
        )
        .await;

        let claimed = repo.claim_pending(10).await.unwrap();
        let mut ids: Vec<&str> = claimed.items.iter().map(|i| i.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["a1", "d1", "e1"]);
        for id in ["a1", "d1", "e1"] {
            assert_eq!(row(&repo, id).await.unwrap().0, 9);
        }
        // An unknown type has no endpoint: never claimed.
        assert_eq!(row(&repo, "x1").await.unwrap().0, 0);

        // Nothing is claimed twice while in flight.
        assert!(repo.claim_pending(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn claim_respects_the_limit_in_group_order() {
        let repo = repo().await;
        insert(&repo, "b2", "EVENT", Some("b"), "{}", "2026-01-01 00:00:02").await;
        insert(&repo, "a2", "EVENT", Some("a"), "{}", "2026-01-01 00:00:02").await;
        insert(&repo, "a1", "EVENT", Some("a"), "{}", "2026-01-01 00:00:01").await;
        let claimed = repo.claim_pending(2).await.unwrap();
        let mut ids: Vec<&str> = claimed.items.iter().map(|i| i.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["a1", "a2"]);
    }

    #[tokio::test]
    async fn a_row_that_is_not_json_is_claimed_as_invalid() {
        let repo = repo().await;
        insert(
            &repo,
            "bad",
            "EVENT",
            Some("g"),
            "{oops",
            "2026-01-01 00:00:01",
        )
        .await;
        let claimed = repo.claim_pending(10).await.unwrap();
        assert!(claimed.items.is_empty());
        assert_eq!(claimed.invalid.len(), 1);
        assert_eq!(claimed.invalid[0].message_group.as_deref(), Some("g"));
    }

    #[tokio::test]
    async fn outcomes_are_written_to_the_row() {
        let repo = repo().await;
        for id in ["ok", "retry", "dead", "rel"] {
            insert(&repo, id, "EVENT", None, "{}", "2026-01-01 00:00:01").await;
        }
        repo.claim_pending(10).await.unwrap();

        repo.mark_success(OutboxItemType::Event, &["ok".into()])
            .await
            .unwrap();
        assert_eq!(row(&repo, "ok").await, None, "accepted rows are deleted");

        repo.mark_failed(
            OutboxItemType::Event,
            &["retry".into()],
            OutboxStatus::GatewayError,
            "503",
            true,
        )
        .await
        .unwrap();
        assert_eq!(row(&repo, "retry").await, Some((0, 1, Some("503".into()))));

        repo.mark_failed(
            OutboxItemType::Event,
            &["dead".into()],
            OutboxStatus::Forbidden,
            "403",
            false,
        )
        .await
        .unwrap();
        assert_eq!(row(&repo, "dead").await, Some((5, 1, Some("403".into()))));

        repo.release(OutboxItemType::Event, &["rel".into(), "dead".into()])
            .await
            .unwrap();
        assert_eq!(row(&repo, "rel").await, Some((0, 0, None)), "no penalty");
        assert_eq!(
            row(&repo, "dead").await.unwrap().0,
            5,
            "only IN_PROGRESS rows"
        );

        repo.requeue(OutboxItemType::Event, &["dead".into()])
            .await
            .unwrap();
        assert_eq!(row(&repo, "dead").await, Some((0, 0, None)));
    }

    #[tokio::test]
    async fn only_rows_stuck_past_the_threshold_are_recovered() {
        let repo = repo().await;
        insert(&repo, "old", "EVENT", None, "{}", "2026-01-01 00:00:01").await;
        insert(&repo, "new", "EVENT", None, "{}", "2026-01-01 00:00:01").await;
        repo.claim_pending(10).await.unwrap();
        sqlx::query(
            "UPDATE outbox_messages SET updated_at = '2020-01-01T00:00:00.000Z' WHERE id = 'old'",
        )
        .execute(repo.pool())
        .await
        .unwrap();
        assert_eq!(
            repo.recover_stuck(Duration::from_secs(300)).await.unwrap(),
            1
        );
        assert_eq!(row(&repo, "old").await.unwrap().0, 0);
        assert_eq!(row(&repo, "new").await.unwrap().0, 9);
    }
}
