//! `fn_versions` (Java `function/FunctionVersionRepository.java`). A
//! version's content is write-once: [`Persist`]'s upsert changes only
//! `state`, `ready_at` and `retired_at` on conflict. Versions are retired,
//! never deleted.
//!
//! **The version number** is `max(version) + 1`, reserved under a
//! `SELECT … FOR UPDATE` on the function row in the publish transaction
//! ([`NextVersionOf`], a [`LockedRead`]), so two concurrent publishes of one
//! function serialise to `n` and `n + 1` (Java `nextVersion`).
//!
//! As in Java, a single-row read of a version whose stored manifest cannot
//! be read fails loudly (a corrupt row), while the batch read
//! [`FunctionVersionRepository::find_by_ids`] leaves such a row out, so one
//! corrupt version never fails a list of other functions.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{FunctionVersion, SignerIdentity, VersionState};
use super::{Digest, DnsLabel, JsonNode, Manifest, LIVE_ALIAS};
use crate::shared::error::{PlatformError, Result};
use crate::usecase::{DbTx, LockedRead, Persist};

/// The next version number of a function, read under its row lock.
#[derive(Debug, Clone)]
pub struct NextVersionOf(pub String);

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

    /// The version of `function_id` that has this digest, if any.
    pub async fn find_by_function_and_digest(
        &self,
        function_id: &str,
        digest: &Digest,
    ) -> Result<Option<FunctionVersion>> {
        let row = sqlx::query_as::<_, VersionRow>(&format!(
            "SELECT {COLUMNS} FROM fn_versions WHERE function_id = $1 AND digest = $2"
        ))
        .bind(function_id)
        .bind(digest.value())
        .fetch_optional(&self.pool)
        .await?;
        row.map(to_entity_or_corrupt).transpose()
    }

    /// `max(version) + 1`, or 1 for the first: a plain read that reserves
    /// nothing, and may be stale under a concurrent publish (the manifest
    /// check's preview; publish uses [`NextVersionOf`]).
    pub async fn next_version_preview(&self, function_id: &str) -> Result<i32> {
        let (next,): (i32,) = sqlx::query_as(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM fn_versions WHERE function_id = $1",
        )
        .bind(function_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(next)
    }

    /// Java `countLiveWarmInPool`: how many functions other than
    /// `excluding_function_id` have a `live` version that is warm and runs
    /// in `pool`. The function being published is excluded because its own
    /// live version is about to be replaced. A live version whose manifest
    /// cannot be read is skipped with an ERROR, never failing every publish:
    /// undercounting a soft cap by one is the safe direction.
    pub async fn count_live_warm_in_pool(
        &self,
        pool: &DnsLabel,
        excluding_function_id: &str,
    ) -> Result<i32> {
        // A stored manifest is normalised, so `warm` is a JSON boolean; the
        // pool is compared on the decoded manifest, as Java does.
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT v.id, v.function_id, v.manifest::text FROM fn_versions v \
             JOIN fn_aliases a ON a.version_id = v.id \
             WHERE a.alias = $1 AND v.function_id <> $2 AND v.manifest -> 'warm' = 'true'::jsonb",
        )
        .bind(LIVE_ALIAS)
        .bind(excluding_function_id)
        .fetch_all(&self.pool)
        .await?;
        let mut count = 0;
        for (version_id, function_id, text) in rows {
            let manifest = JsonNode::parse(&text)
                .map_err(|_| "fn_versions.manifest is not valid JSON".to_string())
                .and_then(|json| Manifest::read_stored(&json).map_err(|e| e.to_string()));
            match manifest {
                Ok(m) if m.warm && &m.pool == pool => count += 1,
                Ok(_) => {}
                Err(_) => tracing::error!(
                    %function_id,
                    %version_id,
                    "fn_versions row has an unreadable manifest; excluded from the warm-capacity count"
                ),
            }
        }
        Ok(count)
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

#[async_trait]
impl LockedRead<NextVersionOf> for FunctionVersionRepository {
    type Output = i32;

    /// Java `nextVersion`: locks the function row, then `max + 1`; `404
    /// Function_NOT_FOUND` when the function is gone.
    async fn read_locked(&self, query: &NextVersionOf, tx: &mut DbTx<'_>) -> Result<i32> {
        let locked: Option<(String,)> =
            sqlx::query_as("SELECT id FROM fn_functions WHERE id = $1 FOR UPDATE")
                .bind(&query.0)
                .fetch_optional(&mut **tx.inner)
                .await?;
        if locked.is_none() {
            return Err(PlatformError::Coded {
                status: axum::http::StatusCode::NOT_FOUND,
                code: "Function_NOT_FOUND".to_string(),
                message: format!("Function not found: {}", query.0),
                details: Default::default(),
            });
        }
        let (next,): (i32,) = sqlx::query_as(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM fn_versions WHERE function_id = $1",
        )
        .bind(&query.0)
        .fetch_one(&mut **tx.inner)
        .await?;
        Ok(next)
    }
}

#[async_trait]
impl Persist<FunctionVersion> for FunctionVersionRepository {
    /// An upsert whose conflict side sets only the state columns: a
    /// version's content is write-once. `ready_at` and `retired_at` are
    /// written only when the new state carries them, so a retired version
    /// keeps the `ready_at` it had.
    async fn persist(&self, v: &FunctionVersion, tx: &mut DbTx<'_>) -> Result<()> {
        let (issuer, subject) = match &v.signer {
            Some(s) => (Some(s.issuer.as_str()), Some(s.subject.as_str())),
            None => (None, None),
        };
        sqlx::query(
            "INSERT INTO fn_versions \
                (id, function_id, version, artifact_ref, digest, signature_bundle, \
                 signature_bundle_ref, signer_issuer, signer_subject, manifest, state, \
                 published_by, published_at, ready_at, retired_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb, $11, $12, $13, $14, $15) \
             ON CONFLICT (id) DO UPDATE SET \
                state = EXCLUDED.state, \
                ready_at = CASE WHEN EXCLUDED.state = 'READY' THEN EXCLUDED.ready_at \
                                ELSE fn_versions.ready_at END, \
                retired_at = CASE WHEN EXCLUDED.state = 'RETIRED' THEN EXCLUDED.retired_at \
                                  ELSE fn_versions.retired_at END",
        )
        .bind(&v.id)
        .bind(&v.function_id)
        .bind(v.version)
        .bind(&v.artifact_ref)
        .bind(v.digest.value())
        .bind(&v.signature_bundle)
        .bind(&v.signature_bundle_ref)
        .bind(issuer)
        .bind(subject)
        .bind(v.manifest.to_json().to_json_string())
        .bind(v.state.name())
        .bind(&v.published_by)
        .bind(v.published_at)
        .bind(v.ready_at())
        .bind(v.retired_at())
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    /// Versions are retired, never deleted (Java throws here too).
    async fn delete(&self, v: &FunctionVersion, _tx: &mut DbTx<'_>) -> Result<()> {
        Err(PlatformError::internal(format!(
            "function versions are retired, never deleted: {}",
            v.id
        )))
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
