//! OAuth2 Authorization Endpoints
//!
//! Implements OAuth2 authorization code flow with PKCE support:
//! - GET /oauth/authorize - Authorization endpoint
//! - POST /oauth/token - Token endpoint
//! - POST /oauth/revoke - Token revocation

use axum::{
    extract::{Form, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Redirect, Response},
    routing::{get, post},
    Router,
};
use base64::Engine as _;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, warn};
use utoipa::{IntoParams, ToSchema};

use crate::auth::auth_service::{extract_bearer_token, AccessTokenClaims};
use crate::auth::authorization_code::{Pkce, PkceMethod};
use crate::auth::oauth_entity::{GrantType, OAuthClient};
use crate::auth::password_service::PasswordService;
use crate::auth::pending_auth_repository::{PendingAuth, PendingAuthRepository};
use crate::login_attempt::entity::{AttemptType, LoginAttempt, LoginOutcome};
use crate::login_attempt::repository::LoginAttemptRepository;
use crate::shared::error::PlatformError;
use crate::AuthService;
use crate::{AuthorizationCode, RefreshToken};
use crate::{
    AuthorizationCodeRepository, OAuthClientRepository, PrincipalRepository, RefreshTokenRepository,
};

/// Authorization request parameters
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuthorizeRequest {
    pub response_type: String,
    pub client_id: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub scope: Option<String>,
    /// Required. Echoed back to the client on the callback so it can detect
    /// CSRF-pinned authorization codes (OAuth 2.0 Security BCP §4.7). PKCE
    /// protects the code itself; `state` protects the redirect flow.
    pub state: Option<String>,
    pub nonce: Option<String>,
    /// PKCE code challenge
    pub code_challenge: Option<String>,
    /// PKCE code challenge method (S256 or plain)
    pub code_challenge_method: Option<String>,
    /// Provider ID for external OIDC
    pub provider: Option<String>,
    /// OIDC max_age: maximum authentication age in seconds
    pub max_age: Option<i64>,
    /// OIDC prompt: space-separated list of prompt values (none, login, consent, select_account)
    pub prompt: Option<String>,
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
    /// Per-`client_id` rate limit on `/oauth/token` (composes with the
    /// per-IP middleware that wraps `/oauth/*`).
    pub client_token_rate_limit: crate::shared::rate_limit_middleware::IpRateLimiterState,
    /// Cluster-wide rate-limit store (Redis or Postgres). Per-client_id
    /// distributed enforcement on `/oauth/token` and `/oauth/authorize`
    /// runs through this on top of the in-memory governor.
    pub rate_limit_store: Arc<dyn crate::shared::rate_limit_store::RateLimitStore>,
    /// Per-bucket policies (window + limit), loaded once from env.
    pub rate_limit_policies: Arc<crate::shared::rate_limit_store::RateLimitPolicies>,
    /// Verifies client secrets. `None` when `FLOWCATALYST_APP_KEY` is unset,
    /// in which case every confidential client is refused.
    pub encryption_service: Option<Arc<crate::shared::encryption_service::EncryptionService>>,
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
        return error_redirect(
            &req.redirect_uri,
            "unsupported_response_type",
            "Only 'code' response type is supported",
            req.state.as_deref(),
        );
    }

    // Require `state` for CSRF protection on the callback. Missing/empty
    // `state` is rejected with 400 (not a redirect) — we can't safely bounce
    // the user-agent back to the caller without proving the caller is who
    // they claim to be, and `state` is the mechanism by which they do that.
    if req.state.as_deref().is_none_or(|s| s.trim().is_empty()) {
        warn!(client_id = %req.client_id, "authorize rejected: missing `state` parameter");
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_request".to_string(),
                error_description: Some(
                    "`state` parameter is required for CSRF protection".to_string(),
                ),
            }),
        )
            .into_response();
    }

    // Cluster-wide per-`client_id` rate limit. Runs before the DB lookup so a
    // client that's spamming us can't amplify load on the OAuth client cache.
    // The per-IP layer wrapping `/oauth/*` already throttles raw volume; this
    // catches a single client_id sprayed across many IPs.
    if let Err(resp) = crate::shared::rate_limit_store::enforce_distributed(
        &*state.rate_limit_store,
        crate::shared::rate_limit_store::Bucket::OAUTH_AUTHORIZE_CLIENT,
        &req.client_id,
        state.rate_limit_policies.oauth_authorize_client,
    )
    .await
    {
        return resp;
    }

    // Validate client
    let client = match state
        .oauth_client_repo
        .find_by_client_id(&req.client_id)
        .await
    {
        Ok(Some(c)) if c.active => c,
        Ok(Some(_)) => {
            return error_redirect(
                &req.redirect_uri,
                "unauthorized_client",
                "Client is not active",
                req.state.as_deref(),
            );
        }
        Ok(None) => {
            return error_redirect(
                &req.redirect_uri,
                "unauthorized_client",
                "Unknown client",
                req.state.as_deref(),
            );
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup client");
            return error_redirect(
                &req.redirect_uri,
                "server_error",
                "Internal error",
                req.state.as_deref(),
            );
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
        )
            .into_response();
    }

    // Validate PKCE if required
    if client.pkce_required && req.code_challenge.is_none() {
        return error_redirect(
            &req.redirect_uri,
            "invalid_request",
            "PKCE code_challenge is required",
            req.state.as_deref(),
        );
    }

    // Validate code_challenge_method
    if let Some(ref method) = req.code_challenge_method {
        match method.parse::<PkceMethod>() {
            Err(_) => {
                return error_redirect(
                    &req.redirect_uri,
                    "invalid_request",
                    "Invalid code_challenge_method",
                    req.state.as_deref(),
                );
            }
            Ok(PkceMethod::Plain) => {
                warn!(client_id = %req.client_id, "PKCE plain method used — S256 is strongly recommended");
            }
            Ok(PkceMethod::S256) => {}
        }
    }
    // The method was validated just above, so this can't fail.
    let pkce = Pkce::from_parts(
        req.code_challenge.clone(),
        req.code_challenge_method.as_deref(),
    )
    .ok()
    .flatten();

    // Validate requested scopes against client's allowed scopes
    if let Some(ref scope_str) = req.scope {
        let standard_scopes: &[&str] = &["openid", "profile", "email", "offline_access"];
        let invalid_scopes: Vec<&str> = scope_str
            .split_whitespace()
            .filter(|s| {
                !standard_scopes.contains(s) && !client.default_scopes.iter().any(|ds| ds == *s)
            })
            .collect();
        if !invalid_scopes.is_empty() {
            return error_redirect(
                &req.redirect_uri,
                "invalid_scope",
                &format!("Invalid scope(s): {}", invalid_scopes.join(", ")),
                req.state.as_deref(),
            );
        }
    }

    // Check if user is already authenticated (has valid session cookie).
    // If so, skip the login redirect and issue the authorization code directly.
    let session_token = jar
        .get("fc_session")
        .map(|c| c.value().to_string())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(extract_bearer_token)
                .map(|t| t.to_string())
        });

    // Handle `prompt` parameter (OIDC Core Section 3.1.2.1)
    let force_login = if let Some(ref prompt) = req.prompt {
        match prompt.as_str() {
            "none" => {
                // prompt=none: if user is not authenticated, return login_required error
                let has_valid_session = session_token
                    .as_ref()
                    .and_then(|t| state.auth_service.validate_token(t).ok())
                    .is_some();
                if !has_valid_session {
                    return error_redirect(
                        &req.redirect_uri,
                        "login_required",
                        "User is not authenticated",
                        req.state.as_deref(),
                    );
                }
                false
            }
            "login" => {
                // prompt=login: force re-authentication — skip session check
                true
            }
            _ => false, // consent, select_account — not applicable
        }
    } else {
        false
    };

    if !force_login {
        if let Some(ref token) = session_token {
            if let Ok(claims) = state.auth_service.validate_token(token) {
                // Check max_age: if session is older than max_age seconds, force re-authentication
                let session_too_old = req.max_age.is_some_and(|max_age| {
                    let now = Utc::now().timestamp();
                    now - claims.iat > max_age
                });

                if !session_too_old {
                    // User is authenticated — issue authorization code immediately
                    let auth_code_str = generate_random_string(64);
                    let mut auth_code = AuthorizationCode {
                        scope: req.scope.clone(),
                        nonce: req.nonce.clone(),
                        state: req.state.clone(),
                        ..AuthorizationCode::new(
                            auth_code_str.clone(),
                            req.client_id.clone(),
                            claims.sub.clone(),
                            req.redirect_uri.clone(),
                        )
                    };

                    auth_code = auth_code.with_pkce(pkce.clone());

                    if let Err(e) = state.auth_code_repo.insert(&auth_code).await {
                        error!(error = %e, "Failed to store authorization code");
                        return error_redirect(
                            &req.redirect_uri,
                            "server_error",
                            "Failed to create authorization code",
                            req.state.as_deref(),
                        );
                    }

                    let mut redirect_url = format!(
                        "{}?code={}",
                        req.redirect_uri,
                        urlencoding::encode(&auth_code_str)
                    );
                    if let Some(ref s) = req.state {
                        redirect_url.push_str(&format!("&state={}", urlencoding::encode(s)));
                    }

                    info!(client_id = %req.client_id, principal_id = %claims.sub, "Issued authorization code (authenticated session)");
                    return Redirect::temporary(&redirect_url).into_response();
                } // !session_too_old
            } // validate_token Ok
        } // session_token Some
    } // !force_login

    // User is not authenticated — proceed with login flow
    // Generate state for CSRF protection if not provided
    let state_param = req
        .state
        .clone()
        .unwrap_or_else(|| generate_random_string(32));

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
        return error_redirect(
            &req.redirect_uri,
            "server_error",
            "Internal error",
            req.state.as_deref(),
        );
    }

    // `?provider=` selected a statically registered external OIDC provider.
    // No such registry exists — federated login goes through
    // `/auth/oidc/login` (IdentityProvider + EmailDomainMapping) — so any
    // provider named here is unknown and the flow can't be initialised.
    if let Some(provider_id) = req.provider {
        error!(provider = %provider_id, "Unknown OIDC provider; failed to get authorization URL");
        return error_redirect(
            &req.redirect_uri,
            "server_error",
            "Failed to initialize OIDC flow",
            req.state.as_deref(),
        );
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
        login_url.push_str(&format!(
            "&code_challenge={}",
            urlencoding::encode(challenge)
        ));
    }
    if let Some(ref method) = req.code_challenge_method {
        login_url.push_str(&format!(
            "&code_challenge_method={}",
            urlencoding::encode(method)
        ));
    }
    if let Some(ref nonce) = req.nonce {
        login_url.push_str(&format!("&nonce={}", urlencoding::encode(nonce)));
    }

    Redirect::temporary(&login_url).into_response()
}

/// Check `provided` against a client's stored secret ref and, when it
/// matches a shape other than the current `hashed:v1:` form (an older
/// `encrypted:` ref, a bare envelope, or a hash under a previous app key),
/// rewrite the ref to [`EncryptionService::hash_secret`] of the secret the
/// caller just proved it holds. Mirrors the Go platform's
/// `acceptClientSecret`, which shares the `oauth_clients` table.
///
/// The rewrite is best-effort: authentication has already succeeded, so a
/// failure is logged and dropped. It goes straight to the repository rather
/// than through a use case: it changes the at-rest format of a secret, not
/// the secret itself, and runs on the token endpoint's hot path. A domain
/// event and audit row per migrated client would record a storage upgrade,
/// not a business fact (same reasoning as the other infrastructure
/// exceptions in `CLAUDE.md`).
///
/// [`EncryptionService::hash_secret`]: crate::shared::encryption_service::EncryptionService::hash_secret
async fn verify_client_secret(
    state: &OAuthState,
    client: &OAuthClient,
    stored: &str,
    provided: &str,
) -> bool {
    let Some(enc) = state.encryption_service.as_deref() else {
        error!(client_id = %client.client_id, "Cannot verify client secret — FLOWCATALYST_APP_KEY not configured");
        return false;
    };
    let (ok, needs_rehash) = enc.verify_secret(stored, provided);
    if ok && needs_rehash {
        let hashed = enc.hash_secret(provided);
        if let Err(e) = state
            .oauth_client_repo
            .rewrite_secret_ref(client, stored, &hashed)
            .await
        {
            warn!(client_id = %client.client_id, error = %e, "Could not migrate client secret to hashed form");
        }
    }
    ok
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
        (
            id.to_string(),
            if secret.is_empty() {
                None
            } else {
                Some(secret.to_string())
            },
        )
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

    // Reject client_secret for public clients (no stored secret).
    // Per RFC 6749 Section 2.1, public clients MUST NOT use client authentication.
    if client.client_secret_ref.is_none() {
        if client_secret.is_some() {
            warn!(client_id = %client_id, "client_secret provided for public client — rejecting");
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some(
                        "Public clients must not provide a client_secret".to_string(),
                    ),
                }),
            )
                .into_response());
        }
        // Public client with no secret provided — OK
        return Ok(client);
    }

    // If confidential client (has a secret), verify it against the stored
    // verify-only ref (see `verify_client_secret`).
    if let Some(ref secret_ref) = client.client_secret_ref {
        let provided_secret = client_secret.ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: Some(
                        "Client secret required for confidential clients".to_string(),
                    ),
                }),
            )
                .into_response()
        })?;

        let verified = verify_client_secret(state, &client, secret_ref, &provided_secret).await;

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
    if let Some(auth_header) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
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
    // Per-client_id rate limit. Composes with the per-IP layer that already
    // wraps `/oauth/*` — this catches a single client running away with
    // refresh-token churn from many IPs (which the per-IP layer wouldn't
    // detect on its own).
    //
    // Two limiters run in series: the in-memory `governor` rejects bursts
    // on this instance (sub-ms), then the cluster-wide store catches the
    // same client_id when traffic is sprayed across replicas (one
    // round-trip to Redis/Postgres).
    if let Some(ref client_id) = req.client_id {
        if let Err(retry_after) = state.client_token_rate_limit.check(client_id) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
                Json(ErrorResponse {
                    error: "rate_limit_exceeded".to_string(),
                    error_description: Some(
                        "this client_id has exceeded its token endpoint rate limit".to_string(),
                    ),
                }),
            )
                .into_response();
        }
        if let Err(resp) = crate::shared::rate_limit_store::enforce_distributed(
            &*state.rate_limit_store,
            crate::shared::rate_limit_store::Bucket::OAUTH_TOKEN_CLIENT,
            client_id,
            state.rate_limit_policies.oauth_token_client,
        )
        .await
        {
            return resp;
        }
    }

    // P0-1: Authenticate the client before processing any grant type.
    // For client_credentials grant, the handler does its own auth (backward compat),
    // but for authorization_code and refresh_token, we authenticate here.
    // An unrecognised grant type still authenticates the client first, then
    // gets `unsupported_grant_type` below.
    let grant_type = req.grant_type.parse::<GrantType>().ok();
    let authenticated_client = match grant_type {
        Some(GrantType::ClientCredentials) => {
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

    match grant_type {
        Some(GrantType::AuthorizationCode) => {
            handle_authorization_code_grant(state, req, authenticated_client).await
        }
        Some(GrantType::RefreshToken) => {
            handle_refresh_token_grant(state, req, authenticated_client).await
        }
        Some(GrantType::ClientCredentials) => handle_client_credentials_grant(state, req).await,
        Some(GrantType::Password) | None => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "unsupported_grant_type".to_string(),
                error_description: Some(format!(
                    "Grant type '{}' is not supported",
                    req.grant_type
                )),
            }),
        )
            .into_response(),
    }
}

async fn handle_authorization_code_grant(
    state: OAuthState,
    req: TokenRequest,
    _authenticated_client: Option<OAuthClient>,
) -> Response {
    let code = match req.code {
        Some(c) => c,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_request".to_string(),
                    error_description: Some("Missing 'code' parameter".to_string()),
                }),
            )
                .into_response();
        }
    };

    // Atomically consume the authorization code (single-use enforcement).
    // Uses UPDATE...WHERE consumed_at IS NULL...RETURNING to prevent race conditions
    // where two concurrent requests could both redeem the same code.
    let auth_code = match state.auth_code_repo.find_and_consume(&code).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Invalid or expired authorization code".to_string()),
                }),
            )
                .into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to consume authorization code");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
                .into_response();
        }
    };

    // Check authorization code TTL (10 minutes per RFC 6749 Section 4.1.2)
    let code_age_secs = (Utc::now() - auth_code.created_at).num_seconds();
    if code_age_secs > 600 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_grant".to_string(),
                error_description: Some("Authorization code has expired".to_string()),
            }),
        )
            .into_response();
    }

    // Validate client_id — code is already consumed, so replay is impossible
    if req.client_id.as_deref() != Some(&auth_code.client_id) {
        warn!("Authorization code client_id mismatch after atomic consume");
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_grant".to_string(),
                error_description: Some("Client ID mismatch".to_string()),
            }),
        )
            .into_response();
    }

    // Validate redirect_uri — code is already consumed, so replay is impossible
    if req.redirect_uri.as_deref() != Some(&auth_code.redirect_uri) {
        warn!("Authorization code redirect_uri mismatch after atomic consume");
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_grant".to_string(),
                error_description: Some("Redirect URI mismatch".to_string()),
            }),
        )
            .into_response();
    }

    // Validate PKCE if code_challenge was provided
    if let Some(ref pkce) = auth_code.pkce {
        let verifier = match req.code_verifier {
            Some(v) => v,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "invalid_grant".to_string(),
                        error_description: Some("Missing code_verifier".to_string()),
                    }),
                )
                    .into_response();
            }
        };

        // Validate code_verifier length (RFC 7636: 43-128 characters)
        if verifier.len() < 43 || verifier.len() > 128 {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("code_verifier must be 43-128 characters".to_string()),
                }),
            )
                .into_response();
        }

        // Validate code_verifier characters (RFC 7636: unreserved characters only)
        if !verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some(
                        "code_verifier contains invalid characters".to_string(),
                    ),
                }),
            )
                .into_response();
        }

        if !pkce.verify(&verifier) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Invalid code_verifier".to_string()),
                }),
            )
                .into_response();
        }
    }

    // Get the principal
    let principal = match state
        .principal_repo
        .find_by_id(&auth_code.principal_id)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Principal not found".to_string()),
                }),
            )
                .into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to get principal");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
                .into_response();
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
            )
                .into_response();
        }
    };

    // Generate ID token when scope includes "openid"
    let has_openid = auth_code
        .scope
        .as_deref()
        .map(|s| s.split_whitespace().any(|sc| sc == "openid"))
        .unwrap_or(false);

    let id_token = if has_openid {
        match state.auth_service.generate_id_token(
            &principal,
            &auth_code.client_id,
            auth_code.nonce.clone(),
        ) {
            Ok(t) => Some(t),
            Err(e) => {
                error!(error = %e, "Failed to generate ID token");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "server_error".to_string(),
                        error_description: None,
                    }),
                )
                    .into_response();
            }
        }
    } else {
        None
    };

    // P1-6: Generate refresh token when scope includes "offline_access"
    let has_offline_access = auth_code
        .scope
        .as_deref()
        .map(|s| s.split_whitespace().any(|sc| sc == "offline_access"))
        .unwrap_or(false);

    let refresh_token = if has_offline_access {
        let (raw_token, token_entity) = RefreshToken::generate_token_pair(&principal.id);
        let scopes: Vec<String> = auth_code
            .scope
            .as_deref()
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
                )
                    .into_response();
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
    )
        .into_response()
}

async fn handle_refresh_token_grant(
    state: OAuthState,
    req: TokenRequest,
    authenticated_client: Option<OAuthClient>,
) -> Response {
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
            )
                .into_response();
        }
    };

    // Hash the provided token and look it up
    let token_hash = RefreshToken::hash_token(&refresh_token_str);

    let stored_token = match state
        .refresh_token_repo
        .find_valid_by_hash(&token_hash)
        .await
    {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Invalid or expired refresh token".to_string()),
                }),
            )
                .into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup refresh token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
                .into_response();
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
            )
                .into_response();
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
        )
            .into_response();
    }

    // Find the principal
    let principal = match state
        .principal_repo
        .find_by_id(&stored_token.principal_id)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Principal not found".to_string()),
                }),
            )
                .into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup principal");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
                .into_response();
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
        )
            .into_response();
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
            )
                .into_response();
        }
    };

    // Generate ID token when the original scope included "openid"
    // P2-8: Only generate ID token if we have a real oauth_client_id for the audience.
    // Never fall back to principal_id as audience — that's semantically wrong.
    let has_openid = stored_token.scopes.iter().any(|s| s == "openid");
    let id_token = if has_openid {
        if let Some(ref client_id) = stored_token.oauth_client_id {
            match state
                .auth_service
                .generate_id_token(&principal, client_id, None)
            {
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
        )
            .into_response();
    }

    info!(principal_id = %principal.id, "Token refreshed via refresh_token grant");

    // Include scope in the response per RFC 6749 Section 5.1
    let scope = if stored_token.scopes.is_empty() {
        None
    } else {
        Some(stored_token.scopes.join(" "))
    };

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
            scope,
        }),
    )
        .into_response()
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
            )
                .into_response();
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
            )
                .into_response();
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
            )
                .into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup client");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
                .into_response();
        }
    };

    // Verify client type is CONFIDENTIAL
    if client.client_type != crate::auth::oauth_entity::OAuthClientType::Confidential {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized_client".to_string(),
                error_description: Some(
                    "Public clients cannot use client_credentials grant".to_string(),
                ),
            }),
        )
            .into_response();
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
            )
                .into_response();
        }
    };

    let verified = verify_client_secret(&state, &client, secret_hash, &client_secret).await;

    if !verified {
        warn!(client_id = %client_id, "Client secret verification failed");
        let attempt = LoginAttempt {
            identifier: Some(client_id.clone()),
            failure_reason: Some("Invalid client secret".to_string()),
            ..LoginAttempt::new(AttemptType::ServiceAccountToken, LoginOutcome::Failure)
        };
        if let Err(e) = state.login_attempt_repo.create(&attempt).await {
            warn!(error = %e, "Failed to log service account login attempt");
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid_client".to_string(),
                error_description: Some("Invalid client credentials".to_string()),
            }),
        )
            .into_response();
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
            )
                .into_response();
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
            )
                .into_response();
        }
        Ok(None) => {
            error!(client_id = %client_id, principal_id = %principal_id, "Service account principal not found");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: Some("Client not properly configured".to_string()),
                }),
            )
                .into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup service account principal");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
                .into_response();
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
            )
                .into_response();
        }
    };

    // Log successful service account login attempt
    let attempt = LoginAttempt {
        identifier: Some(client_id.clone()),
        principal_id: Some(principal.id.clone()),
        ..LoginAttempt::new(AttemptType::ServiceAccountToken, LoginOutcome::Success)
    };
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
    )
        .into_response()
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
    let pending = state
        .pending_auth_repo
        .find_and_consume(pending_state)
        .await
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
    let mut auth_code = AuthorizationCode {
        scope: pending.scope,
        nonce: pending.nonce,
        ..AuthorizationCode::new(
            auth_code_str.clone(),
            pending.client_id,
            principal_id.to_string(),
            pending.redirect_uri,
        )
    };

    let method = pending.code_challenge_method.as_deref();
    let pkce = Pkce::from_parts(pending.code_challenge.clone(), method).map_err(|_| {
        crate::shared::enum_str::corrupt_value(
            "oauth_oidc_payloads",
            "payload.codeChallengeMethod",
            method.unwrap_or_default(),
            pending_state,
        )
    })?;
    auth_code = auth_code.with_pkce(pkce);

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
///
/// Exposed `pub(crate)` so the OIDC RP-Initiated Logout endpoint
/// (`oidc_login_api::session_end`) can reuse the same matcher for the
/// `post_logout_redirect_uri` whitelist check — both surfaces must apply
/// identical rules so a value registered as a callback isn't surprisingly
/// rejected at logout time (or vice versa).
pub(crate) fn matches_redirect_uri(uri: &str, registered: &[String]) -> bool {
    // Exact match first
    if registered.iter().any(|r| r == uri) {
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

fn error_redirect(
    redirect_uri: &str,
    error: &str,
    description: &str,
    state: Option<&str>,
) -> Response {
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

/// Helper to extract and validate bearer token from request headers.
/// The `Err` is an `axum::Response` (~128 bytes) which clippy flags as
/// large — boxing would add an allocation per failed lookup with no real
/// benefit, since the response is consumed immediately by `?` in the
/// caller and returned to axum.
#[allow(clippy::result_large_err)]
fn extract_and_validate_token(
    headers: &HeaderMap,
    auth_service: &AuthService,
) -> Result<AccessTokenClaims, Response> {
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
            )
                .into_response()
        })?;

    let token = extract_bearer_token(auth_header).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid_request".to_string(),
                error_description: Some("Invalid Authorization header format".to_string()),
            }),
        )
            .into_response()
    })?;

    auth_service.validate_token(token).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid_token".to_string(),
                error_description: Some("Token is invalid or expired".to_string()),
            }),
        )
            .into_response()
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
pub async fn userinfo(State(state): State<OAuthState>, headers: HeaderMap) -> Response {
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
            scope: Some(claims.scope.as_str().to_string()),
            principal_type: Some(claims.principal_type.as_str().to_string()),
            client_id: claims.clients.first().and_then(|c| {
                // Extract the raw client ID from "id:identifier" format
                if c == "*" {
                    None
                } else {
                    Some(c.split(':').next().unwrap_or(c).to_string())
                }
            }),
            clients: Some(claims.clients),
            roles: Some(claims.roles.clone()),
            applications: Some(claims.applications),
        }),
    )
        .into_response()
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
    )
    .await
    {
        return resp;
    }

    // Try to validate as access token
    match state.auth_service.validate_token(&req.token) {
        Ok(claims) => (
            StatusCode::OK,
            Json(IntrospectResponse {
                active: true,
                sub: Some(claims.sub),
                scope: Some(claims.scope.as_str().to_string()),
                client_id: claims.clients.first().cloned(),
                email: claims.email,
                name: Some(claims.name),
                principal_type: Some(claims.principal_type.as_str().to_string()),
                exp: Some(claims.exp),
                iat: Some(claims.iat),
                iss: Some(claims.iss),
                token_type: Some("Bearer".to_string()),
            }),
        )
            .into_response(),
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
            )
                .into_response()
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
    )
    .await
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── matches_redirect_uri ──────────────────────────────────────────

    #[test]
    fn test_exact_redirect_uri_match() {
        let registered = vec!["https://app.example.com/callback".to_string()];
        assert!(matches_redirect_uri(
            "https://app.example.com/callback",
            &registered
        ));
    }

    #[test]
    fn test_redirect_uri_no_match() {
        let registered = vec!["https://app.example.com/callback".to_string()];
        assert!(!matches_redirect_uri(
            "https://evil.example.com/callback",
            &registered
        ));
    }

    #[test]
    fn test_redirect_uri_multiple_registered() {
        let registered = vec![
            "https://app.example.com/callback".to_string(),
            "https://staging.example.com/callback".to_string(),
        ];
        assert!(matches_redirect_uri(
            "https://staging.example.com/callback",
            &registered
        ));
        assert!(!matches_redirect_uri(
            "https://prod.example.com/callback",
            &registered
        ));
    }

    #[test]
    fn test_redirect_uri_empty_registered() {
        let registered: Vec<String> = vec![];
        assert!(!matches_redirect_uri(
            "https://app.example.com/callback",
            &registered
        ));
    }

    // ── wildcard_matches ──────────────────────────────────────────────

    #[test]
    fn test_wildcard_single_subdomain() {
        // * matches a single subdomain segment (no dots)
        assert!(wildcard_matches(
            "https://tenant1.example.com/callback",
            "https://*.example.com/callback"
        ));
    }

    #[test]
    fn test_wildcard_does_not_match_dots() {
        // * should NOT match segments with dots
        assert!(!wildcard_matches(
            "https://a.b.example.com/callback",
            "https://*.example.com/callback"
        ));
    }

    #[test]
    fn test_wildcard_at_end_of_pattern() {
        assert!(wildcard_matches(
            "https://example.com/tenant1",
            "https://example.com/*"
        ));
    }

    #[test]
    fn test_wildcard_empty_segment_rejected() {
        // Empty wildcard segment should not match
        assert!(!wildcard_matches(
            "https://.example.com/callback",
            "https://*.example.com/callback"
        ));
    }

    #[test]
    fn test_no_wildcard_requires_exact() {
        // matches_redirect_uri only enters wildcard_matches if pattern contains *
        let registered = vec!["https://app.example.com/callback".to_string()];
        assert!(!matches_redirect_uri(
            "https://app.example.com/callback2",
            &registered
        ));
    }

    #[test]
    fn test_wildcard_pattern_prefix_mismatch() {
        assert!(!wildcard_matches(
            "http://tenant.example.com/callback",
            "https://*.example.com/callback"
        ));
    }

    // ── generate_random_string ────────────────────────────────────────

    #[test]
    fn test_random_string_length() {
        let s = generate_random_string(32);
        assert_eq!(s.len(), 32);
    }

    #[test]
    fn test_random_string_zero_length() {
        let s = generate_random_string(0);
        assert!(s.is_empty());
    }

    #[test]
    fn test_random_string_alphanumeric_only() {
        let s = generate_random_string(1000);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_random_string_uniqueness() {
        let a = generate_random_string(64);
        let b = generate_random_string(64);
        assert_ne!(a, b, "Two random strings of length 64 should differ");
    }

    // ── DTO serialization ─────────────────────────────────────────────

    #[test]
    fn test_token_response_serialization_full() {
        let resp = TokenResponse {
            access_token: "tok_abc".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: 3600,
            refresh_token: Some("rt_xyz".to_string()),
            id_token: Some("id_123".to_string()),
            scope: Some("openid profile".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["access_token"], "tok_abc");
        assert_eq!(json["token_type"], "Bearer");
        assert_eq!(json["expires_in"], 3600);
        assert_eq!(json["refresh_token"], "rt_xyz");
        assert_eq!(json["id_token"], "id_123");
        assert_eq!(json["scope"], "openid profile");
    }

    #[test]
    fn test_token_response_skips_none_fields() {
        let resp = TokenResponse {
            access_token: "tok".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: 900,
            refresh_token: None,
            id_token: None,
            scope: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("refresh_token").is_none());
        assert!(json.get("id_token").is_none());
        assert!(json.get("scope").is_none());
    }

    #[test]
    fn test_error_response_serialization() {
        let resp = ErrorResponse {
            error: "invalid_request".to_string(),
            error_description: Some("Missing field".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["error"], "invalid_request");
        assert_eq!(json["error_description"], "Missing field");
    }

    #[test]
    fn test_error_response_skips_none_description() {
        let resp = ErrorResponse {
            error: "server_error".to_string(),
            error_description: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["error"], "server_error");
        assert!(json.get("error_description").is_none());
    }

    #[test]
    fn test_introspect_response_active_true() {
        let resp = IntrospectResponse {
            active: true,
            sub: Some("user123".to_string()),
            scope: Some("openid".to_string()),
            client_id: Some("client1".to_string()),
            email: Some("user@test.com".to_string()),
            name: Some("Test User".to_string()),
            principal_type: Some("USER".to_string()),
            exp: Some(1700000000),
            iat: Some(1699996400),
            iss: Some("https://auth.example.com".to_string()),
            token_type: Some("Bearer".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["active"], true);
        assert_eq!(json["sub"], "user123");
        // "type" rename
        assert_eq!(json["type"], "USER");
        assert!(
            json.get("principal_type").is_none(),
            "should be renamed to 'type'"
        );
    }

    #[test]
    fn test_introspect_response_inactive() {
        let resp = IntrospectResponse {
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
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["active"], false);
        // All optional fields should be absent
        assert!(json.get("sub").is_none());
        assert!(json.get("scope").is_none());
        assert!(json.get("client_id").is_none());
    }

    #[test]
    fn test_userinfo_response_serialization() {
        let resp = UserInfoResponse {
            sub: "principal_abc".to_string(),
            email: Some("user@example.com".to_string()),
            name: Some("Alice".to_string()),
            scope: Some("ANCHOR".to_string()),
            principal_type: Some("USER".to_string()),
            client_id: Some("clt_123".to_string()),
            clients: Some(vec!["clt_123".to_string()]),
            roles: Some(vec!["admin".to_string()]),
            applications: Some(vec!["app1".to_string()]),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["sub"], "principal_abc");
        assert_eq!(json["email"], "user@example.com");
        // principal_type is renamed to "type"
        assert_eq!(json["type"], "USER");
    }

    #[test]
    fn test_userinfo_response_minimal() {
        let resp = UserInfoResponse {
            sub: "svc_001".to_string(),
            email: None,
            name: None,
            scope: None,
            principal_type: None,
            client_id: None,
            clients: None,
            roles: None,
            applications: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["sub"], "svc_001");
        assert!(json.get("email").is_none());
        assert!(json.get("clients").is_none());
    }

    #[test]
    fn test_token_request_deserialization() {
        let json = r#"{
            "grant_type": "authorization_code",
            "code": "abc123",
            "redirect_uri": "https://app.example.com/callback",
            "client_id": "clt_1"
        }"#;
        let req: TokenRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.grant_type, "authorization_code");
        assert_eq!(req.code, Some("abc123".to_string()));
        assert_eq!(
            req.redirect_uri,
            Some("https://app.example.com/callback".to_string())
        );
        assert_eq!(req.client_id, Some("clt_1".to_string()));
        assert!(req.client_secret.is_none());
        assert!(req.code_verifier.is_none());
    }

    #[test]
    fn test_token_request_minimal() {
        let json = r#"{"grant_type": "client_credentials"}"#;
        let req: TokenRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.grant_type, "client_credentials");
        assert!(req.code.is_none());
        assert!(req.refresh_token.is_none());
    }

    #[test]
    fn test_revoke_request_deserialization() {
        let json = r#"{"token": "rt_abc123", "token_type_hint": "refresh_token"}"#;
        let req: RevokeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.token, "rt_abc123");
        assert_eq!(req.token_type_hint, Some("refresh_token".to_string()));
    }

    #[test]
    fn test_introspect_request_deserialization() {
        let json = r#"{"token": "access_token_xyz"}"#;
        let req: IntrospectRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.token, "access_token_xyz");
        assert!(req.token_type_hint.is_none());
    }
}
