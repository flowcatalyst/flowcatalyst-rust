//! CORS Origin Repository — PostgreSQL via SQLx

use async_trait::async_trait;
use sqlx::{PgPool, Postgres};
use chrono::{DateTime, Utc};

use super::entity::CorsAllowedOrigin;
use crate::shared::error::Result;
use crate::usecase::{HasId, PgPersist};

#[derive(sqlx::FromRow)]
struct CorsOriginRow {
    id: String,
    origin: String,
    description: Option<String>,
    created_by: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<CorsOriginRow> for CorsAllowedOrigin {
    fn from(r: CorsOriginRow) -> Self {
        Self {
            id: r.id,
            origin: r.origin,
            description: r.description,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub struct CorsOriginRepository {
    pool: PgPool,
}

impl CorsOriginRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<CorsAllowedOrigin>> {
        let row = sqlx::query_as::<_, CorsOriginRow>(
            "SELECT * FROM tnt_cors_allowed_origins WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(CorsAllowedOrigin::from))
    }

    pub async fn find_by_origin(&self, origin: &str) -> Result<Option<CorsAllowedOrigin>> {
        let row = sqlx::query_as::<_, CorsOriginRow>(
            "SELECT * FROM tnt_cors_allowed_origins WHERE origin = $1"
        )
        .bind(origin)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(CorsAllowedOrigin::from))
    }

    pub async fn find_all(&self) -> Result<Vec<CorsAllowedOrigin>> {
        let rows = sqlx::query_as::<_, CorsOriginRow>(
            "SELECT * FROM tnt_cors_allowed_origins ORDER BY origin"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(CorsAllowedOrigin::from).collect())
    }

    pub async fn get_allowed_origins(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT origin FROM tnt_cors_allowed_origins"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

}

impl HasId for CorsAllowedOrigin {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for CorsAllowedOrigin {
    async fn pg_upsert(&self, txn: &mut sqlx::Transaction<'_, Postgres>) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO tnt_cors_allowed_origins
                (id, origin, description, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET
                origin = EXCLUDED.origin,
                description = EXCLUDED.description,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&self.id)
        .bind(&self.origin)
        .bind(&self.description)
        .bind(&self.created_by)
        .bind(now)
        .bind(now)
        .execute(&mut **txn)
        .await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &mut sqlx::Transaction<'_, Postgres>) -> Result<()> {
        sqlx::query("DELETE FROM tnt_cors_allowed_origins WHERE id = $1")
            .bind(&self.id)
            .execute(&mut **txn)
            .await?;
        Ok(())
    }
}
