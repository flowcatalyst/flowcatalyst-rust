//! ApplicationClientConfig Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::AppClientConfigId;
use fc_platform_core::shared::id::ApplicationId;
use fc_platform_core::shared::id::ClientId;
use sqlx::PgPool;

use super::client_config::ApplicationClientConfig;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::error::Result;
use fc_platform_core::usecase::unit_of_work::HasId;
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::Persist;

/// Row mapping for app_client_configs table
struct AppClientConfigRow {
    id: AppClientConfigId,
    application_id: ApplicationId,
    client_id: ClientId,
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
        sqlx::query!(
            "INSERT INTO app_client_configs (id, application_id, client_id, enabled, \
             base_url_override, config_json, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &config.id as &AppClientConfigId,
            &config.application_id as &ApplicationId,
            &config.client_id as &ClientId,
            config.enabled,
            config.base_url_override.as_ref(),
            config.config_json.as_ref(),
            now,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(
        &self,
        id: &AppClientConfigId,
    ) -> Result<Option<ApplicationClientConfig>> {
        let row = sqlx::query_as!(
            AppClientConfigRow,
            "SELECT id AS \"id: AppClientConfigId\", \
                    application_id AS \"application_id: ApplicationId\", \
                    client_id AS \"client_id: ClientId\", enabled, base_url_override, \
                    config_json AS \"config_json: serde_json::Value\", created_at, \
                    updated_at \
                    FROM app_client_configs WHERE id = $1",
            id as &AppClientConfigId
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ApplicationClientConfig::from))
    }

    pub async fn find_by_application(
        &self,
        application_id: &ApplicationId,
    ) -> Result<Vec<ApplicationClientConfig>> {
        let rows = sqlx::query_as!(
            AppClientConfigRow,
            "SELECT id AS \"id: AppClientConfigId\", \
                    application_id AS \"application_id: ApplicationId\", \
                    client_id AS \"client_id: ClientId\", enabled, base_url_override, \
                    config_json AS \"config_json: serde_json::Value\", created_at, \
                    updated_at \
                    FROM app_client_configs WHERE application_id = $1",
            application_id as &ApplicationId
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(ApplicationClientConfig::from)
            .collect())
    }

    pub async fn find_by_client(
        &self,
        client_id: &ClientId,
    ) -> Result<Vec<ApplicationClientConfig>> {
        let rows = sqlx::query_as!(
            AppClientConfigRow,
            "SELECT id AS \"id: AppClientConfigId\", \
                    application_id AS \"application_id: ApplicationId\", \
                    client_id AS \"client_id: ClientId\", enabled, base_url_override, \
                    config_json AS \"config_json: serde_json::Value\", created_at, \
                    updated_at \
                    FROM app_client_configs WHERE client_id = $1",
            client_id as &ClientId
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(ApplicationClientConfig::from)
            .collect())
    }

    pub async fn find_by_application_and_client(
        &self,
        application_id: &ApplicationId,
        client_id: &ClientId,
    ) -> Result<Option<ApplicationClientConfig>> {
        let row = sqlx::query_as!(
            AppClientConfigRow,
            "SELECT id AS \"id: AppClientConfigId\", \
                    application_id AS \"application_id: ApplicationId\", \
                    client_id AS \"client_id: ClientId\", enabled, base_url_override, \
                    config_json AS \"config_json: serde_json::Value\", created_at, \
                    updated_at \
                    FROM app_client_configs WHERE application_id = $1 AND client_id = $2",
            application_id as &ApplicationId,
            client_id as &ClientId
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ApplicationClientConfig::from))
    }

    pub async fn find_enabled_for_client(
        &self,
        client_id: &ClientId,
    ) -> Result<Vec<ApplicationClientConfig>> {
        let rows = sqlx::query_as!(
            AppClientConfigRow,
            "SELECT id AS \"id: AppClientConfigId\", \
                    application_id AS \"application_id: ApplicationId\", \
                    client_id AS \"client_id: ClientId\", enabled, base_url_override, \
                    config_json AS \"config_json: serde_json::Value\", created_at, \
                    updated_at \
                    FROM app_client_configs WHERE client_id = $1 AND enabled = TRUE",
            client_id as &ClientId
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(ApplicationClientConfig::from)
            .collect())
    }

    pub async fn enable_for_client(
        &self,
        application_id: &ApplicationId,
        client_id: &ClientId,
    ) -> Result<ApplicationClientConfig> {
        // Check if exists
        let existing = self
            .find_by_application_and_client(application_id, client_id)
            .await?;
        if let Some(config) = existing {
            // Update
            sqlx::query!(
                "UPDATE app_client_configs SET enabled = TRUE, updated_at = $2 WHERE id = $1",
                &config.id as &AppClientConfigId,
                Utc::now()
            )
            .execute(&self.pool)
            .await?;
            Ok(self
                .find_by_id(&config.id)
                .await?
                .ok_or_else(|| PlatformError::NotFound {
                    entity_type: "ApplicationClientConfig".to_string(),
                    id: config.id.to_string(),
                })?)
        } else {
            // Insert new
            let config = ApplicationClientConfig::new(application_id.clone(), client_id.clone());
            self.insert(&config).await?;
            Ok(config)
        }
    }

    pub async fn disable_for_client(
        &self,
        application_id: &ApplicationId,
        client_id: &ClientId,
    ) -> Result<bool> {
        let existing = self
            .find_by_application_and_client(application_id, client_id)
            .await?;
        if let Some(config) = existing {
            sqlx::query!(
                "UPDATE app_client_configs SET enabled = FALSE, updated_at = $2 WHERE id = $1",
                &config.id as &AppClientConfigId,
                Utc::now()
            )
            .execute(&self.pool)
            .await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn update(&self, config: &ApplicationClientConfig) -> Result<()> {
        sqlx::query!(
            "UPDATE app_client_configs SET
                application_id = $2,
                client_id = $3,
                enabled = $4,
                base_url_override = $5,
                config_json = $6,
                updated_at = $7
             WHERE id = $1",
            &config.id as &AppClientConfigId,
            &config.application_id as &ApplicationId,
            &config.client_id as &ClientId,
            config.enabled,
            config.base_url_override.as_ref(),
            config.config_json.as_ref(),
            Utc::now()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &AppClientConfigId) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM app_client_configs WHERE id = $1",
            id as &AppClientConfigId
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_by_application_and_client(
        &self,
        application_id: &ApplicationId,
        client_id: &ClientId,
    ) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM app_client_configs WHERE application_id = $1 AND client_id = $2",
            application_id as &ApplicationId,
            client_id as &ClientId
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── Persist<ApplicationClientConfig> ─────────────────────────────────────────

impl HasId for ApplicationClientConfig {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<ApplicationClientConfig> for ApplicationClientConfigRepository {
    async fn persist(&self, c: &ApplicationClientConfig, tx: &mut DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query!(
            "INSERT INTO app_client_configs (id, application_id, client_id, enabled, \
             base_url_override, config_json, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET
                enabled = EXCLUDED.enabled,
                base_url_override = EXCLUDED.base_url_override,
                config_json = EXCLUDED.config_json,
                updated_at = EXCLUDED.updated_at",
            &c.id as &AppClientConfigId,
            &c.application_id as &ApplicationId,
            &c.client_id as &ClientId,
            c.enabled,
            c.base_url_override.as_ref(),
            c.config_json.as_ref(),
            now,
            now
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, c: &ApplicationClientConfig, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM app_client_configs WHERE id = $1",
            &c.id as &AppClientConfigId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}
