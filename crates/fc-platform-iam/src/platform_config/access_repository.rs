//! PlatformConfigAccess Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::PlatformConfigAccessId;
use sqlx::PgPool;

use super::access_entity::PlatformConfigAccess;
use fc_platform_core::shared::error::Result;
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::HasId;
use fc_platform_core::usecase::Persist;

struct PlatformConfigAccessRow {
    id: PlatformConfigAccessId,
    application_code: String,
    role_code: String,
    can_read: bool,
    can_write: bool,
    created_at: DateTime<Utc>,
}

impl From<PlatformConfigAccessRow> for PlatformConfigAccess {
    fn from(r: PlatformConfigAccessRow) -> Self {
        Self {
            id: r.id,
            application_code: r.application_code,
            role_code: r.role_code,
            can_read: r.can_read,
            can_write: r.can_write,
            created_at: r.created_at,
        }
    }
}

pub struct PlatformConfigAccessRepository {
    pool: PgPool,
}

impl PlatformConfigAccessRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_application(&self, app_code: &str) -> Result<Vec<PlatformConfigAccess>> {
        let rows = sqlx::query_as!(
            PlatformConfigAccessRow,
            "SELECT id AS \"id: PlatformConfigAccessId\", application_code, role_code, \
                    can_read, can_write, created_at \
                    FROM app_platform_config_access WHERE application_code = $1 ORDER BY role_code",
            app_code
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(PlatformConfigAccess::from).collect())
    }

    pub async fn find_by_application_and_role(
        &self,
        app_code: &str,
        role_code: &str,
    ) -> Result<Option<PlatformConfigAccess>> {
        let row = sqlx::query_as!(
            PlatformConfigAccessRow,
            "SELECT id AS \"id: PlatformConfigAccessId\", application_code, role_code, \
                    can_read, can_write, created_at \
                    FROM app_platform_config_access WHERE application_code = $1 AND role_code = $2",
            app_code,
            role_code
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(PlatformConfigAccess::from))
    }

    pub async fn find_by_role_codes(
        &self,
        app_code: &str,
        role_codes: &[String],
    ) -> Result<Vec<PlatformConfigAccess>> {
        let rows = sqlx::query_as!(
            PlatformConfigAccessRow,
            "SELECT id AS \"id: PlatformConfigAccessId\", application_code, role_code, \
                    can_read, can_write, created_at \
                    FROM app_platform_config_access WHERE application_code = $1 AND role_code = ANY($2)",
            app_code,
            role_codes
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(PlatformConfigAccess::from).collect())
    }

    pub async fn insert(&self, access: &PlatformConfigAccess) -> Result<()> {
        sqlx::query!(
            r#"INSERT INTO app_platform_config_access
                (id, application_code, role_code, can_read, can_write, created_at)
            VALUES ($1, $2, $3, $4, $5, NOW())"#,
            &access.id as &PlatformConfigAccessId,
            &access.application_code,
            &access.role_code,
            access.can_read,
            access.can_write
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update(&self, access: &PlatformConfigAccess) -> Result<()> {
        sqlx::query!(
            r#"UPDATE app_platform_config_access SET
                application_code = $2, role_code = $3, can_read = $4, can_write = $5
            WHERE id = $1"#,
            &access.id as &PlatformConfigAccessId,
            &access.application_code,
            &access.role_code,
            access.can_read,
            access.can_write
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_by_application_and_role(
        &self,
        app_code: &str,
        role_code: &str,
    ) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM app_platform_config_access WHERE application_code = $1 AND role_code = $2",
            app_code,
            role_code
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

impl HasId for PlatformConfigAccess {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<PlatformConfigAccess> for PlatformConfigAccessRepository {
    async fn persist(&self, a: &PlatformConfigAccess, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            r#"INSERT INTO app_platform_config_access
                (id, application_code, role_code, can_read, can_write, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET
                application_code = EXCLUDED.application_code,
                role_code = EXCLUDED.role_code,
                can_read = EXCLUDED.can_read,
                can_write = EXCLUDED.can_write"#,
            &a.id as &PlatformConfigAccessId,
            &a.application_code,
            &a.role_code,
            a.can_read,
            a.can_write,
            a.created_at
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, a: &PlatformConfigAccess, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM app_platform_config_access WHERE id = $1",
            &a.id as &PlatformConfigAccessId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}

impl PlatformConfigAccessRepository {
    /// One grant by id (Go `FindAccessByID`).
    pub async fn find_by_id(
        &self,
        id: &PlatformConfigAccessId,
    ) -> Result<Option<PlatformConfigAccess>> {
        let row = sqlx::query_as!(
            PlatformConfigAccessRow,
            "SELECT id AS \"id: PlatformConfigAccessId\", application_code, role_code, \
                    can_read, can_write, created_at \
                    FROM app_platform_config_access WHERE id = $1",
            id as &PlatformConfigAccessId
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(PlatformConfigAccess::from))
    }
}
