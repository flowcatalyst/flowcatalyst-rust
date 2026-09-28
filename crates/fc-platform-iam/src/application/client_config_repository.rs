//! ApplicationClientConfig Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::client_config::ApplicationClientConfig;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::error::Result;
use fc_platform_core::usecase::unit_of_work::HasId;
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::Persist;

/// Row mapping for app_client_configs table
#[derive(sqlx::FromRow)]
struct AppClientConfigRow {
    id: String,
    application_id: String,
    client_id: String,
    enabled: bool,
    base_url_override: Option<String>,
    config_json: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<AppClientConfigRow> for ApplicationClientConfig {
    fn from(r: AppClientConfigRow) -> Self {
        Self {
            id: r.id,
            application_id: r.application_id,
            client_id: r.client_id,
            enabled: r.enabled,
            base_url_override: r.base_url_override,
            config_json: r.config_json,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub struct ApplicationClientConfigRepository {
    pool: PgPool,
}

impl ApplicationClientConfigRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, config: &ApplicationClientConfig) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO app_client_configs (id, application_id, client_id, enabled, \
             base_url_override, config_json, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&config.id)
        .bind(&config.application_id)
        .bind(&config.client_id)
        .bind(config.enabled)
        .bind(&config.base_url_override)
        .bind(&config.config_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ApplicationClientConfig>> {
        let row = sqlx::query_as::<_, AppClientConfigRow>(
            "SELECT * FROM app_client_configs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ApplicationClientConfig::from))
    }

    pub async fn find_by_application(
        &self,
        application_id: &str,
    ) -> Result<Vec<ApplicationClientConfig>> {
        let rows = sqlx::query_as::<_, AppClientConfigRow>(
            "SELECT * FROM app_client_configs WHERE application_id = $1",
        )
        .bind(application_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(ApplicationClientConfig::from)
            .collect())
    }

    pub async fn find_by_client(&self, client_id: &str) -> Result<Vec<ApplicationClientConfig>> {
        let rows = sqlx::query_as::<_, AppClientConfigRow>(
            "SELECT * FROM app_client_configs WHERE client_id = $1",
        )
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(ApplicationClientConfig::from)
            .collect())
    }

    pub async fn find_by_application_and_client(
        &self,
        application_id: &str,
        client_id: &str,
    ) -> Result<Option<ApplicationClientConfig>> {
        let row = sqlx::query_as::<_, AppClientConfigRow>(
            "SELECT * FROM app_client_configs WHERE application_id = $1 AND client_id = $2",
        )
        .bind(application_id)
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ApplicationClientConfig::from))
    }

    pub async fn find_enabled_for_client(
        &self,
        client_id: &str,
    ) -> Result<Vec<ApplicationClientConfig>> {
        let rows = sqlx::query_as::<_, AppClientConfigRow>(
            "SELECT * FROM app_client_configs WHERE client_id = $1 AND enabled = TRUE",
        )
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(ApplicationClientConfig::from)
            .collect())
    }

    pub async fn enable_for_client(
        &self,
        application_id: &str,
        client_id: &str,
    ) -> Result<ApplicationClientConfig> {
        // Check if exists
        let existing = self
            .find_by_application_and_client(application_id, client_id)
            .await?;
        if let Some(config) = existing {
            // Update
            sqlx::query(
                "UPDATE app_client_configs SET enabled = TRUE, updated_at = $2 WHERE id = $1",
            )
            .bind(&config.id)
            .bind(Utc::now())
            .execute(&self.pool)
            .await?;
            Ok(self
                .find_by_id(&config.id)
                .await?
                .ok_or_else(|| PlatformError::NotFound {
                    entity_type: "ApplicationClientConfig".to_string(),
                    id: config.id.clone(),
                })?)
        } else {
            // Insert new
            let config = ApplicationClientConfig::new(application_id, client_id);
            self.insert(&config).await?;
            Ok(config)
        }
    }

    pub async fn disable_for_client(&self, application_id: &str, client_id: &str) -> Result<bool> {
        let existing = self
            .find_by_application_and_client(application_id, client_id)
            .await?;
        if let Some(config) = existing {
            sqlx::query(
                "UPDATE app_client_configs SET enabled = FALSE, updated_at = $2 WHERE id = $1",
            )
            .bind(&config.id)
            .bind(Utc::now())
            .execute(&self.pool)
            .await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn update(&self, config: &ApplicationClientConfig) -> Result<()> {
        sqlx::query(
            "UPDATE app_client_configs SET
                application_id = $2,
                client_id = $3,
                enabled = $4,
                base_url_override = $5,
                config_json = $6,
                updated_at = $7
             WHERE id = $1",
        )
        .bind(&config.id)
        .bind(&config.application_id)
        .bind(&config.client_id)
        .bind(config.enabled)
        .bind(&config.base_url_override)
        .bind(&config.config_json)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM app_client_configs WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_by_application_and_client(
        &self,
        application_id: &str,
        client_id: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM app_client_configs WHERE application_id = $1 AND client_id = $2",
        )
        .bind(application_id)
        .bind(client_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── Persist<ApplicationClientConfig> ─────────────────────────────────────────

impl HasId for ApplicationClientConfig {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Persist<ApplicationClientConfig> for ApplicationClientConfigRepository {
    async fn persist(&self, c: &ApplicationClientConfig, tx: &mut DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO app_client_configs (id, application_id, client_id, enabled, \
             base_url_override, config_json, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET
                enabled = EXCLUDED.enabled,
                base_url_override = EXCLUDED.base_url_override,
                config_json = EXCLUDED.config_json,
                updated_at = EXCLUDED.updated_at",
        )
        .bind(&c.id)
        .bind(&c.application_id)
        .bind(&c.client_id)
        .bind(c.enabled)
        .bind(&c.base_url_override)
        .bind(&c.config_json)
        .bind(now)
        .bind(now)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, c: &ApplicationClientConfig, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM app_client_configs WHERE id = $1")
            .bind(&c.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
