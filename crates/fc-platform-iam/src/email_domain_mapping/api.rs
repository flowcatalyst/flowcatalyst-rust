//! Email Domain Mappings Admin API

use std::sync::Arc;

use axum::extract::Query;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::entity::EmailDomainMapping;
use super::entity::ScopeType;
use super::operations::move_provider::{
    MoveMappingToProviderCommand, MoveMappingToProviderUseCase,
};
use super::repository::EmailDomainMappingRepository;
use crate::identity_provider::repository::IdentityProviderRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = CreateMappingRequest)]
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
    #[schema(value_type = Option<i64>)]
    pub remember_device_days: Option<i32>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UpdateMappingRequest)]
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
    #[schema(value_type = Option<i64>)]
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
#[schema(as = MappingResponse)]
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
    #[schema(value_type = i64)]
    pub remember_device_days: i32,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
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
#[schema(as = MappingListResponse)]
pub struct EmailDomainMappingsListResponse {
    pub mappings: Vec<EmailDomainMappingResponse>,
    #[schema(value_type = i64)]
    pub total: usize,
}

#[derive(Clone)]
pub struct EmailDomainMappingsState {
    pub edm_repo: Arc<EmailDomainMappingRepository>,
    pub idp_repo: Arc<IdentityProviderRepository>,
    /// Role definitions (the role ceiling now applies to the identity
    /// provider's `allowedRoleIds`).
    pub role_repo: Arc<crate::role::repository::RoleRepository>,
    pub create_use_case: Arc<
        crate::email_domain_mapping::operations::CreateEmailDomainMappingUseCase<
            fc_platform_core::usecase::PgUnitOfWork,
        >,
    >,
    pub update_use_case: Arc<
        crate::email_domain_mapping::operations::UpdateEmailDomainMappingUseCase<
            fc_platform_core::usecase::PgUnitOfWork,
        >,
    >,
    pub delete_use_case: Arc<
        crate::email_domain_mapping::operations::DeleteEmailDomainMappingUseCase<
            fc_platform_core::usecase::PgUnitOfWork,
        >,
    >,
}

/// Create a new email domain mapping
#[utoipa::path(
    post,
    path = "",
    tag = "email-domain-mappings",
    operation_id = "createEmailDomainMapping",
    request_body = CreateEmailDomainMappingRequest,
    responses(
        (status = 201, description = "Email domain mapping created", body = fc_platform_core::shared::api_common::CreatedResponse),
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
        Json<fc_platform_core::shared::api_common::CreatedResponse>,
    ),
    PlatformError,
> {
    use crate::email_domain_mapping::operations::CreateEmailDomainMappingCommand;
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    fc_platform_core::shared::authorization_service::checks::can_create_email_domain_mappings(
        &auth.0,
    )?;

    let cmd = CreateEmailDomainMappingCommand {
        email_domain: req.email_domain,
        identity_provider_id: req.identity_provider_id,
        scope_type: parse_scope_type(&req.scope_type)?,
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
    let ctx = ExecutionContext::from_auth(&auth.0);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(fc_platform_core::shared::api_common::CreatedResponse::new(
            event.mapping_id,
        )),
    ))
}

/// List all email domain mappings
#[utoipa::path(
    get,
    path = "",
    tag = "email-domain-mappings",
    operation_id = "listEmailDomainMappings",
    responses(
        (status = 200, description = "List of email domain mappings", body = EmailDomainMappingsListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_email_domain_mappings(
    State(state): State<EmailDomainMappingsState>,
    auth: Authenticated,
) -> Result<Json<EmailDomainMappingsListResponse>, PlatformError> {
    fc_platform_core::shared::authorization_service::checks::can_read_email_domain_mappings(
        &auth.0,
    )?;

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
    operation_id = "getEmailDomainMapping",
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
    fc_platform_core::shared::authorization_service::checks::can_read_email_domain_mappings(
        &auth.0,
    )?;

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
    operation_id = "updateEmailDomainMapping",
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
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    fc_platform_core::shared::authorization_service::checks::can_update_email_domain_mappings(
        &auth.0,
    )?;

    let cmd = UpdateEmailDomainMappingCommand {
        mapping_id: id,
        identity_provider_id: req.identity_provider_id,
        scope_type: fc_platform_core::shared::enum_str::parse_opt(req.scope_type.as_deref())?,
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
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.update_use_case.run(cmd, ctx).await.into_result()?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Delete an email domain mapping
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "email-domain-mappings",
    operation_id = "deleteEmailDomainMapping",
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
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    fc_platform_core::shared::authorization_service::checks::can_delete_email_domain_mappings(
        &auth.0,
    )?;

    let cmd = DeleteEmailDomainMappingCommand { mapping_id: id };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ─── Go-parity lookup routes (formerly lookup_api.rs) ─────────────────────────
//
// Email-domain-mapping routes Go serves that Rust lacked
// (`emaildomainmapping/api/api.go:45-49`):
//
// - `GET  /api/email-domain-mappings/lookup?domain=`         (no auth; `{found:false}` when absent)
// - `GET  /api/email-domain-mappings/by-domain/{domain}`     (anchor + `…:email-domain-mapping:view`)
// - `POST /api/email-domain-mappings/{id}/move-provider`     (anchor + `…:email-domain-mapping:update`)
//
// Both lookups match the domain exactly, as Go's do.

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
    #[schema(value_type = i64)]
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
    params(("domain" = Option<String>, Query, description = "Email domain to look up (e.g. example.com)")),
    responses(
        (status = 200, description = "The mapping, or {found: false}", body = serde_json::Value),
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
        fc_platform_core::permissions::admin::EMAIL_DOMAIN_MAPPING_READ,
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
