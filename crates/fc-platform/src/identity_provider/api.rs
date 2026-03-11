//! Identity Providers Admin API

use axum::{
    routing::{get, post},
    extract::{State, Path},
    Json, Router,
};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::entity::{IdentityProvider, IdentityProviderType};
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
}

#[utoipa::path(
    post,
    path = "",
    tag = "identity-providers",
    operation_id = "postApiAdminIdentityProviders",
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
    _auth: Authenticated,
    Json(req): Json<CreateIdentityProviderRequest>,
) -> Result<(axum::http::StatusCode, Json<IdentityProviderResponse>), PlatformError> {
    if state.idp_repo.find_by_code(&req.code).await?.is_some() {
        return Err(PlatformError::duplicate("IdentityProvider", "code", &req.code));
    }

    let idp_type = IdentityProviderType::from_str(&req.r#type);
    let mut idp = IdentityProvider::new(&req.code, &req.name, idp_type);
    idp.oidc_issuer_url = req.oidc_issuer_url;
    idp.oidc_client_id = req.oidc_client_id;
    idp.oidc_client_secret_ref = req.oidc_client_secret_ref;
    idp.oidc_multi_tenant = req.oidc_multi_tenant.unwrap_or(false);
    idp.oidc_issuer_pattern = req.oidc_issuer_pattern;
    idp.allowed_email_domains = req.allowed_email_domains.unwrap_or_default();

    state.idp_repo.insert(&idp).await?;
    Ok((axum::http::StatusCode::CREATED, Json(idp.into())))
}

#[utoipa::path(
    get,
    path = "",
    tag = "identity-providers",
    operation_id = "getApiAdminIdentityProviders",
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
    operation_id = "getApiAdminIdentityProvidersById",
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
    let idp = state.idp_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("IdentityProvider", &id))?;
    Ok(Json(idp.into()))
}

#[utoipa::path(
    put,
    path = "/{id}",
    tag = "identity-providers",
    operation_id = "putApiAdminIdentityProvidersById",
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
    _auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateIdentityProviderRequest>,
) -> Result<Json<IdentityProviderResponse>, PlatformError> {
    let mut idp = state.idp_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("IdentityProvider", &id))?;

    if let Some(name) = req.name { idp.name = name; }
    if let Some(url) = req.oidc_issuer_url { idp.oidc_issuer_url = Some(url); }
    if let Some(cid) = req.oidc_client_id { idp.oidc_client_id = Some(cid); }
    if let Some(secret) = req.oidc_client_secret_ref { idp.oidc_client_secret_ref = Some(secret); }
    if let Some(mt) = req.oidc_multi_tenant { idp.oidc_multi_tenant = mt; }
    if let Some(pattern) = req.oidc_issuer_pattern { idp.oidc_issuer_pattern = Some(pattern); }
    if let Some(domains) = req.allowed_email_domains { idp.allowed_email_domains = domains; }
    idp.updated_at = chrono::Utc::now();

    state.idp_repo.update(&idp).await?;
    Ok(Json(idp.into()))
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "identity-providers",
    operation_id = "deleteApiAdminIdentityProvidersById",
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
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, PlatformError> {
    let _ = state.idp_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("IdentityProvider", &id))?;
    state.idp_repo.delete(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub fn identity_providers_router(state: IdentityProvidersState) -> Router {
    Router::new()
        .route("/", post(create_identity_provider).get(list_identity_providers))
        .route("/:id", get(get_identity_provider).put(update_identity_provider).delete(delete_identity_provider))
        .with_state(state)
}
