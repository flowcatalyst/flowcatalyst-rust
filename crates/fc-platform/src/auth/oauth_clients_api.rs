//! OAuth Clients Admin API
//!
//! REST endpoints for OAuth client management.

use axum::{
    extract::{State, Path, Query},
    http::StatusCode,
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
// rand::Rng removed — now using rand::RngCore directly
// SHA-256 removed — secrets now use encrypted: format via EncryptionService
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::auth::oauth_entity::{OAuthClient, OAuthClientType};
use crate::OAuthClientRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::{PaginationParams, SuccessResponse};
use crate::shared::middleware::Authenticated;

/// Create OAuth client request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOAuthClientRequest {
    /// OAuth client_id (public identifier). Auto-generated if not provided.
    pub client_id: Option<String>,

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
#[derive(Debug, Deserialize, Serialize, ToSchema)]
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

    /// Allowed CORS origins
    pub allowed_origins: Option<Vec<String>>,

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
    pub default_scopes: Vec<String>,
    pub pkce_required: bool,
    pub application_ids: Vec<String>,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_principal_id: Option<String>,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
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
                .map(|g| g.as_str().to_string())
                .collect(),
            default_scopes: c.default_scopes,
            pkce_required: c.pkce_required,
            application_ids: c.application_ids,
            allowed_origins: c.allowed_origins,
            service_account_principal_id: c.service_account_principal_id,
            active: c.active,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
            created_by: c.created_by,
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
    pub create_oauth_client_use_case: Arc<
        crate::auth::operations::CreateOAuthClientUseCase<crate::usecase::PgUnitOfWork>,
    >,
    pub update_oauth_client_use_case: Arc<
        crate::auth::operations::UpdateOAuthClientUseCase<crate::usecase::PgUnitOfWork>,
    >,
    pub delete_oauth_client_use_case: Arc<
        crate::auth::operations::DeleteOAuthClientUseCase<crate::usecase::PgUnitOfWork>,
    >,
    pub activate_oauth_client_use_case: Arc<
        crate::auth::operations::ActivateOAuthClientUseCase<crate::usecase::PgUnitOfWork>,
    >,
    pub deactivate_oauth_client_use_case: Arc<
        crate::auth::operations::DeactivateOAuthClientUseCase<crate::usecase::PgUnitOfWork>,
    >,
    pub rotate_oauth_client_secret_use_case: Arc<
        crate::auth::operations::RotateOAuthClientSecretUseCase<crate::usecase::PgUnitOfWork>,
    >,
}

fn parse_client_type(s: &str) -> OAuthClientType {
    match s.to_uppercase().as_str() {
        "CONFIDENTIAL" => OAuthClientType::Confidential,
        _ => OAuthClientType::Public,
    }
}

/// Create a new OAuth client
#[utoipa::path(
    post,
    path = "",
    tag = "oauth-clients",
    operation_id = "postApiAdminOauthClients",
    request_body = CreateOAuthClientRequest,
    responses(
        (status = 201, description = "OAuth client created", body = OAuthClientResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate client_id")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Json(req): Json<CreateOAuthClientRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), PlatformError> {
    use crate::auth::operations::CreateOAuthClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    // Auto-generate client_id if not provided
    let client_id = req.client_id
        .unwrap_or_else(|| crate::TsidGenerator::generate(crate::EntityType::OAuthClient));
    let client_type = req.client_type.clone().unwrap_or_else(|| "PUBLIC".to_string());
    let parsed_client_type = parse_client_type(&client_type);

    // For CONFIDENTIAL clients, generate a secret at the edge. The plaintext
    // is returned once; the encrypted ref is passed into the use case which
    // persists it atomically with the domain event.
    let (client_secret_ref, generated_secret) = if parsed_client_type == OAuthClientType::Confidential {
        use base64::Engine;

        let mut secret_bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut secret_bytes);
        let plaintext = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret_bytes);

        let enc = crate::shared::encryption_service::EncryptionService::from_env()
            .ok_or_else(|| PlatformError::internal("FLOWCATALYST_APP_KEY not configured — cannot encrypt client secret"))?;
        let encrypted = enc.encrypt(&plaintext)
            .map_err(|e| PlatformError::internal(format!("Failed to encrypt client secret: {}", e)))?;
        (Some(format!("encrypted:{}", encrypted)), Some(plaintext))
    } else {
        (None, None)
    };

    // Default grant_types = ["authorization_code"] when not specified, matching the
    // OAuthClient::new default.
    let grant_types = if req.grant_types.is_empty() {
        vec!["authorization_code".to_string()]
    } else {
        req.grant_types
    };

    let oauth_client_id = crate::TsidGenerator::generate(crate::EntityType::OAuthClient);

    let cmd = CreateOAuthClientCommand {
        oauth_client_id: oauth_client_id.clone(),
        client_id: client_id.clone(),
        client_name: req.client_name,
        client_type,
        client_secret_ref,
        redirect_uris: req.redirect_uris,
        grant_types,
        default_scopes: vec![],
        pkce_required: req.pkce_required.unwrap_or(parsed_client_type == OAuthClientType::Public),
        application_ids: req.application_ids,
        allowed_origins: vec![],
        service_account_principal_id: None,
        created_by: Some(auth.0.principal_id.clone()),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.create_oauth_client_use_case.run(cmd, ctx).await.into_result()?;

    let client = state.oauth_client_repo.find_by_id(&oauth_client_id).await?
        .ok_or_else(|| PlatformError::internal("OAuth client created but row not found"))?;

    let mut response = serde_json::json!({
        "client": OAuthClientResponse::from(client),
    });
    if let Some(secret) = generated_secret {
        response["clientSecret"] = serde_json::json!(secret);
    }

    Ok((StatusCode::CREATED, Json(response)))
}

/// Get OAuth client by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "oauth-clients",
    operation_id = "getApiAdminOauthClientsById",
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
    operation_id = "getApiAdminOauthClients",
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
) -> Result<Json<serde_json::Value>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let clients = if query.active.unwrap_or(true) {
        state.oauth_client_repo.find_active().await?
    } else {
        state.oauth_client_repo.find_all().await?
    };

    let response: Vec<OAuthClientResponse> = clients.into_iter()
        .map(|c| c.into())
        .collect();

    Ok(Json(serde_json::json!({ "clients": response })))
}

/// Update OAuth client
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "oauth-clients",
    operation_id = "putApiAdminOauthClientsById",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    request_body = UpdateOAuthClientRequest,
    responses(
        (status = 204, description = "OAuth client updated"),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateOAuthClientRequest>,
) -> Result<StatusCode, PlatformError> {
    use crate::auth::operations::UpdateOAuthClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = UpdateOAuthClientCommand {
        oauth_client_id: id,
        client_name: req.client_name,
        redirect_uris: req.redirect_uris,
        grant_types: req.grant_types,
        pkce_required: req.pkce_required,
        application_ids: req.application_ids,
        allowed_origins: req.allowed_origins,
        active: req.active,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_oauth_client_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Delete OAuth client
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "oauth-clients",
    operation_id = "deleteApiAdminOauthClientsById",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    responses(
        (status = 204, description = "OAuth client deleted"),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    use crate::auth::operations::DeleteOAuthClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = DeleteOAuthClientCommand { oauth_client_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_oauth_client_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Regenerate secret response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateSecretResponse {
    /// The new plaintext client secret (shown once)
    pub client_secret: String,
}

/// Get OAuth client by client_id (public identifier)
#[utoipa::path(
    get,
    path = "/by-client-id/{clientId}",
    tag = "oauth-clients",
    operation_id = "getApiAdminOauthClientsByClientId",
    params(
        ("clientId" = String, Path, description = "OAuth client_id (public identifier)")
    ),
    responses(
        (status = 200, description = "OAuth client found", body = OAuthClientResponse),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_oauth_client_by_client_id(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(client_id): Path<String>,
) -> Result<Json<OAuthClientResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let client = state.oauth_client_repo.find_by_client_id(&client_id).await?
        .ok_or_else(|| PlatformError::not_found("OAuthClient", &client_id))?;

    Ok(Json(client.into()))
}

/// Activate OAuth client
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "oauth-clients",
    operation_id = "postApiAdminOauthClientsActivate",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    responses(
        (status = 200, description = "OAuth client activated", body = SuccessResponse),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn activate_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    use crate::auth::operations::ActivateOAuthClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = ActivateOAuthClientCommand { oauth_client_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.activate_oauth_client_use_case.run(cmd, ctx).await.into_result()?;

    Ok(Json(SuccessResponse::with_message("OAuth client activated")))
}

/// Deactivate OAuth client
#[utoipa::path(
    post,
    path = "/{id}/deactivate",
    tag = "oauth-clients",
    operation_id = "postApiAdminOauthClientsDeactivate",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    responses(
        (status = 200, description = "OAuth client deactivated", body = SuccessResponse),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn deactivate_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    use crate::auth::operations::DeactivateOAuthClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = DeactivateOAuthClientCommand { oauth_client_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.deactivate_oauth_client_use_case.run(cmd, ctx).await.into_result()?;

    Ok(Json(SuccessResponse::with_message("OAuth client deactivated")))
}

/// Regenerate OAuth client secret
#[utoipa::path(
    post,
    path = "/{id}/regenerate-secret",
    tag = "oauth-clients",
    operation_id = "postApiAdminOauthClientsRegenerateSecret",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    responses(
        (status = 200, description = "New client secret generated", body = RegenerateSecretResponse),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn regenerate_oauth_client_secret(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<RegenerateSecretResponse>, PlatformError> {
    use crate::auth::operations::RotateOAuthClientSecretCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    // Generate + encrypt the secret at the edge; the use case gets only the
    // encrypted ref so plaintext never crosses the domain boundary.
    let mut secret_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut secret_bytes);
    let plaintext_secret = URL_SAFE_NO_PAD.encode(secret_bytes);

    let enc = crate::shared::encryption_service::EncryptionService::from_env()
        .ok_or_else(|| PlatformError::internal("FLOWCATALYST_APP_KEY not configured"))?;
    let encrypted = enc.encrypt(&plaintext_secret)
        .map_err(|e| PlatformError::internal(format!("Failed to encrypt secret: {}", e)))?;

    let cmd = RotateOAuthClientSecretCommand {
        oauth_client_id: id,
        new_client_secret_ref: format!("encrypted:{}", encrypted),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.rotate_oauth_client_secret_use_case.run(cmd, ctx).await.into_result()?;

    Ok(Json(RegenerateSecretResponse {
        client_secret: plaintext_secret,
    }))
}

/// Rotate OAuth client secret (alias for regenerate-secret, matches TS API)
#[utoipa::path(
    post,
    path = "/{id}/rotate-secret",
    tag = "oauth-clients",
    operation_id = "postApiAdminOauthClientsRotateSecret",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    responses(
        (status = 200, description = "New client secret generated", body = RegenerateSecretResponse),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn rotate_oauth_client_secret(
    state: State<OAuthClientsState>,
    auth: Authenticated,
    path: Path<String>,
) -> Result<Json<RegenerateSecretResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;
    regenerate_oauth_client_secret(state, auth, path).await
}

/// Create OAuth clients router
pub fn oauth_clients_router(state: OAuthClientsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_oauth_client, list_oauth_clients))
        .routes(routes!(get_oauth_client, update_oauth_client, delete_oauth_client))
        .routes(routes!(get_oauth_client_by_client_id))
        .routes(routes!(activate_oauth_client))
        .routes(routes!(deactivate_oauth_client))
        .routes(routes!(regenerate_oauth_client_secret))
        .routes(routes!(rotate_oauth_client_secret))
        .with_state(state)
}
