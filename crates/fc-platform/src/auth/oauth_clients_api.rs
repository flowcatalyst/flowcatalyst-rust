//! OAuth Clients Admin API
//!
//! REST endpoints for OAuth client management.

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::oauth_entity::{OAuthClient, OAuthClientType, GrantType};
use crate::OAuthClientRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::{PaginationParams, CreatedResponse, SuccessResponse};
use crate::shared::middleware::Authenticated;

/// Create OAuth client request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOAuthClientRequest {
    /// OAuth client_id (public identifier)
    pub client_id: String,

    /// Human-readable name
    pub client_name: String,

    /// Client type (PUBLIC or CONFIDENTIAL)
    #[serde(default)]
    pub client_type: Option<String>,

    /// Allowed redirect URIs
    #[serde(default)]
    pub redirect_uris: Vec<String>,

    /// Allowed grant types
    #[serde(default)]
    pub grant_types: Vec<String>,

    /// Whether PKCE is required
    #[serde(default)]
    pub pkce_required: Option<bool>,

    /// Application IDs this client can access
    #[serde(default)]
    pub application_ids: Vec<String>,
}

/// Update OAuth client request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOAuthClientRequest {
    /// Human-readable name
    pub client_name: Option<String>,

    /// Allowed redirect URIs
    pub redirect_uris: Option<Vec<String>>,

    /// Allowed grant types
    pub grant_types: Option<Vec<String>>,

    /// Whether PKCE is required
    pub pkce_required: Option<bool>,

    /// Application IDs this client can access
    pub application_ids: Option<Vec<String>>,

    /// Whether client is active
    pub active: Option<bool>,
}

/// OAuth client response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClientResponse {
    pub id: String,
    pub client_id: String,
    pub client_name: String,
    pub client_type: String,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    pub pkce_required: bool,
    pub application_ids: Vec<String>,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<OAuthClient> for OAuthClientResponse {
    fn from(c: OAuthClient) -> Self {
        Self {
            id: c.id,
            client_id: c.client_id,
            client_name: c.client_name,
            client_type: format!("{:?}", c.client_type).to_uppercase(),
            redirect_uris: c.redirect_uris,
            grant_types: c.grant_types.iter()
                .map(|g| format!("{:?}", g).to_lowercase())
                .collect(),
            pkce_required: c.pkce_required,
            application_ids: c.application_ids,
            active: c.active,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

/// Query parameters for OAuth clients list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct OAuthClientsQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by active status
    pub active: Option<bool>,
}

/// OAuth Clients service state
#[derive(Clone)]
pub struct OAuthClientsState {
    pub oauth_client_repo: Arc<OAuthClientRepository>,
}

fn parse_client_type(s: &str) -> OAuthClientType {
    match s.to_uppercase().as_str() {
        "CONFIDENTIAL" => OAuthClientType::Confidential,
        _ => OAuthClientType::Public,
    }
}

fn parse_grant_type(s: &str) -> Option<GrantType> {
    match s.to_lowercase().as_str() {
        "authorization_code" => Some(GrantType::AuthorizationCode),
        "client_credentials" => Some(GrantType::ClientCredentials),
        "refresh_token" => Some(GrantType::RefreshToken),
        "password" => Some(GrantType::Password),
        _ => None,
    }
}

/// Create a new OAuth client
#[utoipa::path(
    post,
    path = "",
    tag = "oauth-clients",
    operation_id = "postApiAdminPlatformOauthClients",
    request_body = CreateOAuthClientRequest,
    responses(
        (status = 201, description = "OAuth client created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate client_id")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Json(req): Json<CreateOAuthClientRequest>,
) -> Result<Json<CreatedResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    // Check for duplicate client_id
    if state.oauth_client_repo.exists_by_client_id(&req.client_id).await? {
        return Err(PlatformError::duplicate("OAuthClient", "clientId", &req.client_id));
    }

    let mut client = OAuthClient::new(&req.client_id, &req.client_name);

    if let Some(ref ct) = req.client_type {
        client.client_type = parse_client_type(ct);
    }

    for uri in req.redirect_uris {
        client = client.with_redirect_uri(uri);
    }

    if !req.grant_types.is_empty() {
        client.grant_types.clear();
        for gt in req.grant_types {
            if let Some(grant) = parse_grant_type(&gt) {
                client = client.with_grant_type(grant);
            }
        }
    }

    if let Some(pkce) = req.pkce_required {
        client.pkce_required = pkce;
    }

    client.application_ids = req.application_ids;

    let id = client.id.clone();
    state.oauth_client_repo.insert(&client).await?;

    Ok(Json(CreatedResponse::new(id)))
}

/// Get OAuth client by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "oauth-clients",
    operation_id = "getApiAdminPlatformOauthClientsById",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    responses(
        (status = 200, description = "OAuth client found", body = OAuthClientResponse),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<OAuthClientResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let client = state.oauth_client_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("OAuthClient", &id))?;

    Ok(Json(client.into()))
}

/// List OAuth clients
#[utoipa::path(
    get,
    path = "",
    tag = "oauth-clients",
    operation_id = "getApiAdminPlatformOauthClients",
    params(OAuthClientsQuery),
    responses(
        (status = 200, description = "List of OAuth clients", body = Vec<OAuthClientResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_oauth_clients(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Query(query): Query<OAuthClientsQuery>,
) -> Result<Json<Vec<OAuthClientResponse>>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let clients = if query.active.unwrap_or(true) {
        state.oauth_client_repo.find_active().await?
    } else {
        state.oauth_client_repo.find_all().await?
    };

    let response: Vec<OAuthClientResponse> = clients.into_iter()
        .map(|c| c.into())
        .collect();

    Ok(Json(response))
}

/// Update OAuth client
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "oauth-clients",
    operation_id = "putApiAdminPlatformOauthClientsById",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    request_body = UpdateOAuthClientRequest,
    responses(
        (status = 200, description = "OAuth client updated", body = OAuthClientResponse),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateOAuthClientRequest>,
) -> Result<Json<OAuthClientResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut client = state.oauth_client_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("OAuthClient", &id))?;

    if let Some(name) = req.client_name {
        client.client_name = name;
    }
    if let Some(uris) = req.redirect_uris {
        client.redirect_uris = uris;
    }
    if let Some(grants) = req.grant_types {
        client.grant_types = grants.iter()
            .filter_map(|g| parse_grant_type(g))
            .collect();
    }
    if let Some(pkce) = req.pkce_required {
        client.pkce_required = pkce;
    }
    if let Some(apps) = req.application_ids {
        client.application_ids = apps;
    }
    if let Some(active) = req.active {
        client.active = active;
    }

    client.updated_at = chrono::Utc::now();
    state.oauth_client_repo.update(&client).await?;

    Ok(Json(client.into()))
}

/// Delete OAuth client
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "oauth-clients",
    operation_id = "deleteApiAdminPlatformOauthClientsById",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    responses(
        (status = 200, description = "OAuth client deleted", body = SuccessResponse),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let exists = state.oauth_client_repo.find_by_id(&id).await?.is_some();
    if !exists {
        return Err(PlatformError::not_found("OAuthClient", &id));
    }

    state.oauth_client_repo.delete(&id).await?;

    Ok(Json(SuccessResponse::ok()))
}

/// Create OAuth clients router
pub fn oauth_clients_router(state: OAuthClientsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_oauth_client, list_oauth_clients))
        .routes(routes!(get_oauth_client, update_oauth_client, delete_oauth_client))
        .with_state(state)
}
