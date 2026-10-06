//! OpenAPI spec repository — PostgreSQL via SQLx.
//!
//! Owns the only write path for the `OpenApiSpec` aggregate.
//! `impl Persist<OpenApiSpec> for OpenApiSpecRepository` is used by the sync
//! use case through the UnitOfWork.

use crate::application_openapi_spec::entity::OpenApiSpecStatus;
use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::ApplicationId;
use fc_platform_core::shared::id::ApplicationOpenApiSpecId;
use sqlx::PgPool;

use super::entity::{ChangeNotes, OpenApiSpec};
use fc_platform_core::shared::enum_str::Stored;
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::usecase::unit_of_work::HasId;
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::Persist;
use std::collections::HashMap;

struct OpenApiSpecRow {
    id: ApplicationOpenApiSpecId,
    application_id: ApplicationId,
    version: String,
    status: Stored<OpenApiSpecStatus>,
    spec: serde_json::Value,
    spec_hash: String,
    change_notes: Option<serde_json::Value>,
    change_notes_text: Option<String>,
    synced_at: DateTime<Utc>,
    synced_by: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<OpenApiSpecRow> for OpenApiSpec {
    type Error = PlatformError;
    fn try_from(r: OpenApiSpecRow) -> Result<Self> {
        let status = r
            .status
            .decode("app_application_openapi_specs", "status", r.id.as_str())?;
        Ok(Self {
            id: r.id,
            application_id: r.application_id,
            version: r.version,
            status,
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
        })
    }
}

/// The identifying columns of an application's CURRENT spec.
#[derive(Debug, Clone)]
pub struct CurrentSpecRef {
    pub id: ApplicationOpenApiSpecId,
    pub version: String,
    pub synced_at: DateTime<Utc>,
}

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
        application_id: &ApplicationId,
    ) -> Result<Option<OpenApiSpec>> {
        let row = sqlx::query_as!(
            OpenApiSpecRow,
            "SELECT id AS \"id: ApplicationOpenApiSpecId\", \
                    application_id AS \"application_id: ApplicationId\", \
                    version, \
                    status AS \"status: Stored<OpenApiSpecStatus>\", \
                    spec AS \"spec: serde_json::Value\", \
                    spec_hash, \
                    change_notes AS \"change_notes: serde_json::Value\", \
                    change_notes_text, synced_at, synced_by, created_at, updated_at \
             FROM app_application_openapi_specs \
             WHERE application_id = $1 AND status = 'CURRENT'",
            application_id as &ApplicationId
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(OpenApiSpec::try_from).transpose()
    }

    /// The CURRENT spec's id, version and sync time for each of
    /// `application_ids` that has one, keyed by application id: one query,
    /// without the documents themselves.
    pub async fn find_current_refs_by_applications(
        &self,
        application_ids: &[ApplicationId],
    ) -> Result<HashMap<ApplicationId, CurrentSpecRef>> {
        if application_ids.is_empty() {
            return Ok(Default::default());
        }
        let rows = sqlx::query!(
            "SELECT application_id AS \"application_id: ApplicationId\", \
                    id AS \"id: ApplicationOpenApiSpecId\", \
                    version, synced_at \
             FROM app_application_openapi_specs \
             WHERE application_id = ANY($1) AND status = 'CURRENT'",
            application_ids as &[ApplicationId]
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.application_id,
                    CurrentSpecRef {
                        id: r.id,
                        version: r.version,
                        synced_at: r.synced_at,
                    },
                )
            })
            .collect())
    }

    pub async fn find_all_by_application(
        &self,
        application_id: &ApplicationId,
    ) -> Result<Vec<OpenApiSpec>> {
        let rows = sqlx::query_as!(
            OpenApiSpecRow,
            "SELECT id AS \"id: ApplicationOpenApiSpecId\", \
                    application_id AS \"application_id: ApplicationId\", \
                    version, \
                    status AS \"status: Stored<OpenApiSpecStatus>\", \
                    spec AS \"spec: serde_json::Value\", \
                    spec_hash, \
                    change_notes AS \"change_notes: serde_json::Value\", \
                    change_notes_text, synced_at, synced_by, created_at, updated_at \
             FROM app_application_openapi_specs \
             WHERE application_id = $1 ORDER BY synced_at DESC",
            application_id as &ApplicationId
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(OpenApiSpec::try_from).collect()
    }

    /// Does any row (CURRENT or ARCHIVED) already occupy this version slot?
    /// Used by the sync use case to disambiguate when `info.version` repeats
    /// across syncs (e.g. utoipa-generated specs that pin to the crate
    /// version).
    pub async fn exists_by_application_and_version(
        &self,
        application_id: &ApplicationId,
        version: &str,
    ) -> Result<bool> {
        // EXISTS is never NULL, so `!`.
        let exists = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM app_application_openapi_specs \
             WHERE application_id = $1 AND version = $2) AS \"exists!\"",
            application_id as &ApplicationId,
            version
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    pub async fn find_by_id(&self, id: &ApplicationOpenApiSpecId) -> Result<Option<OpenApiSpec>> {
        let row = sqlx::query_as!(
            OpenApiSpecRow,
            "SELECT id AS \"id: ApplicationOpenApiSpecId\", \
                    application_id AS \"application_id: ApplicationId\", \
                    version, \
                    status AS \"status: Stored<OpenApiSpecStatus>\", \
                    spec AS \"spec: serde_json::Value\", \
                    spec_hash, \
                    change_notes AS \"change_notes: serde_json::Value\", \
                    change_notes_text, synced_at, synced_by, created_at, updated_at \
             FROM app_application_openapi_specs \
             WHERE id = $1",
            id as &ApplicationOpenApiSpecId
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(OpenApiSpec::try_from).transpose()
    }
}

impl HasId for OpenApiSpec {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<OpenApiSpec> for OpenApiSpecRepository {
    async fn persist(&self, spec: &OpenApiSpec, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
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
            &spec.id as &ApplicationOpenApiSpecId,
            &spec.application_id as &ApplicationId,
            spec.version,
            spec.status as OpenApiSpecStatus,
            spec.spec,
            spec.spec_hash,
            spec.change_notes
                .as_ref()
                .map(|cn| serde_json::to_value(cn).unwrap_or(serde_json::Value::Null)),
            spec.change_notes_text,
            spec.synced_at,
            spec.synced_by,
            spec.created_at,
            spec.updated_at
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, spec: &OpenApiSpec, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM app_application_openapi_specs WHERE id = $1",
            &spec.id as &ApplicationOpenApiSpecId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}
