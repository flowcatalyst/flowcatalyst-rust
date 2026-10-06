//! `iam_permissions` repository: the permission catalogue (Go
//! `role/permission_repo.go`, `sqlc/queries/role.sql` Permission*).

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::PermissionId;
use sqlx::PgPool;

use crate::role::permission_catalog::CatalogPermission;
use fc_platform_core::shared::error::Result;
use fc_platform_core::usecase::unit_of_work::HasId;
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::Persist;

struct PermissionRow {
    id: PermissionId,
    code: String,
    subdomain: String,
    context: String,
    aggregate: String,
    action: String,
    description: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<PermissionRow> for CatalogPermission {
    fn from(r: PermissionRow) -> Self {
        Self {
            id: r.id,
            code: r.code,
            subdomain: r.subdomain,
            context: r.context,
            aggregate: r.aggregate,
            action: r.action,
            description: r.description,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub struct PermissionCatalogRepository {
    pool: PgPool,
}

impl PermissionCatalogRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_all(&self) -> Result<Vec<CatalogPermission>> {
        let rows = sqlx::query_as!(
            PermissionRow,
            "SELECT id AS \"id: PermissionId\", code, subdomain, context, aggregate, \
                    action, description, created_at, updated_at \
                    FROM iam_permissions ORDER BY code"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<CatalogPermission>> {
        let row = sqlx::query_as!(
            PermissionRow,
            "SELECT id AS \"id: PermissionId\", code, subdomain, context, aggregate, \
                    action, description, created_at, updated_at \
                    FROM iam_permissions WHERE code = $1",
            code
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }
}

impl HasId for CatalogPermission {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<CatalogPermission> for PermissionCatalogRepository {
    /// Go's `PermissionUpsert`: idempotent by code.
    async fn persist(&self, p: &CatalogPermission, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "INSERT INTO iam_permissions (id, code, subdomain, context, aggregate, action, description)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (code) DO UPDATE SET
                subdomain   = EXCLUDED.subdomain,
                context     = EXCLUDED.context,
                aggregate   = EXCLUDED.aggregate,
                action      = EXCLUDED.action,
                description = EXCLUDED.description,
                updated_at  = NOW()",
            &p.id as &PermissionId,
            &p.code,
            &p.subdomain,
            &p.context,
            &p.aggregate,
            &p.action,
            p.description.as_ref()
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, p: &CatalogPermission, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!("DELETE FROM iam_permissions WHERE code = $1", &p.code)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
