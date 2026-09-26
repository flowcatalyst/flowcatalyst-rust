//! Identity Providers Admin API

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use super::entity::IdentityProvider;
use super::repository::IdentityProviderRepository;
use crate::shared::encryption_service::EncryptionService;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// Go `CreateIdentityProviderRequest` (identityprovider/api/dto.go).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdentityProviderRequest {
    pub code: String,
    pub name: String,
    pub r#type: String,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    /// The OIDC client secret in plaintext. It is encrypted before it is
    /// stored and never returned (see `hasClientSecret`).
    pub oidc_client_secret_ref: Option<String>,
    pub oidc_multi_tenant: Option<bool>,
    pub oidc_issuer_pattern: Option<String>,
    /// Email domains to route to this provider: mappings are created, or
    /// claimed from their current provider.
    pub allowed_email_domains: Option<Vec<String>>,
    /// Scope for mappings this request creates: `ANCHOR`, or `CLIENT`
    /// (requires `primaryClientId`). Required when a new mapping is created.
    pub mapping_scope: Option<String>,
    /// Client linked on mappings that are new or have no primary client.
    pub primary_client_id: Option<String>,
    /// Reconcile users' IDP_SYNC roles from the token's roles claim at login.
    #[serde(default)]
    pub sync_roles_from_idp: bool,
    /// Platform roles (by id) role sync may confer; empty = no restriction.
    pub allowed_role_ids: Option<Vec<String>>,
}

/// Go `UpdateIdentityProviderRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIdentityProviderRequest {
    pub name: Option<String>,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    /// The OIDC client secret in plaintext. It is encrypted before it is
    /// stored and never returned (see `hasClientSecret`).
    pub oidc_client_secret_ref: Option<String>,
    pub oidc_multi_tenant: Option<bool>,
    pub oidc_issuer_pattern: Option<String>,
    /// The desired set of routed domains: additions are mapped or claimed,
    /// removals fall back to internal authentication.
    pub allowed_email_domains: Option<Vec<String>>,
    pub mapping_scope: Option<String>,
    pub primary_client_id: Option<String>,
    pub sync_roles_from_idp: Option<bool>,
    pub allowed_role_ids: Option<Vec<String>>,
}

/// Go `IdentityProviderResponse`: optional members absent when unset, the
/// secret never serialised (only `hasClientSecret`).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityProviderResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    pub has_client_secret: bool,
    pub oidc_multi_tenant: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_pattern: Option<String>,
    /// The domains routed to this provider (its email-domain mappings).
    pub allowed_email_domains: Vec<String>,
    pub sync_roles_from_idp: bool,
    pub allowed_role_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<IdentityProvider> for IdentityProviderResponse {
    fn from(idp: IdentityProvider) -> Self {
        let has_secret = idp.has_client_secret();
        Self {
            id: idp.id,
            code: idp.code,
            name: idp.name,
            r#type: idp.r#type.as_str().to_string(),
            oidc_issuer_url: idp.oidc_issuer_url,
            oidc_client_id: idp.oidc_client_id,
            has_client_secret: has_secret,
            oidc_multi_tenant: idp.oidc_multi_tenant,
            oidc_issuer_pattern: idp.oidc_issuer_pattern,
            allowed_email_domains: idp.allowed_email_domains,
            sync_roles_from_idp: idp.sync_roles_from_idp,
            allowed_role_ids: idp.allowed_role_ids,
            created_at: idp.created_at.to_rfc3339(),
            updated_at: idp.updated_at.to_rfc3339(),
        }
    }
}

/// Go `ParseType`: `INTERNAL` or `OIDC`, exactly.
fn parse_idp_type(value: &str) -> Result<crate::IdentityProviderType, PlatformError> {
    match value {
        "INTERNAL" => Ok(crate::IdentityProviderType::Internal),
        "OIDC" => Ok(crate::IdentityProviderType::Oidc),
        _ => Err(PlatformError::bad_request_code(
            "INVALID_TYPE",
            "type must be INTERNAL or OIDC",
        )),
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityProvidersListResponse {
    pub identity_providers: Vec<IdentityProviderResponse>,
    pub total: usize,
}

#[derive(Clone)]
pub struct IdentityProvidersState {
    pub idp_repo: Arc<IdentityProviderRepository>,
    /// The mappings a create or update routes, and their move side effects.
    pub domains: crate::identity_provider::operations::DomainDeps,
    /// Role definitions, for the role ceiling on `allowedRoleIds`.
    pub role_repo: Arc<crate::RoleRepository>,
    /// Opens the one transaction a create or update runs in.
    pub pg_unit_of_work: Arc<crate::usecase::PgUnitOfWork>,
    pub create_use_case: Arc<
        crate::identity_provider::operations::CreateIdentityProviderUseCase<
            crate::usecase::PgUnitOfWork,
        >,
    >,
    pub update_use_case: Arc<
        crate::identity_provider::operations::UpdateIdentityProviderUseCase<
            crate::usecase::PgUnitOfWork,
        >,
    >,
    pub delete_use_case: Arc<
        crate::identity_provider::operations::DeleteIdentityProviderUseCase<
            crate::usecase::PgUnitOfWork,
        >,
    >,
    /// Encrypts the OIDC client secret before it reaches the command. `None`
    /// when no key is configured; a request that carries a secret then fails.
    pub encryption_service: Option<Arc<EncryptionService>>,
}

/// The stored form of the client secret from a create/update request, as
/// Go's `encryptSecretRef` (identityprovider/api/api.go): a secret-manager
/// reference (`aws-sm://…`, `env://…`, …) or an `encrypted:` value is kept
/// as sent; a plaintext is encrypted. This happens before the command is
/// built, so the plaintext never reaches the command (which the unit of work
/// writes to the audit log). A blank value means "not provided". Without a
/// key a plaintext is a 400 `ENCRYPTION_NOT_CONFIGURED`; an unknown
/// `<scheme>://` is a 400 `UNSUPPORTED_SECRET_SCHEME`. A secret is never
/// stored in plaintext.
pub(crate) fn seal_client_secret(
    secret: Option<String>,
    enc: Option<&EncryptionService>,
) -> Result<Option<String>, PlatformError> {
    use crate::shared::secret_ref::{seal_secret_ref, SecretRefError};
    secret
        .filter(|s| !s.trim().is_empty())
        .map(|s| match seal_secret_ref(enc, &s) {
            Ok(stored) => Ok(stored),
            Err(SecretRefError::NotConfigured) => Err(PlatformError::bad_request_code(
                "ENCRYPTION_NOT_CONFIGURED",
                "cannot store OIDC client secret: FLOWCATALYST_APP_KEY is not configured",
            )),
            Err(e @ SecretRefError::UnsupportedScheme { .. }) => Err(
                PlatformError::bad_request_code("UNSUPPORTED_SECRET_SCHEME", e.to_string()),
            ),
            Err(e) => Err(PlatformError::internal(format!(
                "encrypt OIDC client secret: {e}"
            ))),
        })
        .transpose()
}

#[utoipa::path(
    post,
    path = "",
    tag = "identity-providers",
    operation_id = "postApiIdentityProviders",
    request_body = CreateIdentityProviderRequest,
    responses(
        (status = 201, description = "Identity provider created", body = IdentityProviderResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
async fn create_identity_provider(
    State(state): State<IdentityProvidersState>,
    auth: Authenticated,
    Json(req): Json<CreateIdentityProviderRequest>,
) -> Result<(axum::http::StatusCode, Json<IdentityProviderResponse>), PlatformError> {
    use crate::identity_provider::operations::{
        CreateIdentityProviderCommand, CreateIdentityProviderUseCase,
    };
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_create_identity_providers(&auth.0)?;
    let allowed_role_ids = req.allowed_role_ids.unwrap_or_default();
    // Owner ruling 14: the allow-list bounds the roles a login through this
    // provider may hand out, so it is bounded by the role ceiling.
    crate::role::ceiling::require_role_ref_change(
        &auth.0,
        &state.role_repo,
        &[],
        &allowed_role_ids,
    )
    .await?;

    let cmd = CreateIdentityProviderCommand {
        idp_type: parse_idp_type(&req.r#type)?,
        code: req.code,
        name: req.name,
        oidc_issuer_url: req.oidc_issuer_url,
        oidc_client_id: req.oidc_client_id,
        oidc_client_secret_ref: seal_client_secret(
            req.oidc_client_secret_ref,
            state.encryption_service.as_deref(),
        )?,
        oidc_multi_tenant: req.oidc_multi_tenant.unwrap_or(false),
        oidc_issuer_pattern: req.oidc_issuer_pattern,
        allowed_email_domains: req.allowed_email_domains.unwrap_or_default(),
        mapping_scope: req.mapping_scope,
        primary_client_id: req.primary_client_id,
        sync_roles_from_idp: req.sync_roles_from_idp,
        allowed_role_ids,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let (idp_repo, domains) = (state.idp_repo.clone(), state.domains.clone());
    let event = state
        .pg_unit_of_work
        .run(|session| async move {
            CreateIdentityProviderUseCase::new(idp_repo, domains, session)
                .run(cmd, ctx)
                .await
        })
        .await
        .into_result()?;

    // Go answers with the provider (the SPA's toast reads its name).
    let idp = state
        .idp_repo
        .find_by_id(&event.identity_provider_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("IdentityProvider", &event.identity_provider_id))?;
    Ok((axum::http::StatusCode::CREATED, Json(idp.into())))
}

#[utoipa::path(
    get,
    path = "",
    tag = "identity-providers",
    operation_id = "getApiIdentityProviders",
    responses(
        (status = 200, description = "List of identity providers", body = IdentityProvidersListResponse)
    ),
    security(("bearer_auth" = []))
)]
async fn list_identity_providers(
    State(state): State<IdentityProvidersState>,
    auth: Authenticated,
) -> Result<Json<IdentityProvidersListResponse>, PlatformError> {
    crate::checks::can_read_identity_providers(&auth.0)?;

    let idps = state.idp_repo.find_all().await?;
    let total = idps.len();
    Ok(Json(IdentityProvidersListResponse {
        identity_providers: idps.into_iter().map(|i| i.into()).collect(),
        total,
    }))
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "identity-providers",
    operation_id = "getApiIdentityProvidersById",
    params(
        ("id" = String, Path, description = "Identity provider ID")
    ),
    responses(
        (status = 200, description = "Identity provider found", body = IdentityProviderResponse),
        (status = 404, description = "Identity provider not found")
    ),
    security(("bearer_auth" = []))
)]
async fn get_identity_provider(
    State(state): State<IdentityProvidersState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<IdentityProviderResponse>, PlatformError> {
    crate::checks::can_read_identity_providers(&auth.0)?;

    let idp = state
        .idp_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("IdentityProvider", &id))?;
    Ok(Json(idp.into()))
}

#[utoipa::path(
    put,
    path = "/{id}",
    tag = "identity-providers",
    operation_id = "putApiIdentityProvidersById",
    params(
        ("id" = String, Path, description = "Identity provider ID")
    ),
    request_body = UpdateIdentityProviderRequest,
    responses(
        (status = 200, description = "Identity provider updated", body = IdentityProviderResponse),
        (status = 404, description = "Identity provider not found")
    ),
    security(("bearer_auth" = []))
)]
async fn update_identity_provider(
    State(state): State<IdentityProvidersState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateIdentityProviderRequest>,
) -> Result<Json<IdentityProviderResponse>, PlatformError> {
    use crate::identity_provider::operations::{
        UpdateIdentityProviderCommand, UpdateIdentityProviderUseCase,
    };
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_update_identity_providers(&auth.0)?;
    // Owner ruling 14, as on create; a missing provider is the use case's
    // 404.
    if let Some(after) = req.allowed_role_ids.as_deref() {
        if let Some(existing) = state.idp_repo.find_by_id(&id).await? {
            crate::role::ceiling::require_role_ref_change(
                &auth.0,
                &state.role_repo,
                &existing.allowed_role_ids,
                after,
            )
            .await?;
        }
    }

    let cmd = UpdateIdentityProviderCommand {
        idp_id: id.clone(),
        name: req.name,
        oidc_issuer_url: req.oidc_issuer_url,
        oidc_client_id: req.oidc_client_id,
        oidc_client_secret_ref: seal_client_secret(
            req.oidc_client_secret_ref,
            state.encryption_service.as_deref(),
        )?,
        oidc_multi_tenant: req.oidc_multi_tenant,
        oidc_issuer_pattern: req.oidc_issuer_pattern,
        allowed_email_domains: req.allowed_email_domains,
        mapping_scope: req.mapping_scope,
        primary_client_id: req.primary_client_id,
        sync_roles_from_idp: req.sync_roles_from_idp,
        allowed_role_ids: req.allowed_role_ids,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let (idp_repo, domains) = (state.idp_repo.clone(), state.domains.clone());
    state
        .pg_unit_of_work
        .run(|session| async move {
            UpdateIdentityProviderUseCase::new(idp_repo, domains, session)
                .run(cmd, ctx)
                .await
        })
        .await
        .into_result()?;

    // Go answers 200 with the updated provider (the SPA's detail page sets
    // it as the view's model).
    let idp = state
        .idp_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("IdentityProvider", &id))?;
    Ok(Json(idp.into()))
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "identity-providers",
    operation_id = "deleteApiIdentityProvidersById",
    params(
        ("id" = String, Path, description = "Identity provider ID")
    ),
    responses(
        (status = 204, description = "Identity provider deleted"),
        (status = 404, description = "Identity provider not found")
    ),
    security(("bearer_auth" = []))
)]
async fn delete_identity_provider(
    State(state): State<IdentityProvidersState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, PlatformError> {
    use crate::identity_provider::operations::DeleteIdentityProviderCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_delete_identity_providers(&auth.0)?;

    let cmd = DeleteIdentityProviderCommand { idp_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub fn identity_providers_router(state: IdentityProvidersState) -> Router {
    Router::new()
        .route(
            "/",
            post(create_identity_provider).get(list_identity_providers),
        )
        .route(
            "/{id}",
            get(get_identity_provider)
                .put(update_identity_provider)
                .delete(delete_identity_provider),
        )
        .with_state(state)
}
