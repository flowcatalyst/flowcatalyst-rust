//! OpenAPI spec repository — PostgreSQL via SQLx.
//!
//! Owns the only write path for the `OpenApiSpec` aggregate.
//! `impl Persist<OpenApiSpec> for OpenApiSpecRepository` is used by the sync
//! use case through the UnitOfWork.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{ChangeNotes, OpenApiSpec, OpenApiSpecStatus};
use crate::shared::error::Result;
use crate::usecase::unit_of_work::HasId;

#[derive(sqlx::FromRow)]
struct OpenApiSpecRow {
    id: String,
    application_id: String,
    version: String,
    status: String,
    spec: serde_json::Value,
    spec_hash: String,
    change_notes: Option<serde_json::Value>,
    change_notes_text: Option<String>,
    synced_at: DateTime<Utc>,
    synced_by: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<OpenApiSpecRow> for OpenApiSpec {
    fn from(r: OpenApiSpecRow) -> Self {
        Self {
            id: r.id,
            application_id: r.application_id,
            version: r.version,
            status: OpenApiSpecStatus::from_str(&r.status),
            spec: r.spec,
            spec_hash: r.spec_hash,
            change_notes: r
                .change_notes
                .and_then(|v| serde_json::from_value::<ChangeNotes>(v).ok()),
            change_notes_text: r.change_notes_text,
            synced_at: r.synced_at,
            synced_by: r.synced_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const SELECT_COLS: &str = "id, application_id, version, status, spec, spec_hash, \
                            change_notes, change_notes_text, synced_at, synced_by, \
                            created_at, updated_at";

pub struct OpenApiSpecRepository {
    pool: PgPool,
}

impl OpenApiSpecRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn find_current_by_application(
        &self,
        application_id: &str,
    ) -> Result<Option<OpenApiSpec>> {
        let row = sqlx::query_as::<_, OpenApiSpecRow>(&format!(
            "SELECT {SELECT_COLS} FROM app_application_openapi_specs \
             WHERE application_id = $1 AND status = 'CURRENT'"
        ))
        .bind(application_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(OpenApiSpec::from))
    }

    pub async fn find_all_by_application(
        &self,
        application_id: &str,
    ) -> Result<Vec<OpenApiSpec>> {
        let rows = sqlx::query_as::<_, OpenApiSpecRow>(&format!(
            "SELECT {SELECT_COLS} FROM app_application_openapi_specs \
             WHERE application_id = $1 ORDER BY synced_at DESC"
        ))
        .bind(application_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(OpenApiSpec::from).collect())
    }

    /// Does any row (CURRENT or ARCHIVED) already occupy this version slot?
    /// Used by the sync use case to disambiguate when `info.version` repeats
    /// across syncs (e.g. utoipa-generated specs that pin to the crate
    /// version).
    pub async fn exists_by_application_and_version(
        &self,
        application_id: &str,
        version: &str,
    ) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM app_application_openapi_specs \
             WHERE application_id = $1 AND version = $2)",
        )
        .bind(application_id)
        .bind(version)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<OpenApiSpec>> {
        let row = sqlx::query_as::<_, OpenApiSpecRow>(&format!(
            "SELECT {SELECT_COLS} FROM app_application_openapi_specs WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(OpenApiSpec::from))
    }

    /// Insert a fresh row. Used by the sync use case after the prior CURRENT
    /// has been demoted; the partial unique index ensures only one CURRENT
    /// per application.
    pub async fn insert(&self, spec: &OpenApiSpec) -> Result<()> {
        sqlx::query(
            "INSERT INTO app_application_openapi_specs \
                (id, application_id, version, status, spec, spec_hash, \
                 change_notes, change_notes_text, synced_at, synced_by, \
                 created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&spec.id)
        .bind(&spec.application_id)
        .bind(&spec.version)
        .bind(spec.status.as_str())
        .bind(&spec.spec)
        .bind(&spec.spec_hash)
        .bind(
            spec.change_notes
                .as_ref()
                .map(|cn| serde_json::to_value(cn).unwrap_or(serde_json::Value::Null)),
        )
        .bind(&spec.change_notes_text)
        .bind(spec.synced_at)
        .bind(&spec.synced_by)
        .bind(spec.created_at)
        .bind(spec.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Flip the current CURRENT row to ARCHIVED, attaching the diff that
    /// describes what the *new* spec is dropping vs. this one.
    pub async fn archive_current(
        &self,
        application_id: &str,
        change_notes: &ChangeNotes,
        change_notes_text: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE app_application_openapi_specs \
             SET status = 'ARCHIVED', \
                 change_notes = $2, \
                 change_notes_text = $3, \
                 updated_at = NOW() \
             WHERE application_id = $1 AND status = 'CURRENT'",
        )
        .bind(application_id)
        .bind(serde_json::to_value(change_notes).unwrap_or(serde_json::Value::Null))
        .bind(change_notes_text)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

impl HasId for OpenApiSpec {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl crate::usecase::Persist<OpenApiSpec> for OpenApiSpecRepository {
    async fn persist(&self, spec: &OpenApiSpec, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO app_application_openapi_specs \
                (id, application_id, version, status, spec, spec_hash, \
                 change_notes, change_notes_text, synced_at, synced_by, \
                 created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             ON CONFLICT (id) DO UPDATE SET \
                status = EXCLUDED.status, \
                change_notes = EXCLUDED.change_notes, \
                change_notes_text = EXCLUDED.change_notes_text, \
                updated_at = EXCLUDED.updated_at",
        )
        .bind(&spec.id)
        .bind(&spec.application_id)
        .bind(&spec.version)
        .bind(spec.status.as_str())
        .bind(&spec.spec)
        .bind(&spec.spec_hash)
        .bind(
            spec.change_notes
                .as_ref()
                .map(|cn| serde_json::to_value(cn).unwrap_or(serde_json::Value::Null)),
        )
        .bind(&spec.change_notes_text)
        .bind(spec.synced_at)
        .bind(&spec.synced_by)
        .bind(spec.created_at)
        .bind(spec.updated_at)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, spec: &OpenApiSpec, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM app_application_openapi_specs WHERE id = $1")
            .bind(&spec.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
