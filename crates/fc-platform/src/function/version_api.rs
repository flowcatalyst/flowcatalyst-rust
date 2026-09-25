//! Versions, aliases, the manifest check and artifact upload (Java
//! `function/api/FunctionApi.java`: `publish` :246-255, `checkManifest`
//! :272-301, `uploadArtifact` :319-375, `listVersions`/`getVersion`/`retire`
//! :395-423, `promote`/`removeAlias`/`listAliases` :434-468, and their DTOs
//! :931-1062).
//!
//! Publish, retire, the check and the upload are all gated by
//! `platform:function:version:publish`; promote and alias removal by
//! `platform:function:alias:promote`; the reads by
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
use super::operations::promote_plan::{
    Conflict, PoolAction, PromotePlan, PublicRoutesAction, RouteKey, ScheduleAction,
    SubscriptionAction, Wiring,
};
use super::operations::{PromoteCommand, PublishCommand, RemoveAliasCommand, RetireCommand};
use super::wire::{micros_opt_ser, micros_ser, parse_body};
use super::{ClientCeilings, Digest, JsonNode, Manifest};
use crate::permissions::function::{FUNCTION_PROMOTE, FUNCTION_PUBLISH, FUNCTION_VIEW};
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

/// `201` of a publish: `{id, version, state: "PUBLISHED", digest, signer?}`;
/// `200` with the existing version (its own state) when the same digest and
/// manifest were already published.
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
    /// An existing version, for a publish that was a no-op: its own state.
    fn existing(v: &FunctionVersion) -> PublishResponse {
        PublishResponse {
            id: v.id.clone(),
            version: v.version,
            state: v.state.name().to_string(),
            digest: v.digest.value().to_string(),
            signer: v.signer.as_ref().map(SignerResponse::of),
        }
    }

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

/// `200` of the manifest check. `plan` (what promoting the manifest, as the
/// next version, to `alias` would do) is present only when `valid`.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckManifestResponse {
    pub valid: bool,
    pub errors: Vec<ManifestErrorResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<PromotePlanResponse>,
}

/// A [`PromotePlan`] on the wire (Java `PromotePlanResponse`). For a named
/// alias `httpOnly` is true, `pool` and `publicRoutes` are absent and the
/// lists empty; `settingsMissing` and `conflicts` are always computed.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PromotePlanResponse {
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_version: Option<i32>,
    pub to_version: i32,
    pub settings_missing: Vec<String>,
    pub http_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<PoolActionResponse>,
    pub subscriptions: Vec<SubscriptionActionResponse>,
    pub schedules: Vec<ScheduleActionResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_routes: Option<PublicRoutesActionResponse>,
    pub conflicts: Vec<ConflictResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PoolActionResponse {
    /// `create`, `update` or `unchanged`.
    pub action: &'static str,
    pub key: String,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionActionResponse {
    /// `create`, `update`, `delete` or `unchanged`.
    pub action: &'static str,
    pub trigger_key: String,
    pub event_type: String,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleActionResponse {
    /// `create`, `update`, `delete` or `unchanged`.
    pub action: &'static str,
    pub trigger_key: String,
    pub cron: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicRoutesActionResponse {
    /// `replace` or `unchanged`.
    pub action: &'static str,
    pub added: Vec<RouteKeyResponse>,
    pub removed: Vec<RouteKeyResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouteKeyResponse {
    pub hostname: String,
    pub path_prefix: String,
    pub alias_prefixes: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConflictResponse {
    pub code: String,
    pub message: String,
}

impl PromotePlanResponse {
    pub fn of(plan: &PromotePlan) -> PromotePlanResponse {
        let conflicts = plan.conflicts.iter().map(ConflictResponse::of).collect();
        let base = |http_only| PromotePlanResponse {
            alias: plan.alias.clone(),
            from_version: plan.from_version,
            to_version: plan.to_version,
            settings_missing: plan.settings_missing.clone(),
            http_only,
            pool: None,
            subscriptions: Vec::new(),
            schedules: Vec::new(),
            public_routes: None,
            conflicts,
        };
        match &plan.wiring {
            Wiring::HttpOnly => base(true),
            Wiring::Live {
                pool,
                subscriptions,
                schedules,
                public_routes,
            } => PromotePlanResponse {
                pool: Some(PoolActionResponse::of(pool)),
                subscriptions: subscriptions
                    .iter()
                    .map(SubscriptionActionResponse::of)
                    .collect(),
                schedules: schedules.iter().map(ScheduleActionResponse::of).collect(),
                public_routes: Some(PublicRoutesActionResponse::of(public_routes)),
                ..base(false)
            },
        }
    }
}

impl PoolActionResponse {
    fn of(a: &PoolAction) -> PoolActionResponse {
        let (action, key, changed_fields) = match a {
            PoolAction::Create { key } => ("create", key, Vec::new()),
            PoolAction::Update {
                key,
                changed_fields,
            } => ("update", key, changed_fields.clone()),
            PoolAction::Unchanged { key } => ("unchanged", key, Vec::new()),
        };
        PoolActionResponse {
            action,
            key: key.clone(),
            changed_fields,
        }
    }
}

impl SubscriptionActionResponse {
    fn of(a: &SubscriptionAction) -> SubscriptionActionResponse {
        let (action, trigger_key, event_type, changed_fields) = match a {
            SubscriptionAction::Create {
                trigger_key,
                event_type,
            } => ("create", trigger_key, event_type, Vec::new()),
            SubscriptionAction::Update {
                trigger_key,
                event_type,
                changed_fields,
            } => ("update", trigger_key, event_type, changed_fields.clone()),
            SubscriptionAction::Delete {
                trigger_key,
                event_type,
            } => ("delete", trigger_key, event_type, Vec::new()),
            SubscriptionAction::Unchanged {
                trigger_key,
                event_type,
            } => ("unchanged", trigger_key, event_type, Vec::new()),
        };
        SubscriptionActionResponse {
            action,
            trigger_key: trigger_key.clone(),
            event_type: event_type.clone(),
            changed_fields,
        }
    }
}

impl ScheduleActionResponse {
    fn of(a: &ScheduleAction) -> ScheduleActionResponse {
        let (action, trigger_key, cron, timezone, changed_fields) = match a {
            ScheduleAction::Create {
                trigger_key,
                cron,
                timezone,
            } => ("create", trigger_key, cron, timezone, Vec::new()),
            ScheduleAction::Update {
                trigger_key,
                cron,
                timezone,
                changed_fields,
            } => (
                "update",
                trigger_key,
                cron,
                timezone,
                changed_fields.clone(),
            ),
            ScheduleAction::Delete {
                trigger_key,
                cron,
                timezone,
            } => ("delete", trigger_key, cron, timezone, Vec::new()),
            ScheduleAction::Unchanged {
                trigger_key,
                cron,
                timezone,
            } => ("unchanged", trigger_key, cron, timezone, Vec::new()),
        };
        ScheduleActionResponse {
            action,
            trigger_key: trigger_key.clone(),
            cron: cron.clone(),
            timezone: timezone.clone(),
            changed_fields,
        }
    }
}

impl PublicRoutesActionResponse {
    fn of(a: &PublicRoutesAction) -> PublicRoutesActionResponse {
        match a {
            PublicRoutesAction::Replace { added, removed } => PublicRoutesActionResponse {
                action: "replace",
                added: added.iter().map(RouteKeyResponse::of).collect(),
                removed: removed.iter().map(RouteKeyResponse::of).collect(),
            },
            PublicRoutesAction::Unchanged => PublicRoutesActionResponse {
                action: "unchanged",
                added: Vec::new(),
                removed: Vec::new(),
            },
        }
    }
}

impl RouteKeyResponse {
    fn of(k: &RouteKey) -> RouteKeyResponse {
        RouteKeyResponse {
            hostname: k.hostname.clone(),
            path_prefix: k.path_prefix.clone(),
            alias_prefixes: k.alias_prefixes.clone(),
        }
    }
}

impl ConflictResponse {
    fn of(c: &Conflict) -> ConflictResponse {
        ConflictResponse {
            code: c.code.clone(),
            message: c.message.clone(),
        }
    }
}

/// Body of `PUT …/aliases/{alias}`. An absent `version` is 0, which names
/// no version (Java's `int` record component). `expectedVersion`, when
/// given, is the version the caller expects the alias to point at now (`0`:
/// none yet); a mismatch is `412 ALIAS_VERSION_CONFLICT`. The `If-Match`
/// header carries the same precondition.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PromoteRequest {
    #[serde(default)]
    pub version: i32,
    #[serde(default, rename = "expectedVersion")]
    pub expected_version: Option<i32>,
}

/// The promote precondition from the body's `expectedVersion` and the
/// `If-Match` header (`3`, `"3"` or `W/"3"`): either, or both when they
/// agree. `400 IF_MATCH_INVALID` for a header that is not a version number,
/// `400 EXPECTED_VERSION_CONFLICT` when the two disagree.
fn promote_precondition(
    body: Option<i32>,
    headers: &HeaderMap,
) -> Result<Option<i32>, PlatformError> {
    let header = match headers.get(header::IF_MATCH) {
        None => None,
        Some(raw) => {
            let text = raw.to_str().unwrap_or("").trim();
            let text = text.strip_prefix("W/").unwrap_or(text);
            let text = text
                .strip_prefix('"')
                .and_then(|t| t.strip_suffix('"'))
                .unwrap_or(text);
            match text.parse::<i32>() {
                Ok(n) if n >= 0 => Some(n),
                _ => {
                    return Err(UseCaseError::validation(
                        "IF_MATCH_INVALID",
                        "If-Match must be the version number the alias points at (0 for none)",
                    )
                    .into())
                }
            }
        }
    };
    match (body, header) {
        (Some(b), Some(h)) if b != h => Err(UseCaseError::validation(
            "EXPECTED_VERSION_CONFLICT",
            format!("expectedVersion {b} and If-Match {h} disagree"),
        )
        .into()),
        (Some(b), _) => Ok(Some(b)),
        (None, h) => Ok(h),
    }
}

/// `200` of a promote: `previousVersion` is the prior target's number,
/// absent on a first promotion. `changed` is false when the alias already
/// named the version (a no-op: nothing written, `previousVersion` is the
/// version itself).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PromoteResponse {
    pub alias: String,
    pub version: i32,
    pub version_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<i32>,
    pub changed: bool,
}

/// One alias.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AliasResponse {
    pub alias: String,
    pub version: i32,
    pub version_id: String,
    pub updated_by: String,
    #[serde(serialize_with = "micros_ser")]
    pub updated_at: DateTime<Utc>,
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
        (status = 200, body = PublishResponse, description = "The same digest and manifest are already published: that version, nothing written"),
        (status = 400, description = "ARTIFACT_REF_REQUIRED, ARTIFACT_REF_INVALID, DIGEST_INVALID, a manifest code, SIGNATURE_REQUIRED, SIGNATURE_REJECTED or a publish check"),
        (status = 403, description = "PERMISSION_REQUIRED or SIGNER_NOT_PERMITTED"),
        (status = 404, description = "Function_NOT_FOUND, also when out of reach"),
        (status = 409, description = "FUNCTION_DISABLED, VERSION_DIGEST_EXISTS (the digest under another manifest) or PUBLIC_ROUTE_TAKEN"),
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
        address: address.clone(),
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
    let reach = caller.clone();
    let outcome = state
        .ops
        .unit_of_work
        .run(move |scoped| async move { ops.publish_in(caller, scoped).run(command, ctx).await })
        .await
        .into_result();
    match outcome {
        Ok(event) => Ok((StatusCode::CREATED, Json(PublishResponse::of(&event)))),
        Err(e) if e.is_unchanged() => {
            let number = e.details()["version"]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or_else(|| PlatformError::from(e.clone()))?;
            let f = reachable_function(&state, &address, &reach).await?;
            let v = version_or_not_found(&state, &f, number).await?;
            Ok((StatusCode::OK, Json(PublishResponse::existing(&v))))
        }
        Err(e) => Err(e.into()),
    }
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
    let mut plan_response = None;
    match Manifest::check(req.manifest.as_ref(), f.runtime, &state.limits, &ceilings) {
        Err(rejected) => {
            for problem in rejected.problems() {
                errors.push(ManifestErrorResponse::of(
                    &problem.to_validation_error().into(),
                ));
            }
        }
        Ok(manifest) => {
            let problems = state
                .ops
                .publish_checks
                .check(&f, &manifest, &caller)
                .await?;
            errors.extend(problems.iter().map(ManifestErrorResponse::of));
            if errors.is_empty() {
                // A plain max-plus-one read: nothing is reserved, so it may
                // go stale under a concurrent publish, which a dry run allows.
                let next = state.versions.next_version_preview(&f.id).await?;
                let alias = req
                    .alias
                    .as_deref()
                    .filter(|a| !crate::function::java_is_blank(a))
                    .unwrap_or(crate::function::LIVE_ALIAS);
                let plan = state
                    .ops
                    .trigger_sync
                    .plan(&f, &manifest, next, alias, &caller)
                    .await?;
                plan_response = Some(PromotePlanResponse::of(&plan));
            }
        }
    }
    Ok(Json(CheckManifestResponse {
        valid: errors.is_empty(),
        errors,
        plan: plan_response,
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
        (status = 409, description = "VERSION_IS_LIVE or VERSION_ALIASED. Retiring a retired version is a no-op: 200 with it, nothing written"),
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
    match state
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
        .into_result()
    {
        // Already retired: a no-op, answered with the version as it is.
        Err(e) if !e.is_unchanged() => return Err(e.into()),
        _ => {}
    }
    let f = reachable_function(&state, &address, &caller).await?;
    let v = version_or_not_found(&state, &f, number).await?;
    Ok(Json(VersionResponse::summary(&v, f.is_live(&v.id))))
}

/// Point an alias at a version. For `live` this also reconciles the
/// function's wiring to that version's manifest, in the same transaction.
#[utoipa::path(
    put, path = "/api/functions/{address}/aliases/{alias}", tag = "functions",
    operation_id = "putApiFunctionsByAddressAliasesByAlias",
    params(
        ("address" = String, Path, description = "app.service.name"),
        ("alias" = String, Path, description = "`live` or a named alias"),
    ),
    request_body = PromoteRequest,
    responses(
        (status = 200, body = PromoteResponse),
        (status = 400, description = "ALIAS_INVALID, ADDRESS_INVALID or INVALID_JSON"),
        (status = 403),
        (status = 404, description = "Function_NOT_FOUND or FunctionVersion_NOT_FOUND"),
        (status = 409, description = "VERSION_NOT_READY, SETTINGS_MISSING, VERSION_RETIRED, FUNCTION_DISABLED or PUBLIC_ROUTE_TAKEN. An alias already naming the version is a no-op: 200 with changed false"),
        (status = 412, description = "ALIAS_VERSION_CONFLICT: expectedVersion / If-Match no longer names the alias's version"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn promote(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path((address, alias)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<PromoteResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_PROMOTE)?;
    let address = address_from_path(&address)?;
    let req: PromoteRequest = parse_body(&body)?;
    let expected_version = promote_precondition(req.expected_version, &headers)?;
    let command = PromoteCommand {
        address: address.clone(),
        alias: alias.clone(),
        version: req.version,
        expected_version,
    };
    let caller = state.caller(&auth.0).await?;
    let reach = caller.clone();
    let ctx = ExecutionContext::from_auth(&auth.0);
    // One transaction for the alias change and the wiring, as Java's
    // TxOperation.
    let ops = state.ops.clone();
    let outcome = state
        .ops
        .unit_of_work
        .run(move |scoped| async move { ops.promote_in(caller, scoped).run(command, ctx).await })
        .await
        .into_result();
    let event = match outcome {
        Ok(event) => event,
        // The alias already names this version: nothing written.
        Err(e) if e.is_unchanged() => {
            let f = reachable_function(&state, &address, &reach).await?;
            let v = version_or_not_found(&state, &f, req.version).await?;
            return Ok(Json(PromoteResponse {
                alias,
                version: v.version,
                version_id: v.id,
                previous_version: Some(v.version),
                changed: false,
            }));
        }
        Err(e) => return Err(e.into()),
    };
    let previous_version = match &event.previous_version_id {
        Some(id) => state.versions.find_by_id(id).await?.map(|v| v.version),
        None => None,
    };
    Ok(Json(PromoteResponse {
        alias: event.alias,
        version: event.version,
        version_id: event.version_id,
        previous_version,
        changed: true,
    }))
}

/// Remove a named alias; `live` cannot be removed.
#[utoipa::path(
    delete, path = "/api/functions/{address}/aliases/{alias}", tag = "functions",
    operation_id = "deleteApiFunctionsByAddressAliasesByAlias",
    params(
        ("address" = String, Path, description = "app.service.name"),
        ("alias" = String, Path, description = "A named alias"),
    ),
    responses(
        (status = 204),
        (status = 400, description = "ADDRESS_INVALID"),
        (status = 403),
        (status = 404, description = "Function_NOT_FOUND or Alias_NOT_FOUND"),
        (status = 409, description = "ALIAS_PROTECTED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn remove_alias(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path((address, alias)): Path<(String, String)>,
) -> Result<StatusCode, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_PROMOTE)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    state
        .ops
        .remove_alias(caller)
        .run(
            RemoveAliasCommand { address, alias },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// A function's aliases and the versions they point at.
#[utoipa::path(
    get, path = "/api/functions/{address}/aliases", tag = "functions",
    operation_id = "getApiFunctionsByAddressAliases",
    params(("address" = String, Path, description = "app.service.name")),
    responses((status = 200, body = Vec<AliasResponse>), (status = 400), (status = 403), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn list_aliases(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
) -> Result<Json<Vec<AliasResponse>>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let f = reachable_function(&state, &address, &caller).await?;
    let ids: Vec<String> = f.aliases.iter().map(|a| a.version_id.clone()).collect();
    let versions = state.versions.find_by_ids(&ids).await?;
    Ok(Json(
        f.aliases
            .iter()
            .filter_map(|a| {
                versions.get(&a.version_id).map(|v| AliasResponse {
                    alias: a.alias.clone(),
                    version: v.version,
                    version_id: a.version_id.clone(),
                    updated_by: a.updated_by.clone(),
                    updated_at: a.updated_at,
                })
            })
            .collect(),
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn if_match(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn the_promote_precondition_comes_from_the_body_or_if_match() {
        let none = HeaderMap::new();
        assert_eq!(promote_precondition(None, &none).unwrap(), None);
        assert_eq!(promote_precondition(Some(3), &none).unwrap(), Some(3));
        for value in ["3", "\"3\"", "W/\"3\"", " 3 "] {
            assert_eq!(
                promote_precondition(None, &if_match(value)).unwrap(),
                Some(3),
                "{value}"
            );
        }
        assert_eq!(
            promote_precondition(Some(3), &if_match("\"3\"")).unwrap(),
            Some(3)
        );
        assert_eq!(promote_precondition(None, &if_match("0")).unwrap(), Some(0));
    }

    #[test]
    fn a_bad_or_disagreeing_if_match_is_a_400() {
        let code = |r: Result<Option<i32>, PlatformError>| match r.unwrap_err() {
            PlatformError::Coded { status, code, .. } => (status.as_u16(), code),
            other => panic!("{other:?}"),
        };
        for value in ["*", "latest", "-1", "\"\""] {
            assert_eq!(
                code(promote_precondition(None, &if_match(value))),
                (400, "IF_MATCH_INVALID".to_string()),
                "{value}"
            );
        }
        assert_eq!(
            code(promote_precondition(Some(2), &if_match("3"))),
            (400, "EXPECTED_VERSION_CONFLICT".to_string())
        );
    }
}
