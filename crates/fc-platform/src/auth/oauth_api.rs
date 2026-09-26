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
    /// Requested scope: permission codes to narrow the granted set to (OIDC
    /// scopes such as `openid` are ignored for that purpose)
    pub scope: Option<String>,
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
    pub tier: Option<String>,
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
    pub name: String,
    /// Tenancy tier (`ANCHOR` | `PARTNER` | `CLIENT`)
    pub tier: String,
    /// The token's granted permissions (space-delimited; empty when none)
    pub scope: String,
    #[serde(rename = "type")]
    pub principal_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub clients: Vec<String>,
    pub roles: Vec<String>,
    pub applications: Vec<String>,
}

/// OAuth2 state
#[derive(Clone)]
pub struct OAuthState {
    pub oauth_client_repo: Arc<OAuthClientRepository>,
    /// Stamps a service account's `last_used_at` when it authenticates.
    pub service_account_repo: Arc<crate::ServiceAccountRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    /// Role → permission / application resolution for the minted claims
    pub role_repo: Arc<crate::RoleRepository>,
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
    /// The portal identity plane: redeems authorization codes whose subject
    /// is a `ptu_…` portal identity (Go `State.PortalIdentities` /
    /// `PortalApps`). `None` refuses portal codes (fail closed).
    pub portal: Option<crate::portal::PortalState>,
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
    jar: axum_extra::extract::cookie::CookieJar,
    Query(req): Query<AuthorizeRequest>,
) -> Response {
    // Require `state` for CSRF protection on the callback. Missing/empty
    // `state` is rejected with 400 (not a redirect) — we can't safely bounce
    // the user-agent back to the caller without proving the caller is who
    // they claim to be, and `state` is the mechanism by which they do that.
    if req.state.as_deref().is_none_or(|s| s.trim().is_empty()) {
        warn!(client_id = %req.client_id, "authorize rejected: missing `state` parameter");
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "`state` parameter is required for CSRF protection",
        );
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

    // Resolve and validate the client and redirect_uri BEFORE any error
    // redirect (Go `Authorize`, oauthapi/authorize.go:64-86). RFC 6749
    // §4.1.2.1: with an unknown or inactive client, or an unregistered
    // redirect_uri, the user-agent must not be sent to that URI; every such
    // failure is a direct 400.
    let client = match state
        .oauth_client_repo
        .find_by_client_id(&req.client_id)
        .await
    {
        Ok(Some(c)) if c.active => c,
        Ok(Some(_)) => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "unauthorized_client",
                "Client is not active",
            );
        }
        Ok(None) => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "unauthorized_client",
                "Unknown client",
            );
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup client");
            return oauth_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Internal error",
            );
        }
    };

    // Validate redirect_uri (exact match first, then wildcard pattern matching)
    if !matches_redirect_uri(&req.redirect_uri, &client.redirect_uris) {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid redirect_uri",
        );
    }

    // The redirect_uri is now the client's own: from here on an error may
    // go back to it with OAuth error parameters.
    if req.response_type != "code" {
        return error_redirect(
            &req.redirect_uri,
            "unsupported_response_type",
            "Only 'code' response type is supported",
            req.state.as_deref(),
        );
    }

    // Plane separation (Go oauthapi/authorize.go:86-95): a portal-flagged
    // client belongs to the portal identity plane and must enter through
    // /portal/authorize, or it would sign in platform users instead of
    // portal identities.
    if client
        .portal_client_id
        .as_deref()
        .is_some_and(|p| !p.is_empty())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "unauthorized_client".to_string(),
                error_description: Some("Portal clients must use /portal/authorize".to_string()),
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

    // Validate code_challenge_method. Only S256 is supported; `plain` is
    // refused like any unknown method (as in the Go platform). Absent means
    // S256 (see `Pkce::from_parts`).
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
                return error_redirect(
                    &req.redirect_uri,
                    "invalid_request",
                    "Only the S256 code_challenge_method is supported",
                    req.state.as_deref(),
                );
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

    // The signed-in user: the session cookie only (owner ruling 2026-09-25,
    // item 6; Java 66281bc7), never a Bearer, and only for a principal that
    // exists, is active and is a USER (Java S2.2). An API or identity
    // access token may be narrowed, delegated to an OAuth client or a
    // service account's; a code minted from one would hand a relying party
    // a user session nobody signed in to. Anything else is "no session".
    let session = match signed_in_user(&state, &jar).await {
        Ok(session) => session,
        Err(e) => {
            error!(error = %e, client_id = %req.client_id, "session principal lookup failed");
            return error_redirect(
                &req.redirect_uri,
                "server_error",
                "Internal error",
                req.state.as_deref(),
            );
        }
    };

    // Handle `prompt` parameter (OIDC Core Section 3.1.2.1)
    let force_login = if let Some(ref prompt) = req.prompt {
        match prompt.as_str() {
            "none" => {
                // prompt=none: if user is not authenticated, return login_required error
                if session.is_none() {
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
        if let Some(ref session) = session {
            // Check max_age: if session is older than max_age seconds,
            // force re-authentication. An unknown issue time never
            // forces it; max_age=0 always does.
            let session_too_old = req.max_age.is_some_and(|max_age| {
                let now = Utc::now().timestamp();
                max_age <= 0 || session.issued_at.is_some_and(|iat| now - iat > max_age)
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
                        session.principal_id.clone(),
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

                info!(client_id = %req.client_id, principal_id = %session.principal_id, "Issued authorization code (authenticated session)");
                return Redirect::temporary(&redirect_url).into_response();
            } // !session_too_old
        } // session Some
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

/// The user signed in to this browser: the platform session cookie, when it
/// verifies as a session token and its principal exists, is active and is
/// a USER. `Ok(None)` is "no session" (send the user to log in).
async fn signed_in_user(
    state: &OAuthState,
    jar: &axum_extra::extract::cookie::CookieJar,
) -> Result<Option<crate::auth::auth_service::SessionIdentity>, PlatformError> {
    let Some(cookie) = jar.get(crate::shared::middleware::SESSION_COOKIE_NAME) else {
        return Ok(None);
    };
    let Ok(session) = state.auth_service.validate_session_token(cookie.value()) else {
        return Ok(None);
    };
    Ok(state
        .principal_repo
        .find_by_id(&session.principal_id)
        .await?
        .filter(|p| p.active && p.principal_type == crate::PrincipalType::User)
        .map(|_| session))
}

/// Check `provided` against one stored secret ref and, when it matches a
/// shape other than the current `hashed:v1:` form (an older `encrypted:`
/// ref, a bare envelope, or a hash under a previous app key), report that
/// the ref needs rewriting. Returns `(matched, needs_rehash)`.
fn check_secret_ref(state: &OAuthState, stored: Option<&str>, provided: &str) -> (bool, bool) {
    match (state.encryption_service.as_deref(), stored) {
        (Some(enc), Some(stored)) => enc.verify_secret(stored, provided),
        _ => (false, false),
    }
}

/// How often the previous secret's last-used stamp is written per client. A
/// fleet mid-rollout may authenticate thousands of times an hour on the old
/// secret; one write a minute is plenty (Go's `previousSecretTouchInterval`).
const PREVIOUS_SECRET_TOUCH_INTERVAL_SECS: i64 = 60;

/// Verify `provided` against the client's current secret and, failing that,
/// against a previous secret whose rotation overlap is still open, so a fleet
/// holding the old secret can be rolled gradually. Mirrors Go's
/// `acceptClientSecret` (auth/oauthapi/token.go:347-383), which shares the
/// `oauth_clients` table.
///
/// Both compares always run, so accepting an old secret is not measurably
/// slower or faster than accepting a new one. A match against a ref that
/// isn't yet the current `hashed:v1:` form is rewritten to
/// [`EncryptionService::hash_secret`] of the secret the caller just proved it
/// holds; use of the previous secret is stamped (coalesced to once a
/// minute). Both writes are best-effort: authentication has already
/// succeeded, so a failure is logged and dropped. They go straight to the
/// repository rather than through a use case: they record storage format and
/// usage, not a business fact, and run on the token endpoint's hot path
/// (same reasoning as the other infrastructure exceptions in `CLAUDE.md`).
///
/// [`EncryptionService::hash_secret`]: crate::shared::encryption_service::EncryptionService::hash_secret
async fn accept_client_secret(state: &OAuthState, client: &OAuthClient, provided: &str) -> bool {
    let Some(enc) = state.encryption_service.as_deref() else {
        error!(client_id = %client.client_id, "Cannot verify client secret — FLOWCATALYST_APP_KEY not configured");
        return false;
    };
    let current = client.client_secret_ref.as_deref();
    let previous = client.usable_previous_secret_ref();
    let (current_ok, current_rehash) = check_secret_ref(state, current, provided);
    let (previous_ok, previous_rehash) = check_secret_ref(state, previous, provided);

    if previous_ok {
        let now = chrono::Utc::now();
        let stale_before = now - chrono::Duration::seconds(PREVIOUS_SECRET_TOUCH_INTERVAL_SECS);
        match state
            .oauth_client_repo
            .touch_previous_secret_used(&client.id, now, stale_before)
            .await
        {
            Ok(_) => {
                info!(oauth_client_id = %client.id, "Client authenticated with its superseded secret")
            }
            Err(e) => {
                warn!(oauth_client_id = %client.id, error = %e, "Could not record previous-secret use")
            }
        }
    }
    if let (true, true, Some(stored)) = (current_ok, current_rehash, current) {
        if let Err(e) = state
            .oauth_client_repo
            .rewrite_secret_ref(client, stored, &enc.hash_secret(provided))
            .await
        {
            warn!(client_id = %client.client_id, error = %e, "Could not migrate client secret to hashed form");
        }
    }
    if let (true, true, Some(stored)) = (previous_ok, previous_rehash, previous) {
        if let Err(e) = state
            .oauth_client_repo
            .rewrite_previous_secret_ref(client, stored, &enc.hash_secret(provided))
            .await
        {
            warn!(client_id = %client.client_id, error = %e, "Could not migrate client's previous secret to hashed form");
        }
    }
    current_ok || previous_ok
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
    if client.client_secret_ref.is_some() {
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

        let verified = accept_client_secret(state, &client, &provided_secret).await;

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

// ─── Claim authority (Go oauthapi/token.go + token_apiaccess.go) ─────────

/// The OIDC scopes that are not permission codes (Go `oidcReservedScopes`,
/// oauthapi/token.go:983-985).
const OIDC_RESERVED_SCOPES: &[&str] = &[
    "openid",
    "profile",
    "email",
    "address",
    "phone",
    "offline_access",
];

/// The permissions to put on a token's `scope` claim (Go `grantedScope`,
/// oauthapi/token.go:1009-1034): the principal's ceiling (its roles'
/// permissions), narrowed to the requested permission codes when any were
/// requested. `explicit` reports whether any were.
async fn granted_scope(
    state: &OAuthState,
    principal: &crate::Principal,
    requested: Option<&str>,
) -> crate::shared::error::Result<(Vec<String>, bool)> {
    let ceiling = state
        .role_repo
        .flatten_permissions(&crate::auth::auth_service::role_names(principal))
        .await?;
    let requested: Vec<&str> = requested
        .unwrap_or("")
        .split_whitespace()
        .filter(|s| !OIDC_RESERVED_SCOPES.contains(s))
        .collect();
    if requested.is_empty() {
        return Ok((ceiling, false));
    }
    let granted = requested
        .into_iter()
        .filter(|r| {
            ceiling
                .iter()
                .any(|held| crate::role::entity::matches_pattern(r, held))
        })
        .map(str::to_string)
        .collect();
    Ok((granted, true))
}

/// The principal as an app-scoped client may see it (Go `confineToClient`,
/// oauthapi/token.go:1078-1095): application access intersected with the
/// client's (all-applications off), and the roles narrowed to the client's
/// applications. Returns the principal unchanged, with its full role list,
/// for a client with no applications.
async fn confine_to_client(
    state: &OAuthState,
    principal: &crate::Principal,
    client: &OAuthClient,
) -> crate::shared::error::Result<(crate::Principal, Vec<String>)> {
    let roles = crate::auth::auth_service::role_names(principal);
    if client.application_ids.is_empty() {
        return Ok((principal.clone(), roles));
    }
    let kept = state
        .role_repo
        .filter_roles_for_applications(&roles, &client.application_ids)
        .await?;
    let mut scoped = principal.clone();
    // Go `intersectApps` (oauthapi/token_apiaccess.go:65-83).
    scoped.accessible_application_ids = if principal.all_applications {
        client.application_ids.clone()
    } else {
        client
            .application_ids
            .iter()
            .filter(|id| principal.accessible_application_ids.contains(id))
            .cloned()
            .collect()
    };
    scoped.all_applications = false;
    // Go keeps the role assignments whose name is in the narrowed list
    // (token_apiaccess.go:40-49), matched exactly as Go does.
    scoped.roles.retain(|ra| kept.contains(&ra.role));
    Ok((scoped, kept))
}

/// The access token an interactive login (authorization_code and its
/// refresh) returns, as Go `mintInteractiveAccessToken`
/// (oauthapi/token_apiaccess.go:27-60): an identity-only token
/// (`token_use: identity`, no authority, refused as an API bearer) unless
/// the client is flagged `api_access`; for such a client an
/// authority-bearing token narrowed to the client's applications:
/// `token_use: api`, `scope` = the granted permissions of the narrowed
/// roles, `azp` = the client.
async fn mint_interactive_access_token(
    state: &OAuthState,
    principal: &crate::Principal,
    client: Option<&OAuthClient>,
    requested_scope: Option<&str>,
) -> crate::shared::error::Result<String> {
    let Some(client) = client else {
        return state
            .auth_service
            .generate_identity_access_token(principal, None);
    };
    if !client.api_access {
        return state
            .auth_service
            .generate_identity_access_token(principal, Some(&client.client_id));
    }
    let (narrowed, _) = confine_to_client(state, principal, client).await?;
    let (granted, _) = granted_scope(state, &narrowed, requested_scope).await?;
    state.auth_service.generate_access_token_with_scope(
        &narrowed,
        &granted,
        Some(&client.client_id),
    )
}

/// The ID token for a relying party, confined to what it may know (Go
/// `mintIDToken`, oauthapi/token.go:1056-1066): an app-scoped client sees
/// only its applications' roles and its share of the application access.
async fn mint_id_token(
    state: &OAuthState,
    principal: &crate::Principal,
    client_id_for_aud: &str,
    client: Option<&OAuthClient>,
    nonce: Option<String>,
) -> crate::shared::error::Result<String> {
    match client.filter(|c| !c.application_ids.is_empty()) {
        None => state
            .auth_service
            .generate_id_token(principal, client_id_for_aud, nonce),
        Some(client) => {
            let (scoped, roles) = confine_to_client(state, principal, client).await?;
            state.auth_service.generate_id_token_with_roles(
                &scoped,
                client_id_for_aud,
                nonce,
                roles,
            )
        }
    }
}

fn server_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "server_error".to_string(),
            error_description: None,
        }),
    )
        .into_response()
}

async fn handle_authorization_code_grant(
    state: OAuthState,
    req: TokenRequest,
    authenticated_client: Option<OAuthClient>,
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
        // Only S256 is supported. A `plain` binding can only come from a
        // code minted before `/oauth/authorize` refused it.
        if pkce.method != PkceMethod::S256 {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_request".to_string(),
                    error_description: Some(
                        "Only the S256 code_challenge_method is supported".to_string(),
                    ),
                }),
            )
                .into_response();
        }
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

    // Portal-plane subject (Go token.go:725-733): a code minted by a
    // /portal/authorize flow for a ptu_ portal identity, not a principal.
    if crate::portal::is_portal_subject(&auth_code.principal_id) {
        let Some(portal) = state.portal.as_ref() else {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Portal subjects are not supported".to_string()),
                }),
            )
                .into_response();
        };
        let client_id = authenticated_client
            .as_ref()
            .map_or(auth_code.client_id.as_str(), |c| c.client_id.as_str());
        return crate::portal::token::redeem_portal_code(
            portal,
            &state.auth_service,
            &auth_code,
            client_id,
        )
        .await;
    }

    // Get the principal. One deactivated since the code was issued gets no
    // tokens (Java S2.2).
    let principal = match state
        .principal_repo
        .find_by_id(&auth_code.principal_id)
        .await
    {
        Ok(Some(p)) if p.active => p,
        Ok(Some(_)) => {
            warn!(principal_id = %auth_code.principal_id, client_id = %auth_code.client_id, "authorization code refused: principal is not active");
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_grant".to_string(),
                    error_description: Some("Account is not active".to_string()),
                }),
            )
                .into_response();
        }
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

    // The interactive-login access token (Go handleAuthorizationCodeGrant,
    // oauthapi/token.go:745-761).
    let access_token = match mint_interactive_access_token(
        &state,
        &principal,
        authenticated_client.as_ref(),
        auth_code.scope.as_deref(),
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to generate access token");
            return server_error();
        }
    };

    // Generate ID token when scope includes "openid"
    let has_openid = auth_code
        .scope
        .as_deref()
        .map(|s| s.split_whitespace().any(|sc| sc == "openid"))
        .unwrap_or(false);

    let id_token = if has_openid {
        match mint_id_token(
            &state,
            &principal,
            &auth_code.client_id,
            authenticated_client.as_ref(),
            auth_code.nonce.clone(),
        )
        .await
        {
            Ok(t) => Some(t),
            Err(e) => {
                error!(error = %e, "Failed to generate ID token");
                return server_error();
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
            expires_in: state.auth_service.access_token_expiry_secs(),
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

    // Rotate: atomic single use, family reuse detection with a 10 s
    // sibling leeway, the client binding, and the family's inherited
    // expiry (see `refresh_rotation`).
    let requesting_client_id = authenticated_client.as_ref().map(|c| c.client_id.as_str());
    let rotated = match crate::auth::refresh_rotation::rotate(
        &*state.refresh_token_repo,
        &refresh_token_str,
        requesting_client_id,
    )
    .await
    {
        Ok(Ok(rotated)) => rotated,
        Ok(Err(crate::auth::refresh_rotation::Rejection::Refused { token_client_id })) => {
            warn!(
                stored_client_id = %token_client_id,
                requesting_client_id = ?requesting_client_id,
                "Refresh token client binding mismatch"
            );
            // RFC 6749 §5.2: every token-endpoint error but invalid_client
            // is a 400 (Go handleRefreshTokenGrant, oauthapi/token.go:835-846).
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "Token was not issued to this client",
            );
        }
        Ok(Err(_)) => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "Invalid or expired refresh token",
            );
        }
        Err(e) => {
            error!(error = %e, "Failed to rotate refresh token");
            return oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "");
        }
    };
    let stored_token = rotated.stored;

    // Find the principal
    let principal = match state
        .principal_repo
        .find_by_id(&stored_token.principal_id)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "Principal not found",
            );
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
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "Account is not active",
        );
    }

    // The refreshed access token follows the original login's rule,
    // re-derived from the current principal (Go handleRefreshTokenGrant,
    // oauthapi/token.go:866-881).
    let refresh_client = match authenticated_client.clone() {
        Some(c) => Some(c),
        None => match stored_token.oauth_client_id.as_deref() {
            Some(cid) => state
                .oauth_client_repo
                .find_by_client_id(cid)
                .await
                .ok()
                .flatten(),
            None => None,
        },
    };
    let requested_scope = stored_token.scopes.join(" ");
    let access_token = match mint_interactive_access_token(
        &state,
        &principal,
        refresh_client.as_ref(),
        Some(&requested_scope),
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to generate access token");
            return server_error();
        }
    };

    // Generate ID token when the original scope included "openid"
    // P2-8: Only generate ID token if we have a real oauth_client_id for the audience.
    // Never fall back to principal_id as audience — that's semantically wrong.
    let has_openid = stored_token.scopes.iter().any(|s| s == "openid");
    let id_token = if has_openid {
        if let Some(ref client_id) = stored_token.oauth_client_id {
            match mint_id_token(
                &state,
                &principal,
                client_id,
                authenticated_client.as_ref(),
                None,
            )
            .await
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

    let raw_token = rotated.new_raw;

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
            expires_in: state.auth_service.access_token_expiry_secs(),
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
        // No OAuth client: a USER principal's own id is the self-service
        // developer credential (Go token.go:505-520). The prn_/oac_ prefixes
        // keep the two client_id spaces apart.
        Ok(None) if client_id.starts_with("prn_") => {
            return handle_developer_credential_grant(
                state,
                client_id,
                client_secret,
                req.scope.as_deref(),
            )
            .await;
        }
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
    if client.client_secret_ref.is_none() {
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

    let verified = accept_client_secret(&state, &client, &client_secret).await;

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
    // A confidential client with no linked principal, or a dangling one, is
    // a client-side misconfiguration (RFC 6749 §5.2), not a server fault:
    // 400 `unauthorized_client`, as Go (oauthapi/token.go:555-577).
    let misconfigured = |reason: &'static str| {
        let attempt = LoginAttempt {
            identifier: Some(client_id.clone()),
            failure_reason: Some(reason.to_string()),
            ..LoginAttempt::new(AttemptType::ServiceAccountToken, LoginOutcome::Failure)
        };
        let repo = state.login_attempt_repo.clone();
        async move {
            if let Err(e) = repo.create(&attempt).await {
                warn!(error = %e, "Failed to log service account login attempt");
            }
            oauth_error(
                StatusCode::BAD_REQUEST,
                "unauthorized_client",
                "Client is not configured for this grant",
            )
        }
    };
    let principal_id = match &client.service_account_principal_id {
        Some(id) => id,
        None => {
            warn!(client_id = %client_id, "Client has no service account principal configured");
            return misconfigured("Client not properly configured (no linked principal)").await;
        }
    };

    let principal = match state.principal_repo.find_by_id(principal_id).await {
        // A client authenticates as a service account, never a user: a USER
        // principal here would mint that user's full authority for whoever
        // holds the client secret (Java 6a06a7f0 S2.3).
        Ok(Some(p)) if p.principal_type != crate::PrincipalType::Service => {
            warn!(client_id = %client_id, principal_id = %principal_id, "client_credentials refused: linked principal is not a service account");
            let attempt = LoginAttempt {
                identifier: Some(client_id.clone()),
                principal_id: Some(p.id.clone()),
                failure_reason: Some(
                    "Client not properly configured (linked principal is not a service account)"
                        .to_string(),
                ),
                ..LoginAttempt::new(AttemptType::ServiceAccountToken, LoginOutcome::Failure)
            };
            if let Err(e) = state.login_attempt_repo.create(&attempt).await {
                warn!(error = %e, "Failed to log service account login attempt");
            }
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "unauthorized_client".to_string(),
                    error_description: Some("Client is not configured for this grant".to_string()),
                }),
            )
                .into_response();
        }
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
            warn!(client_id = %client_id, principal_id = %principal_id, "Service account principal not found");
            return misconfigured("Client not properly configured (linked principal not found)")
                .await;
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

    // Go mintClientCredentialsToken (oauthapi/token.go:645-679): the scope
    // claim carries the granted permissions; a request for permissions the
    // service account does not hold is `invalid_scope`.
    let (granted, explicit) = match granted_scope(&state, &principal, req.scope.as_deref()).await {
        Ok(g) => g,
        Err(e) => {
            error!(error = %e, "Failed to resolve granted scope");
            return server_error();
        }
    };
    if explicit && granted.is_empty() {
        let attempt = LoginAttempt {
            identifier: Some(client_id.clone()),
            principal_id: Some(principal.id.clone()),
            failure_reason: Some("requested scope exceeds granted permissions".to_string()),
            ..LoginAttempt::new(AttemptType::ServiceAccountToken, LoginOutcome::Failure)
        };
        if let Err(e) = state.login_attempt_repo.create(&attempt).await {
            warn!(error = %e, "Failed to log service account login attempt");
        }
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_scope".to_string(),
                error_description: Some(
                    "Requested scope exceeds the service account's granted permissions".to_string(),
                ),
            }),
        )
            .into_response();
    }

    let access_token = match state
        .auth_service
        .generate_access_token_with_scope(&principal, &granted, None)
    {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to generate access token");
            return server_error();
        }
    };

    // Go `TouchServiceAccountUsed`: authenticating is the account's
    // day-to-day use. Best-effort; a bookkeeping failure never fails a token.
    if let Some(sa_id) = principal.service_account_id.as_deref() {
        if let Err(e) = state.service_account_repo.touch_last_used(sa_id).await {
            warn!(error = %e, "Failed to stamp service account last_used_at");
        }
    }

    // Log successful service account login attempt
    let attempt = LoginAttempt {
        identifier: Some(client_id.clone()),
        principal_id: Some(principal.id.clone()),
        ip_address: caller_ip.clone(),
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
            expires_in: state.auth_service.access_token_expiry_secs(),
            refresh_token: None,
            id_token: None,
            scope: Some(granted.join(" ")).filter(|s| !s.is_empty()),
        }),
    )
        .into_response()
}

/// The developer-credential branch of client_credentials (Go
/// `handleDeveloperCredentialGrant` + `mintClientCredentialsToken`,
/// token.go:595-679): `client_id` is an active USER principal's id,
/// `client_secret` its developer secret. The developer role is re-checked
/// live, so revoking the role stops new tokens at once. Every refusal is
/// the same `invalid_client`.
async fn handle_developer_credential_grant(
    state: OAuthState,
    client_id: String,
    client_secret: String,
    scope: Option<&str>,
) -> Response {
    let invalid = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid_client".to_string(),
                error_description: Some("Invalid client credentials".to_string()),
            }),
        )
            .into_response()
    };
    let principal = match state.principal_repo.find_by_id(&client_id).await {
        Ok(Some(p)) if p.active && p.principal_type == crate::PrincipalType::User => p,
        Ok(_) => return invalid(),
        Err(e) => {
            error!(error = %e, "Failed to look up developer principal");
            return server_error();
        }
    };
    if !principal
        .roles
        .iter()
        .any(|r| r.role == crate::developer_credential::DEVELOPER_ROLE)
    {
        return invalid();
    }
    let stored = match state.principal_repo.find_developer_secret(&principal.id).await {
        Ok(Some((Some(stored), _))) => stored,
        Ok(_) => return invalid(),
        Err(e) => {
            error!(error = %e, "Failed to read developer secret");
            return server_error();
        }
    };
    let record = |outcome, reason: Option<&str>| LoginAttempt {
        identifier: Some(client_id.clone()),
        principal_id: Some(principal.id.clone()),
        failure_reason: reason.map(String::from),
        ..LoginAttempt::new(AttemptType::DeveloperToken, outcome)
    };
    let (ok, rehash) = check_secret_ref(&state, Some(&stored), &client_secret);
    if !ok {
        let attempt = record(LoginOutcome::Failure, Some("Invalid developer client secret"));
        if let Err(e) = state.login_attempt_repo.create(&attempt).await {
            warn!(error = %e, "Failed to log developer token attempt");
        }
        return invalid();
    }
    if rehash {
        if let Some(enc) = state.encryption_service.as_deref() {
            if let Err(e) = state
                .principal_repo
                .rewrite_developer_secret_ref(&principal.id, &enc.hash_secret(&client_secret))
                .await
            {
                warn!(principal_id = %principal.id, error = %e, "Could not migrate developer client secret to hashed form");
            }
        }
    }

    let (granted, explicit) = match granted_scope(&state, &principal, scope).await {
        Ok(g) => g,
        Err(e) => {
            error!(error = %e, "Failed to resolve granted scope");
            return server_error();
        }
    };
    if explicit && granted.is_empty() {
        let attempt = record(
            LoginOutcome::Failure,
            Some("requested scope exceeds granted permissions"),
        );
        if let Err(e) = state.login_attempt_repo.create(&attempt).await {
            warn!(error = %e, "Failed to log developer token attempt");
        }
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_scope".to_string(),
                error_description: Some(
                    "Requested scope exceeds your granted permissions".to_string(),
                ),
            }),
        )
            .into_response();
    }
    let access_token = match state
        .auth_service
        .generate_access_token_with_scope(&principal, &granted, None)
    {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to generate access token");
            return server_error();
        }
    };
    let attempt = record(LoginOutcome::Success, None);
    if let Err(e) = state.login_attempt_repo.create(&attempt).await {
        warn!(error = %e, "Failed to log developer token attempt");
    }
    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(TokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in: state.auth_service.access_token_expiry_secs(),
            refresh_token: None,
            id_token: None,
            scope: Some(granted.join(" ")).filter(|s| !s.is_empty()),
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

/// Every OAuth error response is uncacheable: Go's `writeOAuthError`
/// (oauthapi/token.go:1152-1162) sets `Cache-Control: no-store` and
/// `Pragma: no-cache` on each one, whichever endpoint answers. Mounted on
/// the `/oauth` router so no handler can forget it.
pub async fn oauth_errors_no_store(mut response: Response) -> Response {
    if response.status().is_client_error() || response.status().is_server_error() {
        let headers = response.headers_mut();
        if !headers.contains_key(header::CACHE_CONTROL) {
            headers.insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-store"),
            );
        }
        if !headers.contains_key(header::PRAGMA) {
            headers.insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
        }
    }
    response
}

/// An OAuth error answered directly (never redirected), as Go's
/// `writeOAuthError` (oauthapi/token.go:1152-1162): `{error,
/// error_description}` with `Cache-Control: no-store` and `Pragma:
/// no-cache`.
fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    (
        status,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(ErrorResponse {
            error: error.to_string(),
            error_description: (!description.is_empty()).then(|| description.to_string()),
        }),
    )
        .into_response()
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

    // Go Userinfo (oauthapi/userinfo.go:58-95): the authority is recomputed
    // from the current principal and confined to the token's `azp` client,
    // falling back to the token's own claims when the principal can't be
    // loaded. The scope is the credential's, never recomputed.
    let mut roles = claims.roles.clone();
    let mut applications = claims.applications.clone();
    let mut clients = claims.clients.clone();
    if let Ok(Some(principal)) = state.principal_repo.find_by_id(&claims.sub).await {
        if principal.active {
            roles = crate::auth::auth_service::role_names(&principal);
            applications = crate::auth::auth_service::applications_claim(&principal);
            clients = crate::auth::auth_service::clients_claim(&principal);
            let client = match claims.azp.as_deref().filter(|a| !a.is_empty()) {
                Some(azp) => state
                    .oauth_client_repo
                    .find_by_client_id(azp)
                    .await
                    .ok()
                    .flatten(),
                None => None,
            };
            if let Some(client) = client.filter(|c| !c.application_ids.is_empty()) {
                if let Ok((scoped, narrowed)) = confine_to_client(&state, &principal, &client).await
                {
                    roles = narrowed;
                    applications = crate::auth::auth_service::applications_claim(&scoped);
                }
            }
        }
    }

    // Go userinfoClientID (userinfo.go:143-155).
    let client_id = clients
        .first()
        .filter(|c| c.as_str() != "*")
        .map(|c| c.split(':').next().unwrap_or(c).to_string());

    (
        StatusCode::OK,
        Json(UserInfoResponse {
            sub: claims.sub,
            email: claims.email,
            name: claims.name,
            tier: claims.tier.as_str().to_string(),
            scope: claims.scope.unwrap_or_default(),
            principal_type: claims.principal_type.as_str().to_string(),
            client_id,
            clients,
            roles,
            applications,
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
                // RFC 7662 `scope` = the granted permissions; the tier rides
                // `tier` (Go Introspect, oauthapi/introspect_revoke.go:80-93).
                scope: claims.scope.filter(|s| !s.is_empty()),
                tier: Some(claims.tier.as_str().to_string()),
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
                    tier: None,
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
            scope: Some("platform:iam:user:view".to_string()),
            tier: Some("CLIENT".to_string()),
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
        assert_eq!(json["tier"], "CLIENT");
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
            tier: None,
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
            name: "Alice".to_string(),
            tier: "ANCHOR".to_string(),
            scope: "platform:iam:user:view".to_string(),
            principal_type: "USER".to_string(),
            client_id: Some("clt_123".to_string()),
            clients: vec!["clt_123".to_string()],
            roles: vec!["admin".to_string()],
            applications: vec!["app1".to_string()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["sub"], "principal_abc");
        assert_eq!(json["email"], "user@example.com");
        assert_eq!(json["tier"], "ANCHOR");
        // principal_type is renamed to "type"
        assert_eq!(json["type"], "USER");
    }

    #[test]
    fn test_userinfo_response_minimal() {
        // Go's userInfoResponse (oauthapi/userinfo.go:25-36): only email and
        // client_id are omitted when empty; the arrays are always present.
        let resp = UserInfoResponse {
            sub: "svc_001".to_string(),
            email: None,
            name: String::new(),
            tier: "CLIENT".to_string(),
            scope: String::new(),
            principal_type: "SERVICE".to_string(),
            client_id: None,
            clients: vec![],
            roles: vec![],
            applications: vec![],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["sub"], "svc_001");
        assert!(json.get("email").is_none());
        assert!(json.get("client_id").is_none());
        assert_eq!(json["clients"], serde_json::json!([]));
        assert_eq!(json["scope"], "");
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
