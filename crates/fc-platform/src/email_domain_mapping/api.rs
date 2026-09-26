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
    /// Go's per-domain 2FA policy (emaildomainmapping/api/dto.go:20-25).
    #[serde(default, rename = "require2fa")]
    pub require_2fa: Option<bool>,
    #[serde(default, rename = "allowed2faMethods")]
    pub allowed_2fa_methods: Option<Vec<String>>,
    #[serde(default)]
    pub remember_device_enabled: Option<bool>,
    #[serde(default)]
    pub remember_device_days: Option<i32>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEmailDomainMappingRequest {
    /// Not in Go's update (the provider moves through `move-provider`);
    /// still honoured here.
    pub identity_provider_id: Option<String>,
    /// The SPA's edit form sends it; Go ignores it, Rust applies it.
    pub scope_type: Option<String>,
    /// `null` clears the link (the SPA's ANCHOR edit); absent leaves it.
    #[serde(default, deserialize_with = "nullable")]
    pub primary_client_id: Option<Option<String>>,
    pub additional_client_ids: Option<Vec<String>>,
    pub granted_client_ids: Option<Vec<String>>,
    /// `null` or `""` clears the pin; absent leaves it.
    #[serde(default, deserialize_with = "nullable")]
    pub required_oidc_tenant_id: Option<Option<String>>,
    #[serde(default, rename = "require2fa")]
    pub require_2fa: Option<bool>,
    #[serde(default, rename = "allowed2faMethods")]
    pub allowed_2fa_methods: Option<Vec<String>>,
    #[serde(default)]
    pub remember_device_enabled: Option<bool>,
    #[serde(default)]
    pub remember_device_days: Option<i32>,
}

/// A member that tells an explicit `null` (`Some(None)`) from an absent one
/// (`None`, with `#[serde(default)]`).
fn nullable<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

/// Go `MappingResponse` (emaildomainmapping/api/dto.go): optional members
/// absent when unset. Role sync lives on the identity provider (Go's 040).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailDomainMappingResponse {
    pub id: String,
    pub email_domain: String,
    pub identity_provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_provider_name: Option<String>,
    pub scope_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    pub additional_client_ids: Vec<String>,
    pub granted_client_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_oidc_tenant_id: Option<String>,
    #[serde(rename = "require2fa")]
    pub require_2fa: bool,
    #[serde(rename = "allowed2faMethods")]
    pub allowed_2fa_methods: Vec<String>,
    pub remember_device_enabled: bool,
    pub remember_device_days: i32,
    pub created_at: String,
    pub updated_at: String,
}

impl EmailDomainMappingResponse {
    pub(crate) fn from_entity(
        m: EmailDomainMapping,
        identity_provider_name: Option<String>,
    ) -> Self {
        Self {
            id: m.id,
            email_domain: m.email_domain,
            identity_provider_id: m.identity_provider_id,
            scope_type: m.scope_type.as_str().to_string(),
            primary_client_id: m.primary_client_id,
            additional_client_ids: m.additional_client_ids,
            granted_client_ids: m.granted_client_ids,
            required_oidc_tenant_id: m.required_oidc_tenant_id,
            identity_provider_name,
            require_2fa: m.require_2fa,
            allowed_2fa_methods: m.allowed_2fa_methods,
            remember_device_enabled: m.remember_device_enabled,
            remember_device_days: m.remember_device_days,
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
    /// Role definitions (the role ceiling now applies to the identity
    /// provider's `allowedRoleIds`).
    pub role_repo: Arc<crate::RoleRepository>,
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

    crate::checks::can_create_email_domain_mappings(&auth.0)?;

    let cmd = CreateEmailDomainMappingCommand {
        email_domain: req.email_domain,
        identity_provider_id: req.identity_provider_id,
        scope_type: super::lookup_api::parse_scope_type(&req.scope_type)?,
        primary_client_id: req.primary_client_id,
        additional_client_ids: req.additional_client_ids.unwrap_or_default(),
        granted_client_ids: req.granted_client_ids.unwrap_or_default(),
        required_oidc_tenant_id: req.required_oidc_tenant_id,
        // Role sync lives on the identity provider (Go's 040).
        allowed_role_ids: Vec::new(),
        sync_roles_from_idp: false,
        two_factor: crate::email_domain_mapping::operations::TwoFactorPolicyInput {
            require_2fa: req.require_2fa.unwrap_or(false),
            allowed_2fa_methods: req.allowed_2fa_methods.unwrap_or_default(),
            remember_device_enabled: req.remember_device_enabled.unwrap_or(false),
            remember_device_days: req.remember_device_days.unwrap_or(0),
        },
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
    auth: Authenticated,
) -> Result<Json<EmailDomainMappingsListResponse>, PlatformError> {
    crate::checks::can_read_email_domain_mappings(&auth.0)?;

    let mappings = state.edm_repo.find_all().await?;
    let total = mappings.len();

    // Identity-provider names in one query (Go leaves an unresolvable one
    // out).
    let mut idp_ids: Vec<String> = mappings
        .iter()
        .map(|m| m.identity_provider_id.clone())
        .collect();
    idp_ids.sort();
    idp_ids.dedup();
    let idp_name_map = state.idp_repo.find_names_by_ids(&idp_ids).await?;

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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<EmailDomainMappingResponse>, PlatformError> {
    crate::checks::can_read_email_domain_mappings(&auth.0)?;

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

    crate::checks::can_update_email_domain_mappings(&auth.0)?;

    let cmd = UpdateEmailDomainMappingCommand {
        mapping_id: id,
        identity_provider_id: req.identity_provider_id,
        scope_type: crate::shared::enum_str::parse_opt(req.scope_type.as_deref())?,
        // An explicit null clears: passed as blank, which the use case reads
        // as "no link".
        primary_client_id: req.primary_client_id.map(Option::unwrap_or_default),
        sync_roles_from_idp: None,
        additional_client_ids: req.additional_client_ids,
        granted_client_ids: req.granted_client_ids,
        required_oidc_tenant_id: req.required_oidc_tenant_id.map(Option::unwrap_or_default),
        allowed_role_ids: None,
        two_factor: crate::email_domain_mapping::operations::TwoFactorPolicyUpdate {
            require_2fa: req.require_2fa,
            allowed_2fa_methods: req.allowed_2fa_methods,
            remember_device_enabled: req.remember_device_enabled,
            remember_device_days: req.remember_device_days,
        },
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

    crate::checks::can_delete_email_domain_mappings(&auth.0)?;

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
