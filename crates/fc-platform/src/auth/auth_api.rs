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
use crate::shared::middleware::{ClientIp, OptionalAuth};
use crate::AuthService;
use crate::LoginOutcome;
use crate::PasswordService;
use crate::{EmailDomainMappingRepository, IdentityProviderRepository, LoginAttemptRepository};
use crate::{PrincipalRepository, RefreshTokenRepository};

/// Login request. Absent members read as empty, as Go's decoder leaves
/// them, and an empty email or password is an invalid-credentials 401.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    /// Email address
    #[serde(default)]
    pub email: String,

    /// Password
    #[serde(default)]
    pub password: String,

    /// Remember me (extends session duration)
    #[serde(default)]
    pub remember_me: bool,
}

/// Login response: Go's `loginResponse` (auth/login/endpoint.go:412-430).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    /// `ok` for a completed login
    pub status: String,
    /// Principal ID
    pub principal_id: String,
    /// Display name
    pub name: String,
    /// Email address
    pub email: String,
    /// Assigned roles
    pub roles: Vec<String>,
    /// Effective permissions (Go `buildPermissionList`: the roles'
    /// permissions, then `*` when they include `platform:*:*:*`)
    pub permissions: Vec<String>,
    /// Home client ID; `null` when none
    pub client_id: Option<String>,
    /// Whether the account signs in through a federated identity provider
    pub sso_managed: bool,
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

    /// Principal ID, under Go's name (`principalId`)
    pub principal_id: String,

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

    /// Effective permissions: every permission the principal's roles grant,
    /// de-duplicated and sorted, then `"*"` when they include the
    /// super-admin `platform:*:*:*` (Go `buildPermissionList`,
    /// auth/login/endpoint.go:437-450). The SPA gates pages on these.
    pub permissions: Vec<String>,

    /// Whether the account signs in through a federated identity provider
    /// (a linked external identity, or an email domain mapped to an OIDC
    /// provider); the SPA hides password self-service for it (Go
    /// `ssoManaged`, auth/login/endpoint.go:616-634).
    pub sso_managed: bool,
}

/// Auth service state
#[derive(Clone)]
pub struct AuthState {
    pub auth_service: Arc<AuthService>,
    pub principal_repo: Arc<PrincipalRepository>,
    /// Flattens roles to permissions for `/auth/me`
    pub role_repo: Arc<crate::RoleRepository>,
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
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, PlatformError> {
    // Go decodes the body itself (auth/login/endpoint.go:448-452): an
    // unreadable body is 400 `INVALID_JSON`, absent members are empty.
    let req: LoginRequest = serde_json::from_slice(&body)
        .map_err(|e| PlatformError::bad_request_code("INVALID_JSON", e.to_string()))?;
    // Lower-cased up front so the backoff identifier matches across
    // attempts whatever the casing typed.
    let email = req.email.trim().to_lowercase();
    if email.is_empty() || req.password.is_empty() {
        // Constant-shape error: which field is missing is not said.
        return Err(PlatformError::session_unauthorized("Invalid credentials"));
    }
    let ip = client_ip.as_deref();

    // Brute-force backoff: per-(email, IP) exponential delay plus a
    // per-email ceiling, before credentials are evaluated.
    if let BackoffDecision::Reject {
        retry_after_secs, ..
    } =
        login_backoff::check(&state.login_attempt_repo, &state.backoff_policy, &email, ip).await?
    {
        return Err(PlatformError::login_backoff(retry_after_secs));
    }

    let record_failure = |principal_id: Option<String>, reason: &'static str| {
        let repo = state.login_attempt_repo.clone();
        let email = email.clone();
        let ip = client_ip.clone();
        async move {
            record_user_login_attempt(
                &repo,
                Some(&email),
                principal_id.as_deref(),
                ip.as_deref(),
                LoginOutcome::Failure,
                Some(reason),
            )
            .await;
        }
    };

    // SSO enforcement: a domain mapped to an OIDC identity provider signs in
    // there, and the password path is closed (Go, endpoint.go:480-497).
    if let Some((_, domain)) = email.split_once('@').filter(|(_, d)| !d.is_empty()) {
        if state
            .email_domain_mapping_repo
            .is_federated_domain(domain)
            .await?
        {
            record_failure(None, "SSO required").await;
            return Err(PlatformError::forbidden_code(
                "SSO_REQUIRED",
                "This email domain signs in through its identity provider; password login is disabled",
            ));
        }
    }

    // Not found, inactive, and password-less accounts all read as invalid
    // credentials (Go, endpoint.go:499-508).
    let principal = state.principal_repo.find_by_email(&email).await?;
    let stored_hash = principal
        .as_ref()
        .filter(|p| p.active)
        .and_then(|p| p.user_identity.as_ref())
        .and_then(|id| id.password_hash.clone());
    let (Some(principal), Some(stored_hash)) = (principal, stored_hash) else {
        record_failure(None, "Invalid credentials").await;
        return Err(PlatformError::session_unauthorized("Invalid credentials"));
    };
    // Argon2id, or a bcrypt hash migrated from a Laravel app (Go
    // passwordhash.Verify).
    let password_valid = state
        .password_service
        .verify_password(&req.password, &stored_hash)
        .unwrap_or(false);
    if !password_valid {
        record_failure(None, "Invalid credentials").await;
        return Err(PlatformError::session_unauthorized("Invalid credentials"));
    }

    // Lazy upgrade: a hash that isn't Argon2id at the current parameters (a
    // migrated bcrypt hash, say) is re-encoded now the user has proved the
    // password. Best-effort, as Go's login (auth/login/endpoint.go:519-525):
    // a failure is logged and the login goes on.
    if state.password_service.needs_rehash(&stored_hash) {
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

    // Go `completeLogin` (endpoint.go:559-611): the session cookie, the
    // recorded success, and the login payload.
    let session_token = state.auth_service.generate_session_token(&principal)?;
    let jar = jar.add(state.session_cookie.build_cookie(session_token));
    let principal_email = principal.email().unwrap_or_default().to_string();
    record_user_login_attempt(
        &state.login_attempt_repo,
        Some(&email),
        Some(&principal.id),
        ip,
        LoginOutcome::Success,
        None,
    )
    .await;

    let roles = crate::auth::auth_service::role_names(&principal);
    // An unresolvable permission set leaves the list empty; the user is
    // signed in regardless (Go, endpoint.go:586-592).
    let (permissions, sso_managed) = tokio::join!(
        effective_permissions(&state, &roles),
        sso_managed(&state, &principal),
    );
    let response = LoginResponse {
        status: "ok".to_string(),
        principal_id: principal.id.clone(),
        name: principal.name.clone(),
        email: principal_email,
        roles,
        permissions: permissions.unwrap_or_default(),
        client_id: principal.client_id.clone(),
        sso_managed: sso_managed.unwrap_or(false),
    };

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
pub async fn logout(State(state): State<AuthState>, jar: CookieJar) -> impl IntoResponse {
    // Go clears the cookie whatever the request carries: a stale or missing
    // session logs out all the same (auth/login/endpoint.go handleLogout).
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
    State(state): State<AuthState>,
    auth: OptionalAuth,
) -> Result<Json<CurrentUserResponse>, PlatformError> {
    // Reload the principal so the answer is current rather than whatever the
    // token was stamped with; a deactivated or deleted principal is not
    // authenticated (Go handleMe, auth/login/endpoint.go:656-694), and
    // neither is a request without a session: Go's 401 `UNAUTHENTICATED`
    // with `WWW-Authenticate: Cookie realm="fc_session"`.
    let not_authenticated = || PlatformError::session_unauthorized("Not authenticated");
    let Some(auth) = auth.0 else {
        return Err(not_authenticated());
    };
    let principal = state
        .principal_repo
        .find_by_id(&auth.principal_id)
        .await?
        .filter(|p| p.active)
        .ok_or_else(not_authenticated)?;

    let roles = crate::auth::auth_service::role_names(&principal);
    let (permissions, sso_managed) = tokio::try_join!(
        effective_permissions(&state, &roles),
        sso_managed(&state, &principal),
    )?;

    Ok(Json(CurrentUserResponse {
        id: principal.id.clone(),
        principal_id: principal.id.clone(),
        principal_type: principal.principal_type.as_str().to_string(),
        email: principal.email().map(String::from),
        name: principal.name.clone(),
        scope: principal.scope.as_str().to_string(),
        // Only a CLIENT-tier principal carries one: the SPA reads a missing
        // clientId as "may act for other owners" (stores/permissions.ts).
        client_id: principal
            .client_id
            .clone()
            .filter(|_| principal.scope == crate::UserScope::Client),
        clients: crate::auth::auth_service::clients_claim(&principal),
        roles,
        permissions,
        sso_managed,
    }))
}

/// Go `buildPermissionList` (auth/login/endpoint.go:437-450) over the
/// flattened role permissions, sorted.
async fn effective_permissions(
    state: &AuthState,
    roles: &[String],
) -> Result<Vec<String>, PlatformError> {
    let mut permissions = state.role_repo.flatten_permissions(roles).await?;
    if permissions
        .iter()
        .any(|p| p == crate::role::entity::permissions::ADMIN_ALL)
    {
        permissions.push("*".to_string());
    }
    Ok(permissions)
}

/// Go `ssoManaged` (auth/login/endpoint.go:616-634).
async fn sso_managed(
    state: &AuthState,
    principal: &crate::Principal,
) -> Result<bool, PlatformError> {
    if principal.external_identity.is_some() {
        return Ok(true);
    }
    let Some(domain) = principal
        .email()
        .and_then(|e| e.split_once('@'))
        .map(|(_, d)| d)
        .filter(|d| !d.is_empty())
    else {
        return Ok(false);
    };
    let Some(mapping) = state
        .email_domain_mapping_repo
        .find_by_email_domain(domain)
        .await?
    else {
        return Ok(false);
    };
    Ok(state
        .identity_provider_repo
        .find_by_id(&mapping.identity_provider_id)
        .await?
        .is_some_and(|idp| idp.r#type == IdentityProviderType::Oidc))
}

/// Refresh token request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTokenRequest {
    /// The refresh token
    #[serde(default)]
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
    body: axum::body::Bytes,
) -> Result<Json<TokenRefreshResponse>, PlatformError> {
    // Go decodes the body itself (auth/login/endpoint.go:341-350).
    let req: RefreshTokenRequest = serde_json::from_slice(&body)
        .map_err(|e| PlatformError::bad_request_code("INVALID_JSON", e.to_string()))?;
    if req.refresh_token.is_empty() {
        return Err(PlatformError::session_unauthorized(
            "Invalid or expired refresh token",
        ));
    }
    // Rotate through the same contract as the /oauth/token refresh grant
    // (Go grantstore.Rotate). This endpoint authenticates no client, so a
    // token issued to an OAuth client is refused and not consumed: it
    // refreshes through /oauth/token, which checks the client (Go
    // handleRefresh, auth/login/endpoint.go:352-360).
    let rotated = match crate::auth::refresh_rotation::rotate(
        &*state.refresh_token_repo,
        &req.refresh_token,
        None,
    )
    .await?
    {
        Ok(rotated) => rotated,
        Err(crate::auth::refresh_rotation::Rejection::Refused { .. }) => {
            return Err(PlatformError::session_unauthorized(
                "Token was not issued to this client",
            ));
        }
        Err(_) => {
            return Err(PlatformError::session_unauthorized(
                "Invalid or expired refresh token",
            ));
        }
    };
    let raw_token = rotated.new_raw;
    let stored_token = rotated.stored;

    // Find the principal
    let principal = state
        .principal_repo
        .find_by_id(&stored_token.principal_id)
        .await?
        .ok_or_else(|| PlatformError::session_unauthorized("Invalid or expired refresh token"))?;

    // Check if principal is still active
    if !principal.active {
        return Err(PlatformError::session_unauthorized("Account is not active"));
    }

    // Generate new access token
    let access_token = state.auth_service.generate_access_token(&principal)?;

    Ok(Json(TokenRefreshResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: state.auth_service.access_token_expiry_secs(),
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
            status: "ok".to_string(),
            principal_id: "principal-123".to_string(),
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            roles: vec!["admin".to_string()],
            permissions: vec!["platform:*:*:*".to_string(), "*".to_string()],
            client_id: None,
            sso_managed: false,
        };

        let json: serde_json::Value = serde_json::to_value(&response).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "status": "ok",
                "principalId": "principal-123",
                "name": "Test User",
                "email": "test@example.com",
                "roles": ["admin"],
                "permissions": ["platform:*:*:*", "*"],
                "clientId": null,
                "ssoManaged": false,
            })
        );
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
