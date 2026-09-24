//! `fn_versions`, read side (Java `function/FunctionVersionRepository.java`).
//! Publishing and the version lifecycle writes are P4; this workstream
//! reads versions for a function's `live`, its status and the `declared`
//! keys of its config and secrets.
//!
//! As in Java, a single-row read of a version whose stored manifest cannot
//! be read fails loudly (a corrupt row), while the batch read
//! [`FunctionVersionRepository::find_by_ids`] leaves such a row out, so one
//! corrupt version never fails a list of other functions.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{FunctionVersion, SignerIdentity, VersionState};
use super::{Digest, JsonNode, Manifest};
use crate::shared::error::{PlatformError, Result};

#[derive(sqlx::FromRow)]
struct VersionRow {
    id: String,
    function_id: String,
    version: i32,
    artifact_ref: String,
    digest: String,
    signature_bundle: Option<String>,
    signature_bundle_ref: Option<String>,
    signer_issuer: Option<String>,
    signer_subject: Option<String>,
    manifest: String,
    state: String,
    published_by: String,
    published_at: DateTime<Utc>,
    ready_at: Option<DateTime<Utc>>,
    retired_at: Option<DateTime<Utc>>,
}

const COLUMNS: &str = "id, function_id, version, artifact_ref, digest, signature_bundle, \
                       signature_bundle_ref, signer_issuer, signer_subject, manifest::text AS manifest, \
                       state, published_by, published_at, ready_at, retired_at";

pub struct FunctionVersionRepository {
    pool: PgPool,
}

impl FunctionVersionRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<FunctionVersion>> {
        let row = sqlx::query_as::<_, VersionRow>(&format!(
            "SELECT {COLUMNS} FROM fn_versions WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(to_entity_or_corrupt).transpose()
    }

    pub async fn find_by_function_and_version(
        &self,
        function_id: &str,
        version: i32,
    ) -> Result<Option<FunctionVersion>> {
        let row = sqlx::query_as::<_, VersionRow>(&format!(
            "SELECT {COLUMNS} FROM fn_versions WHERE function_id = $1 AND version = $2"
        ))
        .bind(function_id)
        .bind(version)
        .fetch_optional(&self.pool)
        .await?;
        row.map(to_entity_or_corrupt).transpose()
    }

    /// Newest first. Scoped to one function, so a corrupt row fails it.
    pub async fn list_by_function(&self, function_id: &str) -> Result<Vec<FunctionVersion>> {
        let rows = sqlx::query_as::<_, VersionRow>(&format!(
            "SELECT {COLUMNS} FROM fn_versions WHERE function_id = $1 ORDER BY version DESC"
        ))
        .bind(function_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity_or_corrupt).collect()
    }

    /// The highest-numbered `PUBLISHED` or `READY` version: the `declared`
    /// candidate when no `?version=` is given. A live version is never
    /// retired, so this is at least the live version once one is set.
    pub async fn find_newest_non_retired(
        &self,
        function_id: &str,
    ) -> Result<Option<FunctionVersion>> {
        let row = sqlx::query_as::<_, VersionRow>(&format!(
            "SELECT {COLUMNS} FROM fn_versions WHERE function_id = $1 AND state <> 'RETIRED' \
             ORDER BY version DESC LIMIT 1"
        ))
        .bind(function_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(to_entity_or_corrupt).transpose()
    }

    /// Every version named by `ids`, by id. A missing id, and a row whose
    /// manifest cannot be read, are both simply absent.
    pub async fn find_by_ids(&self, ids: &[String]) -> Result<HashMap<String, FunctionVersion>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query_as::<_, VersionRow>(&format!(
            "SELECT {COLUMNS} FROM fn_versions WHERE id = ANY($1)"
        ))
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let id = row.id.clone();
                match to_entity(row) {
                    Ok(v) => Some((v.id.clone(), v)),
                    Err(cause) => {
                        tracing::warn!(version_id = %id, %cause, "skipping a corrupt function version");
                        None
                    }
                }
            })
            .collect())
    }
}

fn to_entity_or_corrupt(row: VersionRow) -> Result<FunctionVersion> {
    let id = row.id.clone();
    to_entity(row).map_err(|cause| {
        tracing::error!(version_id = %id, %cause, "corrupt function version");
        PlatformError::internal(format!("function version {id} is corrupt: {cause}"))
    })
}

/// Why a stored row cannot be read. Never carries manifest content.
fn to_entity(row: VersionRow) -> std::result::Result<FunctionVersion, String> {
    let json = JsonNode::parse(&row.manifest)
        .map_err(|_| "fn_versions.manifest is not valid JSON".to_string())?;
    let manifest = Manifest::read_stored(&json).map_err(|e| e.to_string())?;
    let state = match (row.state.as_str(), row.ready_at, row.retired_at) {
        ("PUBLISHED", _, _) => VersionState::Published,
        ("READY", Some(at), _) => VersionState::Ready(at),
        ("RETIRED", _, Some(at)) => VersionState::Retired(at),
        (other, _, _) => return Err(format!("unrecognised fn_versions.state: {other}")),
    };
    let digest = Digest::parse(&row.digest).map_err(|e| e.message().to_string())?;
    let signer = match (row.signer_issuer, row.signer_subject) {
        (Some(issuer), Some(subject)) => Some(SignerIdentity { issuer, subject }),
        _ => None,
    };
    Ok(FunctionVersion {
        id: row.id,
        function_id: row.function_id,
        version: row.version,
        artifact_ref: row.artifact_ref,
        digest,
        signature_bundle: row.signature_bundle,
        signature_bundle_ref: row.signature_bundle_ref,
        signer,
        manifest,
        state,
        published_by: row.published_by,
        published_at: row.published_at,
    })
}
