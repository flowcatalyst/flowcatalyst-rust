//! DispatchPool Repository — PostgreSQL via SQLx

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{DispatchPool, DispatchPoolStatus};
use crate::shared::enum_str::decode;
use crate::shared::error::{PlatformError, Result};
use crate::usecase::unit_of_work::HasId;

/// Row mapping for msg_dispatch_pools table
#[derive(sqlx::FromRow)]
struct DispatchPoolRow {
    id: String,
    code: String,
    name: String,
    description: Option<String>,
    rate_limit: Option<i32>,
    concurrency: i32,
    client_id: Option<String>,
    client_identifier: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<DispatchPoolRow> for DispatchPool {
    type Error = PlatformError;
    fn try_from(r: DispatchPoolRow) -> Result<Self> {
        let status = decode(&r.status, "msg_dispatch_pools", "status", &r.id)?;
        Ok(Self {
            id: r.id,
            code: r.code,
            name: r.name,
            description: r.description,
            rate_limit: r.rate_limit,
            concurrency: r.concurrency,
            client_id: r.client_id,
            client_identifier: r.client_identifier,
            status,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

pub struct DispatchPoolRepository {
    pool: PgPool,
}

impl DispatchPoolRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, pool: &DispatchPool) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO msg_dispatch_pools (id, code, name, description, rate_limit, concurrency, client_id, client_identifier, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
        )
        .bind(&pool.id)
        .bind(&pool.code)
        .bind(&pool.name)
        .bind(&pool.description)
        .bind(pool.rate_limit)
        .bind(pool.concurrency)
        .bind(&pool.client_id)
        .bind(&pool.client_identifier)
        .bind(pool.status.as_str())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<DispatchPool>> {
        let row =
            sqlx::query_as::<_, DispatchPoolRow>("SELECT * FROM msg_dispatch_pools WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        row.map(DispatchPool::try_from).transpose()
    }

    /// Every pool named by `ids`; an id with no row is simply absent.
    pub async fn find_by_ids(&self, ids: &[String]) -> Result<Vec<DispatchPool>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, DispatchPoolRow>(
            "SELECT * FROM msg_dispatch_pools WHERE id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(DispatchPool::try_from).collect()
    }

    pub async fn find_by_code(
        &self,
        code: &str,
        client_id: Option<&str>,
    ) -> Result<Option<DispatchPool>> {
        let row = if let Some(cid) = client_id {
            sqlx::query_as::<_, DispatchPoolRow>(
                "SELECT * FROM msg_dispatch_pools WHERE code = $1 AND client_id = $2",
            )
            .bind(code)
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, DispatchPoolRow>(
                "SELECT * FROM msg_dispatch_pools WHERE code = $1 AND client_id IS NULL",
            )
            .bind(code)
            .fetch_optional(&self.pool)
            .await?
        };
        row.map(DispatchPool::try_from).transpose()
    }

    /// The anchor-level (no client) pools with any of `codes`, in one
    /// query. The batch form of `find_by_code(code, None)`; codes with no
    /// pool are simply absent from the result.
    pub async fn find_anchor_by_codes(&self, codes: &[String]) -> Result<Vec<DispatchPool>> {
        if codes.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, DispatchPoolRow>(
            "SELECT * FROM msg_dispatch_pools WHERE code = ANY($1) AND client_id IS NULL",
        )
        .bind(codes)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(DispatchPool::try_from).collect()
    }

    pub async fn find_all(&self) -> Result<Vec<DispatchPool>> {
        let rows = sqlx::query_as::<_, DispatchPoolRow>(
            "SELECT * FROM msg_dispatch_pools ORDER BY code ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(DispatchPool::try_from).collect()
    }

    pub async fn find_by_status(&self, status: DispatchPoolStatus) -> Result<Vec<DispatchPool>> {
        let rows = sqlx::query_as::<_, DispatchPoolRow>(
            "SELECT * FROM msg_dispatch_pools WHERE status = $1 ORDER BY code ASC",
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(DispatchPool::try_from).collect()
    }

    /// Search dispatch pools by code or name (case-insensitive partial match)
    pub async fn search(&self, term: &str) -> Result<Vec<DispatchPool>> {
        let pattern = format!("%{}%", term);
        let rows = sqlx::query_as::<_, DispatchPoolRow>(
            "SELECT * FROM msg_dispatch_pools WHERE code ILIKE $1 OR name ILIKE $1",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(DispatchPool::try_from).collect()
    }

    pub async fn find_active(&self) -> Result<Vec<DispatchPool>> {
        let rows = sqlx::query_as::<_, DispatchPoolRow>(
            "SELECT * FROM msg_dispatch_pools WHERE status = 'ACTIVE'",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(DispatchPool::try_from).collect()
    }

    pub async fn find_by_client(&self, client_id: Option<&str>) -> Result<Vec<DispatchPool>> {
        let rows = if let Some(cid) = client_id {
            sqlx::query_as::<_, DispatchPoolRow>(
                "SELECT * FROM msg_dispatch_pools WHERE client_id = $1 OR client_id IS NULL",
            )
            .bind(cid)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, DispatchPoolRow>(
                "SELECT * FROM msg_dispatch_pools WHERE client_id IS NULL",
            )
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter().map(DispatchPool::try_from).collect()
    }

    pub async fn update(&self, pool: &DispatchPool) -> Result<()> {
        sqlx::query(
            "UPDATE msg_dispatch_pools SET
                code = $2,
                name = $3,
                description = $4,
                rate_limit = $5,
                concurrency = $6,
                client_id = $7,
                client_identifier = $8,
                status = $9,
                updated_at = $10
             WHERE id = $1",
        )
        .bind(&pool.id)
        .bind(&pool.code)
        .bind(&pool.name)
        .bind(&pool.description)
        .bind(pool.rate_limit)
        .bind(pool.concurrency)
        .bind(&pool.client_id)
        .bind(&pool.client_identifier)
        .bind(pool.status.as_str())
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM msg_dispatch_pools WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── Persist<DispatchPool> ────────────────────────────────────────────────────

impl HasId for DispatchPool {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl crate::usecase::Persist<DispatchPool> for DispatchPoolRepository {
    async fn persist(&self, p: &DispatchPool, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO msg_dispatch_pools (id, code, name, description, rate_limit, concurrency, client_id, client_identifier, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET
                code = EXCLUDED.code,
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                rate_limit = EXCLUDED.rate_limit,
                concurrency = EXCLUDED.concurrency,
                client_id = EXCLUDED.client_id,
                client_identifier = EXCLUDED.client_identifier,
                status = EXCLUDED.status,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&p.id)
        .bind(&p.code)
        .bind(&p.name)
        .bind(&p.description)
        .bind(p.rate_limit)
        .bind(p.concurrency)
        .bind(&p.client_id)
        .bind(&p.client_identifier)
        .bind(p.status.as_str())
        .bind(now)
        .bind(now)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, p: &DispatchPool, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM msg_dispatch_pools WHERE id = $1")
            .bind(&p.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
