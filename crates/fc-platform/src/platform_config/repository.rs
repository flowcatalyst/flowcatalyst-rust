//! PlatformConfig Repository — PostgreSQL via SQLx

use async_trait::async_trait;
use sqlx::PgPool;
use chrono::{DateTime, Utc};

use super::entity::{PlatformConfig, ConfigScope, ConfigValueType};
use crate::shared::error::Result;

#[derive(sqlx::FromRow)]
struct PlatformConfigRow {
    id: String,
    application_code: String,
    section: String,
    property: String,
    scope: String,
    client_id: Option<String>,
    value_type: String,
    value: String,
    description: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<PlatformConfigRow> for PlatformConfig {
    fn from(r: PlatformConfigRow) -> Self {
        Self {
            id: r.id,
            application_code: r.application_code,
            section: r.section,
            property: r.property,
            scope: ConfigScope::from_str(&r.scope),
            client_id: r.client_id,
            value_type: ConfigValueType::from_str(&r.value_type),
            value: r.value,
            description: r.description,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub struct PlatformConfigRepository {
    pool: PgPool,
}

impl PlatformConfigRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<PlatformConfig>> {
        let row = sqlx::query_as::<_, PlatformConfigRow>(
            "SELECT * FROM app_platform_configs WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(PlatformConfig::from))
    }

    pub async fn find_by_key(
        &self,
        app_code: &str,
        section: &str,
        property: &str,
        scope: &str,
        client_id: Option<&str>,
    ) -> Result<Option<PlatformConfig>> {
        let row = if let Some(cid) = client_id {
            sqlx::query_as::<_, PlatformConfigRow>(
                "SELECT * FROM app_platform_configs \
                 WHERE application_code = $1 AND section = $2 AND property = $3 \
                 AND scope = $4 AND client_id = $5"
            )
            .bind(app_code)
            .bind(section)
            .bind(property)
            .bind(scope)
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, PlatformConfigRow>(
                "SELECT * FROM app_platform_configs \
                 WHERE application_code = $1 AND section = $2 AND property = $3 \
                 AND scope = $4 AND client_id IS NULL"
            )
            .bind(app_code)
            .bind(section)
            .bind(property)
            .bind(scope)
            .fetch_optional(&self.pool)
            .await?
        };
        Ok(row.map(PlatformConfig::from))
    }

    pub async fn find_by_section(
        &self,
        app_code: &str,
        section: &str,
        scope: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<Vec<PlatformConfig>> {
        let mut conditions = vec![
            "application_code = $1".to_string(),
            "section = $2".to_string(),
        ];
        let mut params: Vec<String> = vec![app_code.to_string(), section.to_string()];
        let mut idx = 2u32;

        if let Some(s) = scope {
            idx += 1;
            conditions.push(format!("scope = ${}", idx));
            params.push(s.to_string());
        }
        if let Some(cid) = client_id {
            idx += 1;
            conditions.push(format!("client_id = ${}", idx));
            params.push(cid.to_string());
        }

        let sql = format!(
            "SELECT * FROM app_platform_configs WHERE {} ORDER BY property",
            conditions.join(" AND ")
        );

        let mut query = sqlx::query_as::<_, PlatformConfigRow>(&sql);
        for p in &params {
            query = query.bind(p);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(PlatformConfig::from).collect())
    }

    pub async fn find_by_application(
        &self,
        app_code: &str,
        scope: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<Vec<PlatformConfig>> {
        let mut conditions = vec!["application_code = $1".to_string()];
        let mut params: Vec<String> = vec![app_code.to_string()];
        let mut idx = 1u32;

        if let Some(s) = scope {
            idx += 1;
            conditions.push(format!("scope = ${}", idx));
            params.push(s.to_string());
        }
        if let Some(cid) = client_id {
            idx += 1;
            conditions.push(format!("client_id = ${}", idx));
            params.push(cid.to_string());
        }

        let sql = format!(
            "SELECT * FROM app_platform_configs WHERE {} ORDER BY section, property",
            conditions.join(" AND ")
        );

        let mut query = sqlx::query_as::<_, PlatformConfigRow>(&sql);
        for p in &params {
            query = query.bind(p);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(PlatformConfig::from).collect())
    }

    pub async fn insert(&self, config: &PlatformConfig) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO app_platform_configs
                (id, application_code, section, property, scope, client_id,
                 value_type, value, description, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())"#
        )
        .bind(&config.id)
        .bind(&config.application_code)
        .bind(&config.section)
        .bind(&config.property)
        .bind(config.scope.as_str())
        .bind(&config.client_id)
        .bind(config.value_type.as_str())
        .bind(&config.value)
        .bind(&config.description)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update(&self, config: &PlatformConfig) -> Result<()> {
        sqlx::query(
            r#"UPDATE app_platform_configs SET
                application_code = $2, section = $3, property = $4, scope = $5,
                client_id = $6, value_type = $7, value = $8, description = $9,
                updated_at = NOW()
            WHERE id = $1"#
        )
        .bind(&config.id)
        .bind(&config.application_code)
        .bind(&config.section)
        .bind(&config.property)
        .bind(config.scope.as_str())
        .bind(&config.client_id)
        .bind(config.value_type.as_str())
        .bind(&config.value)
        .bind(&config.description)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_by_key(
        &self,
        app_code: &str,
        section: &str,
        property: &str,
        scope: &str,
        client_id: Option<&str>,
    ) -> Result<bool> {
        let result = if let Some(cid) = client_id {
            sqlx::query(
                "DELETE FROM app_platform_configs \
                 WHERE application_code = $1 AND section = $2 AND property = $3 \
                 AND scope = $4 AND client_id = $5"
            )
            .bind(app_code)
            .bind(section)
            .bind(property)
            .bind(scope)
            .bind(cid)
            .execute(&self.pool)
            .await?
        } else {
            sqlx::query(
                "DELETE FROM app_platform_configs \
                 WHERE application_code = $1 AND section = $2 AND property = $3 \
                 AND scope = $4 AND client_id IS NULL"
            )
            .bind(app_code)
            .bind(section)
            .bind(property)
            .bind(scope)
            .execute(&self.pool)
            .await?
        };
        Ok(result.rows_affected() > 0)
    }
}

impl crate::usecase::HasId for PlatformConfig {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl crate::usecase::Persist<PlatformConfig> for PlatformConfigRepository {
    async fn persist(&self, c: &PlatformConfig, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO app_platform_configs
                (id, application_code, section, property, scope, client_id,
                 value_type, value, description, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())
            ON CONFLICT (id) DO UPDATE SET
                application_code = EXCLUDED.application_code,
                section = EXCLUDED.section,
                property = EXCLUDED.property,
                scope = EXCLUDED.scope,
                client_id = EXCLUDED.client_id,
                value_type = EXCLUDED.value_type,
                value = EXCLUDED.value,
                description = EXCLUDED.description,
                updated_at = NOW()"#,
        )
        .bind(&c.id)
        .bind(&c.application_code)
        .bind(&c.section)
        .bind(&c.property)
        .bind(c.scope.as_str())
        .bind(&c.client_id)
        .bind(c.value_type.as_str())
        .bind(&c.value)
        .bind(&c.description)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, c: &PlatformConfig, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM app_platform_configs WHERE id = $1")
            .bind(&c.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
