//! Auth API Endpoints
//!
//! Embedded authentication endpoints for direct login/logout.
//! - POST /auth/login - Password-based login
//! - POST /auth/logout - Logout / token revocation
//! - GET /auth/check-domain - Check if email domain requires external IDP
//! - GET /auth/me - Get current user info

use axum::{
    extract::{Query, State},
    Json,
    http::StatusCode,
    response::IntoResponse,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{PrincipalRepository, RefreshTokenRepository};
use crate::RefreshToken;
use crate::AuthService;
use crate::PasswordService;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

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

/// Login response - matches Java LoginResponse record
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
    /// Session cookie name (default: "fc_session")
    pub session_cookie_name: String,
    /// Whether to set Secure flag on cookie
    pub session_cookie_secure: bool,
    /// SameSite policy for cookie
    pub session_cookie_same_site: String,
    /// Session token expiry in seconds
    pub session_token_expiry_secs: i64,
}

impl AuthState {
    /// Create with default cookie settings
    pub fn new(
        auth_service: Arc<AuthService>,
        principal_repo: Arc<PrincipalRepository>,
        password_service: Arc<PasswordService>,
        refresh_token_repo: Arc<RefreshTokenRepository>,
    ) -> Self {
        Self {
            auth_service,
            principal_repo,
            password_service,
            refresh_token_repo,
            session_cookie_name: "fc_session".to_string(),
            session_cookie_secure: false,
            session_cookie_same_site: "Lax".to_string(),
            session_token_expiry_secs: 28800, // 8 hours
        }
    }

    /// Configure session cookie settings
    pub fn with_session_cookie_settings(
        mut self,
        name: &str,
        secure: bool,
        same_site: &str,
        expiry_secs: i64,
    ) -> Self {
        self.session_cookie_name = name.to_string();
        self.session_cookie_secure = secure;
        self.session_cookie_same_site = same_site.to_string();
        self.session_token_expiry_secs = expiry_secs;
        self
    }
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
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, PlatformError> {
    // Find principal by email
    let principal = state
        .principal_repo
        .find_by_email(&req.email)
        .await?
        .ok_or_else(|| PlatformError::Unauthorized {
            message: "Invalid credentials".to_string(),
        })?;

    // Verify password using Argon2id
    let password_valid = principal.user_identity
        .as_ref()
        .and_then(|id| id.password_hash.as_ref())
        .map(|hash| {
            state.password_service
                .verify_password(&req.password, hash)
                .unwrap_or(false)
        })
        .unwrap_or(false);

    if !password_valid {
        return Err(PlatformError::Unauthorized {
            message: "Invalid credentials".to_string(),
        });
    }

    // Check if user is active
    if !principal.active {
        return Err(PlatformError::Unauthorized {
            message: "Account is not active".to_string(),
        });
    }

    // Generate session token
    let session_token = state.auth_service.generate_access_token(&principal)?;

    // Build session cookie
    let same_site = match state.session_cookie_same_site.to_lowercase().as_str() {
        "strict" => SameSite::Strict,
        "none" => SameSite::None,
        _ => SameSite::Lax,
    };

    let cookie = Cookie::build((state.session_cookie_name.clone(), session_token))
        .path("/")
        .http_only(true)
        .secure(state.session_cookie_secure)
        .same_site(same_site)
        .max_age(time::Duration::seconds(state.session_token_expiry_secs))
        .build();

    let jar = jar.add(cookie);

    // Build response with user info (matches Java LoginResponse)
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
    let cookie = Cookie::build((state.session_cookie_name.clone(), ""))
        .path("/")
        .http_only(true)
        .max_age(time::Duration::ZERO)
        .build();

    let jar = jar.add(cookie);

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
    Query(req): Query<DomainCheckRequest>,
) -> Json<DomainCheckResponse> {
    // Extract domain from email
    let domain = req
        .email
        .split('@')
        .nth(1)
        .unwrap_or("")
        .to_lowercase();

    Json(DomainCheckResponse {
        domain,
        auth_method: AuthMethod::Internal,
        provider_id: None,
        authorization_url: None,
    })
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
        principal_type: ctx.principal_type.clone(),
        email: ctx.email.clone(),
        name: ctx.name.clone(),
        scope: ctx.scope.clone(),
        client_id: if ctx.scope == "CLIENT" {
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

    let stored_token = state.refresh_token_repo
        .find_valid_by_hash(&token_hash)
        .await?
        .ok_or_else(|| PlatformError::InvalidToken {
            message: "Invalid or expired refresh token".to_string(),
        })?;

    // Revoke the old token (token rotation for security)
    state.refresh_token_repo.revoke_by_hash(&token_hash).await?;

    // Find the principal
    let principal = state.principal_repo
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
    let token_entity = token_entity
        .with_accessible_clients(stored_token.accessible_clients.clone());

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
