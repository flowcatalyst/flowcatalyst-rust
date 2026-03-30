//! OAuth2 Authorization Endpoints
//!
//! Implements OAuth2 authorization code flow with PKCE support:
//! - GET /oauth/authorize - Authorization endpoint
//! - POST /oauth/token - Token endpoint
//! - POST /oauth/revoke - Token revocation

use axum::{
    routing::{get, post},
    extract::{State, Query, Form},
    response::{Json, Redirect, IntoResponse, Response},
    http::{StatusCode, header, HeaderMap},
    Router,
};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::sync::Arc;
use chrono::Utc;
use tracing::{info, warn, error};

use crate::{AuthorizationCode, RefreshToken};
use crate::{OAuthClientRepository, PrincipalRepository, AuthorizationCodeRepository, RefreshTokenRepository};
use crate::auth::pending_auth_repository::{PendingAuthRepository, PendingAuth};
use crate::AuthService;
use crate::auth::auth_service::{AccessTokenClaims, extract_bearer_token};
use crate::auth::password_service::PasswordService;
use crate::auth::oauth_entity::OAuthClient;
use crate::OidcService;
use crate::login_attempt::entity::{LoginAttempt, AttemptType, LoginOutcome};
use crate::login_attempt::repository::LoginAttemptRepository;
use crate::shared::error::PlatformError;

/// Authorization request parameters
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuthorizeRequest {
    pub response_type: String,
    pub client_id: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub scope: Option<String>,
    pub state: Option<String>,
    pub nonce: Option<String>,
    /// PKCE code challenge
    pub code_challenge: Option<String>,
    /// PKCE code challenge method (S256 or plain)
    pub code_challenge_method: Option<String>,
    /// Provider ID for external OIDC
    pub provider: Option<String>,
}

/// Token request (form-urlencoded)
#[derive(Debug, Deserialize, ToSchema)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    /// PKCE code verifier
    pub code_verifier: Option<String>,
    /// For refresh token grant
    pub refresh_token: Option<String>,
    /// For password grant (not recommended)
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Token response
#[derive(Debug, Serialize, ToSchema)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Error response (RFC 6749)
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

/// Token introspection request (RFC 7662)
#[derive(Debug, Deserialize, ToSchema)]
pub struct IntrospectRequest {
    pub token: String,
    #[serde(default)]
    pub token_type_hint: Option<String>,
}

/// Token introspection response (RFC 7662)
#[derive(Debug, Serialize, ToSchema)]
pub struct IntrospectResponse {
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub principal_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
}

/// Token revocation request (RFC 7009)
#[derive(Debug, Deserialize, ToSchema)]
pub struct RevokeRequest {
    pub token: String,
    #[serde(default)]
    pub token_type_hint: Option<String>,
}

/// OIDC UserInfo response
#[derive(Debug, Serialize, ToSchema)]
pub struct UserInfoResponse {
    pub sub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub principal_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clients: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applications: Option<Vec<String>>,
}

/// OAuth2 state
#[derive(Clone)]
pub struct OAuthState {
    pub oauth_client_repo: Arc<OAuthClientRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub auth_service: Arc<AuthService>,
    pub oidc_service: Arc<OidcService>,
    /// Authorization code storage (PostgreSQL)
    pub auth_code_repo: Arc<AuthorizationCodeRepository>,
    /// Refresh token storage for token rotation
    pub refresh_token_repo: Arc<RefreshTokenRepository>,
    /// Pending authorization states (PostgreSQL, survives restarts)
    pub pending_auth_repo: Arc<PendingAuthRepository>,
    /// Password service for verifying client secrets
    pub password_service: Arc<PasswordService>,
    /// Login attempt logging
    pub login_attempt_repo: Arc<LoginAttemptRepository>,
}

impl OAuthState {
    pub fn new(
        oauth_client_repo: Arc<OAuthClientRepository>,
        principal_repo: Arc<PrincipalRepository>,
        auth_service: Arc<AuthService>,
        oidc_service: Arc<OidcService>,
        auth_code_repo: Arc<AuthorizationCodeRepository>,
        refresh_token_repo: Arc<RefreshTokenRepository>,
        pending_auth_repo: Arc<PendingAuthRepository>,
        password_service: Arc<PasswordService>,
        login_attempt_repo: Arc<LoginAttemptRepository>,
    ) -> Self {
        Self {
            oauth_client_repo,
            principal_repo,
            auth_service,
            oidc_service,
            auth_code_repo,
            refresh_token_repo,
            pending_auth_repo,
            password_service,
            login_attempt_repo,
        }
    }
}

/// Authorization endpoint - initiates the OAuth2 flow
#[utoipa::path(
    get,
    path = "/authorize",
    tag = "oauth",
    params(AuthorizeRequest),
    responses(
        (status = 302, description = "Redirect to login or IDP"),
        (status = 400, description = "Invalid request")
    )
)]
pub async fn authorize(
    State(state): State<OAuthState>,
    headers: HeaderMap,
    jar: axum_extra::extract::cookie::CookieJar,
    Query(req): Query<AuthorizeRequest>,
) -> Response {
    // Validate response_type
    if req.response_type != "code" {
        return error_redirect(&req.redirect_uri, "unsupported_response_type", "Only 'code' response type is supported", req.state.as_deref());
    }

    // Validate client
    let client = match state.oauth_client_repo.find_by_client_id(&req.client_id).await {
        Ok(Some(c)) if c.active => c,
        Ok(Some(_)) => {
            return error_redirect(&req.redirect_uri, "unauthorized_client", "Client is not active", req.state.as_deref());
        }
        Ok(None) => {
            return error_redirect(&req.redirect_uri, "unauthorized_client", "Unknown client", req.state.as_deref());
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup client");
            return error_redirect(&req.redirect_uri, "server_error", "Internal error", req.state.as_deref());
        }
    };

    // Validate redirect_uri (exact match first, then wildcard pattern matching)
    if !matches_redirect_uri(&req.redirect_uri, &client.redirect_uris) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_request".to_string(),
                error_description: Some("Invalid redirect_uri".to_string()),
            }),
        ).into_response();
    }

    // Validate PKCE if required
    if client.pkce_required && req.code_challenge.is_none() {
        return error_redirect(&req.redirect_uri, "invalid_request", "PKCE code_challenge is required", req.state.as_deref());
    }

    // Validate code_challenge_method
    if let Some(ref method) = req.code_challenge_method {
        if method != "S256" && method != "plain" {
            return error_redirect(&req.redirect_uri, "invalid_request", "Invalid code_challenge_method", req.state.as_deref());
        }
    }

    // Check if user is already authenticated (has valid session cookie).
    // If so, skip the login redirect and issue the authorization code directly.
    let session_token = jar.get("fc_session").map(|c| c.value().to_string())
        .or_else(|| {
            headers.get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(extract_bearer_token)
                .map(|t| t.to_string())
        });

    if let Some(ref token) = session_token {
        if let Ok(claims) = state.auth_service.validate_token(token) {
            // User is authenticated — issue authorization code immediately
            let auth_code_str = generate_random_string(64);
            let mut auth_code = AuthorizationCode::new(
                auth_code_str.clone(),
                req.client_id.clone(),
                claims.sub.clone(),
                req.redirect_uri.clone(),
            )
            .with_scope(req.scope.clone())
            .with_nonce(req.nonce.clone())
            .with_state(req.state.clone());

            if let (Some(challenge), Some(method)) = (&req.code_challenge, &req.code_challenge_method) {
                auth_code = auth_code.with_pkce(challenge.clone(), method.clone());
            }

            if let Err(e) = state.auth_code_repo.insert(&auth_code).await {
                error!(error = %e, "Failed to store authorization code");
                return error_redirect(&req.redirect_uri, "server_error", "Failed to create authorization code", req.state.as_deref());
            }

            let mut redirect_url = format!("{}?code={}", req.redirect_uri, urlencoding::encode(&auth_code_str));
            if let Some(ref s) = req.state {
                redirect_url.push_str(&format!("&state={}", urlencoding::encode(s)));
            }

            info!(client_id = %req.client_id, principal_id = %claims.sub, "Issued authorization code (authenticated session)");
            return Redirect::temporary(&redirect_url).into_response();
        }
    }

    // User is not authenticated — proceed with login flow
    // Generate state for CSRF protection if not provided
    let state_param = req.state.clone().unwrap_or_else(|| generate_random_string(32));

    // Store pending authorization in PostgreSQL (survives restarts)
    let pending = PendingAuth {
        client_id: req.client_id.clone(),
        redirect_uri: req.redirect_uri.clone(),
        scope: req.scope.clone(),
        code_challenge: req.code_challenge.clone(),
        code_challenge_method: req.code_challenge_method.clone(),
        nonce: req.nonce.clone(),
        created_at: Utc::now(),
    };

    if let Err(e) = state.pending_auth_repo.insert(&state_param, &pending).await {
        error!(error = %e, "Failed to store pending auth state");
        return error_redirect(&req.redirect_uri, "server_error", "Internal error", req.state.as_deref());
    }

    // If external provider specified, redirect to OIDC provider
    if let Some(provider_id) = req.provider {
        match state.oidc_service.get_authorization_url(&provider_id, &state_param, req.nonce.as_deref()).await {
            Ok(url) => {
                info!(provider = %provider_id, "Redirecting to OIDC provider");
                return Redirect::temporary(&url).into_response();
            }
            Err(e) => {
                error!(error = %e, "Failed to get authorization URL");
                return error_redirect(&req.redirect_uri, "server_error", "Failed to initialize OIDC flow", req.state.as_deref());
            }
        }
    }

    // Redirect to SPA login page with all OAuth params so the SPA can route back
    // after authentication. The SPA checks for oauth=true and rebuilds the authorize URL.
    let mut login_url = format!(
        "/auth/login?oauth=true&response_type=code&client_id={}&redirect_uri={}&state={}",
        urlencoding::encode(&req.client_id),
        urlencoding::encode(&req.redirect_uri),
        urlencoding::encode(&state_param),
    );
    if let Some(ref scope) = req.scope {
        login_url.push_str(&format!("&scope={}", urlencoding::encode(scope)));
    }
    if let Some(ref challenge) = req.code_challenge {
        login_url.push_str(&format!("&code_challenge={}", urlencoding::encode(challenge)));
    }
    if let Some(ref method) = req.code_challenge_method {
        login_url.push_str(&format!("&code_challenge_method={}", urlencoding::encode(method)));
    }
    if let Some(ref nonce) = req.nonce {
        login_url.push_str(&format!("&nonce={}", urlencoding::encode(nonce)));
    }

    Redirect::temporary(&login_url).into_response()
}

/// Authenticate an OAuth client from the request.
/// Supports both HTTP Basic auth and POST body credentials.
/// Returns the authenticated client, or an error response.
///
/// For confidential clients (those with a `client_secret_ref`), the secret MUST be provided.
/// For public clients (no secret stored), the secret is not required.
async fn authenticate_client(
    state: &OAuthState,
    headers: &HeaderMap,
    client_id_body: Option<&str>,
    client_secret_body: Option<&str>,
) -> Result<OAuthClient, Response> {
    // Extract client credentials from Basic auth header or POST body
    let (client_id, client_secret) = if let Some(basic) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
    {
        // Decode Basic auth: base64(client_id:client_secret)
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(basic)
            .map_err(|_| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorResponse {
                        error: "invalid_client".to_string(),
                        error_description: Some("Invalid Basic auth encoding".to_string()),
                    }),
                )
                    .into_response()
            })?;
        let decoded_str = String::from_utf8(decoded).map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Invalid Basic auth encoding".to_string()),
                }),
            )
                .into_response()
        })?;
        let (id, secret) = decoded_str.split_once(':').ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Invalid Basic auth format".to_string()),
                }),
            )
                .into_response()
        })?;
        (id.to_string(), if secret.is_empty() { None } else { Some(secret.to_string()) })
    } else if let Some(id) = client_id_body {
        (id.to_string(), client_secret_body.map(|s| s.to_string()))
    } else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid_client".to_string(),
                error_description: Some("Missing client credentials".to_string()),
            }),
        )
            .into_response());
    };

    // Look up the client
    let client = match state.oauth_client_repo.find_by_client_id(&client_id).await {
        Ok(Some(c)) if c.active => c,
        Ok(Some(_)) => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Client is not active".to_string()),
                }),
            )
                .into_response());
        }
        Ok(None) => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Unknown client".to_string()),
                }),
            )
                .into_response());
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup client");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
                .into_response());
        }
    };

    // If confidential client (has a secret), verify it.
    // Secrets are stored in one of two formats:
    // - "encrypted:..." — TS format, encrypted with FLOWCATALYST_APP_KEY (decrypt and compare)
    // - hex string — Rust format, SHA-256 hash of the plaintext secret
    if let Some(ref secret_ref) = client.client_secret_ref {
        let provided_secret = client_secret.ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Client secret required for confidential clients".to_string()),
                }),
            )
                .into_response()
        })?;

        let verified = if secret_ref.starts_with("encrypted:") {
            // TS format: decrypt the stored secret and compare directly
            match crate::shared::encryption_service::EncryptionService::from_env() {
                Some(enc) => match enc.decrypt(secret_ref) {
                    Ok(decrypted) => decrypted == provided_secret,
                    Err(e) => {
                        error!(client_id = %client_id, error = %e, "Failed to decrypt client secret");
                        false
                    }
                },
                None => {
                    error!(client_id = %client_id, "Cannot verify encrypted client secret — FLOWCATALYST_APP_KEY not configured");
                    false
                }
            }
        } else {
            // Rust format: SHA-256 hex hash comparison
            let mut hasher = Sha256::new();
            hasher.update(provided_secret.as_bytes());
            let provided_hash = format!("{:x}", hasher.finalize());
            provided_hash == *secret_ref
        };

        if !verified {
            warn!(client_id = %client_id, "Client secret verification failed");
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Invalid client credentials".to_string()),
                }),
            )
                .into_response());
        }
    }
    // Public clients (no secret_ref) pass without secret verification

    Ok(client)
}

/// Authenticate a client or bearer token for protected endpoints (introspect/revoke).
/// Returns the authenticated client_id, or an error response.
async fn authenticate_client_or_bearer(
    state: &OAuthState,
    headers: &HeaderMap,
    client_id_body: Option<&str>,
    client_secret_body: Option<&str>,
) -> Result<String, Response> {
    // Try Bearer token first
    if let Some(auth_header) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = extract_bearer_token(auth_header) {
            return match state.auth_service.validate_token(token) {
                Ok(claims) => Ok(claims.sub),
                Err(_) => Err((
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorResponse {
                        error: "invalid_token".to_string(),
                        error_description: Some("Token is invalid or expired".to_string()),
                    }),
                )
                    .into_response()),
            };
        }
        // If it starts with "Basic ", fall through to client auth
    }

    // Try client credentials (Basic auth or body)
    let client = authenticate_client(state, headers, client_id_body, client_secret_body).await?;
    Ok(client.client_id)
}

/// Token endpoint - exchanges authorization code for tokens
#[utoipa::path(
    post,
    path = "/token",
    tag = "oauth",
    request_body = TokenRequest,
    responses(
        (status = 200, description = "Token issued", body = TokenResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Invalid client", body = ErrorResponse)
    )
)]
pub async fn token(
    State(state): State<OAuthState>,
    headers: HeaderMap,
    Form(req): Form<TokenRequest>,
) -> Response {
    // P0-1: Authenticate the client before processing any grant type.
    // For client_credentials grant, the handler does its own auth (backward compat),
    // but for authorization_code and refresh_token, we authenticate here.
    let authenticated_client = match req.grant_type.as_str() {
        "client_credentials" => {
            // client_credentials handler does its own full auth including type checks
            None
        }
        _ => {
            match authenticate_client(
                &state,
                &headers,
                req.client_id.as_deref(),
                req.client_secret.as_deref(),
            )
            .await
            {
                Ok(client) => Some(client),
                Err(resp) => return resp,
            }
        }
    };

    match req.grant_type.as_str() {
        "authorization_code" => handle_authorization_code_grant(state, req, authenticated_client).await,
        "refresh_token" => handle_refresh_token_grant(state, req, authenticated_client).await,
        "client_credentials" => handle_client_credentials_grant(state, req).await,
        _ => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "unsupported_grant_type".to_string(),
                error_description: Some(format!("Grant type '{}' is not supported", req.grant_type)),
            }),
        ).into_response(),
    }
}

async fn handle_authorization_code_grant(state: OAuthState, req: TokenRequest, _authenticated_client: Option<OAuthClient>) -> Response {
    let code = match req.code {
        Some(c) => c,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_request".to_string(),
                    error_description: Some("Missing 'code' parameter".to_string()),
                }),
            ).into_response();
        }
    };

    // Find valid authorization code (not used, not expired)
    let auth_code = match state.auth_code_repo.find_valid_code(&code).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Invalid or expired authorization code".to_string()),
                }),
            ).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup authorization code");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    };

    // Mark code as used (single-use enforcement)
    if let Err(e) = state.auth_code_repo.mark_as_used(&code).await {
        error!(error = %e, "Failed to mark authorization code as used");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "server_error".to_string(),
                error_description: None,
            }),
        ).into_response();
    }

    // Validate redirect_uri
    if req.redirect_uri.as_deref() != Some(&auth_code.redirect_uri) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_grant".to_string(),
                error_description: Some("Redirect URI mismatch".to_string()),
            }),
        ).into_response();
    }

    // Validate client_id
    if req.client_id.as_deref() != Some(&auth_code.client_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_grant".to_string(),
                error_description: Some("Client ID mismatch".to_string()),
            }),
        ).into_response();
    }

    // Validate PKCE if code_challenge was provided
    if let Some(ref challenge) = auth_code.code_challenge {
        let verifier = match req.code_verifier {
            Some(v) => v,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "invalid_grant".to_string(),
                        error_description: Some("Missing code_verifier".to_string()),
                    }),
                ).into_response();
            }
        };

        let method = auth_code.code_challenge_method.as_deref().unwrap_or("S256");
        let computed_challenge = if method == "S256" {
            let mut hasher = Sha256::new();
            hasher.update(verifier.as_bytes());
            URL_SAFE_NO_PAD.encode(hasher.finalize())
        } else {
            verifier.clone()
        };

        if computed_challenge != *challenge {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Invalid code_verifier".to_string()),
                }),
            ).into_response();
        }
    }

    // Get the principal
    let principal = match state.principal_repo.find_by_id(&auth_code.principal_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Principal not found".to_string()),
                }),
            ).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to get principal");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    };

    // Generate access token
    let access_token = match state.auth_service.generate_access_token(&principal) {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to generate access token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    };

    // Generate ID token when scope includes "openid"
    let has_openid = auth_code.scope.as_deref()
        .map(|s| s.split_whitespace().any(|sc| sc == "openid"))
        .unwrap_or(false);

    let id_token = if has_openid {
        match state.auth_service.generate_id_token(&principal, &auth_code.client_id, auth_code.nonce.clone()) {
            Ok(t) => Some(t),
            Err(e) => {
                error!(error = %e, "Failed to generate ID token");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "server_error".to_string(),
                        error_description: None,
                    }),
                ).into_response();
            }
        }
    } else {
        None
    };

    // P1-6: Generate refresh token when scope includes "offline_access"
    let has_offline_access = auth_code.scope.as_deref()
        .map(|s| s.split_whitespace().any(|sc| sc == "offline_access"))
        .unwrap_or(false);

    let refresh_token = if has_offline_access {
        let (raw_token, token_entity) = RefreshToken::generate_token_pair(&principal.id);
        let scopes: Vec<String> = auth_code.scope.as_deref()
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        let token_entity = token_entity
            .with_oauth_client(auth_code.client_id.clone())
            .with_scopes(scopes);

        match state.refresh_token_repo.insert(&token_entity).await {
            Ok(_) => Some(raw_token),
            Err(e) => {
                error!(error = %e, "Failed to store refresh token");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "server_error".to_string(),
                        error_description: None,
                    }),
                ).into_response();
            }
        }
    } else {
        None
    };

    info!(principal_id = %principal.id, client_id = %auth_code.client_id, "Token issued via authorization code grant");

    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(TokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in: 3600,
            refresh_token,
            id_token,
            scope: auth_code.scope,
        }),
    ).into_response()
}

async fn handle_refresh_token_grant(state: OAuthState, req: TokenRequest, authenticated_client: Option<OAuthClient>) -> Response {
    // Validate refresh_token parameter
    let refresh_token_str = match req.refresh_token {
        Some(t) => t,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_request".to_string(),
                    error_description: Some("Missing refresh_token parameter".to_string()),
                }),
            ).into_response();
        }
    };

    // Hash the provided token and look it up
    let token_hash = RefreshToken::hash_token(&refresh_token_str);

    let stored_token = match state.refresh_token_repo.find_valid_by_hash(&token_hash).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Invalid or expired refresh token".to_string()),
                }),
            ).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup refresh token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    };

    // P1-3: Validate that the requesting client_id matches the stored token's oauth_client_id
    if let Some(ref stored_client_id) = stored_token.oauth_client_id {
        let requesting_client_id = authenticated_client.as_ref().map(|c| c.client_id.as_str());
        if requesting_client_id != Some(stored_client_id.as_str()) {
            warn!(
                stored_client_id = %stored_client_id,
                requesting_client_id = ?requesting_client_id,
                "Refresh token client binding mismatch"
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Token was not issued to this client".to_string()),
                }),
            ).into_response();
        }
    }

    // Revoke the old token (token rotation for security)
    if let Err(e) = state.refresh_token_repo.revoke_by_hash(&token_hash).await {
        error!(error = %e, "Failed to revoke old refresh token");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "server_error".to_string(),
                error_description: None,
            }),
        ).into_response();
    }

    // Find the principal
    let principal = match state.principal_repo.find_by_id(&stored_token.principal_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Principal not found".to_string()),
                }),
            ).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup principal");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    };

    // Check if principal is still active
    if !principal.active {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid_grant".to_string(),
                error_description: Some("Account is not active".to_string()),
            }),
        ).into_response();
    }

    // Generate new access token
    let access_token = match state.auth_service.generate_access_token(&principal) {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to generate access token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    };

    // Generate ID token when the original scope included "openid"
    // P2-8: Only generate ID token if we have a real oauth_client_id for the audience.
    // Never fall back to principal_id as audience — that's semantically wrong.
    let has_openid = stored_token.scopes.iter().any(|s| s == "openid");
    let id_token = if has_openid {
        if let Some(ref client_id) = stored_token.oauth_client_id {
            match state.auth_service.generate_id_token(&principal, client_id, None) {
                Ok(t) => Some(t),
                Err(e) => {
                    error!(error = %e, "Failed to generate ID token on refresh");
                    None // Non-fatal: still return access + refresh tokens
                }
            }
        } else {
            // No oauth_client_id — skip ID token entirely
            None
        }
    } else {
        None
    };

    // Generate new refresh token (rotation)
    let (raw_token, token_entity) = RefreshToken::generate_token_pair(&principal.id);
    let token_entity = token_entity
        .with_accessible_clients(stored_token.accessible_clients.clone())
        .with_scopes(stored_token.scopes.clone());

    // Preserve oauth_client_id on rotated token
    let token_entity = if let Some(ref cid) = stored_token.oauth_client_id {
        token_entity.with_oauth_client(cid.clone())
    } else {
        token_entity
    };

    if let Err(e) = state.refresh_token_repo.insert(&token_entity).await {
        error!(error = %e, "Failed to store new refresh token");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "server_error".to_string(),
                error_description: None,
            }),
        ).into_response();
    }

    info!(principal_id = %principal.id, "Token refreshed via refresh_token grant");

    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(TokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in: 3600,
            refresh_token: Some(raw_token),
            id_token,
            scope: None,
        }),
    ).into_response()
}

async fn handle_client_credentials_grant(state: OAuthState, req: TokenRequest) -> Response {
    let client_id = match req.client_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_request".to_string(),
                    error_description: Some("Missing client_id".to_string()),
                }),
            ).into_response();
        }
    };

    let client_secret = match req.client_secret {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_request".to_string(),
                    error_description: Some("Missing client_secret".to_string()),
                }),
            ).into_response();
        }
    };

    // Lookup client
    let client = match state.oauth_client_repo.find_by_client_id(&client_id).await {
        Ok(Some(c)) if c.active => c,
        Ok(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Invalid client credentials".to_string()),
                }),
            ).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup client");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    };

    // Verify client type is CONFIDENTIAL
    if client.client_type != crate::auth::oauth_entity::OAuthClientType::Confidential {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized_client".to_string(),
                error_description: Some("Public clients cannot use client_credentials grant".to_string()),
            }),
        ).into_response();
    }

    // Verify client_secret against stored hash
    let secret_hash = match &client.client_secret_ref {
        Some(hash) => hash,
        None => {
            warn!(client_id = %client_id, "Client has no secret configured");
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Invalid client credentials".to_string()),
                }),
            ).into_response();
        }
    };

    match state.password_service.verify_password(&client_secret, secret_hash) {
        Ok(true) => { /* secret verified */ }
        Ok(false) => {
            warn!(client_id = %client_id, "Client secret verification failed");
            // Log failed service account login attempt
            let mut attempt = LoginAttempt::new(AttemptType::ServiceAccountToken, LoginOutcome::Failure);
            attempt.identifier = Some(client_id.clone());
            attempt.failure_reason = Some("Invalid client secret".to_string());
            if let Err(e) = state.login_attempt_repo.create(&attempt).await {
                warn!(error = %e, "Failed to log service account login attempt");
            }
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Invalid client credentials".to_string()),
                }),
            ).into_response();
        }
        Err(e) => {
            error!(client_id = %client_id, error = %e, "Client secret verification error");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    }

    // Look up the real service account principal (with roles/permissions)
    let principal_id = match &client.service_account_principal_id {
        Some(id) => id,
        None => {
            error!(client_id = %client_id, "Client has no service account principal configured");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: Some("Client not properly configured".to_string()),
                }),
            ).into_response();
        }
    };

    let principal = match state.principal_repo.find_by_id(principal_id).await {
        Ok(Some(p)) if p.active => p,
        Ok(Some(_)) => {
            warn!(client_id = %client_id, "Service account principal is inactive");
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some("Service account is not active".to_string()),
                }),
            ).into_response();
        }
        Ok(None) => {
            error!(client_id = %client_id, principal_id = %principal_id, "Service account principal not found");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: Some("Client not properly configured".to_string()),
                }),
            ).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup service account principal");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    };

    let access_token = match state.auth_service.generate_access_token(&principal) {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to generate access token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            ).into_response();
        }
    };

    // Log successful service account login attempt
    let mut attempt = LoginAttempt::new(AttemptType::ServiceAccountToken, LoginOutcome::Success);
    attempt.identifier = Some(client_id.clone());
    attempt.principal_id = Some(principal.id.clone());
    if let Err(e) = state.login_attempt_repo.create(&attempt).await {
        warn!(error = %e, "Failed to log service account login attempt");
    }

    info!(client_id = %client_id, "Token issued via client credentials grant");

    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(TokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in: 3600,
            refresh_token: None,
            id_token: None,
            scope: None,
        }),
    ).into_response()
}

// P0-2: oidc_callback removed. The OIDC login flow in oidc_login_api.rs handles
// external IDP callbacks and carries OAuth params through via OidcLoginState.
// The authorize endpoint redirects to the SPA login or directly to the IDP login flow,
// both of which use `issue_code()` below after the principal is authenticated.

/// Issue authorization code after successful login
pub async fn issue_code(
    state: &OAuthState,
    principal_id: &str,
    pending_state: &str,
) -> Result<String, PlatformError> {
    let pending = state.pending_auth_repo.find_and_consume(pending_state).await
        .map_err(|e| {
            error!(error = %e, "Failed to lookup pending auth state");
            PlatformError::Internal {
                message: "Failed to lookup pending auth state".to_string(),
            }
        })?
        .ok_or_else(|| PlatformError::InvalidToken {
            message: "Invalid or expired state".to_string(),
        })?;

    let auth_code_str = generate_random_string(64);

    // Build authorization code using domain model
    let mut auth_code = AuthorizationCode::new(
        auth_code_str.clone(),
        pending.client_id,
        principal_id.to_string(),
        pending.redirect_uri,
    )
    .with_scope(pending.scope)
    .with_nonce(pending.nonce);

    if let (Some(challenge), Some(method)) = (pending.code_challenge, pending.code_challenge_method) {
        auth_code = auth_code.with_pkce(challenge, method);
    }

    // Store authorization code
    state.auth_code_repo.insert(&auth_code).await.map_err(|e| {
        error!(error = %e, "Failed to store authorization code");
        PlatformError::Internal {
            message: "Failed to create authorization code".to_string(),
        }
    })?;

    Ok(auth_code_str)
}

/// Check if a redirect URI matches any of the registered URIs.
/// Supports exact matches and wildcard patterns where `*` matches a single
/// subdomain segment (e.g. `https://*.example.com/callback` matches
/// `https://app.example.com/callback` but not `https://a.b.example.com/callback`).
fn matches_redirect_uri(uri: &str, registered: &[String]) -> bool {
    // Exact match first
    if registered.contains(&uri.to_string()) {
        return true;
    }

    // Wildcard pattern matching
    for pattern in registered {
        if !pattern.contains('*') {
            continue;
        }
        if wildcard_matches(uri, pattern) {
            return true;
        }
    }

    false
}

/// Match a URI against a pattern containing `*` wildcards.
/// Each `*` matches exactly one subdomain segment (no dots).
fn wildcard_matches(uri: &str, pattern: &str) -> bool {
    // Split pattern on '*' and verify the URI matches all parts in order
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.is_empty() {
        return false;
    }

    // The URI must start with the first part
    let Some(remainder) = uri.strip_prefix(parts[0]) else {
        return false;
    };

    let mut remaining = remainder;
    for (i, part) in parts[1..].iter().enumerate() {
        let is_last = i == parts.len() - 2;
        if is_last {
            // Last part must match the end exactly
            if !remaining.ends_with(part) {
                return false;
            }
            // The wildcard segment (between previous part and this part) must not contain dots
            let wildcard_segment = &remaining[..remaining.len() - part.len()];
            if wildcard_segment.contains('.') || wildcard_segment.is_empty() {
                return false;
            }
            return true;
        } else {
            // Find the next occurrence of this part
            if let Some(pos) = remaining.find(part) {
                let wildcard_segment = &remaining[..pos];
                if wildcard_segment.contains('.') || wildcard_segment.is_empty() {
                    return false;
                }
                remaining = &remaining[pos + part.len()..];
            } else {
                return false;
            }
        }
    }

    // If pattern ends with '*', remaining must be a single segment (no dots)
    !remaining.contains('.') && !remaining.is_empty()
}

fn error_redirect(redirect_uri: &str, error: &str, description: &str, state: Option<&str>) -> Response {
    let mut url = redirect_uri.to_string();
    url.push_str(&format!(
        "?error={}&error_description={}",
        urlencoding::encode(error),
        urlencoding::encode(description),
    ));
    if let Some(s) = state {
        url.push_str(&format!("&state={}", urlencoding::encode(s)));
    }
    Redirect::temporary(&url).into_response()
}

fn generate_random_string(len: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..len)
        .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
        .collect()
}

/// Helper to extract and validate bearer token from request headers
fn extract_and_validate_token(headers: &HeaderMap, auth_service: &AuthService) -> Result<AccessTokenClaims, Response> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_request".to_string(),
                    error_description: Some("Missing Authorization header".to_string()),
                }),
            ).into_response()
        })?;

    let token = extract_bearer_token(auth_header).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid_request".to_string(),
                error_description: Some("Invalid Authorization header format".to_string()),
            }),
        ).into_response()
    })?;

    auth_service.validate_token(token).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid_token".to_string(),
                error_description: Some("Token is invalid or expired".to_string()),
            }),
        ).into_response()
    })
}

/// UserInfo endpoint (OIDC Core 1.0 Section 5.3)
///
/// Returns claims about the authenticated user based on the access token.
#[utoipa::path(
    get,
    path = "/userinfo",
    tag = "oauth",
    responses(
        (status = 200, description = "User info", body = UserInfoResponse),
        (status = 401, description = "Invalid or missing token", body = ErrorResponse)
    )
)]
pub async fn userinfo(
    State(state): State<OAuthState>,
    headers: HeaderMap,
) -> Response {
    let claims = match extract_and_validate_token(&headers, &state.auth_service) {
        Ok(c) => c,
        Err(r) => return r,
    };

    (
        StatusCode::OK,
        Json(UserInfoResponse {
            sub: claims.sub,
            email: claims.email,
            name: Some(claims.name),
            scope: Some(claims.scope),
            principal_type: Some(claims.principal_type),
            client_id: claims.clients.first().and_then(|c| {
                // Extract the raw client ID from "id:identifier" format
                if c == "*" { None } else { Some(c.split(':').next().unwrap_or(c).to_string()) }
            }),
            clients: Some(claims.clients),
            roles: Some(claims.roles.clone()),
            applications: Some(claims.applications),
        }),
    ).into_response()
}

/// Token introspection request with optional client credentials in body
#[derive(Debug, Deserialize, ToSchema)]
pub struct IntrospectRequestFull {
    pub token: String,
    #[serde(default)]
    pub token_type_hint: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
}

/// Token introspection endpoint (RFC 7662)
///
/// Returns metadata about a token, including whether it is active.
/// Requires authentication via Bearer token or client credentials.
#[utoipa::path(
    post,
    path = "/introspect",
    tag = "oauth",
    request_body = IntrospectRequest,
    responses(
        (status = 200, description = "Token introspection result", body = IntrospectResponse),
        (status = 401, description = "Authentication required", body = ErrorResponse),
    )
)]
pub async fn introspect(
    State(state): State<OAuthState>,
    headers: HeaderMap,
    Form(req): Form<IntrospectRequestFull>,
) -> Response {
    // P1-4: Require authentication (Bearer token or client credentials)
    if let Err(resp) = authenticate_client_or_bearer(
        &state,
        &headers,
        req.client_id.as_deref(),
        req.client_secret.as_deref(),
    ).await {
        return resp;
    }

    // Try to validate as access token
    match state.auth_service.validate_token(&req.token) {
        Ok(claims) => {
            (
                StatusCode::OK,
                Json(IntrospectResponse {
                    active: true,
                    sub: Some(claims.sub),
                    scope: Some(claims.scope),
                    client_id: claims.clients.first().cloned(),
                    email: claims.email,
                    name: Some(claims.name),
                    principal_type: Some(claims.principal_type),
                    exp: Some(claims.exp),
                    iat: Some(claims.iat),
                    iss: Some(claims.iss),
                    token_type: Some("Bearer".to_string()),
                }),
            ).into_response()
        }
        Err(_) => {
            // Per RFC 7662: inactive tokens just return active=false
            (
                StatusCode::OK,
                Json(IntrospectResponse {
                    active: false,
                    sub: None,
                    scope: None,
                    client_id: None,
                    email: None,
                    name: None,
                    principal_type: None,
                    exp: None,
                    iat: None,
                    iss: None,
                    token_type: None,
                }),
            ).into_response()
        }
    }
}

/// Token revocation request with optional client credentials in body
#[derive(Debug, Deserialize, ToSchema)]
pub struct RevokeRequestFull {
    pub token: String,
    #[serde(default)]
    pub token_type_hint: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
}

/// Token revocation endpoint (RFC 7009)
///
/// Revokes an access token or refresh token. Always returns 200 per spec.
/// Requires authentication via Bearer token or client credentials.
#[utoipa::path(
    post,
    path = "/revoke",
    tag = "oauth",
    request_body = RevokeRequest,
    responses(
        (status = 200, description = "Token revoked (or was already invalid)"),
        (status = 401, description = "Authentication required", body = ErrorResponse),
    )
)]
pub async fn revoke(
    State(state): State<OAuthState>,
    headers: HeaderMap,
    Form(req): Form<RevokeRequestFull>,
) -> Response {
    // P1-5: Require authentication (Bearer token or client credentials)
    if let Err(resp) = authenticate_client_or_bearer(
        &state,
        &headers,
        req.client_id.as_deref(),
        req.client_secret.as_deref(),
    ).await {
        return resp;
    }

    // Determine token type
    let is_refresh = req.token_type_hint.as_deref() == Some("refresh_token");

    if is_refresh {
        // Revoke refresh token by hash
        let token_hash = RefreshToken::hash_token(&req.token);
        if let Err(e) = state.refresh_token_repo.revoke_by_hash(&token_hash).await {
            warn!(error = %e, "Failed to revoke refresh token");
        }
    } else {
        // For access tokens (JWTs), we can try revoking as refresh token too
        // since the caller might not know the token type. JWT access tokens
        // are stateless and can't be revoked server-side without a blocklist.
        let token_hash = RefreshToken::hash_token(&req.token);
        let _ = state.refresh_token_repo.revoke_by_hash(&token_hash).await;
    }

    // RFC 7009: Always return 200, even if token was invalid
    StatusCode::OK.into_response()
}

/// Create OAuth router
pub fn oauth_router(state: OAuthState) -> Router {
    Router::new()
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .route("/userinfo", get(userinfo).post(userinfo))
        .route("/introspect", post(introspect))
        .route("/revoke", post(revoke))
        .with_state(state)
}
