//! PostgreSQL Outbox Repository Implementation
//!
//! Reads the outbox table every SDK creates, whatever its column types: the
//! Go, TypeScript, Java and Laravel migrations (`payload TEXT`, `status` and
//! `retry_count SMALLINT`, Laravel's timestamps without a zone) and the Rust
//! SDK's (`payload JSONB`, `INTEGER`). Columns are cast in SQL, so each reads
//! the same way, and only the processor's columns are read (Go
//! `internal/outbox/postgres`).
//!
//! The claim is Go's: one `UPDATE … FROM (SELECT … FOR UPDATE SKIP LOCKED)
//! RETURNING`, so concurrent processors never claim the same row.

use crate::repository::{checked_table_name, ClaimedBatch, OutboxRepository, OutboxTableConfig};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fc_common::{OutboxItemType, OutboxStatus};
use sqlx::{PgPool, Row};
use std::time::Duration;
use tracing::info;

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

    fn table(&self, item_type: OutboxItemType) -> Result<&str> {
        checked_table_name(self.table_config.table_for_type(item_type))
    }
}

#[async_trait]
impl OutboxRepository for PostgresOutboxRepository {
    async fn claim_pending(&self, limit: u32) -> Result<ClaimedBatch> {
        let mut batch = ClaimedBatch::default();
        for (table, types) in self.table_config.tables_with_types() {
            let remaining = (limit as usize).saturating_sub(batch.len());
            if remaining == 0 {
                break;
            }
            let table = checked_table_name(table)?;
            let sql = format!(
                "WITH claimed AS ( \
                   SELECT id FROM {table} \
                    WHERE status = {pending} AND type = ANY($1) \
                    ORDER BY message_group, created_at, id \
                    LIMIT $2 \
                    FOR UPDATE SKIP LOCKED \
                 ) \
                 UPDATE {table} m SET status = {in_progress}, updated_at = NOW() \
                   FROM claimed WHERE m.id = claimed.id \
                 RETURNING m.id::text AS id, m.type::text AS type, \
                   m.message_group::text AS message_group, m.payload::text AS payload, \
                   m.retry_count::int4 AS retry_count, m.error_message::text AS error_message, \
                   m.created_at::timestamptz AS created_at, m.updated_at::timestamptz AS updated_at",
                pending = OutboxStatus::Pending.code(),
                in_progress = OutboxStatus::InProgress.code(),
            );
            let type_names: Vec<&str> = types.iter().map(|t| t.as_str()).collect();
            let rows = sqlx::query(&sql)
                .bind(&type_names)
                .bind(remaining as i64)
                .fetch_all(&self.pool)
                .await?;
            for row in &rows {
                let type_str: String = row.try_get("type")?;
                let payload: Option<String> = row.try_get("payload")?;
                batch.push_row(
                    row.try_get("id")?,
                    type_str.parse()?,
                    row.try_get("message_group")?,
                    payload.as_deref().unwrap_or_default(),
                    row.try_get("retry_count")?,
                    row.try_get("error_message")?,
                    row.try_get::<Option<DateTime<Utc>>, _>("created_at")?
                        .unwrap_or_else(Utc::now),
                    row.try_get::<Option<DateTime<Utc>>, _>("updated_at")?
                        .unwrap_or_else(Utc::now),
                );
            }
        }
        Ok(batch)
    }

    async fn mark_success(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let sql = format!("DELETE FROM {} WHERE id = ANY($1)", self.table(item_type)?);
        sqlx::query(&sql).bind(ids).execute(&self.pool).await?;
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
        if ids.is_empty() {
            return Ok(());
        }
        let new_status = if requeue {
            OutboxStatus::Pending
        } else {
            status
        };
        let sql = format!(
            "UPDATE {} SET status = {}, error_message = $1, retry_count = retry_count + 1, \
             updated_at = NOW() WHERE id = ANY($2)",
            self.table(item_type)?,
            new_status.code()
        );
        sqlx::query(&sql)
            .bind(error_message)
            .bind(ids)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn release(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let sql = format!(
            "UPDATE {} SET status = {}, updated_at = NOW() WHERE id = ANY($1) AND status = {}",
            self.table(item_type)?,
            OutboxStatus::Pending.code(),
            OutboxStatus::InProgress.code()
        );
        sqlx::query(&sql).bind(ids).execute(&self.pool).await?;
        Ok(())
    }

    async fn requeue(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let sql = format!(
            "UPDATE {} SET status = {}, retry_count = 0, error_message = NULL, updated_at = NOW() \
             WHERE id = ANY($1)",
            self.table(item_type)?,
            OutboxStatus::Pending.code()
        );
        sqlx::query(&sql).bind(ids).execute(&self.pool).await?;
        Ok(())
    }

    async fn recover_stuck(&self, older_than: Duration) -> Result<u64> {
        let mut total = 0;
        for table in self.table_config.unique_tables() {
            // Compared against NOW() in SQL: a column without a zone is then
            // read in the session's zone on both sides, as it was written.
            let sql = format!(
                "UPDATE {} SET status = {}, updated_at = NOW() \
                 WHERE status = {} AND updated_at < NOW() - ($1::float8 * INTERVAL '1 second')",
                checked_table_name(table)?,
                OutboxStatus::Pending.code(),
                OutboxStatus::InProgress.code()
            );
            total += sqlx::query(&sql)
                .bind(older_than.as_secs_f64())
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
            // The SDK migration's shape (Go outboxpgx, TS, Java).
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
            );
            sqlx::raw_sql(&schema).execute(&self.pool).await?;
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

/// Against a real PostgreSQL (testcontainers; `cargo test -p fc-outbox --
/// --ignored` with Docker running).
#[cfg(test)]
mod tests {
    use super::*;
    use testcontainers::runners::AsyncRunner;
    use testcontainers::ContainerAsync;
    use testcontainers_modules::postgres::Postgres;

    /// The table shapes the SDKs create.
    const GO_TS_JAVA: &str = "CREATE TABLE outbox_messages (
        id VARCHAR(26) PRIMARY KEY, type VARCHAR(20) NOT NULL, message_group VARCHAR(255),
        payload TEXT NOT NULL, status SMALLINT NOT NULL DEFAULT 0,
        retry_count SMALLINT NOT NULL DEFAULT 0,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        error_message TEXT, client_id VARCHAR(26), payload_size INTEGER, headers JSONB)";
    const RUST_SDK: &str = "CREATE TABLE outbox_messages (
        id TEXT PRIMARY KEY, type TEXT NOT NULL, message_group TEXT, payload JSONB NOT NULL,
        status INTEGER NOT NULL DEFAULT 0, retry_count INTEGER NOT NULL DEFAULT 0,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        error_message TEXT, client_id TEXT, payload_size INTEGER, headers JSONB)";
    /// Laravel's migration on PostgreSQL (`timestamp` has no zone; `json`).
    const LARAVEL: &str = "CREATE TABLE outbox_messages (
        id VARCHAR(26) PRIMARY KEY, type VARCHAR(20) NOT NULL, message_group VARCHAR(255),
        payload TEXT NOT NULL, status SMALLINT NOT NULL DEFAULT 0,
        retry_count SMALLINT NOT NULL DEFAULT 0,
        created_at TIMESTAMP(0) WITHOUT TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP(0) WITHOUT TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
        error_message TEXT, client_id VARCHAR(26), payload_size INTEGER, headers JSON)";

    async fn database() -> (ContainerAsync<Postgres>, PgPool) {
        let container = Postgres::default().start().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let pool = PgPool::connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
        ))
        .await
        .unwrap();
        (container, pool)
    }

    async fn reset(pool: &PgPool, ddl: &str) -> PostgresOutboxRepository {
        sqlx::raw_sql("DROP TABLE IF EXISTS outbox_messages")
            .execute(pool)
            .await
            .unwrap();
        sqlx::raw_sql(ddl).execute(pool).await.unwrap();
        PostgresOutboxRepository::new(pool.clone())
    }

    /// A row in the Go/TS/Java table shape.
    async fn insert(pool: &PgPool, id: &str, item_type: &str, group: Option<&str>, payload: &str) {
        sqlx::query(
            "INSERT INTO outbox_messages (id, type, message_group, payload, status, created_at, updated_at) \
             SELECT $1, $2, $3, $4, 0, clock_timestamp(), clock_timestamp()",
        )
        .bind(id)
        .bind(item_type)
        .bind(group)
        .bind(payload)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn status(pool: &PgPool, id: &str) -> Option<(i32, i32, Option<String>)> {
        sqlx::query_as::<_, (i32, i32, Option<String>)>(
            "SELECT status::int4, retry_count::int4, error_message FROM outbox_messages WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    async fn exercise(pool: &PgPool, ddl: &str) {
        let repo = reset(pool, ddl).await;
        // The payload column may be JSONB: insert through a text cast.
        let insert_sql = "INSERT INTO outbox_messages (id, type, message_group, payload, status) \
                          VALUES ($1, $2, $3, CAST($4::text AS {}), 0)";
        let payload_type: String = sqlx::query_scalar(
            "SELECT data_type FROM information_schema.columns \
             WHERE table_name = 'outbox_messages' AND column_name = 'payload'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let sql = insert_sql.replace(
            "{}",
            if payload_type == "jsonb" {
                "jsonb"
            } else {
                "text"
            },
        );
        for (id, t, g) in [
            ("e1", "EVENT", Some("g1")),
            ("e2", "EVENT", Some("g1")),
            ("d1", "DISPATCH_JOB", None),
            ("a1", "AUDIT_LOG", None),
            ("x1", "OTHER", None),
        ] {
            sqlx::query(&sql)
                .bind(id)
                .bind(t)
                .bind(g)
                .bind(format!(r#"{{"id":"{id}"}}"#))
                .execute(pool)
                .await
                .unwrap();
        }

        let claimed = repo.claim_pending(100).await.unwrap();
        let mut ids: Vec<&str> = claimed.items.iter().map(|i| i.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["a1", "d1", "e1", "e2"], "{ddl}");
        assert_eq!(claimed.items[0].payload["id"], claimed.items[0].id.as_str());
        assert_eq!(
            status(pool, "x1").await.unwrap().0,
            0,
            "unknown type untouched"
        );
        assert!(
            repo.claim_pending(100).await.unwrap().is_empty(),
            "no double claim"
        );

        repo.mark_success(OutboxItemType::Event, &["e1".into()])
            .await
            .unwrap();
        assert_eq!(status(pool, "e1").await, None);
        repo.mark_failed(
            OutboxItemType::DispatchJob,
            &["d1".into()],
            OutboxStatus::GatewayError,
            "503",
            true,
        )
        .await
        .unwrap();
        assert_eq!(status(pool, "d1").await, Some((0, 1, Some("503".into()))));
        repo.mark_failed(
            OutboxItemType::AuditLog,
            &["a1".into()],
            OutboxStatus::Forbidden,
            "403",
            false,
        )
        .await
        .unwrap();
        assert_eq!(status(pool, "a1").await, Some((5, 1, Some("403".into()))));
        repo.release(OutboxItemType::Event, &["e2".into()])
            .await
            .unwrap();
        assert_eq!(status(pool, "e2").await, Some((0, 0, None)));
        repo.requeue(OutboxItemType::AuditLog, &["a1".into()])
            .await
            .unwrap();
        assert_eq!(status(pool, "a1").await, Some((0, 0, None)));

        // Recovery: only rows IN_PROGRESS past the threshold.
        let again = repo.claim_pending(100).await.unwrap();
        assert_eq!(again.items.len(), 3);
        sqlx::query("UPDATE outbox_messages SET updated_at = updated_at - INTERVAL '10 minutes' WHERE id = 'd1'")
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(
            repo.recover_stuck(Duration::from_secs(300)).await.unwrap(),
            1
        );
        assert_eq!(status(pool, "d1").await.unwrap().0, 0);
        assert_eq!(status(pool, "e2").await.unwrap().0, 9);
    }

    #[tokio::test]
    #[ignore = "needs Docker"]
    async fn every_sdk_table_shape_is_read() {
        let (_container, pool) = database().await;
        for ddl in [GO_TS_JAVA, RUST_SDK, LARAVEL] {
            exercise(&pool, ddl).await;
        }
    }

    #[tokio::test]
    #[ignore = "needs Docker"]
    async fn concurrent_claims_never_share_a_row() {
        let (_container, pool) = database().await;
        let repo = std::sync::Arc::new(reset(&pool, GO_TS_JAVA).await);
        for i in 0..200 {
            insert(&pool, &format!("r{i:03}"), "EVENT", None, "{}").await;
        }
        let mut handles = Vec::new();
        for _ in 0..4 {
            let repo = repo.clone();
            handles.push(tokio::spawn(async move {
                let mut mine = Vec::new();
                loop {
                    let batch = repo.claim_pending(7).await.unwrap();
                    if batch.is_empty() {
                        break;
                    }
                    mine.extend(batch.items.into_iter().map(|i| i.id));
                }
                mine
            }));
        }
        let mut all = Vec::new();
        for h in handles {
            all.extend(h.await.unwrap());
        }
        let total = all.len();
        all.sort();
        all.dedup();
        assert_eq!(total, 200);
        assert_eq!(all.len(), 200);
    }

    #[tokio::test]
    #[ignore = "needs Docker"]
    async fn init_schema_creates_the_sdk_shape() {
        let (_container, pool) = database().await;
        let repo = PostgresOutboxRepository::new(pool.clone());
        repo.init_schema().await.unwrap();
        repo.init_schema().await.unwrap();
        insert(&pool, "e1", "EVENT", None, "{}").await;
        assert_eq!(repo.claim_pending(10).await.unwrap().items.len(), 1);
    }
}
