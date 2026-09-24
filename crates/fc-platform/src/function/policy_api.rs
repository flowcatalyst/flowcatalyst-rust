//! `/api/function-policies…` (Java `function/api/FunctionPolicyApi.java`).
//! `{owner}` is a client id or the literal `platform`. Every route is gated
//! by anchor scope and `platform:function:policy:manage`, in that order;
//! there is no per-resource reach.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::api::FunctionsState;
use super::entity::ClientPolicy;
use super::operations::{PutPolicyCommand, SignerInput};
use super::wire::{micros_opt_ser, parse_body};
use super::{java_is_blank, ClientCeilings, FunctionLimits, FunctionOwner};
use crate::permissions::function::FUNCTION_POLICY_MANAGE;
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UseCase, UseCaseError};

fn owner_from_path(raw: &str) -> Result<FunctionOwner, PlatformError> {
    if java_is_blank(raw) {
        return Err(UseCaseError::validation("OWNER_REQUIRED", "owner is required").into());
    }
    FunctionOwner::from_wire(raw)
        .map_err(|_| UseCaseError::validation("OWNER_REQUIRED", "owner is required").into())
}

// ── DTOs ────────────────────────────────────────────────────────────────────

/// Body of `PUT /api/function-policies/{owner}`: a full replacement.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PutPolicyRequest {
    pub signers: Option<Vec<SignerRequest>>,
    pub ceilings: Option<CeilingsRequest>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct SignerRequest {
    pub issuer: Option<String>,
    pub subject: Option<String>,
    /// `jvm` / `wasm`, in any case.
    pub runtimes: Option<Vec<String>>,
}

/// An absent ceiling means the platform default applies.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CeilingsRequest {
    pub max_duration_ms: Option<i32>,
    pub max_concurrency: Option<i32>,
    pub max_wasm_memory_mb: Option<i32>,
    pub max_db_pool_size: Option<i32>,
}

impl PutPolicyRequest {
    fn into_command(self, owner: FunctionOwner) -> PutPolicyCommand {
        let ceilings = self.ceilings.unwrap_or_default();
        PutPolicyCommand {
            owner,
            signers: self
                .signers
                .unwrap_or_default()
                .into_iter()
                .map(|s| SignerInput {
                    issuer: s.issuer,
                    subject: s.subject,
                    runtimes: s.runtimes.unwrap_or_default(),
                })
                .collect(),
            max_duration_ms: ceilings.max_duration_ms,
            max_concurrency: ceilings.max_concurrency,
            max_wasm_memory_mb: ceilings.max_wasm_memory_mb,
            max_db_pool_size: ceilings.max_db_pool_size,
        }
    }
}

/// `{owner, signers, ceilings, stored, updatedAt?}`. `ceilings` are always
/// the effective values; `updatedAt` is absent on the no-row default.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PolicyResponse {
    pub owner: String,
    pub signers: Vec<SignerResponse>,
    pub ceilings: CeilingsResponse,
    pub stored: bool,
    #[serde(
        serialize_with = "micros_opt_ser",
        skip_serializing_if = "Option::is_none"
    )]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SignerResponse {
    pub issuer: String,
    pub subject: String,
    /// `jvm` / `wasm`.
    pub runtimes: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CeilingsResponse {
    pub max_duration_ms: i32,
    pub max_concurrency: i32,
    pub max_wasm_memory_mb: i32,
    pub max_db_pool_size: i32,
}

impl From<ClientCeilings> for CeilingsResponse {
    fn from(c: ClientCeilings) -> Self {
        Self {
            max_duration_ms: c.max_duration_ms(),
            max_concurrency: c.max_concurrency(),
            max_wasm_memory_mb: c.wasm_memory_mb(),
            max_db_pool_size: c.db_pool_size(),
        }
    }
}

impl PolicyResponse {
    fn from(p: &ClientPolicy, defaults: &FunctionLimits) -> Result<Self, PlatformError> {
        let ceilings = p
            .ceilings(defaults)
            .map_err(|e| PlatformError::internal(format!("stored policy ceiling: {e}")))?;
        Ok(Self {
            owner: p.owner.to_wire().to_string(),
            signers: p
                .signers
                .iter()
                .map(|r| SignerResponse {
                    issuer: r.issuer.clone(),
                    subject: r.subject.clone(),
                    runtimes: r
                        .runtimes
                        .iter()
                        .map(|rt| rt.wire_value().to_string())
                        .collect(),
                })
                .collect(),
            ceilings: ceilings.into(),
            stored: true,
            updated_at: Some(p.updated_at),
        })
    }

    /// No stored row: no signers, the platform defaults, `stored: false`.
    fn effective_default(owner: &FunctionOwner, defaults: &FunctionLimits) -> Self {
        Self {
            owner: owner.to_wire().to_string(),
            signers: Vec::new(),
            ceilings: ClientCeilings::of(defaults).into(),
            stored: false,
            updated_at: None,
        }
    }
}

/// `{policies: [...]}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyListResponse {
    pub policies: Vec<PolicyResponse>,
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// Every stored policy: the platform's first, then client ids ascending.
#[utoipa::path(
    get, path = "/api/function-policies", tag = "function-policies",
    operation_id = "getApiFunctionPolicies",
    responses(
        (status = 200, body = PolicyListResponse),
        (status = 403, description = "ANCHOR_REQUIRED or PERMISSION_REQUIRED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_policies(
    State(state): State<FunctionsState>,
    auth: Authenticated,
) -> Result<Json<PolicyListResponse>, PlatformError> {
    checks::require_anchor_scope(&auth.0)?;
    checks::require_permission(&auth.0, FUNCTION_POLICY_MANAGE)?;
    let policies = state
        .policies
        .list_all()
        .await?
        .iter()
        .map(|p| PolicyResponse::from(p, &state.limits))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(PolicyListResponse { policies }))
}

/// One owner's policy, or the effective default (`stored: false`) when it
/// has none.
#[utoipa::path(
    get, path = "/api/function-policies/{owner}", tag = "function-policies",
    operation_id = "getApiFunctionPoliciesByOwner",
    params(("owner" = String, Path, description = "A client id, or `platform`")),
    responses((status = 200, body = PolicyResponse), (status = 403)),
    security(("bearer_auth" = []))
)]
pub async fn get_policy(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(owner): Path<String>,
) -> Result<Json<PolicyResponse>, PlatformError> {
    checks::require_anchor_scope(&auth.0)?;
    checks::require_permission(&auth.0, FUNCTION_POLICY_MANAGE)?;
    let owner = owner_from_path(&owner)?;
    Ok(Json(match state.policies.find_by_owner(&owner).await? {
        Some(p) => PolicyResponse::from(&p, &state.limits)?,
        None => PolicyResponse::effective_default(&owner, &state.limits),
    }))
}

/// Replace one owner's policy.
#[utoipa::path(
    put, path = "/api/function-policies/{owner}", tag = "function-policies",
    operation_id = "putApiFunctionPoliciesByOwner",
    params(("owner" = String, Path, description = "A client id, or `platform`")),
    request_body = PutPolicyRequest,
    responses(
        (status = 200, body = PolicyResponse),
        (status = 400, description = "SIGNER_INVALID, RUNTIME_INVALID, SIGNER_DUPLICATE or CEILING_INVALID"),
        (status = 403), (status = 404, description = "Client_NOT_FOUND"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn put_policy(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(owner): Path<String>,
    body: Bytes,
) -> Result<Json<PolicyResponse>, PlatformError> {
    checks::require_anchor_scope(&auth.0)?;
    checks::require_permission(&auth.0, FUNCTION_POLICY_MANAGE)?;
    let owner = owner_from_path(&owner)?;
    let req: PutPolicyRequest = parse_body(&body)?;
    state
        .ops
        .put_policy()
        .run(
            req.into_command(owner.clone()),
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    let saved = state
        .policies
        .find_by_owner(&owner)
        .await?
        .ok_or_else(|| PlatformError::internal("REPO: policy written but row not found"))?;
    Ok(Json(PolicyResponse::from(&saved, &state.limits)?))
}
