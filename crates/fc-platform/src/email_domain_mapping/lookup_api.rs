//! Email-domain-mapping routes Go serves that Rust lacked
//! (`emaildomainmapping/api/api.go:45-49`):
//!
//! - `GET  /api/email-domain-mappings/lookup?domain=`         (no auth; `{found:false}` when absent)
//! - `GET  /api/email-domain-mappings/by-domain/{domain}`     (anchor + `…:email-domain-mapping:view`)
//! - `POST /api/email-domain-mappings/{id}/move-provider`     (anchor + `…:email-domain-mapping:update`)
//!
//! Both lookups match the domain exactly, as Go's do.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::EmailDomainMappingResponse;
use super::entity::{EmailDomainMapping, ScopeType};
use super::operations::move_provider::{
    MoveMappingToProviderCommand, MoveMappingToProviderUseCase,
};
use super::repository::EmailDomainMappingRepository;
use crate::identity_provider::repository::IdentityProviderRepository;
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

#[derive(Clone)]
pub struct EdmLookupState {
    pub edm_repo: Arc<EmailDomainMappingRepository>,
    pub idp_repo: Arc<IdentityProviderRepository>,
    pub move_use_case: Arc<MoveMappingToProviderUseCase<PgUnitOfWork>>,
}

/// Go's scope parse on create: 400 `INVALID_SCOPE_TYPE`.
pub fn parse_scope_type(s: &str) -> Result<ScopeType, PlatformError> {
    s.parse().map_err(|_| {
        PlatformError::bad_request_code(
            "INVALID_SCOPE_TYPE",
            "scopeType must be ANCHOR, PARTNER, or CLIENT",
        )
    })
}

#[derive(Debug, Deserialize)]
pub struct LookupQuery {
    pub domain: Option<String>,
}

/// Go `MoveProviderRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveProviderRequest {
    /// Required, as in Go's huma schema (absent is a 400 `VALIDATION`).
    pub identity_provider_id: String,
}

/// Go `MoveProviderResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveProviderResponse {
    pub mapping_id: String,
    pub email_domain: String,
    pub from_identity_provider_id: String,
    pub to_identity_provider_id: String,
    pub users_reset: usize,
}

async fn with_idp_name(
    state: &EdmLookupState,
    m: EmailDomainMapping,
) -> Result<EmailDomainMappingResponse, PlatformError> {
    // Go resolves the name best-effort: a lookup failure leaves it out.
    let name = state
        .idp_repo
        .find_by_id(&m.identity_provider_id)
        .await
        .ok()
        .flatten()
        .map(|i| i.name);
    Ok(EmailDomainMappingResponse::from_entity(m, name))
}

/// The mapping for a domain, before login (Go `lookupEmailDomainMapping`):
/// unauthenticated; 200 `{found:false}` when there is none.
#[utoipa::path(
    get,
    path = "/api/email-domain-mappings/lookup",
    tag = "email-domain-mappings",
    operation_id = "lookupEmailDomainMapping",
    params(("domain" = String, Query, description = "Email domain, matched exactly")),
    responses(
        (status = 200, description = "The mapping, or {found: false}"),
        (status = 400, description = "No domain given")
    )
)]
pub async fn lookup_email_domain_mapping_by_query(
    State(state): State<EdmLookupState>,
    Query(q): Query<LookupQuery>,
) -> Result<Json<serde_json::Value>, PlatformError> {
    let domain = q.domain.unwrap_or_default();
    if domain.is_empty() {
        return Err(PlatformError::bad_request_code(
            "DOMAIN_REQUIRED",
            "domain query param is required",
        ));
    }
    match state.edm_repo.find_by_email_domain(&domain).await? {
        Some(m) => Ok(Json(
            serde_json::to_value(with_idp_name(&state, m).await?)
                .map_err(|e| PlatformError::internal(e.to_string()))?,
        )),
        None => Ok(Json(serde_json::json!({ "found": false }))),
    }
}

/// The mapping for a domain (Go `getEmailDomainMappingByDomain`).
#[utoipa::path(
    get,
    path = "/api/email-domain-mappings/by-domain/{domain}",
    tag = "email-domain-mappings",
    operation_id = "getEmailDomainMappingByDomain",
    params(("domain" = String, Path, description = "Email domain, matched exactly")),
    responses(
        (status = 200, description = "The mapping", body = EmailDomainMappingResponse),
        (status = 404, description = "No mapping for the domain")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_email_domain_mapping_by_domain(
    State(state): State<EdmLookupState>,
    auth: Authenticated,
    Path(domain): Path<String>,
) -> Result<Json<EmailDomainMappingResponse>, PlatformError> {
    checks::require_anchor_scope(&auth.0)?;
    checks::require_permission(
        &auth.0,
        crate::permissions::admin::EMAIL_DOMAIN_MAPPING_READ,
    )?;
    let m = state
        .edm_repo
        .find_by_email_domain(&domain)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("EmailDomainMapping", &domain))?;
    Ok(Json(with_idp_name(&state, m).await?))
}

/// Move a mapping to another identity provider (Go `moveEmailDomainMappingProvider`).
#[utoipa::path(
    post,
    path = "/api/email-domain-mappings/{id}/move-provider",
    tag = "email-domain-mappings",
    operation_id = "moveEmailDomainMappingProvider",
    params(("id" = String, Path, description = "Mapping id")),
    request_body = MoveProviderRequest,
    responses(
        (status = 200, description = "Moved", body = MoveProviderResponse),
        (status = 404, description = "Mapping or provider not found"),
        (status = 409, description = "Already on that provider")
    ),
    security(("bearer_auth" = []))
)]
pub async fn move_email_domain_mapping_provider(
    State(state): State<EdmLookupState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<MoveProviderRequest>,
) -> Result<Json<MoveProviderResponse>, PlatformError> {
    checks::can_update_email_domain_mappings(&auth.0)?;
    let event = state
        .move_use_case
        .run(
            MoveMappingToProviderCommand {
                mapping_id: id,
                identity_provider_id: req.identity_provider_id,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(Json(MoveProviderResponse {
        mapping_id: event.mapping_id,
        email_domain: event.email_domain,
        from_identity_provider_id: event.from_identity_provider_id,
        to_identity_provider_id: event.to_identity_provider_id,
        users_reset: event.users_reset,
    }))
}

/// Full-path router; merged at the root.
pub fn edm_lookup_router(state: EdmLookupState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(lookup_email_domain_mapping_by_query))
        .routes(routes!(get_email_domain_mapping_by_domain))
        .routes(routes!(move_email_domain_mapping_provider))
        .with_state(state)
}
