//! OAuth Clients Admin API
//!
//! REST endpoints for OAuth client management.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
// rand::Rng removed — now using rand::RngCore directly
// Client secrets are stored as `hashed:v1:` refs (EncryptionService::hash_secret).
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

use crate::auth::oauth_entity::{GrantType, OAuthClient, OAuthClientType};
use crate::shared::api_common::SuccessResponse;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::OAuthClientRepository;

/// Create OAuth client request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOAuthClientRequest {
    /// OAuth client_id (public identifier). Auto-generated if not provided.
    pub client_id: Option<String>,

    /// Human-readable name
    pub client_name: String,

    /// Client type (PUBLIC or CONFIDENTIAL). Required, as in Go's huma
    /// schema: absent is a 400 `VALIDATION`.
    pub client_type: String,

    /// Allowed redirect URIs
    #[serde(default)]
    pub redirect_uris: Vec<String>,

    /// Allowed post-logout redirect URIs (OIDC RP-Initiated Logout)
    #[serde(default)]
    pub post_logout_redirect_uris: Vec<String>,

    /// Allowed grant types
    #[serde(default)]
    pub grant_types: Vec<String>,

    /// The client's scope list
    #[serde(default)]
    pub default_scopes: Vec<String>,

    /// Whether PKCE is required (absent keeps Go's default, `true`)
    #[serde(default)]
    pub pkce_required: Option<bool>,

    /// Allowed CORS origins
    #[serde(default)]
    pub allowed_origins: Vec<String>,

    /// Application IDs this client can access
    #[serde(default)]
    pub application_ids: Vec<String>,

    /// Authority-bearing interactive access tokens (`token_use=api`),
    /// narrowed to the client's applications. Not for a portal client.
    #[serde(default)]
    pub api_access: Option<bool>,

    /// Marks this client as a portal entry point owned by that tenant client
    /// (Go `portalClientId`).
    #[serde(default)]
    pub portal_client_id: Option<String>,

    /// Links this portal client to one of the client's portal apps (Go
    /// `portalAppId`); its client becomes the portal owner.
    #[serde(default)]
    pub portal_app_id: Option<String>,
}

/// Update OAuth client request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOAuthClientRequest {
    /// Human-readable name
    pub client_name: Option<String>,

    /// Allowed redirect URIs
    pub redirect_uris: Option<Vec<String>>,

    /// Allowed post-logout redirect URIs (OIDC RP-Initiated Logout)
    pub post_logout_redirect_uris: Option<Vec<String>>,

    /// Allowed grant types
    pub grant_types: Option<Vec<String>>,

    /// The client's scope list
    pub default_scopes: Option<Vec<String>>,

    /// Whether PKCE is required
    pub pkce_required: Option<bool>,

    /// Application IDs this client can access
    pub application_ids: Option<Vec<String>>,

    /// Allowed CORS origins
    pub allowed_origins: Option<Vec<String>>,

    /// Whether client is active
    pub active: Option<bool>,

    /// Portal owner: empty clears the portal flag (and the app link).
    #[serde(default)]
    pub portal_client_id: Option<String>,

    /// Portal app link: empty unlinks.
    #[serde(default)]
    pub portal_app_id: Option<String>,

    /// Authority-bearing interactive access tokens
    #[serde(default)]
    pub api_access: Option<bool>,
}

/// Go's `OAuthClientApplicationRef`: an application id with its name (the
/// id when the application no longer exists).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClientApplicationRef {
    pub id: String,
    pub name: String,
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
    #[serde(default)]
    pub post_logout_redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    pub default_scopes: Vec<String>,
    pub pkce_required: bool,
    pub application_ids: Vec<String>,
    /// `{id, name}` of each application id (Go `applications`; the SPA's
    /// list page reads its length unconditionally).
    pub applications: Vec<OAuthClientApplicationRef>,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_principal_id: Option<String>,
    /// Portal entry point owner (Go `portalClientId`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_client_id: Option<String>,
    /// The linked portal app (Go `portalAppId`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_app_id: Option<String>,
    pub active: bool,
    /// Authority-bearing interactive access tokens (Go `apiAccess`).
    pub api_access: bool,
    pub created_at: String,
    pub updated_at: String,
    /// When a secret-rotation overlap lapses. Absent when none is in flight
    /// (Go's shape, auth/api/dto.go:157-166).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_secret_expires_at: Option<String>,
    /// When the superseded secret was last accepted, while the overlap is
    /// open. Absent means unused since the rotation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_secret_last_used_at: Option<String>,
}

impl From<OAuthClient> for OAuthClientResponse {
    fn from(c: OAuthClient) -> Self {
        let overlap_open = c.usable_previous_secret_ref().is_some();
        Self {
            id: c.id,
            client_id: c.client_id,
            client_name: c.client_name,
            client_type: c.client_type.as_str().to_string(),
            redirect_uris: c.redirect_uris,
            post_logout_redirect_uris: c.post_logout_redirect_uris,
            grant_types: c
                .grant_types
                .iter()
                .map(|g| g.as_str().to_string())
                .collect(),
            default_scopes: c.default_scopes,
            pkce_required: c.pkce_required,
            application_ids: c.application_ids,
            applications: Vec::new(),
            allowed_origins: c.allowed_origins,
            service_account_principal_id: c.service_account_principal_id,
            portal_client_id: c.portal_client_id,
            portal_app_id: c.portal_app_id,
            active: c.active,
            api_access: c.api_access,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
            previous_secret_expires_at: overlap_open
                .then(|| c.previous_secret_expires_at.map(|t| t.to_rfc3339()))
                .flatten(),
            previous_secret_last_used_at: overlap_open
                .then(|| c.previous_secret_last_used_at.map(|t| t.to_rfc3339()))
                .flatten(),
        }
    }
}

/// Go `State.fillApplicationRefs`: each response's `applications` from its
/// application ids, names resolved in one query; an id whose application is
/// gone keeps the id as its name.
async fn fill_application_refs(
    apps: &crate::ApplicationRepository,
    responses: &mut [OAuthClientResponse],
) -> Result<(), PlatformError> {
    let mut ids: Vec<String> = responses
        .iter()
        .flat_map(|r| {
            r.application_ids
                .iter()
                .filter(|id| !id.is_empty())
                .cloned()
        })
        .collect();
    ids.sort();
    ids.dedup();
    let names = apps.find_names_by_ids(&ids).await?;
    for r in responses.iter_mut() {
        r.applications = r
            .application_ids
            .iter()
            .map(|id| OAuthClientApplicationRef {
                id: id.clone(),
                name: names.get(id).cloned().unwrap_or_else(|| id.clone()),
            })
            .collect();
    }
    Ok(())
}

/// One client's response, application refs filled.
async fn client_response(
    state: &OAuthClientsState,
    client: OAuthClient,
) -> Result<OAuthClientResponse, PlatformError> {
    let mut responses = [OAuthClientResponse::from(client)];
    fill_application_refs(&state.application_repo, &mut responses).await?;
    let [response] = responses;
    Ok(response)
}

/// Wrapper response from `POST /api/oauth-clients`. Includes the freshly
/// generated `client_secret` exactly once for confidential clients — it is
/// never retrievable afterwards.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOAuthClientResponse {
    pub client: OAuthClientResponse,
    /// Plaintext client secret. Only present on creation of CONFIDENTIAL
    /// clients. Capture this on the first response — the platform stores
    /// only a keyed hash and cannot return it again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

/// List response wrapper for `GET /api/oauth-clients`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClientListResponse {
    pub clients: Vec<OAuthClientResponse>,
}

/// Query parameters for OAuth clients list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct OAuthClientsQuery {
    /// Filter by active status: `true` or `false`; absent (or anything
    /// else) lists every client, as Go's unfiltered list does
    /// (auth/api/api.go:170-187). Taken as a string: a typed bool beside a
    /// flattened struct rejects `?active=true`.
    pub active: Option<String>,
}

/// OAuth Clients service state
#[derive(Clone)]
pub struct OAuthClientsState {
    pub oauth_client_repo: Arc<OAuthClientRepository>,
    /// Resolves application ids to names for the `applications` refs.
    pub application_repo: Arc<crate::ApplicationRepository>,
    /// Resolves `portalAppId` to its owning client (Go `State.PortalApps`).
    pub portal_apps: Arc<crate::portal::repository::PortalAppRepository>,
    pub create_oauth_client_use_case:
        Arc<crate::auth::operations::CreateOAuthClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_oauth_client_use_case:
        Arc<crate::auth::operations::UpdateOAuthClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_oauth_client_use_case:
        Arc<crate::auth::operations::DeleteOAuthClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub activate_oauth_client_use_case:
        Arc<crate::auth::operations::ActivateOAuthClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub deactivate_oauth_client_use_case:
        Arc<crate::auth::operations::DeactivateOAuthClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub rotate_oauth_client_secret_use_case:
        Arc<crate::auth::operations::RotateOAuthClientSecretUseCase<crate::usecase::PgUnitOfWork>>,
    pub revoke_oauth_client_previous_secret_use_case: Arc<
        crate::auth::operations::RevokeOAuthClientPreviousSecretUseCase<
            crate::usecase::PgUnitOfWork,
        >,
    >,
}

/// Parses request grant types; an unknown one is a 400.
fn parse_grant_types(grant_types: &[String]) -> Result<Vec<GrantType>, PlatformError> {
    grant_types
        .iter()
        .map(|g| g.parse().map_err(PlatformError::from))
        .collect()
}

/// Create a new OAuth client
#[utoipa::path(
    post,
    path = "",
    tag = "oauth-clients",
    operation_id = "postApiOauthClients",
    request_body = CreateOAuthClientRequest,
    responses(
        (status = 201, description = "OAuth client created", body = CreateOAuthClientResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate client_id")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_oauth_client(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Json(req): Json<CreateOAuthClientRequest>,
) -> Result<(StatusCode, Json<CreateOAuthClientResponse>), PlatformError> {
    use crate::auth::operations::CreateOAuthClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_create_oauth_clients(&auth.0)?;
    let mut portal_client_id = req.portal_client_id.clone();
    crate::portal::resolve_oauth_client_portal_app(
        &state.portal_apps,
        req.portal_app_id.as_deref(),
        &mut portal_client_id,
    )
    .await?;

    // Auto-generate client_id if not provided
    let client_id = req
        .client_id
        .unwrap_or_else(|| crate::shared::tsid::generate(crate::EntityType::OAuthClient));
    // Go CreateOAuthClient (auth/operations/oauth_client.go): the name is
    // checked first, then the exact client type.
    if req.client_name.trim().is_empty() {
        return Err(PlatformError::bad_request_code(
            "CLIENT_NAME_REQUIRED",
            "clientName is required",
        ));
    }
    let client_type = match req.client_type.as_str() {
        "PUBLIC" => OAuthClientType::Public,
        "CONFIDENTIAL" => OAuthClientType::Confidential,
        _ => {
            return Err(PlatformError::bad_request_code(
                "INVALID_CLIENT_TYPE",
                "clientType must be PUBLIC or CONFIDENTIAL",
            ))
        }
    };

    // For CONFIDENTIAL clients, generate a secret at the edge. The plaintext
    // is returned once; only its keyed hash (`hashed:v1:`) is passed into the
    // use case, which persists it atomically with the domain event.
    let (client_secret_ref, generated_secret) = if client_type == OAuthClientType::Confidential {
        use base64::Engine;

        let mut secret_bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut secret_bytes);
        let plaintext = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret_bytes);

        let enc =
            crate::shared::encryption_service::EncryptionService::from_env().ok_or_else(|| {
                PlatformError::internal(
                    "FLOWCATALYST_APP_KEY not configured — cannot hash client secret",
                )
            })?;
        (Some(enc.hash_secret(&plaintext)), Some(plaintext))
    } else {
        (None, None)
    };

    // Go stores the grant types as sent: none sent is none stored.
    let grant_types = parse_grant_types(&req.grant_types)?;

    let oauth_client_id = crate::shared::tsid::generate(crate::EntityType::OAuthClient);

    let cmd = CreateOAuthClientCommand {
        oauth_client_id: oauth_client_id.clone(),
        client_id: client_id.clone(),
        client_name: req.client_name,
        client_type,
        client_secret_ref,
        redirect_uris: req.redirect_uris,
        post_logout_redirect_uris: req.post_logout_redirect_uris,
        grant_types,
        default_scopes: req.default_scopes,
        // Go's entity default is `true` for both types.
        pkce_required: req.pkce_required.unwrap_or(true),
        application_ids: req.application_ids,
        allowed_origins: req.allowed_origins,
        service_account_principal_id: None,
        created_by: Some(auth.0.principal_id.clone()),
        portal_client_id,
        portal_app_id: req.portal_app_id,
        api_access: req.api_access.unwrap_or(false),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .create_oauth_client_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let client = state
        .oauth_client_repo
        .find_by_id(&oauth_client_id)
        .await?
        .ok_or_else(|| PlatformError::internal("OAuth client created but row not found"))?;

    let response = CreateOAuthClientResponse {
        client: client_response(&state, client).await?,
        client_secret: generated_secret,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

/// Get OAuth client by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "oauth-clients",
    operation_id = "getApiOauthClientsById",
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
    crate::checks::can_read_oauth_clients(&auth.0)?;

    let client = state
        .oauth_client_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("OAuthClient", &id))?;

    Ok(Json(client_response(&state, client).await?))
}

/// List OAuth clients
#[utoipa::path(
    get,
    path = "",
    tag = "oauth-clients",
    operation_id = "getApiOauthClients",
    params(OAuthClientsQuery),
    responses(
        (status = 200, description = "List of OAuth clients", body = OAuthClientListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_oauth_clients(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Query(query): Query<OAuthClientsQuery>,
) -> Result<Json<OAuthClientListResponse>, PlatformError> {
    crate::checks::can_read_oauth_clients(&auth.0)?;

    let want_active = match query.active.as_deref() {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    let mut clients: Vec<_> = state
        .oauth_client_repo
        .find_all()
        .await?
        .into_iter()
        .filter(|c| want_active.is_none_or(|active| c.active == active))
        .collect();
    // Go orders by client_name (sqlc queries/auth.sql:30-37).
    clients.sort_by(|a, b| a.client_name.cmp(&b.client_name));

    let mut clients: Vec<OAuthClientResponse> = clients.into_iter().map(Into::into).collect();
    fill_application_refs(&state.application_repo, &mut clients).await?;
    Ok(Json(OAuthClientListResponse { clients }))
}

/// Update OAuth client
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "oauth-clients",
    operation_id = "putApiOauthClientsById",
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

    crate::checks::can_update_oauth_clients(&auth.0)?;
    let mut portal_client_id = req.portal_client_id.clone();
    crate::portal::resolve_oauth_client_portal_app(
        &state.portal_apps,
        req.portal_app_id.as_deref(),
        &mut portal_client_id,
    )
    .await?;

    let cmd = UpdateOAuthClientCommand {
        oauth_client_id: id,
        client_name: req.client_name,
        redirect_uris: req.redirect_uris,
        post_logout_redirect_uris: req.post_logout_redirect_uris,
        grant_types: req
            .grant_types
            .as_deref()
            .map(parse_grant_types)
            .transpose()?,
        pkce_required: req.pkce_required,
        application_ids: req.application_ids,
        allowed_origins: req.allowed_origins,
        active: req.active,
        portal_client_id,
        portal_app_id: req.portal_app_id,
        default_scopes: req.default_scopes,
        api_access: req.api_access,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .update_oauth_client_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Delete OAuth client
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "oauth-clients",
    operation_id = "deleteApiOauthClientsById",
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

    crate::checks::can_delete_oauth_clients(&auth.0)?;

    let cmd = DeleteOAuthClientCommand {
        oauth_client_id: id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .delete_oauth_client_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Regenerate secret response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateSecretResponse {
    /// The public client_id (Go's shape).
    pub client_id: String,
    /// The new plaintext client secret (shown once)
    pub client_secret: String,
    /// When the superseded secret stops being accepted. Absent when the
    /// rotation was an immediate cutover.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_secret_expires_at: Option<String>,
}

/// Optional body for rotate-secret / regenerate-secret. Sent body-less, the
/// outgoing secret keeps working for the default overlap (24h).
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RotateSecretRequest {
    /// How long the outgoing secret stays acceptable, in seconds. Omit for
    /// the default overlap; 0 cuts over immediately (for a secret that may
    /// be compromised).
    #[serde(default)]
    pub grace_seconds: Option<i64>,
}

/// Get OAuth client by client_id (public identifier)
#[utoipa::path(
    get,
    path = "/by-client-id/{clientId}",
    tag = "oauth-clients",
    operation_id = "getApiOauthClientsByClientId",
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
    crate::checks::can_read_oauth_clients(&auth.0)?;

    let client = state
        .oauth_client_repo
        .find_by_client_id(&client_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("OAuthClient", &client_id))?;

    Ok(Json(client_response(&state, client).await?))
}

/// Activate OAuth client
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "oauth-clients",
    operation_id = "postApiOauthClientsActivate",
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

    crate::checks::can_update_oauth_clients(&auth.0)?;

    let cmd = ActivateOAuthClientCommand {
        oauth_client_id: id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .activate_oauth_client_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    Ok(Json(SuccessResponse::with_message(
        "OAuth client activated",
    )))
}

/// Deactivate OAuth client
#[utoipa::path(
    post,
    path = "/{id}/deactivate",
    tag = "oauth-clients",
    operation_id = "postApiOauthClientsDeactivate",
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

    crate::checks::can_update_oauth_clients(&auth.0)?;

    let cmd = DeactivateOAuthClientCommand {
        oauth_client_id: id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .deactivate_oauth_client_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    Ok(Json(SuccessResponse::with_message(
        "OAuth client deactivated",
    )))
}

/// Parse the optional rotate body. The frontend and SDKs POST body-less,
/// which must keep working, as in Go (auth/api/api.go:309-316).
fn parse_rotate_body(body: &[u8]) -> Result<RotateSecretRequest, PlatformError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(RotateSecretRequest::default());
    }
    serde_json::from_slice(body)
        .map_err(|e| PlatformError::validation(format!("Invalid request body: {}", e)))
}

/// Regenerate OAuth client secret
///
/// Keeps the outgoing secret acceptable for `graceSeconds` (default 24h; 0
/// is an immediate cutover), as Go does.
#[utoipa::path(
    post,
    path = "/{id}/regenerate-secret",
    tag = "oauth-clients",
    operation_id = "postApiOauthClientsRegenerateSecret",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    request_body(content = Option<RotateSecretRequest>, description = "Optional overlap window"),
    responses(
        (status = 200, description = "New client secret generated", body = RegenerateSecretResponse),
        (status = 404, description = "OAuth client not found"),
        (status = 409, description = "Not a CONFIDENTIAL client")
    ),
    security(("bearer_auth" = []))
)]
pub async fn regenerate_oauth_client_secret(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> Result<Json<RegenerateSecretResponse>, PlatformError> {
    use crate::auth::operations::RotateOAuthClientSecretCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_write_oauth_client_secrets(&auth.0)?;
    let req = parse_rotate_body(&body)?;

    // Generate + hash the secret at the edge; the use case gets only the
    // `hashed:v1:` ref so plaintext never crosses the domain boundary.
    let mut secret_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut secret_bytes);
    let plaintext_secret = URL_SAFE_NO_PAD.encode(secret_bytes);

    let enc = crate::shared::encryption_service::EncryptionService::from_env()
        .ok_or_else(|| PlatformError::internal("FLOWCATALYST_APP_KEY not configured"))?;
    let cmd = RotateOAuthClientSecretCommand {
        oauth_client_id: id,
        new_client_secret_ref: enc.hash_secret(&plaintext_secret),
        grace_seconds: req.grace_seconds,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state
        .rotate_oauth_client_secret_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    // The event carries only Go's payload, so re-fetch the public client_id
    // the response returns, as Go does (auth/api/api.go:331-339).
    let client = state
        .oauth_client_repo
        .find_by_id(&event.oauth_client_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("OAuthClient", &event.oauth_client_id))?;
    Ok(Json(RegenerateSecretResponse {
        client_id: client.client_id,
        client_secret: plaintext_secret,
        previous_secret_expires_at: event.previous_secret_expires_at.map(|t| t.to_rfc3339()),
    }))
}

/// Rotate OAuth client secret (alias for regenerate-secret, matches TS API)
#[utoipa::path(
    post,
    path = "/{id}/rotate-secret",
    tag = "oauth-clients",
    operation_id = "postApiOauthClientsRotateSecret",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    request_body(content = Option<RotateSecretRequest>, description = "Optional overlap window"),
    responses(
        (status = 200, description = "New client secret generated", body = RegenerateSecretResponse),
        (status = 404, description = "OAuth client not found"),
        (status = 409, description = "Not a CONFIDENTIAL client")
    ),
    security(("bearer_auth" = []))
)]
pub async fn rotate_oauth_client_secret(
    state: State<OAuthClientsState>,
    auth: Authenticated,
    path: Path<String>,
    body: axum::body::Bytes,
) -> Result<Json<RegenerateSecretResponse>, PlatformError> {
    crate::checks::can_write_oauth_client_secrets(&auth.0)?;
    regenerate_oauth_client_secret(state, auth, path, body).await
}

/// End a secret-rotation overlap now
///
/// The superseded secret stops authenticating immediately instead of lapsing
/// on its timer. Idempotent (Go's `revoke-previous-secret`).
#[utoipa::path(
    post,
    path = "/{id}/revoke-previous-secret",
    tag = "oauth-clients",
    operation_id = "postApiOauthClientsRevokePreviousSecret",
    params(
        ("id" = String, Path, description = "OAuth client ID")
    ),
    responses(
        (status = 200, description = "Previous client secret revoked", body = SuccessResponse),
        (status = 404, description = "OAuth client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn revoke_oauth_client_previous_secret(
    State(state): State<OAuthClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    use crate::auth::operations::RevokeOAuthClientPreviousSecretCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_write_oauth_client_secrets(&auth.0)?;
    let cmd = RevokeOAuthClientPreviousSecretCommand {
        oauth_client_id: id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .revoke_oauth_client_previous_secret_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    Ok(Json(SuccessResponse::with_message(
        "Previous client secret revoked",
    )))
}

/// Create OAuth clients router
pub fn oauth_clients_router(state: OAuthClientsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_oauth_client, list_oauth_clients))
        .routes(routes!(
            get_oauth_client,
            update_oauth_client,
            delete_oauth_client
        ))
        .routes(routes!(get_oauth_client_by_client_id))
        .routes(routes!(activate_oauth_client))
        .routes(routes!(deactivate_oauth_client))
        .routes(routes!(regenerate_oauth_client_secret))
        .routes(routes!(rotate_oauth_client_secret))
        .routes(routes!(revoke_oauth_client_previous_secret))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `?active=true` parses (a typed bool beside a flattened struct didn't).
    #[test]
    fn the_oauth_clients_query_parses_through_the_real_query_parser() {
        for (uri, want) in [
            ("/api/oauth-clients?active=true", Some("true")),
            (
                "/api/oauth-clients?active=false&page=0&size=20",
                Some("false"),
            ),
            ("/api/oauth-clients", None),
        ] {
            let uri: axum::http::Uri = uri.parse().unwrap();
            let q = axum::extract::Query::<OAuthClientsQuery>::try_from_uri(&uri)
                .unwrap()
                .0;
            assert_eq!(q.active.as_deref(), want);
        }
    }
}
