//! Role Repository
//!
//! PostgreSQL persistence for AuthRole entities using SQLx.
//! Permissions are stored in the iam_role_permissions junction table.

use async_trait::async_trait;
use sqlx::{PgPool, Postgres, QueryBuilder};
use chrono::{DateTime, Utc};

use super::entity::{AuthRole, RoleSource};
use crate::shared::error::Result;
use crate::usecase::unit_of_work::HasId;

/// Row mapping for iam_roles table
#[derive(sqlx::FromRow)]
struct RoleRow {
    id: String,
    application_id: Option<String>,
    application_code: Option<String>,
    name: String,
    display_name: String,
    description: Option<String>,
    source: String,
    client_managed: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<RoleRow> for AuthRole {
    fn from(r: RoleRow) -> Self {
        // Extract application_code from the role name (part before first colon) if not set
        let application_code = r.application_code.unwrap_or_else(|| {
            r.name.split(':').next().unwrap_or("unknown").to_string()
        });

        Self {
            id: r.id,
            application_id: r.application_id,
            name: r.name,
            display_name: r.display_name,
            description: r.description,
            application_code,
            permissions: std::collections::HashSet::new(), // loaded from junction table
            source: RoleSource::from_str(&r.source),
            client_managed: r.client_managed,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Row mapping for iam_role_permissions junction table
#[derive(sqlx::FromRow)]
struct RolePermissionRow {
    role_id: String,
    permission: String,
}

pub struct RoleRepository {
    pool: PgPool,
}

impl RoleRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, role: &AuthRole) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO iam_roles (id, application_id, application_code, name, display_name, description, source, client_managed, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
        )
        .bind(&role.id)
        .bind(&role.application_id)
        .bind(Some(&role.application_code))
        .bind(&role.name)
        .bind(&role.display_name)
        .bind(&role.description)
        .bind(role.source.as_str())
        .bind(role.client_managed)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Insert permissions into junction table
        self.insert_permissions(&role.id, &role.permissions).await?;

        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<AuthRole>> {
        let row = sqlx::query_as::<_, RoleRow>(
            "SELECT * FROM iam_roles WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let mut role = AuthRole::from(r);
                role.permissions = self.load_permissions(&role.id).await?;
                Ok(Some(role))
            }
            None => Ok(None),
        }
    }

    /// Find role by name (formerly find_by_code)
    pub async fn find_by_name(&self, name: &str) -> Result<Option<AuthRole>> {
        let row = sqlx::query_as::<_, RoleRow>(
            "SELECT * FROM iam_roles WHERE name = $1"
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let mut role = AuthRole::from(r);
                role.permissions = self.load_permissions(&role.id).await?;
                Ok(Some(role))
            }
            None => Ok(None),
        }
    }

    /// Backward-compatible alias for find_by_name
    pub async fn find_by_code(&self, code: &str) -> Result<Option<AuthRole>> {
        self.find_by_name(code).await
    }

    pub async fn find_all(&self) -> Result<Vec<AuthRole>> {
        let rows = sqlx::query_as::<_, RoleRow>(
            "SELECT * FROM iam_roles"
        )
        .fetch_all(&self.pool)
        .await?;

        self.hydrate_roles(rows).await
    }

    pub async fn find_by_application(&self, application_code: &str) -> Result<Vec<AuthRole>> {
        let rows = sqlx::query_as::<_, RoleRow>(
            "SELECT * FROM iam_roles WHERE application_code = $1"
        )
        .bind(application_code)
        .fetch_all(&self.pool)
        .await?;

        self.hydrate_roles(rows).await
    }

    pub async fn find_by_application_id(&self, application_id: &str) -> Result<Vec<AuthRole>> {
        let rows = sqlx::query_as::<_, RoleRow>(
            "SELECT * FROM iam_roles WHERE application_id = $1"
        )
        .bind(application_id)
        .fetch_all(&self.pool)
        .await?;

        self.hydrate_roles(rows).await
    }

    pub async fn find_by_source(&self, source: RoleSource) -> Result<Vec<AuthRole>> {
        let rows = sqlx::query_as::<_, RoleRow>(
            "SELECT * FROM iam_roles WHERE source = $1"
        )
        .bind(source.as_str())
        .fetch_all(&self.pool)
        .await?;

        self.hydrate_roles(rows).await
    }

    pub async fn find_client_managed(&self) -> Result<Vec<AuthRole>> {
        let rows = sqlx::query_as::<_, RoleRow>(
            "SELECT * FROM iam_roles WHERE client_managed = true"
        )
        .fetch_all(&self.pool)
        .await?;

        self.hydrate_roles(rows).await
    }

    /// Find roles with optional combined filters (AND logic).
    pub async fn find_with_filters(
        &self,
        application_code: Option<&str>,
        source: Option<&str>,
        client_managed: Option<bool>,
    ) -> Result<Vec<AuthRole>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM iam_roles");
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(app) = application_code {
            push_where(&mut qb, &mut has_where);
            qb.push("application_code = ").push_bind(app.to_string());
        }
        if let Some(s) = source {
            push_where(&mut qb, &mut has_where);
            qb.push("source = ").push_bind(s.to_string());
        }
        if let Some(cm) = client_managed {
            push_where(&mut qb, &mut has_where);
            qb.push("client_managed = ").push_bind(cm);
        }

        let rows: Vec<RoleRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        self.hydrate_roles(rows).await
    }

    pub async fn find_by_codes(&self, codes: &[String]) -> Result<Vec<AuthRole>> {
        if codes.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query_as::<_, RoleRow>(
            "SELECT * FROM iam_roles WHERE name = ANY($1)"
        )
        .bind(codes)
        .fetch_all(&self.pool)
        .await?;

        self.hydrate_roles(rows).await
    }

    /// Search roles by name or display_name (case-insensitive partial match)
    pub async fn search(&self, term: &str) -> Result<Vec<AuthRole>> {
        let pattern = format!("%{}%", term);
        let rows = sqlx::query_as::<_, RoleRow>(
            "SELECT * FROM iam_roles WHERE name ILIKE $1 OR display_name ILIKE $1"
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;

        self.hydrate_roles(rows).await
    }

    pub async fn find_with_permission(&self, permission: &str) -> Result<Vec<AuthRole>> {
        let rows = sqlx::query_as::<_, RoleRow>(
            "SELECT r.* FROM iam_roles r
             INNER JOIN iam_role_permissions rp ON rp.role_id = r.id
             WHERE rp.permission = $1"
        )
        .bind(permission)
        .fetch_all(&self.pool)
        .await?;

        self.hydrate_roles(rows).await
    }

    pub async fn exists(&self, id: &str) -> Result<bool> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_roles WHERE id = $1"
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    /// Check if a role with the given name exists
    pub async fn exists_by_name(&self, name: &str) -> Result<bool> {
        self.exists_by_code(name).await
    }

    pub async fn exists_by_code(&self, code: &str) -> Result<bool> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_roles WHERE name = $1"
        )
        .bind(code)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    pub async fn update(&self, role: &AuthRole) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE iam_roles SET
                application_id = $2, application_code = $3, name = $4, display_name = $5,
                description = $6, source = $7, client_managed = $8, updated_at = $9
             WHERE id = $1"
        )
        .bind(&role.id)
        .bind(&role.application_id)
        .bind(Some(&role.application_code))
        .bind(&role.name)
        .bind(&role.display_name)
        .bind(&role.description)
        .bind(role.source.as_str())
        .bind(role.client_managed)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Sync permissions: delete all then re-insert
        sqlx::query("DELETE FROM iam_role_permissions WHERE role_id = $1")
            .bind(&role.id)
            .execute(&self.pool)
            .await?;

        self.insert_permissions(&role.id, &role.permissions).await?;

        Ok(())
    }

    /// Delete a role and cascade the non-FK junction — `iam_principal_roles`
    /// references roles by **name** (no DB-level FK), so deletion must remove
    /// those rows atomically or we leak orphaned role assignments. Role
    /// permissions have a real FK and cascade at the DB level.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        // Look up the name so we can cascade the text-keyed junction.
        let role_name: Option<String> = sqlx::query_scalar(
            "SELECT name FROM iam_roles WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(name) = role_name {
            sqlx::query("DELETE FROM iam_principal_roles WHERE role_name = $1")
                .bind(&name)
                .execute(&mut *tx)
                .await?;
        }

        let result = sqlx::query("DELETE FROM iam_roles WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// Count principals currently holding this role — callers that want to
    /// refuse to delete when assignments exist (e.g. code role sync) can gate
    /// on this instead of silently dropping user assignments.
    pub async fn count_assignments(&self, role_name: &str) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_principal_roles WHERE role_name = $1",
        )
        .bind(role_name)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    /// Count orphaned role assignments — junction rows whose `role_name`
    /// has no matching `iam_roles.name`. Startup scans use this to detect
    /// integrity drift.
    pub async fn count_orphaned_assignments(&self) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_principal_roles pr \
             WHERE NOT EXISTS (SELECT 1 FROM iam_roles r WHERE r.name = pr.role_name)",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    // ── Helpers ──────────────────────────────────────────────

    /// Load permissions for a role from the junction table
    async fn load_permissions(&self, role_id: &str) -> Result<std::collections::HashSet<String>> {
        let perms: Vec<String> = sqlx::query_scalar(
            "SELECT permission FROM iam_role_permissions WHERE role_id = $1"
        )
        .bind(role_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(perms.into_iter().collect())
    }

    /// Insert permissions into the junction table using UNNEST
    async fn insert_permissions(&self, role_id: &str, permissions: &std::collections::HashSet<String>) -> Result<()> {
        if permissions.is_empty() {
            return Ok(());
        }

        let role_ids: Vec<String> = std::iter::repeat(role_id.to_string()).take(permissions.len()).collect();
        let perms: Vec<String> = permissions.iter().cloned().collect();

        sqlx::query(
            "INSERT INTO iam_role_permissions (role_id, permission)
             SELECT * FROM UNNEST($1::text[], $2::text[])"
        )
        .bind(&role_ids)
        .bind(&perms)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Convert a list of DB rows to domain entities with permissions loaded (batch)
    async fn hydrate_roles(&self, rows: Vec<RoleRow>) -> Result<Vec<AuthRole>> {
        if rows.is_empty() {
            return Ok(vec![]);
        }

        // Batch-load all permissions for these roles
        let role_ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let all_perms = sqlx::query_as::<_, RolePermissionRow>(
            "SELECT role_id, permission FROM iam_role_permissions WHERE role_id = ANY($1)"
        )
        .bind(&role_ids)
        .fetch_all(&self.pool)
        .await?;

        // Group permissions by role_id
        let mut perm_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for rp in all_perms {
            perm_map
                .entry(rp.role_id)
                .or_default()
                .insert(rp.permission);
        }

        // Build domain entities
        let roles = rows
            .into_iter()
            .map(|r| {
                let id = r.id.clone();
                let mut role = AuthRole::from(r);
                if let Some(perms) = perm_map.remove(&id) {
                    role.permissions = perms;
                }
                role
            })
            .collect();

        Ok(roles)
    }
}

// ── Persist<AuthRole> ────────────────────────────────────────────────────────

impl HasId for AuthRole {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl crate::usecase::Persist<AuthRole> for RoleRepository {
    async fn persist(&self, r: &AuthRole, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO iam_roles (id, application_id, application_code, name, display_name, description, source, client_managed, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET
                application_id = EXCLUDED.application_id,
                application_code = EXCLUDED.application_code,
                name = EXCLUDED.name,
                display_name = EXCLUDED.display_name,
                description = EXCLUDED.description,
                source = EXCLUDED.source,
                client_managed = EXCLUDED.client_managed,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&r.id)
        .bind(&r.application_id)
        .bind(Some(&r.application_code))
        .bind(&r.name)
        .bind(&r.display_name)
        .bind(&r.description)
        .bind(r.source.as_str())
        .bind(r.client_managed)
        .bind(now)
        .bind(now)
        .execute(&mut **tx.inner).await?;

        sqlx::query("DELETE FROM iam_role_permissions WHERE role_id = $1")
            .bind(&r.id)
            .execute(&mut **tx.inner).await?;

        for perm in &r.permissions {
            sqlx::query(
                "INSERT INTO iam_role_permissions (role_id, permission) VALUES ($1, $2)"
            )
            .bind(&r.id)
            .bind(perm)
            .execute(&mut **tx.inner).await?;
        }

        Ok(())
    }

    async fn delete(&self, r: &AuthRole, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        // iam_principal_roles has no DB-level FK on role_name (by design —
        // integrity lives in code). Cascade inline inside the same tx as the
        // role row delete so the invariant holds on every write path.
        sqlx::query("DELETE FROM iam_principal_roles WHERE role_name = $1")
            .bind(&r.name)
            .execute(&mut **tx.inner).await?;

        // iam_role_permissions has a real FK + ON DELETE CASCADE — no manual cleanup needed.
        sqlx::query("DELETE FROM iam_roles WHERE id = $1")
            .bind(&r.id)
            .execute(&mut **tx.inner).await?;
        Ok(())
    }
}
