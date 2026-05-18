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
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdentityProviderRequest {
    pub code: String,
    pub name: String,
    pub r#type: String,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret_ref: Option<String>,
    pub oidc_multi_tenant: Option<bool>,
    pub oidc_issuer_pattern: Option<String>,
    pub allowed_email_domains: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIdentityProviderRequest {
    pub name: Option<String>,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret_ref: Option<String>,
    pub oidc_multi_tenant: Option<bool>,
    pub oidc_issuer_pattern: Option<String>,
    pub allowed_email_domains: Option<Vec<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityProviderResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub r#type: String,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub has_client_secret: bool,
    pub oidc_multi_tenant: bool,
    pub oidc_issuer_pattern: Option<String>,
    pub allowed_email_domains: Vec<String>,
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
            created_at: idp.created_at.to_rfc3339(),
            updated_at: idp.updated_at.to_rfc3339(),
        }
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
}

#[utoipa::path(
    post,
    path = "",
    tag = "identity-providers",
    operation_id = "postApiIdentityProviders",
    request_body = CreateIdentityProviderRequest,
    responses(
        (status = 201, description = "Identity provider created", body = crate::shared::api_common::CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
async fn create_identity_provider(
    State(state): State<IdentityProvidersState>,
    auth: Authenticated,
    Json(req): Json<CreateIdentityProviderRequest>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<crate::shared::api_common::CreatedResponse>,
    ),
    PlatformError,
> {
    use crate::identity_provider::operations::CreateIdentityProviderCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = CreateIdentityProviderCommand {
        code: req.code,
        name: req.name,
        idp_type: req.r#type,
        oidc_issuer_url: req.oidc_issuer_url,
        oidc_client_id: req.oidc_client_id,
        oidc_client_secret_ref: req.oidc_client_secret_ref,
        oidc_multi_tenant: req.oidc_multi_tenant.unwrap_or(false),
        oidc_issuer_pattern: req.oidc_issuer_pattern,
        allowed_email_domains: req.allowed_email_domains.unwrap_or_default(),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(crate::shared::api_common::CreatedResponse::new(
            event.idp_id,
        )),
    ))
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
    _auth: Authenticated,
) -> Result<Json<IdentityProvidersListResponse>, PlatformError> {
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
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<IdentityProviderResponse>, PlatformError> {
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
        (status = 204, description = "Identity provider updated"),
        (status = 404, description = "Identity provider not found")
    ),
    security(("bearer_auth" = []))
)]
async fn update_identity_provider(
    State(state): State<IdentityProvidersState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateIdentityProviderRequest>,
) -> Result<axum::http::StatusCode, PlatformError> {
    use crate::identity_provider::operations::UpdateIdentityProviderCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = UpdateIdentityProviderCommand {
        idp_id: id,
        name: req.name,
        oidc_issuer_url: req.oidc_issuer_url,
        oidc_client_id: req.oidc_client_id,
        oidc_client_secret_ref: req.oidc_client_secret_ref,
        oidc_multi_tenant: req.oidc_multi_tenant,
        oidc_issuer_pattern: req.oidc_issuer_pattern,
        allowed_email_domains: req.allowed_email_domains,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;
    Ok(axum::http::StatusCode::NO_CONTENT)
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

    crate::checks::require_anchor(&auth.0)?;

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
