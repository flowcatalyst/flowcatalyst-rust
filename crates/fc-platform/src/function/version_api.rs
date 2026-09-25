//! Versions, the manifest check and artifact upload (Java
//! `function/api/FunctionApi.java`: `publish` :246-255, `checkManifest`
//! :272-301, `uploadArtifact` :319-375, `listVersions`/`getVersion`/`retire`
//! :395-423, and their DTOs).
//!
//! Publish, retire, the check and the upload are all gated by
//! `platform:function:version:publish`; the version reads by
//! `platform:function:function:view`. A function out of reach is
//! `404 Function_NOT_FOUND` everywhere, never a 403.

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::api::{address_from_path, parse_version_number, reachable_function, FunctionsState};
use super::artifact::{self, PlatformArtifactRef};
use super::entity::{FunctionVersion, SignerIdentity};
use super::operations::events::VersionPublished;
use super::operations::{PublishCommand, RetireCommand};
use super::wire::{micros_opt_ser, micros_ser, parse_body};
use super::{ClientCeilings, Digest, JsonNode, Manifest};
use crate::permissions::function::{FUNCTION_PUBLISH, FUNCTION_VIEW};
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UseCase, UseCaseError};

// ── DTOs ────────────────────────────────────────────────────────────────────

/// Body of `POST /api/functions/{address}/versions`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublishRequest {
    /// `oci://`, `file://`, `s3://` or `platform://`.
    pub artifact_ref: Option<String>,
    /// `sha256:<64 hex>`.
    pub digest: Option<String>,
    /// A Sigstore bundle v0.3, as JSON text.
    pub signature_bundle: Option<String>,
    #[schema(value_type = Object)]
    pub manifest: Option<JsonNode>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SignerResponse {
    pub issuer: String,
    pub subject: String,
}

impl SignerResponse {
    fn of(signer: &SignerIdentity) -> SignerResponse {
        SignerResponse {
            issuer: signer.issuer.clone(),
            subject: signer.subject.clone(),
        }
    }
}

/// `201` of a publish: `{id, version, state: "PUBLISHED", digest, signer?}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublishResponse {
    pub id: String,
    pub version: i32,
    pub state: String,
    pub digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<SignerResponse>,
}

impl PublishResponse {
    fn of(event: &VersionPublished) -> PublishResponse {
        PublishResponse {
            id: event.version_id.clone(),
            version: event.version,
            state: "PUBLISHED".to_string(),
            digest: event.digest.clone(),
            signer: match (&event.signer_issuer, &event.signer_subject) {
                (Some(issuer), Some(subject)) => Some(SignerResponse {
                    issuer: issuer.clone(),
                    subject: subject.clone(),
                }),
                _ => None,
            },
        }
    }
}

/// `200` of an upload.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadArtifactResponse {
    /// `platform://<functionId>/<hex>`, what a publish refers to.
    pub artifact_ref: String,
    pub digest: String,
    pub bytes: u64,
}

/// One version. The list leaves `manifest` out; the single read adds it,
/// normalised. `live` is whether this version is the function's `live`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionResponse {
    pub id: String,
    pub version: i32,
    /// `PUBLISHED`, `READY` or `RETIRED`.
    pub state: String,
    pub digest: String,
    pub artifact_ref: String,
    pub pool: String,
    pub warm: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<SignerResponse>,
    pub published_by: String,
    #[serde(serialize_with = "micros_ser")]
    pub published_at: DateTime<Utc>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "micros_opt_ser"
    )]
    pub ready_at: Option<DateTime<Utc>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "micros_opt_ser"
    )]
    pub retired_at: Option<DateTime<Utc>>,
    pub live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub manifest: Option<JsonNode>,
}

impl VersionResponse {
    fn of(v: &FunctionVersion, live: bool, manifest: Option<JsonNode>) -> VersionResponse {
        VersionResponse {
            id: v.id.clone(),
            version: v.version,
            state: v.state.name().to_string(),
            digest: v.digest.value().to_string(),
            artifact_ref: v.artifact_ref.clone(),
            pool: v.manifest.pool.value().to_string(),
            warm: v.manifest.warm,
            signer: v.signer.as_ref().map(SignerResponse::of),
            published_by: v.published_by.clone(),
            published_at: v.published_at,
            ready_at: v.ready_at(),
            retired_at: v.retired_at(),
            live,
            manifest,
        }
    }

    fn summary(v: &FunctionVersion, live: bool) -> VersionResponse {
        Self::of(v, live, None)
    }

    fn detail(v: &FunctionVersion, live: bool) -> VersionResponse {
        Self::of(v, live, Some(v.manifest.to_json()))
    }
}

/// Body of `POST …/manifest/check`. `alias` absent or blank means `live`;
/// it only shapes the promote plan.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct CheckManifestRequest {
    #[schema(value_type = Object)]
    pub manifest: Option<JsonNode>,
    pub alias: Option<String>,
}

/// One problem: a manifest problem (`details.pointer`) or a publish check
/// (its own details, often none).
#[derive(Debug, Serialize, ToSchema)]
pub struct ManifestErrorResponse {
    pub code: String,
    pub message: String,
    #[schema(value_type = Object)]
    pub details: std::collections::HashMap<String, serde_json::Value>,
}

impl ManifestErrorResponse {
    fn of(e: &UseCaseError) -> ManifestErrorResponse {
        ManifestErrorResponse {
            code: e.code().to_string(),
            message: e.message().to_string(),
            details: e.details().clone(),
        }
    }
}

/// `200` of the manifest check. `plan` (what promoting would do) is absent
/// until promote wiring lands (P5).
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckManifestResponse {
    pub valid: bool,
    pub errors: Vec<ManifestErrorResponse>,
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// Publish a new version.
#[utoipa::path(
    post, path = "/api/functions/{address}/versions", tag = "functions",
    operation_id = "postApiFunctionsByAddressVersions",
    params(("address" = String, Path, description = "app.service.name")),
    request_body = PublishRequest,
    responses(
        (status = 201, body = PublishResponse),
        (status = 400, description = "ARTIFACT_REF_REQUIRED, ARTIFACT_REF_INVALID, DIGEST_INVALID, a manifest code, SIGNATURE_REQUIRED, SIGNATURE_REJECTED or a publish check"),
        (status = 403, description = "PERMISSION_REQUIRED or SIGNER_NOT_PERMITTED"),
        (status = 404, description = "Function_NOT_FOUND, also when out of reach"),
        (status = 409, description = "FUNCTION_DISABLED, VERSION_DIGEST_EXISTS or PUBLIC_ROUTE_TAKEN"),
        (status = 422, description = "ARTIFACT_REF_MISMATCH or ARTIFACT_NOT_UPLOADED"),
        (status = 503, description = "ARTIFACT_STORE_NOT_CONFIGURED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn publish_version(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<PublishResponse>), PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_PUBLISH)?;
    let address = address_from_path(&address)?;
    let req: PublishRequest = parse_body(&body)?;
    let command = PublishCommand {
        address,
        artifact_ref: req.artifact_ref,
        digest: req.digest,
        signature_bundle: req.signature_bundle,
        manifest: req.manifest,
    };
    let caller = state.caller(&auth.0).await?;
    let ctx = ExecutionContext::from_auth(&auth.0);
    // One transaction for the version number's row lock and the commit, as
    // Java's TxOperation.
    let ops = state.ops.clone();
    let event = state
        .ops
        .unit_of_work
        .run(move |scoped| async move { ops.publish_in(caller, scoped).run(command, ctx).await })
        .await
        .into_result()?;
    Ok((StatusCode::CREATED, Json(PublishResponse::of(&event))))
}

/// Validate a manifest as a publish would, writing nothing.
#[utoipa::path(
    post, path = "/api/functions/{address}/manifest/check", tag = "functions",
    operation_id = "postApiFunctionsByAddressManifestCheck",
    params(("address" = String, Path, description = "app.service.name")),
    request_body = CheckManifestRequest,
    responses(
        (status = 200, body = CheckManifestResponse),
        (status = 400, description = "ADDRESS_INVALID or INVALID_JSON"),
        (status = 403), (status = 404),
    ),
    security(("bearer_auth" = []))
)]
pub async fn check_manifest(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
    body: Bytes,
) -> Result<Json<CheckManifestResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_PUBLISH)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let f = reachable_function(&state, &address, &caller).await?;
    let req: CheckManifestRequest = parse_body(&body)?;

    let policy = state.policies.find_by_owner(&f.owner).await?;
    let ceilings = match &policy {
        Some(p) => p.ceilings(&state.limits).map_err(|e| {
            PlatformError::from(UseCaseError::internal("CEILINGS_INVALID", e.to_string()))
        })?,
        None => ClientCeilings::of(&state.limits),
    };
    let mut errors = Vec::new();
    match Manifest::check(req.manifest.as_ref(), f.runtime, &state.limits, &ceilings) {
        Err(rejected) => {
            for problem in rejected.problems() {
                errors.push(ManifestErrorResponse::of(&problem.to_use_case_error()));
            }
        }
        Ok(manifest) => {
            let problems = state
                .ops
                .publish_checks
                .check(&f, &manifest, &caller)
                .await?;
            errors.extend(problems.iter().map(ManifestErrorResponse::of));
            // TODO(P5): with no errors, add `plan`: TriggerSync.plan for
            // promoting this manifest, as the next version, to `alias`.
        }
    }
    Ok(Json(CheckManifestResponse {
        valid: errors.is_empty(),
        errors,
    }))
}

/// A function's versions, newest first.
#[utoipa::path(
    get, path = "/api/functions/{address}/versions", tag = "functions",
    operation_id = "getApiFunctionsByAddressVersions",
    params(("address" = String, Path, description = "app.service.name")),
    responses((status = 200, body = Vec<VersionResponse>), (status = 400), (status = 403), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn list_versions(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
) -> Result<Json<Vec<VersionResponse>>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let f = reachable_function(&state, &address, &caller).await?;
    let versions = state.versions.list_by_function(&f.id).await?;
    Ok(Json(
        versions
            .iter()
            .map(|v| VersionResponse::summary(v, f.is_live(&v.id)))
            .collect(),
    ))
}

/// One version, with its normalised manifest.
#[utoipa::path(
    get, path = "/api/functions/{address}/versions/{version}", tag = "functions",
    operation_id = "getApiFunctionsByAddressVersionsByVersion",
    params(
        ("address" = String, Path, description = "app.service.name"),
        ("version" = i32, Path, description = "A positive version number"),
    ),
    responses(
        (status = 200, body = VersionResponse),
        (status = 400, description = "ADDRESS_INVALID or VERSION_INVALID"),
        (status = 403),
        (status = 404, description = "Function_NOT_FOUND or FunctionVersion_NOT_FOUND"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_version(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path((address, version)): Path<(String, String)>,
) -> Result<Json<VersionResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let f = reachable_function(&state, &address, &caller).await?;
    let number = parse_version_number(&version)?;
    let v = version_or_not_found(&state, &f, number).await?;
    Ok(Json(VersionResponse::detail(&v, f.is_live(&v.id))))
}

async fn version_or_not_found(
    state: &FunctionsState,
    f: &super::entity::Function,
    number: i32,
) -> Result<FunctionVersion, PlatformError> {
    state
        .versions
        .find_by_function_and_version(&f.id, number)
        .await?
        .ok_or_else(|| {
            super::operations::access::resource_not_found(
                "FunctionVersion",
                &format!("{}#{number}", f.address.render()),
            )
            .into()
        })
}

/// Retire a version.
#[utoipa::path(
    post, path = "/api/functions/{address}/versions/{version}/retire", tag = "functions",
    operation_id = "postApiFunctionsByAddressVersionsByVersionRetire",
    params(
        ("address" = String, Path, description = "app.service.name"),
        ("version" = i32, Path, description = "A positive version number"),
    ),
    responses(
        (status = 200, body = VersionResponse),
        (status = 400, description = "ADDRESS_INVALID or VERSION_INVALID"),
        (status = 403),
        (status = 404, description = "Function_NOT_FOUND or FunctionVersion_NOT_FOUND"),
        (status = 409, description = "VERSION_IS_LIVE, VERSION_ALIASED or VERSION_ALREADY_RETIRED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn retire_version(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path((address, version)): Path<(String, String)>,
) -> Result<Json<VersionResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_PUBLISH)?;
    let address = address_from_path(&address)?;
    let number = parse_version_number(&version)?;
    let caller = state.caller(&auth.0).await?;
    state
        .ops
        .retire(caller.clone())
        .run(
            RetireCommand {
                address: address.clone(),
                version: number,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    let f = reachable_function(&state, &address, &caller).await?;
    let v = version_or_not_found(&state, &f, number).await?;
    Ok(Json(VersionResponse::summary(&v, f.is_live(&v.id))))
}

/// Upload an artifact: the raw body, streamed to a temp file and hashed as
/// it is written, never buffered. Checked in Java's order: a store (503),
/// the permission, reach (404), the digest's shape, the declared length
/// (413), each before a byte is read; then the running count (413), an
/// empty body (422) and the digest (422). Idempotent: an upload of a digest
/// already stored is a 200 with the same body, the bytes still read and
/// hashed. Infrastructure: no event and no audit row (see `artifact`).
#[utoipa::path(
    put, path = "/api/functions/{address}/artifacts/{digest}", tag = "functions",
    operation_id = "putApiFunctionsByAddressArtifactsByDigest",
    params(
        ("address" = String, Path, description = "app.service.name"),
        ("digest" = String, Path, description = "sha256:<64 hex>, of the body"),
    ),
    request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    responses(
        (status = 200, body = UploadArtifactResponse),
        (status = 400, description = "ADDRESS_INVALID or DIGEST_INVALID"),
        (status = 403), (status = 404),
        (status = 413, description = "ARTIFACT_TOO_LARGE (256 MiB)"),
        (status = 422, description = "ARTIFACT_EMPTY or DIGEST_MISMATCH"),
        (status = 503, description = "ARTIFACT_STORE_NOT_CONFIGURED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn upload_artifact(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path((address, digest)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<UploadArtifactResponse>, PlatformError> {
    let store = state
        .ops
        .artifacts
        .clone()
        .ok_or_else(artifact::store_not_configured)?;
    checks::require_permission(&auth.0, FUNCTION_PUBLISH)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let f = reachable_function(&state, &address, &caller).await?;
    let digest = Digest::parse(&digest)?;
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let received =
        artifact::upload::receive(&*store, &f.id, &digest, declared, body.into_data_stream())
            .await?;
    Ok(Json(UploadArtifactResponse {
        artifact_ref: PlatformArtifactRef::of(&f.id, &digest).render(),
        digest: digest.value().to_string(),
        bytes: received.bytes,
    }))
}
