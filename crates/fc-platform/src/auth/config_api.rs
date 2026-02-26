//! Auth Configuration Admin API
//!
//! REST endpoints for authentication configuration management.
//! Includes anchor domains, client auth configs, and IDP role mappings.

use axum::{
    routing::{get, post, delete},
    extract::{State, Path, Query},
    Json, Router,
};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::config_entity::{
    AnchorDomain, ClientAuthConfig, IdpRoleMapping,
    AuthConfigType, AuthProvider,
};
use crate::{
    AnchorDomainRepository, ClientAuthConfigRepository, IdpRoleMappingRepository,
};
use crate::shared::error::PlatformError;
use crate::shared::api_common::{CreatedResponse, SuccessResponse};
use crate::shared::middleware::Authenticated;

// ============================================================================
// Anchor Domains
// ============================================================================

/// Create anchor domain request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAnchorDomainRequest {
    /// Email domain (e.g., "flowcatalyst.tech")
    pub domain: String,
}

/// Anchor domain response DTO (matches Java AnchorDomainDto)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnchorDomainResponse {
    pub id: String,
    pub domain: String,
    /// Number of users with this email domain
    pub user_count: i64,
    pub created_at: String,
}

/// Anchor domain list response (wrapped)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnchorDomainListResponse {
    pub domains: Vec<AnchorDomainResponse>,
    pub total: usize,
}

impl AnchorDomainResponse {
    /// Create response with user count (matches Java toDto method)
    pub fn from_domain(d: AnchorDomain, user_count: i64) -> Self {
        Self {
            id: d.id,
            domain: d.domain,
            user_count,
            created_at: d.created_at.to_rfc3339(),
        }
    }
}

// ============================================================================
// Client Auth Configs
// ============================================================================

/// Create client auth config request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateClientAuthConfigRequest {
    /// Email domain this config applies to
    pub email_domain: String,

    /// Config type: ANCHOR, PARTNER, or CLIENT
    #[serde(default)]
    pub config_type: Option<String>,

    /// Primary client ID (for CLIENT type)
    pub primary_client_id: Option<String>,

    /// Auth provider: INTERNAL or OIDC
    #[serde(default)]
    pub auth_provider: Option<String>,

    /// OIDC issuer URL
    pub oidc_issuer_url: Option<String>,

    /// OIDC client ID
    pub oidc_client_id: Option<String>,
}

/// Update client auth config request
#[derive(Debug, Deserialize, ToSchema)]
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
}

/// Create internal auth config request
#[derive(Debug, Deserialize, ToSchema)]
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
#[derive(Debug, Deserialize, ToSchema)]
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
    /// OIDC client secret reference (optional)
    pub oidc_client_secret_ref: Option<String>,
}

/// Update OIDC config request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOidcConfigRequest {
    /// OIDC issuer URL
    pub oidc_issuer_url: Option<String>,
    /// OIDC client ID
    pub oidc_client_id: Option<String>,
    /// OIDC client secret reference
    pub oidc_client_secret_ref: Option<String>,
}

/// Update client binding request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientBindingRequest {
    /// Primary client ID
    pub primary_client_id: String,
}

/// Update additional clients request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAdditionalClientsRequest {
    /// Additional client IDs
    pub additional_client_ids: Vec<String>,
}

/// Update granted clients request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGrantedClientsRequest {
    /// Granted client IDs
    pub granted_client_ids: Vec<String>,
}

/// Validate secret request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateSecretRequest {
    /// Secret reference to validate
    pub secret_ref: String,
}

/// Validate secret response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateSecretResponse {
    /// Whether the secret is valid
    pub valid: bool,
    /// Error message if invalid
    pub error: Option<String>,
}

/// Client auth config response DTO (matches Java AuthConfigDto)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAuthConfigResponse {
    pub id: String,
    pub email_domain: String,
    pub config_type: String,
    pub primary_client_id: Option<String>,
    pub additional_client_ids: Vec<String>,
    /// Granted client IDs (for PARTNER type configs)
    pub granted_client_ids: Vec<String>,
    /// Deprecated - use primaryClientId
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub auth_provider: String,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    /// Whether a client secret is configured
    pub has_client_secret: bool,
    /// Whether OIDC is multi-tenant
    pub oidc_multi_tenant: bool,
    /// Issuer pattern for multi-tenant validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_pattern: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Client auth config list response (wrapped)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigListResponse {
    pub configs: Vec<ClientAuthConfigResponse>,
    pub total: usize,
}

impl From<ClientAuthConfig> for ClientAuthConfigResponse {
    fn from(c: ClientAuthConfig) -> Self {
        Self {
            id: c.id.clone(),
            email_domain: c.email_domain,
            config_type: format!("{:?}", c.config_type).to_uppercase(),
            primary_client_id: c.primary_client_id.clone(),
            additional_client_ids: c.additional_client_ids.clone(),
            granted_client_ids: c.granted_client_ids,
            client_id: c.primary_client_id, // deprecated
            auth_provider: format!("{:?}", c.auth_provider).to_uppercase(),
            oidc_issuer_url: c.oidc_issuer_url,
            oidc_client_id: c.oidc_client_id,
            has_client_secret: c.oidc_client_secret_ref.is_some(),
            oidc_multi_tenant: c.oidc_multi_tenant,
            oidc_issuer_pattern: c.oidc_issuer_pattern,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

// ============================================================================
// IDP Role Mappings
// ============================================================================

/// Create IDP role mapping request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdpRoleMappingRequest {
    /// IDP type (e.g., "OIDC", "AZURE_AD")
    pub idp_type: String,

    /// Role name from the IDP
    pub idp_role_name: String,

    /// Platform role name to map to
    pub platform_role_name: String,
}

/// IDP role mapping response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMappingResponse {
    pub id: String,
    pub idp_type: String,
    pub idp_role_name: String,
    pub platform_role_name: String,
    pub created_at: String,
}

/// IDP role mapping list response (wrapped)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMappingListResponse {
    pub mappings: Vec<IdpRoleMappingResponse>,
    pub total: usize,
}

impl From<IdpRoleMapping> for IdpRoleMappingResponse {
    fn from(m: IdpRoleMapping) -> Self {
        Self {
            id: m.id,
            idp_type: m.idp_type,
            idp_role_name: m.idp_role_name,
            platform_role_name: m.platform_role_name,
            created_at: m.created_at.to_rfc3339(),
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
    /// Optional - needed for counting users by email domain
    pub principal_repo: Option<Arc<crate::PrincipalRepository>>,
}

fn parse_config_type(s: &str) -> AuthConfigType {
    match s.to_uppercase().as_str() {
        "ANCHOR" => AuthConfigType::Anchor,
        "PARTNER" => AuthConfigType::Partner,
        _ => AuthConfigType::Client,
    }
}

fn parse_auth_provider(s: &str) -> AuthProvider {
    match s.to_uppercase().as_str() {
        "OIDC" => AuthProvider::Oidc,
        _ => AuthProvider::Internal,
    }
}

// ============================================================================
// Anchor Domain Handlers
// ============================================================================

/// Create anchor domain
#[utoipa::path(
    post,
    path = "",
    tag = "auth-config",
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
) -> Result<Json<CreatedResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let domain = req.domain.to_lowercase();

    // Check for duplicate
    if state.anchor_domain_repo.is_anchor_domain(&domain).await? {
        return Err(PlatformError::duplicate("AnchorDomain", "domain", &domain));
    }

    let anchor_domain = AnchorDomain::new(&domain);
    let id = anchor_domain.id.clone();

    state.anchor_domain_repo.insert(&anchor_domain).await?;

    Ok(Json(CreatedResponse::new(id)))
}

/// List anchor domains
#[utoipa::path(
    get,
    path = "",
    tag = "auth-config",
    responses(
        (status = 200, description = "List of anchor domains", body = AnchorDomainListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_anchor_domains(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
) -> Result<Json<AnchorDomainListResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let anchor_domains = state.anchor_domain_repo.find_all().await?;

    // Convert to response DTOs with user counts (matches Java toDto)
    let mut domains = Vec::with_capacity(anchor_domains.len());
    for d in anchor_domains {
        let user_count = if let Some(ref principal_repo) = state.principal_repo {
            principal_repo.count_by_email_domain(&d.domain).await.unwrap_or(0)
        } else {
            0
        };
        domains.push(AnchorDomainResponse::from_domain(d, user_count));
    }

    let total = domains.len();
    Ok(Json(AnchorDomainListResponse { domains, total }))
}

/// Get anchor domain by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "auth-config",
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
    crate::checks::require_anchor(&auth.0)?;

    let domain = state.anchor_domain_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("AnchorDomain", &id))?;

    // Count users from this domain (matches Java toDto)
    let user_count = if let Some(ref principal_repo) = state.principal_repo {
        principal_repo.count_by_email_domain(&domain.domain).await.unwrap_or(0)
    } else {
        0
    };

    Ok(Json(AnchorDomainResponse::from_domain(domain, user_count)))
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
    tag = "auth-config",
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
    crate::checks::require_anchor(&auth.0)?;

    let is_anchor = state.anchor_domain_repo.is_anchor_domain(&domain.to_lowercase()).await?;

    Ok(Json(CheckAnchorDomainResponse {
        is_anchor_domain: is_anchor,
    }))
}

/// Delete anchor domain
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "auth-config",
    params(
        ("id" = String, Path, description = "Anchor domain ID")
    ),
    responses(
        (status = 200, description = "Anchor domain deleted", body = SuccessResponse),
        (status = 404, description = "Anchor domain not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_anchor_domain(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let exists = state.anchor_domain_repo.find_by_id(&id).await?.is_some();
    if !exists {
        return Err(PlatformError::not_found("AnchorDomain", &id));
    }

    state.anchor_domain_repo.delete(&id).await?;

    Ok(Json(SuccessResponse::ok()))
}

// ============================================================================
// Client Auth Config Handlers
// ============================================================================

/// Create client auth config
#[utoipa::path(
    post,
    path = "",
    tag = "auth-config",
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
) -> Result<Json<CreatedResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let email_domain = req.email_domain.to_lowercase();

    // Check for duplicate
    if state.client_auth_config_repo.find_by_email_domain(&email_domain).await?.is_some() {
        return Err(PlatformError::duplicate("ClientAuthConfig", "emailDomain", &email_domain));
    }

    let config_type = req.config_type.as_deref()
        .map(parse_config_type)
        .unwrap_or(AuthConfigType::Client);

    let mut config = match config_type {
        AuthConfigType::Partner => ClientAuthConfig::new_partner(&email_domain),
        _ => {
            let client_id = req.primary_client_id.unwrap_or_default();
            ClientAuthConfig::new_client(&email_domain, &client_id)
        }
    };

    if let Some(ref provider) = req.auth_provider {
        config.auth_provider = parse_auth_provider(provider);
    }

    if let Some(ref issuer) = req.oidc_issuer_url {
        if let Some(ref client_id) = req.oidc_client_id {
            config = config.with_oidc(issuer, client_id);
        }
    }

    let id = config.id.clone();
    state.client_auth_config_repo.insert(&config).await?;

    Ok(Json(CreatedResponse::new(id)))
}

/// Get client auth config by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "auth-config",
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
    crate::checks::require_anchor(&auth.0)?;

    let config = state.client_auth_config_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &id))?;

    Ok(Json(config.into()))
}

/// List client auth configs
#[utoipa::path(
    get,
    path = "",
    tag = "auth-config",
    responses(
        (status = 200, description = "List of client auth configs", body = AuthConfigListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_client_auth_configs(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
) -> Result<Json<AuthConfigListResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let configs = state.client_auth_config_repo.find_all().await?;
    let configs: Vec<ClientAuthConfigResponse> = configs.into_iter()
        .map(|c| c.into())
        .collect();
    let total = configs.len();

    Ok(Json(AuthConfigListResponse { configs, total }))
}

/// Update client auth config
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "auth-config",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateClientAuthConfigRequest,
    responses(
        (status = 200, description = "Client auth config updated", body = ClientAuthConfigResponse),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_client_auth_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateClientAuthConfigRequest>,
) -> Result<Json<ClientAuthConfigResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut config = state.client_auth_config_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &id))?;

    if let Some(client_id) = req.primary_client_id {
        config.primary_client_id = Some(client_id);
    }
    if let Some(ref provider) = req.auth_provider {
        config.auth_provider = parse_auth_provider(provider);
    }
    if let Some(issuer) = req.oidc_issuer_url {
        config.oidc_issuer_url = Some(issuer);
    }
    if let Some(client_id) = req.oidc_client_id {
        config.oidc_client_id = Some(client_id);
    }
    if let Some(additional) = req.additional_client_ids {
        config.additional_client_ids = additional;
    }

    config.updated_at = chrono::Utc::now();
    state.client_auth_config_repo.update(&config).await?;

    Ok(Json(config.into()))
}

/// Delete client auth config
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "auth-config",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    responses(
        (status = 200, description = "Client auth config deleted", body = SuccessResponse),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_client_auth_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let exists = state.client_auth_config_repo.find_by_id(&id).await?.is_some();
    if !exists {
        return Err(PlatformError::not_found("ClientAuthConfig", &id));
    }

    state.client_auth_config_repo.delete(&id).await?;

    Ok(Json(SuccessResponse::ok()))
}

/// Update config type request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfigTypeRequest {
    /// Config type: ANCHOR, PARTNER, or CLIENT
    pub config_type: String,
}

/// Update client auth config type
#[utoipa::path(
    put,
    path = "/{id}/config-type",
    tag = "auth-config",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateConfigTypeRequest,
    responses(
        (status = 200, description = "Config type updated", body = ClientAuthConfigResponse),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_config_type(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateConfigTypeRequest>,
) -> Result<Json<ClientAuthConfigResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut config = state.client_auth_config_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &id))?;

    config.config_type = parse_config_type(&req.config_type);
    config.updated_at = chrono::Utc::now();

    state.client_auth_config_repo.update(&config).await?;

    Ok(Json(config.into()))
}

/// Get client auth config by email domain
#[utoipa::path(
    get,
    path = "/by-domain/{domain}",
    tag = "auth-config",
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
    crate::checks::require_anchor(&auth.0)?;

    let config = state.client_auth_config_repo.find_by_email_domain(&domain.to_lowercase()).await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &domain))?;

    Ok(Json(config.into()))
}

/// Create internal auth config
#[utoipa::path(
    post,
    path = "/internal",
    tag = "auth-config",
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
    crate::checks::require_anchor(&auth.0)?;

    let email_domain = req.email_domain.to_lowercase();

    if state.client_auth_config_repo.find_by_email_domain(&email_domain).await?.is_some() {
        return Err(PlatformError::duplicate("ClientAuthConfig", "emailDomain", &email_domain));
    }

    let config_type = parse_config_type(&req.config_type);
    let config = match config_type {
        AuthConfigType::Partner => ClientAuthConfig::new_partner(&email_domain),
        _ => {
            let client_id = req.primary_client_id.unwrap_or_default();
            ClientAuthConfig::new_client(&email_domain, &client_id)
        }
    };

    let id = config.id.clone();
    state.client_auth_config_repo.insert(&config).await?;

    Ok(Json(CreatedResponse::new(id)))
}

/// Create OIDC auth config
#[utoipa::path(
    post,
    path = "/oidc",
    tag = "auth-config",
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
    crate::checks::require_anchor(&auth.0)?;

    let email_domain = req.email_domain.to_lowercase();

    if state.client_auth_config_repo.find_by_email_domain(&email_domain).await?.is_some() {
        return Err(PlatformError::duplicate("ClientAuthConfig", "emailDomain", &email_domain));
    }

    let config_type = parse_config_type(&req.config_type);
    let mut config = match config_type {
        AuthConfigType::Partner => ClientAuthConfig::new_partner(&email_domain),
        _ => {
            let client_id = req.primary_client_id.unwrap_or_default();
            ClientAuthConfig::new_client(&email_domain, &client_id)
        }
    };

    config = config.with_oidc(&req.oidc_issuer_url, &req.oidc_client_id);

    let id = config.id.clone();
    state.client_auth_config_repo.insert(&config).await?;

    Ok(Json(CreatedResponse::new(id)))
}

/// Update OIDC config
#[utoipa::path(
    put,
    path = "/{id}/oidc",
    tag = "auth-config",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateOidcConfigRequest,
    responses(
        (status = 200, description = "OIDC config updated", body = ClientAuthConfigResponse),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_oidc_config(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateOidcConfigRequest>,
) -> Result<Json<ClientAuthConfigResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut config = state.client_auth_config_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &id))?;

    if let Some(issuer) = req.oidc_issuer_url {
        config.oidc_issuer_url = Some(issuer);
    }
    if let Some(client_id) = req.oidc_client_id {
        config.oidc_client_id = Some(client_id);
    }
    config.auth_provider = AuthProvider::Oidc;
    config.updated_at = chrono::Utc::now();

    state.client_auth_config_repo.update(&config).await?;

    Ok(Json(config.into()))
}

/// Update client binding
#[utoipa::path(
    put,
    path = "/{id}/client-binding",
    tag = "auth-config",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateClientBindingRequest,
    responses(
        (status = 200, description = "Client binding updated", body = ClientAuthConfigResponse),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_client_binding(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateClientBindingRequest>,
) -> Result<Json<ClientAuthConfigResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut config = state.client_auth_config_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &id))?;

    config.primary_client_id = Some(req.primary_client_id);
    config.updated_at = chrono::Utc::now();

    state.client_auth_config_repo.update(&config).await?;

    Ok(Json(config.into()))
}

/// Update additional clients
#[utoipa::path(
    put,
    path = "/{id}/additional-clients",
    tag = "auth-config",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateAdditionalClientsRequest,
    responses(
        (status = 200, description = "Additional clients updated", body = ClientAuthConfigResponse),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_additional_clients(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateAdditionalClientsRequest>,
) -> Result<Json<ClientAuthConfigResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut config = state.client_auth_config_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &id))?;

    config.additional_client_ids = req.additional_client_ids;
    config.updated_at = chrono::Utc::now();

    state.client_auth_config_repo.update(&config).await?;

    Ok(Json(config.into()))
}

/// Update granted clients
#[utoipa::path(
    put,
    path = "/{id}/granted-clients",
    tag = "auth-config",
    params(
        ("id" = String, Path, description = "Client auth config ID")
    ),
    request_body = UpdateGrantedClientsRequest,
    responses(
        (status = 200, description = "Granted clients updated", body = ClientAuthConfigResponse),
        (status = 404, description = "Client auth config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_granted_clients(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateGrantedClientsRequest>,
) -> Result<Json<ClientAuthConfigResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut config = state.client_auth_config_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("ClientAuthConfig", &id))?;

    config.additional_client_ids = req.granted_client_ids;
    config.updated_at = chrono::Utc::now();

    state.client_auth_config_repo.update(&config).await?;

    Ok(Json(config.into()))
}

/// Validate secret reference
#[utoipa::path(
    post,
    path = "/validate-secret",
    tag = "auth-config",
    request_body = ValidateSecretRequest,
    responses(
        (status = 200, description = "Secret validation result", body = ValidateSecretResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn validate_secret(
    State(_state): State<AuthConfigState>,
    auth: Authenticated,
    Json(req): Json<ValidateSecretRequest>,
) -> Result<Json<ValidateSecretResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    // Basic validation - check if secret ref format is valid
    // In a real implementation, this would verify the secret exists in a vault
    let valid = !req.secret_ref.is_empty() && req.secret_ref.starts_with("secret://");

    Ok(Json(ValidateSecretResponse {
        valid,
        error: if valid { None } else { Some("Invalid secret reference format".to_string()) },
    }))
}

// ============================================================================
// IDP Role Mapping Handlers
// ============================================================================

/// Create IDP role mapping
#[utoipa::path(
    post,
    path = "",
    tag = "auth-config",
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
) -> Result<Json<CreatedResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    // Check for duplicate
    if state.idp_role_mapping_repo.find_by_idp_role(&req.idp_type, &req.idp_role_name).await?.is_some() {
        return Err(PlatformError::duplicate("IdpRoleMapping", "idpRole", &format!("{}:{}", req.idp_type, req.idp_role_name)));
    }

    let mapping = IdpRoleMapping::new(&req.idp_type, &req.idp_role_name, &req.platform_role_name);
    let id = mapping.id.clone();

    state.idp_role_mapping_repo.insert(&mapping).await?;

    Ok(Json(CreatedResponse::new(id)))
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
    tag = "auth-config",
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
    crate::checks::require_anchor(&auth.0)?;

    let mappings = if let Some(ref idp_type) = query.idp_type {
        state.idp_role_mapping_repo.find_by_idp_type(idp_type).await?
    } else {
        state.idp_role_mapping_repo.find_all().await?
    };

    let mappings: Vec<IdpRoleMappingResponse> = mappings.into_iter()
        .map(|m| m.into())
        .collect();
    let total = mappings.len();

    Ok(Json(IdpRoleMappingListResponse { mappings, total }))
}

/// Delete IDP role mapping
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "auth-config",
    params(
        ("id" = String, Path, description = "IDP role mapping ID")
    ),
    responses(
        (status = 200, description = "IDP role mapping deleted", body = SuccessResponse),
        (status = 404, description = "IDP role mapping not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_idp_role_mapping(
    State(state): State<AuthConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let exists = state.idp_role_mapping_repo.find_by_id(&id).await?.is_some();
    if !exists {
        return Err(PlatformError::not_found("IdpRoleMapping", &id));
    }

    state.idp_role_mapping_repo.delete(&id).await?;

    Ok(Json(SuccessResponse::ok()))
}

// ============================================================================
// Routers
// ============================================================================

/// Create anchor domains router
pub fn anchor_domains_router(state: AuthConfigState) -> Router {
    Router::new()
        .route("/", post(create_anchor_domain).get(list_anchor_domains))
        .route("/check/:domain", get(check_anchor_domain))
        .route("/:id", get(get_anchor_domain).delete(delete_anchor_domain))
        .with_state(state)
}

/// Create client auth configs router
pub fn client_auth_configs_router(state: AuthConfigState) -> Router {
    Router::new()
        .route("/", post(create_client_auth_config).get(list_client_auth_configs))
        .route("/internal", post(create_internal_auth_config))
        .route("/oidc", post(create_oidc_auth_config))
        .route("/validate-secret", post(validate_secret))
        .route("/by-domain/:domain", get(get_by_domain))
        .route("/:id", get(get_client_auth_config).put(update_client_auth_config).delete(delete_client_auth_config))
        .route("/:id/config-type", axum::routing::put(update_config_type))
        .route("/:id/oidc", axum::routing::put(update_oidc_config))
        .route("/:id/client-binding", axum::routing::put(update_client_binding))
        .route("/:id/additional-clients", axum::routing::put(update_additional_clients))
        .route("/:id/granted-clients", axum::routing::put(update_granted_clients))
        .with_state(state)
}

/// Create IDP role mappings router
pub fn idp_role_mappings_router(state: AuthConfigState) -> Router {
    Router::new()
        .route("/", post(create_idp_role_mapping).get(list_idp_role_mappings))
        .route("/:id", delete(delete_idp_role_mapping))
        .with_state(state)
}
