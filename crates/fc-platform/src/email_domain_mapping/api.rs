//! Email Domain Mappings Admin API

use axum::{
    extract::{State, Path},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::entity::{EmailDomainMapping, ScopeType};
use super::repository::EmailDomainMappingRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEmailDomainMappingRequest {
    pub email_domain: String,
    pub identity_provider_id: String,
    pub scope_type: String,
    pub primary_client_id: Option<String>,
    pub additional_client_ids: Option<Vec<String>>,
    pub granted_client_ids: Option<Vec<String>>,
    pub required_oidc_tenant_id: Option<String>,
    pub allowed_role_ids: Option<Vec<String>>,
    pub sync_roles_from_idp: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEmailDomainMappingRequest {
    pub identity_provider_id: Option<String>,
    pub scope_type: Option<String>,
    pub primary_client_id: Option<String>,
    pub additional_client_ids: Option<Vec<String>>,
    pub granted_client_ids: Option<Vec<String>>,
    pub required_oidc_tenant_id: Option<String>,
    pub allowed_role_ids: Option<Vec<String>>,
    pub sync_roles_from_idp: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailDomainMappingResponse {
    pub id: String,
    pub email_domain: String,
    pub identity_provider_id: String,
    pub scope_type: String,
    pub primary_client_id: Option<String>,
    pub additional_client_ids: Vec<String>,
    pub granted_client_ids: Vec<String>,
    pub required_oidc_tenant_id: Option<String>,
    pub allowed_role_ids: Vec<String>,
    pub sync_roles_from_idp: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<EmailDomainMapping> for EmailDomainMappingResponse {
    fn from(m: EmailDomainMapping) -> Self {
        Self {
            id: m.id,
            email_domain: m.email_domain,
            identity_provider_id: m.identity_provider_id,
            scope_type: m.scope_type.as_str().to_string(),
            primary_client_id: m.primary_client_id,
            additional_client_ids: m.additional_client_ids,
            granted_client_ids: m.granted_client_ids,
            required_oidc_tenant_id: m.required_oidc_tenant_id,
            allowed_role_ids: m.allowed_role_ids,
            sync_roles_from_idp: m.sync_roles_from_idp,
            created_at: m.created_at.to_rfc3339(),
            updated_at: m.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailDomainMappingsListResponse {
    pub mappings: Vec<EmailDomainMappingResponse>,
    pub total: usize,
}

#[derive(Clone)]
pub struct EmailDomainMappingsState {
    pub edm_repo: Arc<EmailDomainMappingRepository>,
}

/// Create a new email domain mapping
#[utoipa::path(
    post,
    path = "",
    tag = "email-domain-mappings",
    operation_id = "postApiAdminEmailDomainMappings",
    request_body = CreateEmailDomainMappingRequest,
    responses(
        (status = 201, description = "Email domain mapping created", body = EmailDomainMappingResponse),
        (status = 409, description = "Duplicate email domain")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_email_domain_mapping(
    State(state): State<EmailDomainMappingsState>,
    _auth: Authenticated,
    Json(req): Json<CreateEmailDomainMappingRequest>,
) -> Result<(axum::http::StatusCode, Json<EmailDomainMappingResponse>), PlatformError> {
    if state.edm_repo.find_by_email_domain(&req.email_domain).await?.is_some() {
        return Err(PlatformError::duplicate("EmailDomainMapping", "emailDomain", &req.email_domain));
    }

    let scope = ScopeType::from_str(&req.scope_type);
    let mut edm = EmailDomainMapping::new(&req.email_domain, &req.identity_provider_id, scope);
    edm.primary_client_id = req.primary_client_id;
    edm.additional_client_ids = req.additional_client_ids.unwrap_or_default();
    edm.granted_client_ids = req.granted_client_ids.unwrap_or_default();
    edm.required_oidc_tenant_id = req.required_oidc_tenant_id;
    edm.allowed_role_ids = req.allowed_role_ids.unwrap_or_default();
    edm.sync_roles_from_idp = req.sync_roles_from_idp.unwrap_or(false);

    state.edm_repo.insert(&edm).await?;
    Ok((axum::http::StatusCode::CREATED, Json(edm.into())))
}

/// List all email domain mappings
#[utoipa::path(
    get,
    path = "",
    tag = "email-domain-mappings",
    operation_id = "getApiAdminEmailDomainMappings",
    responses(
        (status = 200, description = "List of email domain mappings", body = EmailDomainMappingsListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_email_domain_mappings(
    State(state): State<EmailDomainMappingsState>,
    _auth: Authenticated,
) -> Result<Json<EmailDomainMappingsListResponse>, PlatformError> {
    let mappings = state.edm_repo.find_all().await?;
    let total = mappings.len();
    Ok(Json(EmailDomainMappingsListResponse {
        mappings: mappings.into_iter().map(|m| m.into()).collect(),
        total,
    }))
}

/// Get email domain mapping by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "email-domain-mappings",
    operation_id = "getApiAdminEmailDomainMappingsById",
    params(
        ("id" = String, Path, description = "Email domain mapping ID")
    ),
    responses(
        (status = 200, description = "Email domain mapping found", body = EmailDomainMappingResponse),
        (status = 404, description = "Email domain mapping not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_email_domain_mapping(
    State(state): State<EmailDomainMappingsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<EmailDomainMappingResponse>, PlatformError> {
    let edm = state.edm_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("EmailDomainMapping", &id))?;
    Ok(Json(edm.into()))
}

/// Lookup email domain mapping by domain
#[utoipa::path(
    get,
    path = "/lookup/{domain}",
    tag = "email-domain-mappings",
    operation_id = "getApiAdminEmailDomainMappingsLookupByDomain",
    params(
        ("domain" = String, Path, description = "Email domain to look up")
    ),
    responses(
        (status = 200, description = "Email domain mapping found", body = EmailDomainMappingResponse),
        (status = 404, description = "Email domain mapping not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn lookup_email_domain_mapping(
    State(state): State<EmailDomainMappingsState>,
    _auth: Authenticated,
    Path(domain): Path<String>,
) -> Result<Json<EmailDomainMappingResponse>, PlatformError> {
    let edm = state.edm_repo.find_by_email_domain(&domain).await?
        .ok_or_else(|| PlatformError::not_found("EmailDomainMapping", &domain))?;
    Ok(Json(edm.into()))
}

/// Update an email domain mapping
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "email-domain-mappings",
    operation_id = "putApiAdminEmailDomainMappingsById",
    params(
        ("id" = String, Path, description = "Email domain mapping ID")
    ),
    request_body = UpdateEmailDomainMappingRequest,
    responses(
        (status = 200, description = "Email domain mapping updated", body = EmailDomainMappingResponse),
        (status = 404, description = "Email domain mapping not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_email_domain_mapping(
    State(state): State<EmailDomainMappingsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateEmailDomainMappingRequest>,
) -> Result<Json<EmailDomainMappingResponse>, PlatformError> {
    let mut edm = state.edm_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("EmailDomainMapping", &id))?;

    if let Some(idp_id) = req.identity_provider_id { edm.identity_provider_id = idp_id; }
    if let Some(scope) = req.scope_type { edm.scope_type = ScopeType::from_str(&scope); }
    if let Some(pcid) = req.primary_client_id { edm.primary_client_id = Some(pcid); }
    if let Some(ac) = req.additional_client_ids { edm.additional_client_ids = ac; }
    if let Some(gc) = req.granted_client_ids { edm.granted_client_ids = gc; }
    if let Some(tenant) = req.required_oidc_tenant_id { edm.required_oidc_tenant_id = Some(tenant); }
    if let Some(roles) = req.allowed_role_ids { edm.allowed_role_ids = roles; }
    if let Some(sync) = req.sync_roles_from_idp { edm.sync_roles_from_idp = sync; }
    edm.updated_at = chrono::Utc::now();

    state.edm_repo.update(&edm).await?;
    Ok(Json(edm.into()))
}

/// Delete an email domain mapping
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "email-domain-mappings",
    operation_id = "deleteApiAdminEmailDomainMappingsById",
    params(
        ("id" = String, Path, description = "Email domain mapping ID")
    ),
    responses(
        (status = 204, description = "Email domain mapping deleted"),
        (status = 404, description = "Email domain mapping not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_email_domain_mapping(
    State(state): State<EmailDomainMappingsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, PlatformError> {
    let _ = state.edm_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("EmailDomainMapping", &id))?;
    state.edm_repo.delete(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub fn email_domain_mappings_router(state: EmailDomainMappingsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_email_domain_mapping, list_email_domain_mappings))
        .routes(routes!(lookup_email_domain_mapping))
        .routes(routes!(get_email_domain_mapping, update_email_domain_mapping, delete_email_domain_mapping))
        .with_state(state)
}
