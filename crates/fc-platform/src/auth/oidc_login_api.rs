//! OIDC Federation Login Endpoints
//!
//! Handles login flows where FlowCatalyst acts as an OIDC client,
//! federating authentication to external identity providers (Entra ID, Keycloak, etc.)
//!
//! Flow:
//! 1. POST /auth/check-domain - Check auth method for email domain
//! 2. GET /auth/oidc/login?domain=example.com - Redirects to external IDP
//! 3. User authenticates at external IDP
//! 4. GET /auth/oidc/callback?code=...&state=... - Handles callback, creates session

use axum::{
    routing::{get, post},
    extract::{State, Query},
    response::{Json, IntoResponse, Response},
    http::{StatusCode, header, Uri},
    Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use axum_extra::extract::Host;
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::sync::Arc;
use tracing::{info, warn, error, debug};
use rand::Rng;

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};

use crate::UserScope;
use crate::identity_provider::entity::{IdentityProvider, IdentityProviderType};
use crate::email_domain_mapping::entity::ScopeType;
use crate::auth::jwks_cache::JwksCache;
use crate::principal::operations::events::UserLoggedIn;
use crate::usecase::ExecutionContext;
use crate::{
    OidcLoginStateRepository, AnchorDomainRepository,
    IdentityProviderRepository, EmailDomainMappingRepository,
    PgUnitOfWork, UnitOfWork,
};
use crate::{AuthService, OidcSyncService};

/// OIDC Login API State
#[derive(Clone)]
pub struct OidcLoginApiState {
    pub anchor_domain_repo: Arc<AnchorDomainRepository>,
    pub identity_provider_repo: Arc<IdentityProviderRepository>,
    pub email_domain_mapping_repo: Arc<EmailDomainMappingRepository>,
    pub oidc_login_state_repo: Arc<OidcLoginStateRepository>,
    pub oidc_sync_service: Arc<OidcSyncService>,
    pub auth_service: Arc<AuthService>,
    pub jwks_cache: Arc<JwksCache>,
    pub unit_of_work: Arc<PgUnitOfWork>,
    /// External base URL for callbacks (e.g., "https://platform.example.com")
    pub external_base_url: Option<String>,
    /// Session cookie settings
    pub session_cookie_name: String,
    pub session_cookie_secure: bool,
    pub session_cookie_same_site: String,
    pub session_token_expiry_secs: i64,
}

impl OidcLoginApiState {
    pub fn new(
        anchor_domain_repo: Arc<AnchorDomainRepository>,
        identity_provider_repo: Arc<IdentityProviderRepository>,
        email_domain_mapping_repo: Arc<EmailDomainMappingRepository>,
        oidc_login_state_repo: Arc<OidcLoginStateRepository>,
        oidc_sync_service: Arc<OidcSyncService>,
        auth_service: Arc<AuthService>,
        unit_of_work: Arc<PgUnitOfWork>,
    ) -> Self {
        Self {
            anchor_domain_repo,
            identity_provider_repo,
            email_domain_mapping_repo,
            oidc_login_state_repo,
            oidc_sync_service,
            auth_service,
            jwks_cache: Arc::new(JwksCache::default()),
            unit_of_work,
            external_base_url: None,
            session_cookie_name: "fc_session".to_string(),
            session_cookie_secure: true,
            session_cookie_same_site: "Lax".to_string(),
            session_token_expiry_secs: 86400, // 24 hours
        }
    }

    pub fn with_external_base_url(mut self, url: impl Into<String>) -> Self {
        self.external_base_url = Some(url.into());
        self
    }

    pub fn with_session_cookie_settings(
        mut self,
        name: impl Into<String>,
        secure: bool,
        same_site: impl Into<String>,
        expiry_secs: i64,
    ) -> Self {
        self.session_cookie_name = name.into();
        self.session_cookie_secure = secure;
        self.session_cookie_same_site = same_site.into();
        self.session_token_expiry_secs = expiry_secs;
        self
    }
}

// ==================== Request/Response Types ====================

/// Domain check request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DomainCheckRequest {
    pub email: String,
}

/// Domain check response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DomainCheckResponse {
    /// "internal" for password auth, "external" for OIDC
    pub auth_method: String,
    /// URL to redirect to for login (for external: /auth/oidc/login?domain=...)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_url: Option<String>,
    /// External IDP issuer URL (informational)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idp_issuer: Option<String>,
}

/// OIDC login query parameters
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct OidcLoginParams {
    /// Email domain to authenticate
    pub domain: String,
    /// URL to return to after login
    pub return_url: Option<String>,
    // OAuth flow chaining parameters
    pub oauth_client_id: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub oauth_scope: Option<String>,
    pub oauth_state: Option<String>,
    pub oauth_code_challenge: Option<String>,
    pub oauth_code_challenge_method: Option<String>,
    pub oauth_nonce: Option<String>,
}

/// OIDC callback query parameters
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct OidcCallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Error response
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}

// ==================== Endpoints ====================

/// Check authentication method for email domain
#[utoipa::path(
    post,
    path = "/check-domain",
    tag = "auth-discovery",
    request_body = DomainCheckRequest,
    responses(
        (status = 200, description = "Domain check result", body = DomainCheckResponse),
        (status = 400, description = "Invalid email"),
        (status = 500, description = "Internal error")
    )
)]
pub async fn check_domain(
    State(state): State<OidcLoginApiState>,
    Json(body): Json<DomainCheckRequest>,
) -> Response {
    let email = body.email.trim().to_lowercase();

    // Validate email format
    let at_index = match email.find('@') {
        Some(idx) if idx > 0 && idx < email.len() - 1 => idx,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid email format".to_string(),
                }),
            ).into_response();
        }
    };

    let domain = &email[at_index + 1..];
    debug!(domain = %domain, "Checking auth method");

    // Check if anchor domain (god mode)
    match state.anchor_domain_repo.is_anchor_domain(domain).await {
        Ok(true) => {
            // Anchor domains can use internal auth
            return Json(DomainCheckResponse {
                auth_method: "internal".to_string(),
                login_url: None,
                idp_issuer: None,
            }).into_response();
        }
        Ok(false) => {}
        Err(e) => {
            error!(error = %e, "Failed to check anchor domain");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to check domain".to_string(),
                }),
            ).into_response();
        }
    }

    // Look up email domain mapping
    let mapping = match state.email_domain_mapping_repo.find_by_email_domain(domain).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            // Default to internal auth if no mapping
            debug!(domain = %domain, "No email domain mapping, defaulting to internal");
            return Json(DomainCheckResponse {
                auth_method: "internal".to_string(),
                login_url: None,
                idp_issuer: None,
            }).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup email domain mapping");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to check domain".to_string(),
                }),
            ).into_response();
        }
    };

    // Load the identity provider
    let idp = match state.identity_provider_repo.find_by_id(&mapping.identity_provider_id).await {
        Ok(Some(idp)) => idp,
        Ok(None) => {
            debug!(domain = %domain, "Identity provider not found, defaulting to internal");
            return Json(DomainCheckResponse {
                auth_method: "internal".to_string(),
                login_url: None,
                idp_issuer: None,
            }).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup identity provider");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to check domain".to_string(),
                }),
            ).into_response();
        }
    };

    if idp.r#type == IdentityProviderType::Oidc && idp.oidc_issuer_url.is_some() {
        let login_url = format!("/auth/oidc/login?domain={}", domain);
        debug!(domain = %domain, login_url = %login_url, "Domain uses OIDC");
        Json(DomainCheckResponse {
            auth_method: "external".to_string(),
            login_url: Some(login_url),
            idp_issuer: idp.oidc_issuer_url,
        }).into_response()
    } else {
        Json(DomainCheckResponse {
            auth_method: "internal".to_string(),
            login_url: None,
            idp_issuer: None,
        }).into_response()
    }
}

/// Initiate OIDC login - redirects to external IDP
#[utoipa::path(
    get,
    path = "/oidc/login",
    tag = "oidc-federation",
    params(OidcLoginParams),
    responses(
        (status = 303, description = "Redirect to IDP"),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Domain not found"),
        (status = 500, description = "Internal error")
    )
)]
pub async fn oidc_login(
    State(state): State<OidcLoginApiState>,
    Host(host): Host,
    uri: Uri,
    Query(params): Query<OidcLoginParams>,
) -> Response {
    let domain = params.domain.trim().to_lowercase();

    if domain.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "domain parameter is required".to_string(),
            }),
        ).into_response();
    }

    // Look up email domain mapping
    let mapping = match state.email_domain_mapping_repo.find_by_email_domain(&domain).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("No authentication configuration found for domain: {}", domain),
                }),
            ).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup email domain mapping");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal error".to_string(),
                }),
            ).into_response();
        }
    };

    // Load the identity provider
    let idp = match state.identity_provider_repo.find_by_id(&mapping.identity_provider_id).await {
        Ok(Some(idp)) => idp,
        Ok(None) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Identity provider not found for domain: {}", domain),
                }),
            ).into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup identity provider");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal error".to_string(),
                }),
            ).into_response();
        }
    };

    if idp.r#type != IdentityProviderType::Oidc {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Domain {} uses internal authentication, not OIDC", domain),
            }),
        ).into_response();
    }

    if idp.oidc_issuer_url.is_none() || idp.oidc_client_id.is_none() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("OIDC configuration incomplete for domain: {}", domain),
            }),
        ).into_response();
    }

    // Validate email domain is allowed by this IDP
    if !idp.allowed_email_domains.is_empty() && !idp.allowed_email_domains.contains(&domain) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: format!("Domain {} is not allowed by the identity provider", domain),
            }),
        ).into_response();
    }

    // Generate state, nonce, and PKCE
    let oidc_state = generate_random_string(32);
    let nonce = generate_random_string(32);
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);

    // Build login state with actual IDP and mapping IDs
    let login_state = crate::OidcLoginState::new(
        &oidc_state,
        &domain,
        &idp.id,
        &mapping.id,
        &nonce,
        &code_verifier,
    )
    .with_oauth_params(
        params.oauth_client_id,
        params.oauth_redirect_uri,
        params.oauth_scope,
        params.oauth_state,
        params.oauth_code_challenge,
        params.oauth_code_challenge_method,
        params.oauth_nonce,
    );

    // Store state
    if let Err(e) = state.oidc_login_state_repo.insert(&login_state).await {
        error!(error = %e, "Failed to store login state");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to initiate login".to_string(),
            }),
        ).into_response();
    }

    // Build authorization URL
    let callback_url = get_callback_url(&state, &host, &uri);
    let auth_url = build_authorization_url_from_idp(
        &idp,
        &oidc_state,
        &nonce,
        &code_challenge,
        &callback_url,
    );

    info!(
        domain = %domain,
        issuer = %idp.oidc_issuer_url.as_deref().unwrap_or(""),
        "Redirecting to OIDC provider"
    );

    // Redirect to IDP
    (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, auth_url)],
    ).into_response()
}

/// Handle OIDC callback from external IDP
#[utoipa::path(
    get,
    path = "/oidc/callback",
    tag = "oidc-federation",
    params(OidcCallbackParams),
    responses(
        (status = 303, description = "Redirect to application"),
        (status = 400, description = "Callback error")
    )
)]
pub async fn oidc_callback(
    State(state): State<OidcLoginApiState>,
    Host(host): Host,
    uri: Uri,
    Query(params): Query<OidcCallbackParams>,
    jar: CookieJar,
) -> Response {
    // Handle IDP errors
    if let Some(error) = &params.error {
        warn!(
            error = %error,
            description = params.error_description.as_deref().unwrap_or(""),
            "OIDC callback error"
        );
        return error_redirect(params.error_description.as_deref().unwrap_or(error));
    }

    let code = match &params.code {
        Some(c) if !c.is_empty() => c,
        _ => {
            return error_redirect("No authorization code received");
        }
    };

    let oidc_state = match &params.state {
        Some(s) if !s.is_empty() => s,
        _ => {
            return error_redirect("No state parameter received");
        }
    };

    // Atomically consume state (find + delete in single query to prevent race conditions)
    let login_state = match state.oidc_login_state_repo.find_and_consume_state(oidc_state).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            warn!(state = %oidc_state, "Invalid, expired, or already consumed OIDC state");
            return error_redirect("Invalid or expired login session. Please try again.");
        }
        Err(e) => {
            error!(error = %e, "Failed to consume state");
            return error_redirect("Failed to validate login session");
        }
    };

    // Load identity provider and email domain mapping from stored IDs
    let idp = match state.identity_provider_repo.find_by_id(&login_state.identity_provider_id).await {
        Ok(Some(idp)) => idp,
        Ok(None) => {
            return error_redirect("Identity provider no longer exists");
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup identity provider");
            return error_redirect("Failed to validate configuration");
        }
    };

    let mapping = match state.email_domain_mapping_repo.find_by_id(&login_state.email_domain_mapping_id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return error_redirect("Email domain mapping no longer exists");
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup email domain mapping");
            return error_redirect("Failed to validate configuration");
        }
    };

    // Exchange code for tokens
    let callback_url = get_callback_url(&state, &host, &uri);
    let tokens = match exchange_code_for_tokens_from_idp(&idp, code, &login_state.code_verifier, &callback_url).await {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Token exchange failed");
            return error_redirect("Failed to exchange authorization code");
        }
    };

    // Parse and validate ID token (with JWKS signature verification)
    let claims = match validate_id_token_with_jwks(&tokens.id_token, &idp, &login_state.nonce, &state.jwks_cache).await {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "ID token validation failed");
            return error_redirect("Failed to validate identity token");
        }
    };

    // Validate email domain matches the mapping
    let email_domain = claims.email.split('@').nth(1).unwrap_or("");
    if email_domain != mapping.email_domain {
        warn!(
            expected_domain = %mapping.email_domain,
            actual_domain = %email_domain,
            email = %claims.email,
            "Email domain mismatch"
        );
        return error_redirect("Email domain does not match the expected domain");
    }

    // Validate tenant ID if required by the mapping
    if let Some(ref required_tenant_id) = mapping.required_oidc_tenant_id {
        if let Some(ref tid) = claims.tenant_id {
            if tid != required_tenant_id {
                warn!(
                    expected = %required_tenant_id,
                    actual = %tid,
                    "OIDC tenant ID mismatch"
                );
                return error_redirect("OIDC tenant ID does not match");
            }
        } else {
            warn!("Required OIDC tenant ID not present in token");
            return error_redirect("OIDC tenant ID not present in token");
        }
    }

    // Determine user scope from email domain mapping
    let user_scope = match mapping.scope_type {
        ScopeType::Anchor => UserScope::Anchor,
        ScopeType::Partner => UserScope::Partner,
        ScopeType::Client => UserScope::Client,
    };

    // Sync user and roles (with allowed_role_ids filter from email domain mapping)
    let allowed_roles = if mapping.allowed_role_ids.is_empty() {
        None
    } else {
        Some(mapping.allowed_role_ids.as_slice())
    };
    let principal = match state.oidc_sync_service.sync_oidc_login_with_allowed_roles(
        &claims.email,
        claims.name.as_deref().unwrap_or(&claims.email),
        &claims.subject,
        idp.oidc_issuer_url.as_deref().unwrap_or("unknown"),
        mapping.primary_client_id.as_deref(),
        user_scope,
        &claims.roles.unwrap_or_default(),
        allowed_roles,
    ).await {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "User sync failed");
            return error_redirect("Failed to create user session");
        }
    };

    // Issue session token using the principal (which has roles already synced)
    let session_token = match state.auth_service.generate_access_token(&principal) {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to issue session token");
            return error_redirect("Failed to create session");
        }
    };

    // Build session cookie with same settings as regular login
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

    // Determine redirect URL
    let redirect_url = determine_redirect_url(&state, &host, &uri, &login_state);

    // Emit UserLoggedIn domain event (best-effort — don't fail the login if this fails)
    {
        let ctx = ExecutionContext::create(&principal.id);
        let login_event = UserLoggedIn::new(
            &ctx,
            &principal.id,
            &claims.email,
            user_scope,
            &idp.id,
            mapping.primary_client_id.as_deref(),
        );

        #[derive(serde::Serialize)]
        struct OidcLoginCommand { email: String, identity_provider_id: String }
        let command = OidcLoginCommand {
            email: claims.email.clone(),
            identity_provider_id: idp.id.clone(),
        };

        if let Err(e) = state.unit_of_work.emit_event(login_event, &command).await.into_result() {
            warn!(error = %e, "Failed to emit UserLoggedIn event (login still succeeded)");
        }
    }

    info!(
        email = %claims.email,
        principal_id = %principal.id,
        "OIDC login successful"
    );

    // Redirect with cookie
    (
        jar,
        (
            StatusCode::SEE_OTHER,
            [(header::LOCATION, redirect_url)],
        ),
    ).into_response()
}

// ==================== Helper Functions ====================

fn generate_random_string(length: usize) -> String {
    let bytes: Vec<u8> = (0..length).map(|_| rand::rng().random()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

fn generate_code_verifier() -> String {
    generate_random_string(32)
}

fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash)
}

fn get_external_base_url(state: &OidcLoginApiState, host: &str, _uri: &Uri) -> String {
    state.external_base_url.clone().unwrap_or_else(|| {
        // Fall back to request host
        let scheme = if state.session_cookie_secure { "https" } else { "http" };
        format!("{}://{}", scheme, host)
    })
}

fn get_callback_url(state: &OidcLoginApiState, host: &str, uri: &Uri) -> String {
    format!("{}/auth/oidc/callback", get_external_base_url(state, host, uri))
}

fn build_authorization_url_from_idp(
    idp: &IdentityProvider,
    state: &str,
    nonce: &str,
    code_challenge: &str,
    callback_url: &str,
) -> String {
    let issuer = idp.oidc_issuer_url.as_deref().unwrap_or("");
    let auth_endpoint = get_authorization_endpoint(issuer);
    let client_id = idp.oidc_client_id.as_deref().unwrap_or("");

    format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        auth_endpoint,
        urlencoding::encode(client_id),
        urlencoding::encode(callback_url),
        urlencoding::encode("openid profile email"),
        urlencoding::encode(state),
        urlencoding::encode(nonce),
        urlencoding::encode(code_challenge),
    )
}

fn get_authorization_endpoint(issuer_url: &str) -> String {
    if issuer_url.contains("login.microsoftonline.com") {
        issuer_url.replace("/v2.0", "/oauth2/v2.0/authorize")
    } else {
        let base = issuer_url.trim_end_matches('/');
        format!("{}/authorize", base)
    }
}

fn get_token_endpoint(issuer_url: &str) -> String {
    if issuer_url.contains("login.microsoftonline.com") {
        issuer_url.replace("/v2.0", "/oauth2/v2.0/token")
    } else {
        let base = issuer_url.trim_end_matches('/');
        format!("{}/token", base)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct TokenExchangeResponse {
    access_token: String,
    id_token: String,
    refresh_token: Option<String>,
}

async fn exchange_code_for_tokens_from_idp(
    idp: &IdentityProvider,
    code: &str,
    code_verifier: &str,
    callback_url: &str,
) -> Result<TokenExchangeResponse, String> {
    let issuer = idp.oidc_issuer_url.as_deref().ok_or("Missing issuer URL")?;
    let token_endpoint = get_token_endpoint(issuer);
    let client_id = idp.oidc_client_id.as_deref().ok_or("Missing client ID")?;

    let mut params = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", callback_url),
        ("client_id", client_id),
        ("code_verifier", code_verifier),
    ];

    // Add client secret if present
    let client_secret = idp.oidc_client_secret_ref.clone();
    if let Some(ref secret) = client_secret {
        params.push(("client_secret", secret));
    }

    let client = reqwest::Client::new();
    let response = client
        .post(&token_endpoint)
        .form(&params)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Token endpoint returned {}: {}", status, body));
    }

    let json: serde_json::Value = response.json().await
        .map_err(|e| format!("Failed to parse token response: {}", e))?;

    let id_token = json["id_token"]
        .as_str()
        .ok_or("No ID token in response")?
        .to_string();

    Ok(TokenExchangeResponse {
        access_token: json["access_token"].as_str().unwrap_or("").to_string(),
        id_token,
        refresh_token: json["refresh_token"].as_str().map(String::from),
    })
}

#[derive(Debug)]
#[allow(dead_code)]
struct IdTokenClaims {
    issuer: String,
    subject: String,
    email: String,
    name: Option<String>,
    tenant_id: Option<String>,
    roles: Option<Vec<String>>,
}

/// Validate an ID token using JWKS signature verification.
///
/// This fetches (or uses cached) JWKS from the IDP's discovery endpoint,
/// finds the matching key by `kid`, and verifies the JWT signature before
/// extracting claims.
async fn validate_id_token_with_jwks(
    id_token: &str,
    idp: &IdentityProvider,
    expected_nonce: &str,
    jwks_cache: &JwksCache,
) -> Result<IdTokenClaims, String> {
    let issuer_url = idp.oidc_issuer_url.as_deref().ok_or("Missing issuer URL on IDP")?;
    let expected_client_id = idp.oidc_client_id.as_deref().ok_or("Missing client ID on IDP")?;

    // Decode JWT header to get `kid` (key ID)
    let header = decode_header(id_token)
        .map_err(|e| format!("Invalid ID token header: {}", e))?;

    // Fetch JWKS for this issuer
    let jwks = jwks_cache.get_jwks(issuer_url).await?;

    // Find the matching key by kid
    let jwk = jwks.keys.iter()
        .find(|k| {
            header.kid.as_ref().map_or(true, |kid| k.kid.as_ref() == Some(kid))
        })
        .ok_or_else(|| format!(
            "No matching key found in JWKS for kid: {:?}",
            header.kid
        ))?;

    // Build DecodingKey from JWK (RSA only for now — covers Entra ID, Keycloak, Google, etc.)
    let decoding_key = match jwk.kty.as_str() {
        "RSA" => {
            let n = jwk.n.as_ref().ok_or("Missing 'n' in RSA JWK")?;
            let e = jwk.e.as_ref().ok_or("Missing 'e' in RSA JWK")?;
            DecodingKey::from_rsa_components(n, e)
                .map_err(|e| format!("Invalid RSA key components: {}", e))?
        }
        other => return Err(format!("Unsupported JWK key type: {}", other)),
    };

    // Determine algorithm from header (default RS256)
    let algorithm = match header.alg {
        jsonwebtoken::Algorithm::RS256 => Algorithm::RS256,
        jsonwebtoken::Algorithm::RS384 => Algorithm::RS384,
        jsonwebtoken::Algorithm::RS512 => Algorithm::RS512,
        _ => Algorithm::RS256,
    };

    // Build validation — checks signature, exp, iss, aud
    let mut validation = Validation::new(algorithm);

    // Set issuer validation — for multi-tenant, extract actual issuer from token first
    // and validate against pattern; for single-tenant, validate against configured issuer
    if idp.oidc_multi_tenant {
        // For multi-tenant: disable issuer validation in jsonwebtoken,
        // we'll validate manually with pattern matching
        validation.set_issuer::<String>(&[]);
        validation.validate_aud = false; // audience may vary per tenant
    } else {
        validation.set_issuer(&[issuer_url]);
        validation.set_audience(&[expected_client_id]);
    }

    // Decode and verify signature + standard claims
    let token_data = decode::<serde_json::Value>(id_token, &decoding_key, &validation)
        .map_err(|e| format!("JWT signature validation failed: {}", e))?;

    let payload = token_data.claims;

    // Extract claims
    let issuer = payload["iss"].as_str().ok_or("Missing issuer claim")?.to_string();
    let subject = payload["sub"].as_str().ok_or("Missing subject claim")?.to_string();

    // For multi-tenant: manually validate issuer against pattern
    if idp.oidc_multi_tenant {
        if !is_valid_issuer_for_idp(idp, &issuer) {
            return Err(format!("Invalid issuer for multi-tenant IDP: {}", issuer));
        }
    }

    // Validate nonce
    let nonce = payload["nonce"].as_str();
    if nonce != Some(expected_nonce) {
        return Err("Nonce mismatch".to_string());
    }

    // Extract email
    let email = payload["email"]
        .as_str()
        .or_else(|| payload["preferred_username"].as_str())
        .ok_or("No email claim in ID token")?
        .to_lowercase();

    // Extract name
    let name = payload["name"].as_str().map(String::from);

    // Extract tenant ID (Entra ID uses "tid")
    let tenant_id = payload["tid"].as_str().map(String::from);

    // Extract roles (various claim names used by different IDPs)
    let roles = payload["roles"]
        .as_array()
        .or_else(|| payload["groups"].as_array())
        .or_else(|| payload["realm_access"]["roles"].as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        });

    Ok(IdTokenClaims {
        issuer,
        subject,
        email,
        name,
        tenant_id,
        roles,
    })
}

/// Validate issuer against IDP configuration (exact match or pattern)
fn is_valid_issuer_for_idp(idp: &IdentityProvider, issuer: &str) -> bool {
    // Exact match against configured issuer URL
    if let Some(ref issuer_url) = idp.oidc_issuer_url {
        if issuer_url == issuer {
            return true;
        }
    }
    // Pattern match for multi-tenant IDPs
    if idp.oidc_multi_tenant {
        if let Some(ref pattern) = idp.oidc_issuer_pattern {
            if let Ok(re) = regex::Regex::new(pattern) {
                return re.is_match(issuer);
            }
        }
    }
    false
}

fn determine_redirect_url(
    state: &OidcLoginApiState,
    host: &str,
    uri: &Uri,
    login_state: &crate::OidcLoginState,
) -> String {
    let base_url = get_external_base_url(state, host, uri);

    // If this was part of an OAuth flow, redirect back to authorize endpoint
    if let Some(ref client_id) = login_state.oauth_client_id {
        let mut url = format!("{}/oauth/authorize?response_type=code&client_id={}", base_url, urlencoding::encode(client_id));

        if let Some(ref uri) = login_state.oauth_redirect_uri {
            url.push_str(&format!("&redirect_uri={}", urlencoding::encode(uri)));
        }
        if let Some(ref scope) = login_state.oauth_scope {
            url.push_str(&format!("&scope={}", urlencoding::encode(scope)));
        }
        if let Some(ref state) = login_state.oauth_state {
            url.push_str(&format!("&state={}", urlencoding::encode(state)));
        }
        if let Some(ref challenge) = login_state.oauth_code_challenge {
            url.push_str(&format!("&code_challenge={}", urlencoding::encode(challenge)));
        }
        if let Some(ref method) = login_state.oauth_code_challenge_method {
            url.push_str(&format!("&code_challenge_method={}", urlencoding::encode(method)));
        }
        if let Some(ref nonce) = login_state.oauth_nonce {
            url.push_str(&format!("&nonce={}", urlencoding::encode(nonce)));
        }

        return url;
    }

    // Return to specified URL or default to dashboard
    if let Some(ref return_url) = login_state.return_url {
        if !return_url.is_empty() {
            if return_url.starts_with('/') {
                return format!("{}{}", base_url, return_url);
            }
            return return_url.clone();
        }
    }

    format!("{}/dashboard", base_url)
}

fn error_redirect(message: &str) -> Response {
    let error_url = format!("/?error={}", urlencoding::encode(message));
    (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, error_url)],
    ).into_response()
}

// ==================== OIDC Interaction Endpoints ====================
//
// These provide interaction-based login flows, equivalent to oidc-provider's
// interaction endpoints. They allow a frontend to render custom login/consent
// pages while maintaining the OIDC state.

/// Interaction details response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InteractionDetailsResponse {
    /// Interaction UID
    pub uid: String,
    /// The kind of interaction (login or consent)
    pub prompt: String,
    /// Client info from the original OAuth request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Requested scope
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Email domain for this login
    pub email_domain: String,
    /// Whether OIDC federation is required
    pub requires_oidc: bool,
}

/// Interaction login request (POST body)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InteractionLoginRequest {
    /// Email address
    pub email: String,
    /// Password (for internal auth)
    #[serde(default)]
    pub password: Option<String>,
}

/// Interaction login result
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InteractionLoginResponse {
    /// Whether login succeeded
    pub success: bool,
    /// Redirect URL to continue the flow
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_url: Option<String>,
    /// Error message if login failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Get interaction details by UID
///
/// Returns information about a pending OIDC interaction so the frontend
/// can render the appropriate login/consent UI.
pub async fn get_interaction(
    State(state): State<OidcLoginApiState>,
    axum::extract::Path(uid): axum::extract::Path<String>,
) -> Response {
    // Look up the OIDC login state by interaction_uid
    let login_states = match state.oidc_login_state_repo.find_all().await {
        Ok(states) => states,
        Err(e) => {
            error!(error = %e, "Failed to lookup interaction states");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal error"})),
            ).into_response();
        }
    };

    let login_state = login_states.into_iter()
        .find(|s| s.interaction_uid.as_deref() == Some(&uid) && s.is_valid());

    let login_state = match login_state {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Interaction not found or expired"})),
            ).into_response();
        }
    };

    (
        StatusCode::OK,
        Json(InteractionDetailsResponse {
            uid,
            prompt: "login".to_string(),
            client_id: login_state.oauth_client_id.clone(),
            scope: login_state.oauth_scope.clone(),
            email_domain: login_state.email_domain.clone(),
            requires_oidc: true,
        }),
    ).into_response()
}

/// Submit login for an interaction
///
/// Called by the frontend to complete the login step of an interaction.
/// For OIDC-federated domains, this redirects to the external IDP.
/// For internal auth, it validates credentials directly.
pub async fn post_interaction_login(
    State(state): State<OidcLoginApiState>,
    Host(host): Host,
    uri: Uri,
    axum::extract::Path(uid): axum::extract::Path<String>,
    Json(req): Json<InteractionLoginRequest>,
) -> Response {
    // Look up the OIDC login state by interaction_uid
    let login_states = match state.oidc_login_state_repo.find_all().await {
        Ok(states) => states,
        Err(e) => {
            error!(error = %e, "Failed to lookup interaction states");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(InteractionLoginResponse {
                    success: false,
                    redirect_url: None,
                    error: Some("Internal error".to_string()),
                }),
            ).into_response();
        }
    };

    let login_state = login_states.into_iter()
        .find(|s| s.interaction_uid.as_deref() == Some(&uid) && s.is_valid());

    let login_state = match login_state {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(InteractionLoginResponse {
                    success: false,
                    redirect_url: None,
                    error: Some("Interaction not found or expired".to_string()),
                }),
            ).into_response();
        }
    };

    // Extract email domain from the submitted email
    let email_domain = req.email.split('@').nth(1).unwrap_or("").to_lowercase();

    // Verify the email domain matches the interaction
    if email_domain != login_state.email_domain {
        return (
            StatusCode::BAD_REQUEST,
            Json(InteractionLoginResponse {
                success: false,
                redirect_url: None,
                error: Some("Email domain does not match".to_string()),
            }),
        ).into_response();
    }

    // Load the IDP for this interaction — it requires OIDC federation
    let idp = match state.identity_provider_repo.find_by_id(&login_state.identity_provider_id).await {
        Ok(Some(idp)) if idp.r#type == IdentityProviderType::Oidc => idp,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(InteractionLoginResponse {
                    success: false,
                    redirect_url: None,
                    error: Some("Identity provider not found or not OIDC type".to_string()),
                }),
            ).into_response();
        }
    };

    // Build the OIDC authorization URL
    let callback_url = get_callback_url(&state, &host, &uri);
    let code_challenge = generate_code_challenge(&login_state.code_verifier);

    let auth_url = build_authorization_url_from_idp(
        &idp,
        &login_state.state,
        &login_state.nonce,
        &code_challenge,
        &callback_url,
    );

    (
        StatusCode::OK,
        Json(InteractionLoginResponse {
            success: true,
            redirect_url: Some(auth_url),
            error: None,
        }),
    ).into_response()
}

// ==================== OIDC Session End ====================

/// OIDC Session End query parameters
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SessionEndParams {
    /// ID token hint (optional, used to identify the user)
    #[serde(default)]
    pub id_token_hint: Option<String>,
    /// Post-logout redirect URI
    #[serde(default)]
    pub post_logout_redirect_uri: Option<String>,
    /// State to pass back to the client
    #[serde(default)]
    pub state: Option<String>,
}

/// OIDC Session End / RP-Initiated Logout (OpenID Connect RP-Initiated Logout 1.0)
///
/// Terminates the user's session and optionally redirects to the client's
/// post-logout redirect URI.
#[utoipa::path(
    get,
    path = "/session/end",
    tag = "oidc",
    params(SessionEndParams),
    responses(
        (status = 302, description = "Redirect after session end"),
        (status = 200, description = "Session ended (no redirect)")
    )
)]
pub async fn session_end(
    State(state): State<OidcLoginApiState>,
    jar: CookieJar,
    Query(params): Query<SessionEndParams>,
) -> Response {
    // Clear the session cookie
    let cookie = Cookie::build((state.session_cookie_name.clone(), ""))
        .path("/")
        .http_only(true)
        .max_age(time::Duration::ZERO)
        .build();

    let jar = jar.add(cookie);

    // If id_token_hint is provided, validate it to extract the subject
    // (best-effort — don't fail logout if token is invalid/expired)
    if let Some(ref token_hint) = params.id_token_hint {
        // Attempt to decode without validation (just for logging)
        if let Ok(header) = jsonwebtoken::decode_header(token_hint) {
            debug!(alg = ?header.alg, "Session end with id_token_hint");
        }
    }

    // Redirect to post-logout URI if provided
    if let Some(ref redirect_uri) = params.post_logout_redirect_uri {
        let mut url = redirect_uri.clone();
        if let Some(ref s) = params.state {
            let separator = if url.contains('?') { "&" } else { "?" };
            url.push_str(&format!("{}state={}", separator, urlencoding::encode(s)));
        }

        return (
            jar,
            (
                StatusCode::SEE_OTHER,
                [(header::LOCATION, url)],
            ),
        ).into_response();
    }

    // No redirect — return a simple response
    (
        jar,
        (
            StatusCode::OK,
            Json(serde_json::json!({"message": "Session ended"})),
        ),
    ).into_response()
}

/// Create the OIDC login router
pub fn oidc_login_router(state: OidcLoginApiState) -> Router {
    Router::new()
        .route("/check-domain", post(check_domain))
        .route("/oidc/login", get(oidc_login))
        .route("/oidc/callback", get(oidc_callback))
        .route("/oidc/interaction/{uid}", get(get_interaction))
        .route("/oidc/interaction/{uid}/login", post(post_interaction_login))
        .route("/oidc/session/end", get(session_end))
        .with_state(state)
}
