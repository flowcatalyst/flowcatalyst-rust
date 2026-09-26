//! Auth Configuration Admin API
//!
//! REST endpoints for authentication configuration management.
//! Includes anchor domains, client auth configs, and IDP role mappings.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::auth::config_entity::{AnchorDomain, AuthProvider, ClientAuthConfig, IdpRoleMapping};
use crate::shared::api_common::CreatedResponse;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::{AnchorDomainRepository, ClientAuthConfigRepository, IdpRoleMappingRepository};

// ============================================================================
// Anchor Domains
// ============================================================================

/// Create anchor domain request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAnchorDomainRequest {
    /// Email domain (e.g., "flowcatalyst.tech")
    pub domain: String,
}

/// Anchor domain response DTO: Go's `AnchorDomainResponse`
/// (auth/api/dto.go).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnchorDomainResponse {
    pub id: String,
    pub domain: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Anchor domain list response: Go's `{items}`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnchorDomainListResponse {
    pub items: Vec<AnchorDomainResponse>,
}

impl From<AnchorDomain> for AnchorDomainResponse {
    fn from(d: AnchorDomain) -> Self {
        Self {
            id: d.id,
            domain: d.domain,
            created_at: d.created_at.to_rfc3339(),
            updated_at: d.updated_at.to_rfc3339(),
        }
    }
}

// ============================================================================
// Client Auth Configs
// ============================================================================

/// Create client auth config request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateClientAuthConfigRequest {
    /// Email domain this config applies to
    pub email_domain: String,

    /// Config type: ANCHOR, PARTNER, or CLIENT (required, as Go's huma
    /// schema has it)
    pub config_type: String,

    /// Primary client ID (for CLIENT type)
    pub primary_client_id: Option<String>,

    /// Additional client IDs
    #[serde(default)]
    pub additional_client_ids: Option<Vec<String>>,

    /// Granted client IDs (PARTNER)
    #[serde(default)]
    pub granted_client_ids: Option<Vec<String>>,

    /// Auth provider: INTERNAL or OIDC (required)
    pub auth_provider: String,

    /// OIDC issuer URL
    pub oidc_issuer_url: Option<String>,

    /// OIDC client ID
    pub oidc_client_id: Option<String>,

    /// Multi-tenant OIDC
    #[serde(default)]
    pub oidc_multi_tenant: bool,

    /// Multi-tenant issuer pattern
    pub oidc_issuer_pattern: Option<String>,

    /// OIDC client secret: a secret-manager reference, or a plaintext that
    /// is encrypted before storage
    pub oidc_client_secret_ref: Option<String>,
}

/// Update client auth config request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientAuthConfigRequest {
    /// Primary client ID
    pub primary_client_id: Option<String>,

    /// Auth provider
    pub auth_provider: Option<String>,

    /// OIDC issuer URL
    pub oidc_issuer_url: Option<String>,

    /// OIDC client ID
    pub oidc_client_id: Option<String>,

    /// Additional client IDs
    pub additional_client_ids: Option<Vec<String>>,

    /// Granted client IDs (PARTNER)
    pub granted_client_ids: Option<Vec<String>>,

    /// Multi-tenant OIDC
    pub oidc_multi_tenant: Option<bool>,

    /// Multi-tenant issuer pattern
    pub oidc_issuer_pattern: Option<String>,

    /// OIDC client secret (reference or plaintext to encrypt)
    pub oidc_client_secret_ref: Option<String>,
}

/// Create internal auth config request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateInternalAuthConfigRequest {
    /// Email domain
    pub email_domain: String,
    /// Config type: CLIENT or PARTNER
    pub config_type: String,
    /// Primary client ID (required for CLIENT type)
    pub primary_client_id: Option<String>,
}

/// Create OIDC auth config request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOidcAuthConfigRequest {
    /// Email domain
    pub email_domain: String,
    /// Config type: CLIENT or PARTNER
    pub config_type: String,
    /// Primary client ID (required for CLIENT type)
    pub primary_client_id: Option<String>,
    /// OIDC issuer URL
    pub oidc_issuer_url: String,
    /// OIDC client ID
    pub oidc_client_id: String,
}

/// Update OIDC config request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOidcConfigRequest {
    /// OIDC issuer URL
    pub oidc_issuer_url: Option<String>,
    /// OIDC client ID
    pub oidc_client_id: Option<String>,
}

/// Update client binding request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientBindingRequest {
    /// Primary client ID
    pub primary_client_id: String,
}

/// Update additional clients request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAdditionalClientsRequest {
    /// Additional client IDs
    pub additional_client_ids: Vec<String>,
}

/// Update granted clients request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGrantedClientsRequest {
    /// Granted client IDs
    pub granted_client_ids: Vec<String>,
}

/// Client auth config response DTO: Go's `AuthConfigResponse`
/// (auth/api/dto.go), optional members absent when unset. The stored
/// secret is its reference or `encrypted:` form, never a plaintext.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAuthConfigResponse {
    pub id: String,
    pub email_domain: String,
    pub config_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    pub additional_client_ids: Vec<String>,
    /// Granted client IDs (for PARTNER type configs)
    pub granted_client_ids: Vec<String>,
    pub auth_provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    /// Whether OIDC is multi-tenant
    pub oidc_multi_tenant: bool,
    /// Issuer pattern for multi-tenant validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Client auth config list response: Go's `{items}`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigListResponse {
    pub items: Vec<ClientAuthConfigResponse>,
}

impl From<ClientAuthConfig> for ClientAuthConfigResponse {
    fn from(c: ClientAuthConfig) -> Self {
        Self {
            id: c.id,
            email_domain: c.email_domain,
            config_type: c.config_type.as_str().to_string(),
            primary_client_id: c.primary_client_id,
            additional_client_ids: c.additional_client_ids,
            granted_client_ids: c.granted_client_ids,
            auth_provider: c.auth_provider.as_str().to_string(),
            oidc_issuer_url: c.oidc_issuer_url,
            oidc_client_id: c.oidc_client_id,
            oidc_multi_tenant: c.oidc_multi_tenant,
            oidc_issuer_pattern: c.oidc_issuer_pattern,
            oidc_client_secret_ref: c.oidc_client_secret_ref,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

/// Go's `ParseAuthConfigType`: the exact upper-case names.
fn parse_config_type(
    value: &str,
) -> Result<crate::auth::config_entity::AuthConfigType, PlatformError> {
    use crate::auth::config_entity::AuthConfigType;
    match value {
        "ANCHOR" => Ok(AuthConfigType::Anchor),
        "PARTNER" => Ok(AuthConfigType::Partner),
        "CLIENT" => Ok(AuthConfigType::Client),
        _ => Err(PlatformError::bad_request_code(
            "INVALID_CONFIG_TYPE",
            "configType must be ANCHOR, PARTNER, or CLIENT",
        )),
    }
}

/// Go's `ParseAuthProvider`: `INTERNAL` or `OIDC`.
fn parse_auth_provider(value: &str) -> Result<AuthProvider, PlatformError> {
    match value {
        "INTERNAL" => Ok(AuthProvider::Internal),
        "OIDC" => Ok(AuthProvider::Oidc),
        _ => Err(PlatformError::bad_request_code(
            "INVALID_AUTH_PROVIDER",
            "authProvider must be INTERNAL or OIDC",
        )),
    }
}

// ============================================================================
// IDP Role Mappings
// ============================================================================

/// Create IDP role mapping request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdpRoleMappingRequest {
    /// IDP type (e.g., "OIDC", "AZURE_AD")
    pub idp_type: String,

    /// Role name from the IDP
    pub idp_role_name: String,

    /// Platform role name to map to
    pub platform_role_name: String,
}

/// IDP role mapping response DTO: Go's `IdpRoleMappingResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMappingResponse {
    pub id: String,
    pub idp_type: String,
    pub idp_role_name: String,
    pub platform_role_name: String,
    pub created_at: String,
    pub updated_at: String,
}

/// IDP role mapping list response: Go's `{items}`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMappingListResponse {
    pub items: Vec<IdpRoleMappingResponse>,
}

impl From<IdpRoleMapping> for IdpRoleMappingResponse {
    fn from(m: IdpRoleMapping) -> Self {
        Self {
            id: m.id,
            idp_type: m.idp_type,
            idp_role_name: m.idp_role_name,
            platform_role_name: m.platform_role_name,
            created_at: m.created_at.to_rfc3339(),
            updated_at: m.updated_at.to_rfc3339(),
        }
    }
}

// ============================================================================
// State and Helpers
// ============================================================================

/// Auth config service state
#[derive(Clone)]
pub struct AuthConfigState {
    pub anchor_domain_repo: Arc<AnchorDomainRepository>,
    pub client_auth_config_repo: Arc<ClientAuthConfigRepository>,
    pub idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
    /// Role definitions, for the role ceiling on IdP role mappings.
    pub role_repo: Arc<crate::RoleRepository>,
    /// Used for counting users by email domain
    pub principal_repo: Arc<crate::PrincipalRepository>,
    pub unit_of_work: Arc<crate::usecase::PgUnitOfWork>,
    /// Encrypts an auth config's OIDC client secret before it reaches the
    /// command (Go `encryptOIDCSecretRef`). `None` without an app key.
    pub encryption_service: Option<Arc<crate::shared::encryption_service::EncryptionService>>,

    // Anchor domain use cases
    pub create_anchor_domain_use_case:
        Arc<crate::auth::operations::CreateAnchorDomainUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_anchor_domain_use_case:
        Arc<crate::auth::operations::UpdateAnchorDomainUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_anchor_domain_use_case:
        Arc<crate::auth::operations::DeleteAnchorDomainUseCase<crate::usecase::PgUnitOfWork>>,

    // Auth config use cases
    pub create_auth_config_use_case:
        Arc<crate::auth::operations::CreateAuthConfigUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_auth_config_use_case:
        Arc<crate::auth::operations::UpdateAuthConfigUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_auth_config_use_case:
        Arc<crate::auth::operations::DeleteAuthConfigUseCase<crate::usecase::PgUnitOfWork>>,

    // IdP role mapping use cases
    pub create_idp_role_mapping_use_case:
        Arc<crate::auth::operations::CreateIdpRoleMappingUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_idp_role_mapping_use_case:
        Arc<crate::auth::operations::DeleteIdpRoleMappingUseCase<crate::usecase::PgUnitOfWork>>,
}

// ============================================================================
// Anchor Domain Handlers
// ============================================================================

/// Create anchor domain
#[utoipa::path(
    post,
    path = "",
    tag = "anchor-domains",
    operation_id = "postApiAnchorDomains",
    request_body = CreateAnchorDomainRequest,
    responses(
        (status = 201, description = "Anchor domain created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate domain")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_anchor_domain(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Json(req): Json<CreateAnchorDomainRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), PlatformError> {
    use crate::auth::operations::CreateAnchorDomainCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_create_anchor_domains(&auth.0)?;

    let cmd = CreateAnchorDomainCommand {
        domain: req.domain.trim().to_lowercase(),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state
        .create_anchor_domain_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse::new(event.anchor_domain_id)),
    ))
}

/// List anchor domains
#[utoipa::path(
    get,
    path = "",
    tag = "anchor-domains",
    operation_id = "getApiAnchorDomains",
    responses(
        (status = 200, description = "List of anchor domains", body = AnchorDomainListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_anchor_domains(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
) -> Result<Json<AnchorDomainListResponse>, PlatformError> {
    crate::checks::can_read_anchor_domains(&auth.0)?;

    let items = state
        .anchor_domain_repo
        .find_all()
        .await?
        .into_iter()
        .map(AnchorDomainResponse::from)
        .collect();
    Ok(Json(AnchorDomainListResponse { items }))
}

/// Get anchor domain by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "anchor-domains",
    operation_id = "getApiAnchorDomainsById",
    params(
        ("id" = String, Path, description = "Anchor domain ID")
    ),
    responses(
        (status = 200, description = "Anchor domain found", body = AnchorDomainResponse),
        (status = 404, description = "Anchor domain not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_anchor_domain(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<AnchorDomainResponse>, PlatformError> {
    crate::checks::can_read_anchor_domains(&auth.0)?;

    let domain = state
        .anchor_domain_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("AnchorDomain", &id))?;

    Ok(Json(domain.into()))
}

/// Check anchor domain response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckAnchorDomainResponse {
    /// Whether the domain is an anchor domain
    pub is_anchor_domain: bool,
}

/// Check if domain is anchor domain
#[utoipa::path(
    get,
    path = "/check/{domain}",
    tag = "anchor-domains",
    operation_id = "getApiAnchorDomainsCheckByDomain",
    params(
        ("domain" = String, Path, description = "Domain to check")
    ),
    responses(
        (status = 200, description = "Domain check result", body = CheckAnchorDomainResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn check_anchor_domain(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(domain): Path<String>,
) -> Result<Json<CheckAnchorDomainResponse>, PlatformError> {
    crate::checks::can_read_anchor_domains(&auth.0)?;

    let is_anchor = state
        .anchor_domain_repo
        .is_anchor_domain(&domain.to_lowercase())
        .await?;

    Ok(Json(CheckAnchorDomainResponse {
        is_anchor_domain: is_anchor,
    }))
}

/// Delete anchor domain
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "anchor-domains",
    operation_id = "deleteApiAnchorDomainsById",
    params(
        ("id" = String, Path, description = "Anchor domain ID")
    ),
    responses(
        (status = 204, description = "Anchor domain deleted"),
        (status = 404, description = "Anchor domain not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_anchor_domain(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    use crate::auth::operations::DeleteAnchorDomainCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_delete_anchor_domains(&auth.0)?;

    let cmd = DeleteAnchorDomainCommand {
        anchor_domain_id: id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .delete_anchor_domain_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Update anchor domain request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAnchorDomainRequest {
    /// New domain value
    pub domain: String,
}

/// Update anchor domain
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "anchor-domains",
    operation_id = "putApiAnchorDomainsById",
    params(
        ("id" = String, Path, description = "Anchor domain ID")
    ),
    request_body = UpdateAnchorDomainRequest,
    responses(
        (status = 204, description = "Anchor domain updated"),
        (status = 404, description = "Anchor domain not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_anchor_domain(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateAnchorDomainRequest>,
) -> Result<StatusCode, PlatformError> {
    use crate::auth::operations::UpdateAnchorDomainCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_update_anchor_domains(&auth.0)?;

    let cmd = UpdateAnchorDomainCommand {
        anchor_domain_id: id,
        domain: req.domain,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .update_anchor_domain_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Client Auth Config Handlers
// ============================================================================

/// Create client auth config
#[utoipa::path(
    post,
    path = "",
    tag = "auth-configs",
    operation_id = "postApiAuthConfigs",
    request_body = CreateClientAuthConfigRequest,
    responses(
        (status = 201, description = "Client auth config created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate email domain")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_client_auth_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Json(req): Json<CreateClientAuthConfigRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), PlatformError> {
    use crate::auth::operations::CreateAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_create_auth_configs(&auth.0)?;

    // Go validates the domain before the enums; an invalid domain is the
    // use case's INVALID_EMAIL_DOMAIN whatever the other fields say.
    let domain_ok = {
        let d = req.email_domain.trim();
        !d.is_empty() && d.contains('.')
    };
    let (config_type, auth_provider) = if domain_ok {
        (
            parse_config_type(&req.config_type)?,
            parse_auth_provider(&req.auth_provider)?,
        )
    } else {
        (Default::default(), AuthProvider::Internal)
    };
    let oidc_client_secret_ref = crate::identity_provider::api::seal_client_secret(
        req.oidc_client_secret_ref,
        state.encryption_service.as_deref(),
    )?;
    let cmd = CreateAuthConfigCommand {
        email_domain: req.email_domain.trim().to_lowercase(),
        config_type,
        primary_client_id: req.primary_client_id,
        additional_client_ids: req.additional_client_ids,
        granted_client_ids: req.granted_client_ids,
        auth_provider: Some(auth_provider),
        oidc_issuer_url: req.oidc_issuer_url,
        oidc_client_id: req.oidc_client_id,
        oidc_multi_tenant: req.oidc_multi_tenant,
        oidc_issuer_pattern: req.oidc_issuer_pattern,
        oidc_client_secret_ref,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state
        .create_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse::new(event.auth_config_id)),
    ))
}

/// Get client auth config by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "auth-configs",
    operation_id = "getApiAuthConfigsById",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    responses(
        (status = 200, description = "Client auth config found", body = ClientAuthConfigResponse),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_auth_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ClientAuthConfigResponse>, PlatformError> {
    crate::checks::can_read_auth_configs(&auth.0)?;

    let config = state
        .client_auth_config_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &id))?;

    Ok(Json(config.into()))
}

/// List client auth configs
#[utoipa::path(
    get,
    path = "",
    tag = "auth-configs",
    operation_id = "getApiAuthConfigs",
    responses(
        (status = 200, description = "List of client auth configs", body = AuthConfigListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_client_auth_configs(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
) -> Result<Json<AuthConfigListResponse>, PlatformError> {
    crate::checks::can_read_auth_configs(&auth.0)?;

    let items = state
        .client_auth_config_repo
        .find_all()
        .await?
        .into_iter()
        .map(ClientAuthConfigResponse::from)
        .collect();
    Ok(Json(AuthConfigListResponse { items }))
}

/// Update client auth config
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "auth-configs",
    operation_id = "putApiAuthConfigsById",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateClientAuthConfigRequest,
    responses(
        (status = 204, description = "Client auth config updated"),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_client_auth_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateClientAuthConfigRequest>,
) -> Result<StatusCode, PlatformError> {
    use crate::auth::operations::UpdateAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_update_auth_configs(&auth.0)?;

    let auth_provider = req
        .auth_provider
        .as_deref()
        .map(parse_auth_provider)
        .transpose()?;
    let oidc_client_secret_ref = crate::identity_provider::api::seal_client_secret(
        req.oidc_client_secret_ref,
        state.encryption_service.as_deref(),
    )?;
    let cmd = UpdateAuthConfigCommand {
        auth_config_id: id,
        primary_client_id: req.primary_client_id,
        auth_provider,
        oidc_issuer_url: req.oidc_issuer_url,
        oidc_client_id: req.oidc_client_id,
        oidc_multi_tenant: req.oidc_multi_tenant,
        oidc_issuer_pattern: req.oidc_issuer_pattern,
        oidc_client_secret_ref,
        additional_client_ids: req.additional_client_ids,
        granted_client_ids: req.granted_client_ids,
        config_type: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .update_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete client auth config
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "auth-configs",
    operation_id = "deleteApiAuthConfigsById",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    responses(
        (status = 204, description = "Client auth config deleted"),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_client_auth_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    use crate::auth::operations::DeleteAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_delete_auth_configs(&auth.0)?;

    let cmd = DeleteAuthConfigCommand { auth_config_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .delete_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Update config type request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfigTypeRequest {
    /// Config type: ANCHOR, PARTNER, or CLIENT
    pub config_type: String,
}

/// Update client auth config type
#[utoipa::path(
    put,
    path = "/{id}/config-type",
    tag = "auth-configs",
    operation_id = "putApiAuthConfigsByIdConfigType",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateConfigTypeRequest,
    responses(
        (status = 204, description = "Config type updated"),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_config_type(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateConfigTypeRequest>,
) -> Result<StatusCode, PlatformError> {
    use crate::auth::operations::UpdateAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_update_auth_configs(&auth.0)?;

    let cmd = UpdateAuthConfigCommand {
        auth_config_id: id,
        primary_client_id: None,
        auth_provider: None,
        oidc_issuer_url: None,
        oidc_client_id: None,
        oidc_multi_tenant: None,
        oidc_issuer_pattern: None,
        oidc_client_secret_ref: None,
        additional_client_ids: None,
        granted_client_ids: None,
        config_type: Some(parse_config_type(&req.config_type)?),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .update_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Get client auth config by email domain
#[utoipa::path(
    get,
    path = "/by-domain/{domain}",
    tag = "auth-configs",
    operation_id = "getApiAuthConfigsByDomainByDomain",
    params(
        ("domain" = String, Path, description = "Email domain")
    ),
    responses(
        (status = 200, description = "Client auth config found", body = ClientAuthConfigResponse),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_by_domain(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(domain): Path<String>,
) -> Result<Json<ClientAuthConfigResponse>, PlatformError> {
    crate::checks::can_read_auth_configs(&auth.0)?;

    let config = state
        .client_auth_config_repo
        .find_by_email_domain(&domain.to_lowercase())
        .await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &domain))?;

    Ok(Json(config.into()))
}

/// Create internal auth config
#[utoipa::path(
    post,
    path = "/internal",
    tag = "auth-configs",
    operation_id = "postApiAuthConfigsInternal",
    request_body = CreateInternalAuthConfigRequest,
    responses(
        (status = 201, description = "Internal auth config created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate email domain")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_internal_auth_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Json(req): Json<CreateInternalAuthConfigRequest>,
) -> Result<Json<CreatedResponse>, PlatformError> {
    crate::checks::can_create_auth_configs(&auth.0)?;

    use crate::auth::operations::CreateAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    let email_domain = req.email_domain.to_lowercase();
    let cmd = CreateAuthConfigCommand {
        email_domain: email_domain.clone(),
        config_type: parse_config_type(&req.config_type)?,
        primary_client_id: req.primary_client_id.clone(),
        additional_client_ids: None,
        granted_client_ids: None,
        auth_provider: Some(AuthProvider::Internal),
        oidc_issuer_url: None,
        oidc_client_id: None,
        oidc_multi_tenant: false,
        oidc_issuer_pattern: None,
        oidc_client_secret_ref: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .create_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let created = state
        .client_auth_config_repo
        .find_by_email_domain(&email_domain)
        .await?
        .ok_or_else(|| PlatformError::internal("Auth config commit succeeded but row not found"))?;
    Ok(Json(CreatedResponse::new(created.id)))
}

/// Create OIDC auth config
#[utoipa::path(
    post,
    path = "/oidc",
    tag = "auth-configs",
    operation_id = "postApiAuthConfigsOidc",
    request_body = CreateOidcAuthConfigRequest,
    responses(
        (status = 201, description = "OIDC auth config created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate email domain")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_oidc_auth_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Json(req): Json<CreateOidcAuthConfigRequest>,
) -> Result<Json<CreatedResponse>, PlatformError> {
    use crate::auth::operations::CreateAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_create_auth_configs(&auth.0)?;

    let email_domain = req.email_domain.to_lowercase();
    let cmd = CreateAuthConfigCommand {
        email_domain: email_domain.clone(),
        config_type: parse_config_type(&req.config_type)?,
        primary_client_id: req.primary_client_id.clone(),
        additional_client_ids: None,
        granted_client_ids: None,
        auth_provider: Some(AuthProvider::Oidc),
        oidc_issuer_url: Some(req.oidc_issuer_url.clone()),
        oidc_client_id: Some(req.oidc_client_id.clone()),
        oidc_multi_tenant: false,
        oidc_issuer_pattern: None,
        oidc_client_secret_ref: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .create_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let created = state
        .client_auth_config_repo
        .find_by_email_domain(&email_domain)
        .await?
        .ok_or_else(|| PlatformError::internal("Auth config commit succeeded but row not found"))?;
    Ok(Json(CreatedResponse::new(created.id)))
}

/// Update OIDC config
#[utoipa::path(
    put,
    path = "/{id}/oidc",
    tag = "auth-configs",
    operation_id = "putApiAuthConfigsByIdOidc",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateOidcConfigRequest,
    responses(
        (status = 204, description = "OIDC config updated"),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_oidc_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateOidcConfigRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::checks::can_update_auth_configs(&auth.0)?;

    use crate::auth::operations::UpdateAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    let cmd = UpdateAuthConfigCommand {
        auth_config_id: id,
        primary_client_id: None,
        auth_provider: Some(AuthProvider::Oidc),
        oidc_issuer_url: req.oidc_issuer_url.clone(),
        oidc_client_id: req.oidc_client_id.clone(),
        oidc_multi_tenant: None,
        oidc_issuer_pattern: None,
        oidc_client_secret_ref: None,
        additional_client_ids: None,
        granted_client_ids: None,
        config_type: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .update_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Update client binding
#[utoipa::path(
    put,
    path = "/{id}/client-binding",
    tag = "auth-configs",
    operation_id = "putApiAuthConfigsByIdClientBinding",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateClientBindingRequest,
    responses(
        (status = 204, description = "Client binding updated"),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_client_binding(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateClientBindingRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::checks::can_update_auth_configs(&auth.0)?;

    use crate::auth::operations::UpdateAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    let cmd = UpdateAuthConfigCommand {
        auth_config_id: id,
        primary_client_id: Some(req.primary_client_id),
        auth_provider: None,
        oidc_issuer_url: None,
        oidc_client_id: None,
        oidc_multi_tenant: None,
        oidc_issuer_pattern: None,
        oidc_client_secret_ref: None,
        additional_client_ids: None,
        granted_client_ids: None,
        config_type: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .update_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Update additional clients
#[utoipa::path(
    put,
    path = "/{id}/additional-clients",
    tag = "auth-configs",
    operation_id = "putApiAuthConfigsByIdAdditionalClients",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateAdditionalClientsRequest,
    responses(
        (status = 204, description = "Additional clients updated"),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_additional_clients(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateAdditionalClientsRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::checks::can_update_auth_configs(&auth.0)?;

    use crate::auth::operations::UpdateAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    let cmd = UpdateAuthConfigCommand {
        auth_config_id: id,
        primary_client_id: None,
        auth_provider: None,
        oidc_issuer_url: None,
        oidc_client_id: None,
        oidc_multi_tenant: None,
        oidc_issuer_pattern: None,
        oidc_client_secret_ref: None,
        additional_client_ids: Some(req.additional_client_ids),
        granted_client_ids: None,
        config_type: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .update_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Update granted clients
#[utoipa::path(
    put,
    path = "/{id}/granted-clients",
    tag = "auth-configs",
    operation_id = "putApiAuthConfigsByIdGrantedClients",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateGrantedClientsRequest,
    responses(
        (status = 204, description = "Granted clients updated"),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_granted_clients(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateGrantedClientsRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::checks::can_update_auth_configs(&auth.0)?;

    use crate::auth::operations::UpdateAuthConfigCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    let cmd = UpdateAuthConfigCommand {
        auth_config_id: id,
        primary_client_id: None,
        auth_provider: None,
        oidc_issuer_url: None,
        oidc_client_id: None,
        oidc_multi_tenant: None,
        oidc_issuer_pattern: None,
        oidc_client_secret_ref: None,
        additional_client_ids: None,
        granted_client_ids: Some(req.granted_client_ids),
        config_type: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .update_auth_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// IDP Role Mapping Handlers
// ============================================================================

/// Create IDP role mapping
#[utoipa::path(
    post,
    path = "",
    tag = "idp-role-mappings",
    operation_id = "postApiIdpRoleMappings",
    request_body = CreateIdpRoleMappingRequest,
    responses(
        (status = 201, description = "IDP role mapping created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate mapping")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_idp_role_mapping(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Json(req): Json<CreateIdpRoleMappingRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), PlatformError> {
    use crate::auth::operations::CreateIdpRoleMappingCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_update_identity_providers(&auth.0)?;
    // Owner ruling 14: a mapping hands its role out at login.
    crate::role::ceiling::require_role_change(
        &auth.0,
        &state.role_repo,
        &[],
        std::slice::from_ref(&req.platform_role_name),
    )
    .await?;

    let cmd = CreateIdpRoleMappingCommand {
        idp_type: req.idp_type,
        idp_role_name: req.idp_role_name,
        platform_role_name: req.platform_role_name,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state
        .create_idp_role_mapping_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse::new(event.mapping_id)),
    ))
}

/// Query parameters for IDP role mappings
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct IdpRoleMappingQuery {
    pub idp_type: Option<String>,
}

/// List IDP role mappings
#[utoipa::path(
    get,
    path = "",
    tag = "idp-role-mappings",
    operation_id = "getApiIdpRoleMappings",
    params(IdpRoleMappingQuery),
    responses(
        (status = 200, description = "List of IDP role mappings", body = IdpRoleMappingListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_idp_role_mappings(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Query(query): Query<IdpRoleMappingQuery>,
) -> Result<Json<IdpRoleMappingListResponse>, PlatformError> {
    crate::checks::can_read_identity_providers(&auth.0)?;

    let mappings = if let Some(ref idp_type) = query.idp_type {
        state
            .idp_role_mapping_repo
            .find_by_idp_type(idp_type)
            .await?
    } else {
        state.idp_role_mapping_repo.find_all().await?
    };

    let items = mappings
        .into_iter()
        .map(IdpRoleMappingResponse::from)
        .collect();
    Ok(Json(IdpRoleMappingListResponse { items }))
}

/// Delete IDP role mapping
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "idp-role-mappings",
    operation_id = "deleteApiIdpRoleMappingsById",
    params(
        ("id" = String, Path, description = "IDP role mapping ID")
    ),
    responses(
        (status = 204, description = "IDP role mapping deleted"),
        (status = 404, description = "IDP role mapping not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_idp_role_mapping(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    use crate::auth::operations::DeleteIdpRoleMappingCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_update_identity_providers(&auth.0)?;
    // Owner ruling 14: removing a mapping withdraws its role; a missing
    // mapping is the use case's 404.
    if let Some(mapping) = state.idp_role_mapping_repo.find_by_id(&id).await? {
        crate::role::ceiling::require_role_change(
            &auth.0,
            &state.role_repo,
            std::slice::from_ref(&mapping.platform_role_name),
            &[],
        )
        .await?;
    }

    let cmd = DeleteIdpRoleMappingCommand { mapping_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .delete_idp_role_mapping_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Routers
// ============================================================================

/// Create anchor domains router
pub fn anchor_domains_router(state: AuthConfigState) -> Router {
    Router::new()
        .route("/", post(create_anchor_domain).get(list_anchor_domains))
        .route("/check/{domain}", get(check_anchor_domain))
        .route(
            "/{id}",
            get(get_anchor_domain)
                .put(update_anchor_domain)
                .delete(delete_anchor_domain),
        )
        .with_state(state)
}

/// Create client auth configs router
pub fn client_auth_configs_router(state: AuthConfigState) -> Router {
    Router::new()
        .route(
            "/",
            post(create_client_auth_config).get(list_client_auth_configs),
        )
        .route("/internal", post(create_internal_auth_config))
        .route("/oidc", post(create_oidc_auth_config))
        .route("/by-domain/{domain}", get(get_by_domain))
        .route(
            "/{id}",
            get(get_client_auth_config)
                .put(update_client_auth_config)
                .delete(delete_client_auth_config),
        )
        .route("/{id}/config-type", axum::routing::put(update_config_type))
        .route("/{id}/oidc", axum::routing::put(update_oidc_config))
        .route(
            "/{id}/client-binding",
            axum::routing::put(update_client_binding),
        )
        .route(
            "/{id}/additional-clients",
            axum::routing::put(update_additional_clients),
        )
        .route(
            "/{id}/granted-clients",
            axum::routing::put(update_granted_clients),
        )
        .with_state(state)
}

/// Create IDP role mappings router
pub fn idp_role_mappings_router(state: AuthConfigState) -> Router {
    Router::new()
        .route(
            "/",
            post(create_idp_role_mapping).get(list_idp_role_mappings),
        )
        .route("/{id}", delete(delete_idp_role_mapping))
        .with_state(state)
}
