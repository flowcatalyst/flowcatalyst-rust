//! PlatformConfig Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::PlatformConfigId;
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::PlatformConfig;
use fc_platform_core::shared::enum_str::decode;
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::HasId;
use fc_platform_core::usecase::Persist;

#[derive(sqlx::FromRow)]
struct PlatformConfigRow {
    id: PlatformConfigId,
    application_code: String,
    section: String,
    property: String,
    scope: String,
    client_id: Option<ClientId>,
    value_type: String,
    value: String,
    description: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PlatformConfigRow> for PlatformConfig {
    type Error = PlatformError;
    fn try_from(r: PlatformConfigRow) -> Result<Self> {
        let scope = decode(&r.scope, "app_platform_configs", "scope", r.id.as_str())?;
        let value_type = decode(
            &r.value_type,
            "app_platform_configs",
            "value_type",
            r.id.as_str(),
        )?;
        Ok(Self {
            id: r.id,
            application_code: r.application_code,
            section: r.section,
            property: r.property,
            scope,
            client_id: r.client_id,
            value_type,
            value: r.value,
            description: r.description,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

/// A property's coordinates: application, section, property and scope, by
/// field so the four adjacent strings cannot be passed in the wrong order.
#[derive(Debug, Clone, Copy)]
pub struct PropertyKey<'a> {
    pub app_code: &'a str,
    pub section: &'a str,
    pub property: &'a str,
    pub scope: &'a str,
}

/// A section's coordinates: application and section.
#[derive(Debug, Clone, Copy)]
pub struct SectionKey<'a> {
    pub app_code: &'a str,
    pub section: &'a str,
}

pub struct PlatformConfigRepository {
    pool: PgPool,
}

impl PlatformConfigRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_id(&self, id: &PlatformConfigId) -> Result<Option<PlatformConfig>> {
        let row = sqlx::query_as::<_, PlatformConfigRow>(
            "SELECT * FROM app_platform_configs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(PlatformConfig::try_from).transpose()
    }

    pub async fn find_by_key(
        &self,
        key: &PropertyKey<'_>,
        client_id: Option<&ClientId>,
    ) -> Result<Option<PlatformConfig>> {
        let PropertyKey {
            app_code,
            section,
            property,
            scope,
        } = *key;
        let row = if let Some(cid) = client_id {
            sqlx::query_as::<_, PlatformConfigRow>(
                "SELECT * FROM app_platform_configs \
                 WHERE application_code = $1 AND section = $2 AND property = $3 \
                 AND scope = $4 AND client_id = $5",
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
                 AND scope = $4 AND client_id IS NULL",
            )
            .bind(app_code)
            .bind(section)
            .bind(property)
            .bind(scope)
            .fetch_optional(&self.pool)
            .await?
        };
        row.map(PlatformConfig::try_from).transpose()
    }

    pub async fn find_by_section(
        &self,
        key: &SectionKey<'_>,
        scope: Option<&str>,
        client_id: Option<&ClientId>,
    ) -> Result<Vec<PlatformConfig>> {
        let SectionKey { app_code, section } = *key;
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT * FROM app_platform_configs WHERE application_code = ");
        qb.push_bind(app_code)
            .push(" AND section = ")
            .push_bind(section);
        if let Some(s) = scope {
            qb.push(" AND scope = ").push_bind(s);
        }
        if let Some(cid) = client_id {
            qb.push(" AND client_id = ").push_bind(cid);
        }
        qb.push(" ORDER BY property");
        let rows = qb
            .build_query_as::<PlatformConfigRow>()
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(PlatformConfig::try_from).collect()
    }

    pub async fn find_by_application(
        &self,
        app_code: &str,
        scope: Option<&str>,
        client_id: Option<&ClientId>,
    ) -> Result<Vec<PlatformConfig>> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT * FROM app_platform_configs WHERE application_code = ");
        qb.push_bind(app_code);
        if let Some(s) = scope {
            qb.push(" AND scope = ").push_bind(s);
        }
        if let Some(cid) = client_id {
            qb.push(" AND client_id = ").push_bind(cid);
        }
        qb.push(" ORDER BY section, property");
        let rows = qb
            .build_query_as::<PlatformConfigRow>()
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(PlatformConfig::try_from).collect()
    }

    pub async fn insert(&self, config: &PlatformConfig) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO app_platform_configs
                (id, application_code, section, property, scope, client_id,
                 value_type, value, description, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())"#,
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
            WHERE id = $1"#,
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
        key: &PropertyKey<'_>,
        client_id: Option<&ClientId>,
    ) -> Result<bool> {
        let PropertyKey {
            app_code,
            section,
            property,
            scope,
        } = *key;
        let result = if let Some(cid) = client_id {
            sqlx::query(
                "DELETE FROM app_platform_configs \
                 WHERE application_code = $1 AND section = $2 AND property = $3 \
                 AND scope = $4 AND client_id = $5",
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
                 AND scope = $4 AND client_id IS NULL",
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

impl HasId for PlatformConfig {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<PlatformConfig> for PlatformConfigRepository {
    async fn persist(&self, c: &PlatformConfig, tx: &mut DbTx<'_>) -> Result<()> {
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

    async fn delete(&self, c: &PlatformConfig, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM app_platform_configs WHERE id = $1")
            .bind(&c.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
