//! Email Domain Mappings Admin API

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::entity::EmailDomainMapping;
use super::repository::EmailDomainMappingRepository;
use crate::identity_provider::repository::IdentityProviderRepository;
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
    pub identity_provider_name: Option<String>,
    pub sync_roles_from_idp: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl EmailDomainMappingResponse {
    fn from_entity(m: EmailDomainMapping, identity_provider_name: Option<String>) -> Self {
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
            identity_provider_name,
            sync_roles_from_idp: m.sync_roles_from_idp,
            created_at: m.created_at.to_rfc3339(),
            updated_at: m.updated_at.to_rfc3339(),
        }
    }
}

impl From<EmailDomainMapping> for EmailDomainMappingResponse {
    fn from(m: EmailDomainMapping) -> Self {
        Self::from_entity(m, None)
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
    pub idp_repo: Arc<IdentityProviderRepository>,
    pub create_use_case: Arc<
        crate::email_domain_mapping::operations::CreateEmailDomainMappingUseCase<
            crate::usecase::PgUnitOfWork,
        >,
    >,
    pub update_use_case: Arc<
        crate::email_domain_mapping::operations::UpdateEmailDomainMappingUseCase<
            crate::usecase::PgUnitOfWork,
        >,
    >,
    pub delete_use_case: Arc<
        crate::email_domain_mapping::operations::DeleteEmailDomainMappingUseCase<
            crate::usecase::PgUnitOfWork,
        >,
    >,
}

/// Create a new email domain mapping
#[utoipa::path(
    post,
    path = "",
    tag = "email-domain-mappings",
    operation_id = "postApiEmailDomainMappings",
    request_body = CreateEmailDomainMappingRequest,
    responses(
        (status = 201, description = "Email domain mapping created", body = crate::shared::api_common::CreatedResponse),
        (status = 409, description = "Duplicate email domain")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_email_domain_mapping(
    State(state): State<EmailDomainMappingsState>,
    auth: Authenticated,
    Json(req): Json<CreateEmailDomainMappingRequest>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<crate::shared::api_common::CreatedResponse>,
    ),
    PlatformError,
> {
    use crate::email_domain_mapping::operations::CreateEmailDomainMappingCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = CreateEmailDomainMappingCommand {
        email_domain: req.email_domain,
        identity_provider_id: req.identity_provider_id,
        scope_type: req.scope_type,
        primary_client_id: req.primary_client_id,
        additional_client_ids: req.additional_client_ids.unwrap_or_default(),
        granted_client_ids: req.granted_client_ids.unwrap_or_default(),
        required_oidc_tenant_id: req.required_oidc_tenant_id,
        allowed_role_ids: req.allowed_role_ids.unwrap_or_default(),
        sync_roles_from_idp: req.sync_roles_from_idp.unwrap_or(false),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(crate::shared::api_common::CreatedResponse::new(
            event.mapping_id,
        )),
    ))
}

/// List all email domain mappings
#[utoipa::path(
    get,
    path = "",
    tag = "email-domain-mappings",
    operation_id = "getApiEmailDomainMappings",
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

    // Batch-lookup identity provider names
    let idp_ids: Vec<String> = mappings
        .iter()
        .map(|m| m.identity_provider_id.clone())
        .collect();
    let mut idp_name_map = std::collections::HashMap::new();
    for idp_id in &idp_ids {
        if !idp_name_map.contains_key(idp_id) {
            if let Some(idp) = state.idp_repo.find_by_id(idp_id).await? {
                idp_name_map.insert(idp_id.clone(), idp.name);
            }
        }
    }

    let responses = mappings
        .into_iter()
        .map(|m| {
            let name = idp_name_map.get(&m.identity_provider_id).cloned();
            EmailDomainMappingResponse::from_entity(m, name)
        })
        .collect();

    Ok(Json(EmailDomainMappingsListResponse {
        mappings: responses,
        total,
    }))
}

/// Get email domain mapping by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "email-domain-mappings",
    operation_id = "getApiEmailDomainMappingsById",
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
    let edm = state
        .edm_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EmailDomainMapping", &id))?;
    let idp_name = state
        .idp_repo
        .find_by_id(&edm.identity_provider_id)
        .await?
        .map(|idp| idp.name);
    Ok(Json(EmailDomainMappingResponse::from_entity(edm, idp_name)))
}

/// Lookup email domain mapping by domain
#[utoipa::path(
    get,
    path = "/lookup/{domain}",
    tag = "email-domain-mappings",
    operation_id = "getApiEmailDomainMappingsLookupByDomain",
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
    let edm = state
        .edm_repo
        .find_by_email_domain(&domain)
        .await?
        .ok_or_else(|| PlatformError::not_found("EmailDomainMapping", &domain))?;
    let idp_name = state
        .idp_repo
        .find_by_id(&edm.identity_provider_id)
        .await?
        .map(|idp| idp.name);
    Ok(Json(EmailDomainMappingResponse::from_entity(edm, idp_name)))
}

/// Update an email domain mapping
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "email-domain-mappings",
    operation_id = "putApiEmailDomainMappingsById",
    params(
        ("id" = String, Path, description = "Email domain mapping ID")
    ),
    request_body = UpdateEmailDomainMappingRequest,
    responses(
        (status = 204, description = "Email domain mapping updated"),
        (status = 404, description = "Email domain mapping not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_email_domain_mapping(
    State(state): State<EmailDomainMappingsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateEmailDomainMappingRequest>,
) -> Result<axum::http::StatusCode, PlatformError> {
    use crate::email_domain_mapping::operations::UpdateEmailDomainMappingCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = UpdateEmailDomainMappingCommand {
        mapping_id: id,
        identity_provider_id: req.identity_provider_id,
        scope_type: req.scope_type,
        primary_client_id: req.primary_client_id,
        sync_roles_from_idp: req.sync_roles_from_idp,
        additional_client_ids: req.additional_client_ids,
        granted_client_ids: req.granted_client_ids,
        required_oidc_tenant_id: req.required_oidc_tenant_id,
        allowed_role_ids: req.allowed_role_ids,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Delete an email domain mapping
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "email-domain-mappings",
    operation_id = "deleteApiEmailDomainMappingsById",
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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, PlatformError> {
    use crate::email_domain_mapping::operations::DeleteEmailDomainMappingCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = DeleteEmailDomainMappingCommand { mapping_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub fn email_domain_mappings_router(state: EmailDomainMappingsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            create_email_domain_mapping,
            list_email_domain_mappings
        ))
        .routes(routes!(lookup_email_domain_mapping))
        .routes(routes!(
            get_email_domain_mapping,
            update_email_domain_mapping,
            delete_email_domain_mapping
        ))
        .with_state(state)
}
