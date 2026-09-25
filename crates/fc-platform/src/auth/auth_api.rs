//! Auth API Endpoints
//!
//! Embedded authentication endpoints for direct login/logout.
//! - POST /auth/login - Password-based login
//! - POST /auth/logout - Logout / token revocation
//! - GET /auth/check-domain - Check if email domain requires external IDP
//! - GET /auth/me - Get current user info

use crate::auth::session_cookie::SessionCookieConfig;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::auth::login_backoff::{self, record_user_login_attempt, BackoffDecision, BackoffPolicy};
use crate::identity_provider::entity::IdentityProviderType;
use crate::shared::error::PlatformError;
use crate::shared::middleware::{Authenticated, ClientIp};
use crate::AuthService;
use crate::LoginOutcome;
use crate::PasswordService;
use crate::RefreshToken;
use crate::{EmailDomainMappingRepository, IdentityProviderRepository, LoginAttemptRepository};
use crate::{PrincipalRepository, RefreshTokenRepository};

/// Login request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    /// Email address
    pub email: String,

    /// Password
    pub password: String,

    /// Remember me (extends session duration)
    #[serde(default)]
    pub remember_me: bool,
}

/// Login response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    /// Principal ID
    pub principal_id: String,
    /// Display name
    pub name: String,
    /// Email address
    pub email: String,
    /// Assigned roles
    pub roles: Vec<String>,
    /// Client ID (for CLIENT scope users)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// Domain check request
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DomainCheckRequest {
    /// Email address to check
    pub email: String,
}

/// Domain check response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DomainCheckResponse {
    /// The email domain
    pub domain: String,

    /// Authentication method for this domain
    pub auth_method: AuthMethod,

    /// Provider ID if external IDP is required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,

    /// Authorization URL if external IDP
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,
}

/// Authentication method
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthMethod {
    /// Internal username/password authentication
    Internal,
    /// External OIDC identity provider
    Oidc,
    /// External SAML identity provider
    Saml,
}

/// Current user info response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUserResponse {
    /// Principal ID
    pub id: String,

    /// Principal type (USER, SERVICE)
    pub principal_type: String,

    /// Email address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Display name
    pub name: String,

    /// User scope (ANCHOR, PARTNER, CLIENT)
    pub scope: String,

    /// Client ID (for CLIENT scope users)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Accessible client IDs
    pub clients: Vec<String>,

    /// Assigned roles
    pub roles: Vec<String>,
}

/// Auth service state
#[derive(Clone)]
pub struct AuthState {
    pub auth_service: Arc<AuthService>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub password_service: Arc<PasswordService>,
    pub refresh_token_repo: Arc<RefreshTokenRepository>,
    pub email_domain_mapping_repo: Arc<EmailDomainMappingRepository>,
    pub identity_provider_repo: Arc<IdentityProviderRepository>,
    pub login_attempt_repo: Arc<LoginAttemptRepository>,
    /// Layered failed-login backoff policy. Loaded from env in
    /// `build_platform_routes` so all binaries share the same defaults.
    pub backoff_policy: Arc<BackoffPolicy>,
    pub session_cookie: SessionCookieConfig,
}

/// Login with email and password
///
/// Authenticates a user with email and password credentials.
/// Returns an access token on success and sets a session cookie.
#[utoipa::path(
    post,
    path = "/login",
    tag = "auth",
    operation_id = "postAuthLogin",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = LoginResponse),
        (status = 401, description = "Invalid credentials")
    )
)]
pub async fn login(
    State(state): State<AuthState>,
    ClientIp(client_ip): ClientIp,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, PlatformError> {
    let ip = client_ip.as_deref();

    // Run the layered backoff check BEFORE lookup so the response timing /
    // shape doesn't leak whether the email exists.
    match login_backoff::check(
        &state.login_attempt_repo,
        &state.backoff_policy,
        &req.email,
        ip,
    )
    .await?
    {
        BackoffDecision::Allow => {}
        BackoffDecision::Reject {
            retry_after_secs, ..
        } => {
            return Err(login_backoff::rejection_error(retry_after_secs));
        }
    }

    // Find principal by email
    let principal = match state.principal_repo.find_by_email(&req.email).await? {
        Some(p) => p,
        None => {
            // Record failed attempt (fire-and-forget)
            record_user_login_attempt(
                &state.login_attempt_repo,
                Some(&req.email),
                None,
                ip,
                LoginOutcome::Failure,
                Some("INVALID_CREDENTIALS"),
            )
            .await;
            return Err(PlatformError::Unauthorized {
                message: "Invalid credentials".to_string(),
            });
        }
    };

    // Verify the password: Argon2id, or a bcrypt hash migrated from a
    // Laravel app (Go passwordhash.Verify).
    let stored_hash = principal
        .user_identity
        .as_ref()
        .and_then(|id| id.password_hash.as_deref());
    let password_valid = stored_hash
        .map(|hash| {
            state
                .password_service
                .verify_password(&req.password, hash)
                .unwrap_or(false)
        })
        .unwrap_or(false);

    if !password_valid {
        record_user_login_attempt(
            &state.login_attempt_repo,
            Some(&req.email),
            Some(&principal.id),
            ip,
            LoginOutcome::Failure,
            Some("INVALID_CREDENTIALS"),
        )
        .await;
        return Err(PlatformError::Unauthorized {
            message: "Invalid credentials".to_string(),
        });
    }

    // Check if user is active
    if !principal.active {
        record_user_login_attempt(
            &state.login_attempt_repo,
            Some(&req.email),
            Some(&principal.id),
            ip,
            LoginOutcome::Failure,
            Some("ACCOUNT_INACTIVE"),
        )
        .await;
        return Err(PlatformError::Unauthorized {
            message: "Account is not active".to_string(),
        });
    }

    // Lazy upgrade: a hash that isn't Argon2id at the current parameters (a
    // migrated bcrypt hash, say) is re-encoded now the user has proved the
    // password. Best-effort, as Go's login (auth/login/endpoint.go:519-525):
    // a failure is logged and the login goes on.
    if stored_hash.is_some_and(|h| state.password_service.needs_rehash(h)) {
        match state.password_service.rehash_password(&req.password) {
            Ok(new_hash) => {
                if let Err(e) = state
                    .principal_repo
                    .update_password_hash(&principal.id, &new_hash)
                    .await
                {
                    tracing::warn!(principal_id = %principal.id, error = %e, "password rehash persist failed; login continues");
                }
            }
            Err(e) => {
                tracing::warn!(principal_id = %principal.id, error = %e, "password rehash failed; login continues");
            }
        }
    }

    // Generate session token (uses session_token_expiry_secs, not access_token_expiry_secs)
    let session_token = state.auth_service.generate_session_token(&principal)?;

    let jar = jar.add(state.session_cookie.build_cookie(session_token));

    // Record successful login attempt (fire-and-forget)
    record_user_login_attempt(
        &state.login_attempt_repo,
        Some(&req.email),
        Some(&principal.id),
        ip,
        LoginOutcome::Success,
        None,
    )
    .await;

    // Build response with user info
    let response = LoginResponse {
        principal_id: principal.id.clone(),
        name: principal.name.clone(),
        email: req.email.clone(),
        roles: principal.roles.iter().map(|r| r.role.clone()).collect(),
        client_id: principal.client_id.clone(),
    };

    // Return both the cookie jar and JSON response
    Ok((jar, Json(response)))
}

/// Logout / revoke token
///
/// Invalidates the current session by clearing the session cookie.
#[utoipa::path(
    post,
    path = "/logout",
    tag = "auth",
    operation_id = "postAuthLogout",
    responses(
        (status = 204, description = "Logout successful")
    )
)]
pub async fn logout(
    State(state): State<AuthState>,
    jar: CookieJar,
    auth: Authenticated,
) -> impl IntoResponse {
    // Verify token is valid (the Authenticated extractor handles this)
    let _ctx = &auth.0;

    // Clear the session cookie by setting it to expire immediately
    let jar = jar.add(state.session_cookie.clear_cookie());

    (jar, StatusCode::NO_CONTENT)
}

/// Check email domain authentication method
///
/// Determines how a user with the given email should authenticate:
/// - Internal: username/password
/// - OIDC: external identity provider
///
/// This is called before showing the login form to determine
/// if the user should be redirected to an external IDP.
#[utoipa::path(
    get,
    path = "/check-domain",
    tag = "auth",
    operation_id = "getAuthCheckDomain",
    params(DomainCheckRequest),
    responses(
        (status = 200, description = "Domain check result", body = DomainCheckResponse)
    )
)]
pub async fn check_domain(
    State(state): State<AuthState>,
    Query(req): Query<DomainCheckRequest>,
) -> Result<Json<DomainCheckResponse>, PlatformError> {
    // Extract domain from email
    let domain = req.email.split('@').nth(1).unwrap_or("").to_lowercase();

    // Look up email domain mapping in the database
    if let Some(mapping) = state
        .email_domain_mapping_repo
        .find_by_email_domain(&domain)
        .await?
    {
        // Load the associated identity provider
        if let Some(idp) = state
            .identity_provider_repo
            .find_by_id(&mapping.identity_provider_id)
            .await?
        {
            let auth_method = match idp.r#type {
                IdentityProviderType::Oidc => AuthMethod::Oidc,
                IdentityProviderType::Internal => AuthMethod::Internal,
            };

            return Ok(Json(DomainCheckResponse {
                domain,
                auth_method,
                provider_id: Some(idp.id),
                authorization_url: idp
                    .oidc_issuer_url
                    .map(|url| format!("{}/authorize", url.trim_end_matches('/'))),
            }));
        }
    }

    // Default: internal authentication
    Ok(Json(DomainCheckResponse {
        domain,
        auth_method: AuthMethod::Internal,
        provider_id: None,
        authorization_url: None,
    }))
}

/// Get current user info
///
/// Returns information about the currently authenticated user.
#[utoipa::path(
    get,
    path = "/me",
    tag = "auth",
    operation_id = "getAuthMe",
    responses(
        (status = 200, description = "Current user info", body = CurrentUserResponse),
        (status = 401, description = "Not authenticated")
    )
)]
pub async fn get_current_user(
    auth: Authenticated,
) -> Result<Json<CurrentUserResponse>, PlatformError> {
    let ctx = &auth.0;

    Ok(Json(CurrentUserResponse {
        id: ctx.principal_id.clone(),
        principal_type: ctx.principal_type.as_str().to_string(),
        email: ctx.email.clone(),
        name: ctx.name.clone(),
        scope: ctx.scope.as_str().to_string(),
        client_id: if ctx.scope == crate::UserScope::Client {
            ctx.accessible_clients.first().cloned()
        } else {
            None
        },
        clients: ctx.accessible_clients.clone(),
        roles: ctx.roles.clone(),
    }))
}

/// Refresh token request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTokenRequest {
    /// The refresh token
    pub refresh_token: String,
}

/// Token refresh response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenRefreshResponse {
    /// New access token
    pub access_token: String,
    /// Token type (always "Bearer")
    pub token_type: String,
    /// Expiration time in seconds
    pub expires_in: i64,
    /// New refresh token (rotation)
    pub refresh_token: String,
}

/// Refresh access token
///
/// Exchange a refresh token for a new access token.
/// The refresh token is rotated (old one invalidated, new one issued).
#[utoipa::path(
    post,
    path = "/refresh",
    tag = "auth",
    operation_id = "postAuthRefresh",
    request_body = RefreshTokenRequest,
    responses(
        (status = 200, description = "Token refreshed", body = TokenRefreshResponse),
        (status = 401, description = "Invalid refresh token")
    )
)]
pub async fn refresh_token(
    State(state): State<AuthState>,
    Json(req): Json<RefreshTokenRequest>,
) -> Result<Json<TokenRefreshResponse>, PlatformError> {
    // Hash the provided token and look it up
    let token_hash = RefreshToken::hash_token(&req.refresh_token);

    let stored_token = state
        .refresh_token_repo
        .find_valid_by_hash(&token_hash)
        .await?
        .ok_or_else(|| PlatformError::InvalidToken {
            message: "Invalid or expired refresh token".to_string(),
        })?;

    // Revoke the old token (token rotation for security)
    state.refresh_token_repo.revoke_by_hash(&token_hash).await?;

    // Find the principal
    let principal = state
        .principal_repo
        .find_by_id(&stored_token.principal_id)
        .await?
        .ok_or_else(|| PlatformError::InvalidToken {
            message: "Principal not found".to_string(),
        })?;

    // Check if principal is still active
    if !principal.active {
        return Err(PlatformError::Unauthorized {
            message: "Account is not active".to_string(),
        });
    }

    // Generate new access token
    let access_token = state.auth_service.generate_access_token(&principal)?;

    // Generate new refresh token (rotation)
    let (raw_token, token_entity) = RefreshToken::generate_token_pair(&principal.id);
    let token_entity =
        token_entity.with_accessible_clients(stored_token.accessible_clients.clone());

    state.refresh_token_repo.insert(&token_entity).await?;

    Ok(Json(TokenRefreshResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: 3600,
        refresh_token: raw_token,
    }))
}

/// Create the auth router
pub fn auth_router(state: AuthState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(check_domain))
        .routes(routes!(get_current_user))
        .routes(routes!(refresh_token))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_login_request_deserialization() {
        let json = r#"{"email":"test@example.com","password":"secret","rememberMe":true}"#;
        let req: LoginRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.email, "test@example.com");
        assert_eq!(req.password, "secret");
        assert!(req.remember_me);
    }

    #[test]
    fn test_login_response_serialization() {
        let response = LoginResponse {
            principal_id: "principal-123".to_string(),
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            roles: vec!["admin".to_string()],
            client_id: Some("client-1".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("principalId"));
        assert!(json.contains("test@example.com"));
        assert!(json.contains("admin"));
    }

    #[test]
    fn test_auth_method_serialization() {
        assert_eq!(
            serde_json::to_string(&AuthMethod::Internal).unwrap(),
            "\"INTERNAL\""
        );
        assert_eq!(
            serde_json::to_string(&AuthMethod::Oidc).unwrap(),
            "\"OIDC\""
        );
    }

    #[test]
    fn test_domain_extraction() {
        let email = "user@example.com";
        let domain = email.split('@').nth(1).unwrap_or("");
        assert_eq!(domain, "example.com");
    }
}
