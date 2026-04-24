//! Application Repository — PostgreSQL via SQLx

use async_trait::async_trait;
use sqlx::PgPool;
use chrono::{DateTime, Utc};

use super::entity::{Application, ApplicationType};
use crate::shared::error::Result;
use crate::usecase::unit_of_work::HasId;

/// Row mapping for app_applications table
#[derive(sqlx::FromRow)]
struct ApplicationRow {
    id: String,
    #[sqlx(rename = "type")]
    application_type: String,
    code: String,
    name: String,
    description: Option<String>,
    icon_url: Option<String>,
    website: Option<String>,
    logo: Option<String>,
    logo_mime_type: Option<String>,
    default_base_url: Option<String>,
    service_account_id: Option<String>,
    active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ApplicationRow> for Application {
    fn from(r: ApplicationRow) -> Self {
        Self {
            id: r.id,
            application_type: ApplicationType::from_str(&r.application_type),
            code: r.code,
            name: r.name,
            description: r.description,
            icon_url: r.icon_url,
            website: r.website,
            logo: r.logo,
            logo_mime_type: r.logo_mime_type,
            default_base_url: r.default_base_url,
            service_account_id: r.service_account_id,
            active: r.active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub struct ApplicationRepository {
    pool: PgPool,
}

impl ApplicationRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, app: &Application) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO app_applications (id, type, code, name, description, icon_url, website, logo, logo_mime_type, default_base_url, service_account_id, active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"
        )
        .bind(&app.id)
        .bind(app.application_type.as_str())
        .bind(&app.code)
        .bind(&app.name)
        .bind(&app.description)
        .bind(&app.icon_url)
        .bind(&app.website)
        .bind(&app.logo)
        .bind(&app.logo_mime_type)
        .bind(&app.default_base_url)
        .bind(&app.service_account_id)
        .bind(app.active)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Application>> {
        let row = sqlx::query_as::<_, ApplicationRow>(
            "SELECT * FROM app_applications WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Application::from))
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<Application>> {
        let row = sqlx::query_as::<_, ApplicationRow>(
            "SELECT * FROM app_applications WHERE code = $1"
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Application::from))
    }

    pub async fn find_active(&self) -> Result<Vec<Application>> {
        let rows = sqlx::query_as::<_, ApplicationRow>(
            "SELECT * FROM app_applications WHERE active = TRUE"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Application::from).collect())
    }

    pub async fn find_all(&self) -> Result<Vec<Application>> {
        let rows = sqlx::query_as::<_, ApplicationRow>(
            "SELECT * FROM app_applications"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Application::from).collect())
    }

    pub async fn find_paged(&self, limit: i64, offset: i64) -> Result<(Vec<Application>, i64)> {
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM app_applications"
        )
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, ApplicationRow>(
            "SELECT * FROM app_applications ORDER BY code ASC LIMIT $1 OFFSET $2"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok((
            rows.into_iter().map(Application::from).collect(),
            total.0,
        ))
    }

    pub async fn find_by_type(&self, app_type: ApplicationType) -> Result<Vec<Application>> {
        let rows = sqlx::query_as::<_, ApplicationRow>(
            "SELECT * FROM app_applications WHERE type = $1 AND active = TRUE"
        )
        .bind(app_type.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Application::from).collect())
    }

    pub async fn find_by_service_account(&self, service_account_id: &str) -> Result<Option<Application>> {
        let row = sqlx::query_as::<_, ApplicationRow>(
            "SELECT * FROM app_applications WHERE service_account_id = $1"
        )
        .bind(service_account_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Application::from))
    }

    pub async fn exists(&self, id: &str) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM app_applications WHERE id = $1)"
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn exists_by_code(&self, code: &str) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM app_applications WHERE code = $1)"
        )
        .bind(code)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn update(&self, app: &Application) -> Result<Application> {
        sqlx::query(
            "UPDATE app_applications SET
                type = $2,
                code = $3,
                name = $4,
                description = $5,
                icon_url = $6,
                website = $7,
                logo = $8,
                logo_mime_type = $9,
                default_base_url = $10,
                service_account_id = $11,
                active = $12,
                updated_at = $13
             WHERE id = $1"
        )
        .bind(&app.id)
        .bind(app.application_type.as_str())
        .bind(&app.code)
        .bind(&app.name)
        .bind(&app.description)
        .bind(&app.icon_url)
        .bind(&app.website)
        .bind(&app.logo)
        .bind(&app.logo_mime_type)
        .bind(&app.default_base_url)
        .bind(&app.service_account_id)
        .bind(app.active)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;

        // Reload and return
        self.find_by_id(&app.id).await?.ok_or_else(|| {
            crate::shared::error::PlatformError::NotFound {
                entity_type: "Application".to_string(),
                id: app.id.clone(),
            }
        })
    }

    /// Delete an application and cascade the non-FK junction —
    /// `iam_principal_application_access` references applications by id
    /// with no DB-level FK. Integrity is code-managed; this path must
    /// cascade atomically or we leak orphaned access grants.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM iam_principal_application_access WHERE application_id = $1")
            .bind(id)
            .execute(&mut *tx).await?;

        let result = sqlx::query("DELETE FROM app_applications WHERE id = $1")
            .bind(id)
            .execute(&mut *tx).await?;

        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// Count principals currently granted access to this application.
    /// Used by the delete use case to refuse deletion when user-level
    /// grants still exist — integrity is enforced in code, not the DB.
    pub async fn count_access_grants(&self, application_id: &str) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_principal_application_access WHERE application_id = $1",
        )
        .bind(application_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    /// Count per-client config entries pointing at this application.
    pub async fn count_client_configs(&self, application_id: &str) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM app_client_configs WHERE application_id = $1",
        )
        .bind(application_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    /// Count service accounts attached to this application.
    pub async fn count_service_accounts(&self, application_id: &str) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_service_accounts WHERE application_id = $1",
        )
        .bind(application_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    /// Count roles scoped to this application.
    pub async fn count_roles(&self, application_id: &str) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_roles WHERE application_id = $1",
        )
        .bind(application_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    /// Count principals whose application ref points at this application
    /// (service-account principals, typically).
    pub async fn count_principal_refs(&self, application_id: &str) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_principals WHERE application_id = $1",
        )
        .bind(application_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }
}

// ── Persist<Application> ─────────────────────────────────────────────────────

impl HasId for Application {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl crate::usecase::Persist<Application> for ApplicationRepository {
    async fn persist(&self, a: &Application, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO app_applications (id, type, code, name, description, icon_url, website, logo, logo_mime_type, default_base_url, service_account_id, active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (id) DO UPDATE SET
                type = EXCLUDED.type,
                code = EXCLUDED.code,
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                icon_url = EXCLUDED.icon_url,
                website = EXCLUDED.website,
                logo = EXCLUDED.logo,
                logo_mime_type = EXCLUDED.logo_mime_type,
                default_base_url = EXCLUDED.default_base_url,
                service_account_id = EXCLUDED.service_account_id,
                active = EXCLUDED.active,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&a.id)
        .bind(a.application_type.as_str())
        .bind(&a.code)
        .bind(&a.name)
        .bind(&a.description)
        .bind(&a.icon_url)
        .bind(&a.website)
        .bind(&a.logo)
        .bind(&a.logo_mime_type)
        .bind(&a.default_base_url)
        .bind(&a.service_account_id)
        .bind(a.active)
        .bind(now)
        .bind(now)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, a: &Application, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        // iam_principal_application_access has no DB-level FK on application_id
        // (integrity lives in code). Cascade in the same tx as the app delete
        // so this path holds the invariant even if someone bypasses the use case.
        sqlx::query("DELETE FROM iam_principal_application_access WHERE application_id = $1")
            .bind(&a.id)
            .execute(&mut **tx.inner)
            .await?;

        sqlx::query("DELETE FROM app_applications WHERE id = $1")
            .bind(&a.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
