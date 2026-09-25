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

use crate::auth::session_cookie::SessionCookieConfig;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use axum_extra::extract::cookie::CookieJar;
use axum_extra::extract::Host;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use utoipa::{IntoParams, ToSchema};

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};

use crate::auth::jwks_cache::{JwksCache, JwksError};
use crate::auth::oidc_sync_service::OidcIdentity;
use crate::email_domain_mapping::entity::ScopeType;
use crate::identity_provider::entity::{IdentityProvider, IdentityProviderType};
use crate::principal::operations::events::UserLoggedIn;
use crate::shared::encryption_service::EncryptionService;
use crate::usecase::ExecutionContext;
use crate::UserScope;
use crate::{
    AnchorDomainRepository, EmailDomainMappingRepository, IdentityProviderRepository,
    OAuthClientRepository, OidcLoginStateRepository, PgUnitOfWork, UnitOfWork,
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
    /// Needed by `/auth/oidc/session/end` to look up the caller's registered
    /// `post_logout_redirect_uris` whitelist (OIDC RP-Initiated Logout 1.0
    /// §2). The client is identified by the `aud` claim of `id_token_hint`.
    pub oauth_client_repo: Arc<OAuthClientRepository>,
    /// External base URL for callbacks (e.g., "https://platform.example.com")
    pub external_base_url: Option<String>,
    pub session_cookie: SessionCookieConfig,
    /// Encryption service for decrypting stored secrets (OIDC client secrets, etc.)
    pub encryption_service: Option<Arc<EncryptionService>>,
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

/// Error response carrying a machine-readable `code` beside the message.
#[derive(Debug, Serialize, ToSchema)]
pub struct CodedErrorResponse {
    pub code: String,
    pub error: String,
}

fn coded_error(status: StatusCode, code: &str, error: impl Into<String>) -> Response {
    (
        status,
        Json(CodedErrorResponse {
            code: code.to_string(),
            error: error.into(),
        }),
    )
        .into_response()
}

/// Owner ruling 2026-09-25, item 3 (Java ecb622fe): a multi-tenant
/// provider's shared keys sign tokens for any tenant, and the `email` claim
/// is settable by any tenant admin, so a mapping to one that pins no tenant
/// binds the login to nobody. Such rows predate the save-time rule; a login
/// through one is refused.
fn tenant_not_pinned(
    idp: &IdentityProvider,
    mapping: &crate::email_domain_mapping::entity::EmailDomainMapping,
) -> bool {
    idp.oidc_multi_tenant && !mapping.is_tenant_pinned()
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
            )
                .into_response();
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
            })
            .into_response();
        }
        Ok(false) => {}
        Err(e) => {
            error!(error = %e, "Failed to check anchor domain");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to check domain".to_string(),
                }),
            )
                .into_response();
        }
    }

    // Look up email domain mapping
    let mapping = match state
        .email_domain_mapping_repo
        .find_by_email_domain(domain)
        .await
    {
        Ok(Some(m)) => m,
        Ok(None) => {
            // Default to internal auth if no mapping
            debug!(domain = %domain, "No email domain mapping, defaulting to internal");
            return Json(DomainCheckResponse {
                auth_method: "internal".to_string(),
                login_url: None,
                idp_issuer: None,
            })
            .into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup email domain mapping");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to check domain".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Load the identity provider
    let idp = match state
        .identity_provider_repo
        .find_by_id(&mapping.identity_provider_id)
        .await
    {
        Ok(Some(idp)) => idp,
        Ok(None) => {
            debug!(domain = %domain, "Identity provider not found, defaulting to internal");
            return Json(DomainCheckResponse {
                auth_method: "internal".to_string(),
                login_url: None,
                idp_issuer: None,
            })
            .into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup identity provider");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to check domain".to_string(),
                }),
            )
                .into_response();
        }
    };

    if idp.r#type == IdentityProviderType::Oidc && idp.oidc_issuer_url.is_some() {
        let login_url = format!("/auth/oidc/login?domain={}", domain);
        debug!(domain = %domain, login_url = %login_url, "Domain uses OIDC");
        Json(DomainCheckResponse {
            auth_method: "external".to_string(),
            login_url: Some(login_url),
            idp_issuer: idp.oidc_issuer_url,
        })
        .into_response()
    } else {
        Json(DomainCheckResponse {
            auth_method: "internal".to_string(),
            login_url: None,
            idp_issuer: None,
        })
        .into_response()
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
        )
            .into_response();
    }

    // Look up email domain mapping
    let mapping = match state
        .email_domain_mapping_repo
        .find_by_email_domain(&domain)
        .await
    {
        Ok(Some(m)) => m,
        Ok(None) => {
            // A user's typo or a stale link, not a server fault (owner
            // ruling 2026-09-25, item 8; Java 93367448). A mapped domain
            // whose provider is broken stays a 500 below.
            info!(domain = %domain, "OIDC login for an email domain with no mapping");
            return coded_error(
                StatusCode::NOT_FOUND,
                "EMAIL_DOMAIN_NOT_MAPPED",
                format!(
                    "No authentication configuration found for domain: {}",
                    domain
                ),
            );
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup email domain mapping");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal error".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Load the identity provider
    let idp = match state
        .identity_provider_repo
        .find_by_id(&mapping.identity_provider_id)
        .await
    {
        Ok(Some(idp)) => idp,
        Ok(None) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Identity provider not found for domain: {}", domain),
                }),
            )
                .into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup identity provider");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal error".to_string(),
                }),
            )
                .into_response();
        }
    };

    if idp.r#type != IdentityProviderType::Oidc {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Domain {} uses internal authentication, not OIDC", domain),
            }),
        )
            .into_response();
    }

    if idp.oidc_issuer_url.is_none() || idp.oidc_client_id.is_none() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("OIDC configuration incomplete for domain: {}", domain),
            }),
        )
            .into_response();
    }

    // Validate email domain is allowed by this IDP
    if !idp.allowed_email_domains.is_empty() && !idp.allowed_email_domains.contains(&domain) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: format!("Domain {} is not allowed by the identity provider", domain),
            }),
        )
            .into_response();
    }

    if tenant_not_pinned(&idp, &mapping) {
        warn!(
            domain = %domain,
            identity_provider = %idp.code,
            "OIDC login refused: multi-tenant identity provider pins no tenant"
        );
        return coded_error(
            StatusCode::FORBIDDEN,
            "TENANT_NOT_PINNED",
            "This identity provider accepts any tenant and this domain pins none",
        );
    }

    // Generate state, nonce, and PKCE
    let oidc_state = generate_random_string(32);
    let nonce = generate_random_string(32);
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);

    // Build login state with actual IDP and mapping IDs
    let login_state = crate::OidcLoginState {
        oauth_client_id: params.oauth_client_id,
        oauth_redirect_uri: params.oauth_redirect_uri,
        oauth_scope: params.oauth_scope,
        oauth_state: params.oauth_state,
        oauth_code_challenge: params.oauth_code_challenge,
        oauth_code_challenge_method: params.oauth_code_challenge_method,
        oauth_nonce: params.oauth_nonce,
        ..crate::OidcLoginState::new(
            &oidc_state,
            &domain,
            &idp.id,
            &mapping.id,
            &nonce,
            &code_verifier,
        )
    };

    // Store state
    if let Err(e) = state.oidc_login_state_repo.insert(&login_state).await {
        error!(error = %e, "Failed to store login state");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to initiate login".to_string(),
            }),
        )
            .into_response();
    }

    // Build authorization URL
    let callback_url = get_callback_url(&state, &host, &uri);
    let auth_url =
        build_authorization_url_from_idp(&idp, &oidc_state, &nonce, &code_challenge, &callback_url);

    info!(
        domain = %domain,
        issuer = %idp.oidc_issuer_url.as_deref().unwrap_or(""),
        "Redirecting to OIDC provider"
    );

    // Redirect to IDP
    (StatusCode::SEE_OTHER, [(header::LOCATION, auth_url)]).into_response()
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
    let login_state = match state
        .oidc_login_state_repo
        .find_and_consume_state(oidc_state)
        .await
    {
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
    let idp = match state
        .identity_provider_repo
        .find_by_id(&login_state.identity_provider_id)
        .await
    {
        Ok(Some(idp)) => idp,
        Ok(None) => {
            return error_redirect("Identity provider no longer exists");
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup identity provider");
            return error_redirect("Failed to validate configuration");
        }
    };

    let mapping = match state
        .email_domain_mapping_repo
        .find_by_id(&login_state.email_domain_mapping_id)
        .await
    {
        Ok(Some(m)) => m,
        Ok(None) => {
            return error_redirect("Email domain mapping no longer exists");
        }
        Err(e) => {
            error!(error = %e, "Failed to lookup email domain mapping");
            return error_redirect("Failed to validate configuration");
        }
    };

    // The mapping may have changed since the login began.
    if tenant_not_pinned(&idp, &mapping) {
        warn!(
            domain = %mapping.email_domain,
            identity_provider = %idp.code,
            "OIDC login refused: multi-tenant identity provider pins no tenant"
        );
        return error_redirect("This sign-in method is not configured for your organisation");
    }

    // Exchange code for tokens
    let callback_url = get_callback_url(&state, &host, &uri);
    let tokens = match exchange_code_for_tokens_from_idp(
        &idp,
        code,
        &login_state.code_verifier,
        &callback_url,
        state.encryption_service.as_deref(),
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Token exchange failed");
            return error_redirect("Failed to exchange authorization code");
        }
    };

    // Parse and validate ID token (with JWKS signature verification)
    let claims = match validate_id_token_with_jwks(
        &tokens.id_token,
        &idp,
        &login_state.nonce,
        &state.jwks_cache,
    )
    .await
    {
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
    if let Some(required_tenant_id) = mapping
        .required_oidc_tenant_id
        .as_ref()
        .filter(|_| mapping.is_tenant_pinned())
    {
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
    let principal = match state
        .oidc_sync_service
        .sync_oidc_login(
            &OidcIdentity {
                email: &claims.email,
                name: claims.name.as_deref().unwrap_or(&claims.email),
                external_idp_id: &claims.subject,
                provider_id: idp.oidc_issuer_url.as_deref().unwrap_or("unknown"),
                client_id: mapping.primary_client_id.as_deref(),
                scope: user_scope,
            },
            &claims.roles.unwrap_or_default(),
            allowed_roles,
        )
        .await
    {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "User sync failed");
            return error_redirect("Failed to create user session");
        }
    };

    // The session cookie carries the subject only; the principal's
    // authority is reloaded on every request.
    let session_token = match state.auth_service.generate_session_token(&principal) {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to issue session token");
            return error_redirect("Failed to create session");
        }
    };

    let jar = jar.add(state.session_cookie.build_cookie(session_token));

    // Determine redirect URL
    let redirect_url = determine_redirect_url(&state, &host, &uri, &login_state);

    // Emit UserLoggedIn domain event (best-effort — don't fail the login if this fails)
    {
        use crate::principal::operations::events::{FederatedClaims, FlowcatalystClaims};

        let ctx = ExecutionContext::create(&principal.id);

        // Build role codes from the synced principal
        let roles: Vec<String> = principal.roles.iter().map(|r| r.role.clone()).collect();

        // Extract application codes from role strings (e.g., "ondemand:admin" → "ondemand")
        let applications: Vec<String> = {
            let mut codes = std::collections::BTreeSet::new();
            for role in &roles {
                if let Some(idx) = role.find(':') {
                    if idx > 0 {
                        codes.insert(role[..idx].to_string());
                    }
                }
            }
            codes.into_iter().collect()
        };

        // Build clients list matching TS behaviour
        let clients: Vec<String> = match user_scope {
            UserScope::Anchor => vec!["*".to_string()],
            _ => principal.assigned_clients.clone(),
        };

        let fc_claims = FlowcatalystClaims {
            email: claims.email.clone(),
            principal_type: "USER".to_string(),
            roles,
            clients,
            applications,
        };

        // Decode access token claims (may be a JWT or opaque)
        let access_token_claims = decode_jwt_payload_unsafe(&tokens.access_token)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        let federated_claims = Some(FederatedClaims {
            id_token: claims.raw_claims.clone(),
            access_token: access_token_claims,
        });

        let login_event = UserLoggedIn::new(
            &ctx,
            &principal.id,
            &claims.email,
            "OIDC",
            Some(&idp.code),
            fc_claims,
            federated_claims,
        );

        #[derive(serde::Serialize)]
        struct OidcLoginCommand {
            email: String,
            identity_provider_id: String,
        }
        impl crate::usecase::AuditMasked for OidcLoginCommand {}
        let command = OidcLoginCommand {
            email: claims.email.clone(),
            identity_provider_id: idp.id.clone(),
        };

        if let Err(e) = state
            .unit_of_work
            .emit_event(login_event, &command)
            .await
            .into_result()
        {
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
        (StatusCode::SEE_OTHER, [(header::LOCATION, redirect_url)]),
    )
        .into_response()
}

// ==================== Helper Functions ====================

/// Decode a JWT payload without signature verification.
/// Used to extract claims from access tokens (which may be JWTs or opaque).
/// Returns None if the token is not a valid JWT.
fn decode_jwt_payload_unsafe(token: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&payload_bytes).ok()
}

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
        let scheme = if state.session_cookie.secure {
            "https"
        } else {
            "http"
        };
        format!("{}://{}", scheme, host)
    })
}

fn get_callback_url(state: &OidcLoginApiState, host: &str, uri: &Uri) -> String {
    format!(
        "{}/auth/oidc/callback",
        get_external_base_url(state, host, uri)
    )
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

/// Why the authorization code could not be exchanged at the IDP. Only
/// logged; the user sees a fixed message.
#[derive(Debug, thiserror::Error)]
enum TokenExchangeError {
    #[error("Missing issuer URL")]
    MissingIssuerUrl,
    #[error("Missing client ID")]
    MissingClientId,
    #[error("HTTP request failed: {0}")]
    Http(#[source] reqwest::Error),
    #[error("Token endpoint returned {status}: {body}")]
    TokenEndpoint {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("Failed to parse token response: {0}")]
    ParseResponse(#[source] reqwest::Error),
    #[error("No ID token in response")]
    MissingIdToken,
    #[error("Cannot use the IDP client secret: {0}")]
    ClientSecret(#[source] crate::shared::encryption_service::EncryptionError),
}

async fn exchange_code_for_tokens_from_idp(
    idp: &IdentityProvider,
    code: &str,
    code_verifier: &str,
    callback_url: &str,
    encryption_service: Option<&EncryptionService>,
) -> Result<TokenExchangeResponse, TokenExchangeError> {
    let issuer = idp
        .oidc_issuer_url
        .as_deref()
        .ok_or(TokenExchangeError::MissingIssuerUrl)?;
    let token_endpoint = get_token_endpoint(issuer);
    let client_id = idp
        .oidc_client_id
        .as_deref()
        .ok_or(TokenExchangeError::MissingClientId)?;

    let mut params = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", callback_url),
        ("client_id", client_id),
        ("code_verifier", code_verifier),
    ];

    // Decrypt the stored client secret. It must be an `encrypted:` reference
    // and a key must be configured; otherwise the login fails rather than
    // sending a stored value to the IDP as-is.
    let client_secret = idp
        .oidc_client_secret_ref
        .as_deref()
        .map(|stored| {
            crate::shared::encryption_service::require_configured(encryption_service)
                .and_then(|enc| enc.decrypt_ref(stored))
        })
        .transpose()
        .map_err(TokenExchangeError::ClientSecret)?;
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
        .map_err(TokenExchangeError::Http)?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(TokenExchangeError::TokenEndpoint { status, body });
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(TokenExchangeError::ParseResponse)?;

    let id_token = json["id_token"]
        .as_str()
        .ok_or(TokenExchangeError::MissingIdToken)?
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
    /// Raw ID token claims as JSON (for the UserLoggedIn event's federatedClaims)
    raw_claims: serde_json::Value,
}

/// Why an ID token was rejected. Only logged; the user sees a fixed
/// message.
#[derive(Debug, thiserror::Error)]
enum IdTokenError {
    #[error("Missing issuer URL on IDP")]
    MissingIssuerUrl,
    #[error("Missing client ID on IDP")]
    MissingClientId,
    #[error("Invalid ID token header: {0}")]
    InvalidHeader(#[source] jsonwebtoken::errors::Error),
    #[error(transparent)]
    Jwks(#[from] JwksError),
    #[error("No matching key found in JWKS for kid: {0:?}")]
    NoMatchingKey(Option<String>),
    #[error("Missing '{0}' in RSA JWK")]
    MissingRsaComponent(&'static str),
    #[error("Invalid RSA key components: {0}")]
    InvalidRsaKey(#[source] jsonwebtoken::errors::Error),
    #[error("Unsupported JWK key type: {0}")]
    UnsupportedKeyType(String),
    #[error("JWT signature validation failed: {0}")]
    Signature(#[source] jsonwebtoken::errors::Error),
    #[error("Missing {0} claim")]
    MissingClaim(&'static str),
    #[error("Invalid issuer for multi-tenant IDP: {0}")]
    InvalidIssuer(String),
    #[error("ID token audience {0} does not include this client")]
    InvalidAudience(serde_json::Value),
    #[error("Nonce mismatch")]
    NonceMismatch,
    #[error("No email claim in ID token")]
    MissingEmail,
    #[error(
        "External guest accounts are not supported. Please sign in with your home organization."
    )]
    ExternalGuest,
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
) -> Result<IdTokenClaims, IdTokenError> {
    let issuer_url = idp
        .oidc_issuer_url
        .as_deref()
        .ok_or(IdTokenError::MissingIssuerUrl)?;
    let expected_client_id = idp
        .oidc_client_id
        .as_deref()
        .ok_or(IdTokenError::MissingClientId)?;

    // Decode JWT header to get `kid` (key ID)
    let header = decode_header(id_token).map_err(IdTokenError::InvalidHeader)?;

    // Fetch JWKS for this issuer
    let jwks = jwks_cache.get_jwks(issuer_url).await?;

    // Find the matching key by kid
    let jwk = jwks
        .keys
        .iter()
        .find(|k| {
            header
                .kid
                .as_ref()
                .is_none_or(|kid| k.kid.as_ref() == Some(kid))
        })
        .ok_or_else(|| IdTokenError::NoMatchingKey(header.kid.clone()))?;

    // Build DecodingKey from JWK (RSA only for now — covers Entra ID, Keycloak, Google, etc.)
    let decoding_key = match jwk.kty.as_str() {
        "RSA" => {
            let n = jwk
                .n
                .as_ref()
                .ok_or(IdTokenError::MissingRsaComponent("n"))?;
            let e = jwk
                .e
                .as_ref()
                .ok_or(IdTokenError::MissingRsaComponent("e"))?;
            DecodingKey::from_rsa_components(n, e).map_err(IdTokenError::InvalidRsaKey)?
        }
        other => return Err(IdTokenError::UnsupportedKeyType(other.to_string())),
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
        // we'll validate manually with pattern matching after decode.
        // Setting iss to None skips validation (empty set would reject all issuers).
        validation.iss = None;
        // Only the issuer varies per tenant; `aud` is checked by hand below
        // (jsonwebtoken's check runs with the issuer's, which is off here).
        validation.validate_aud = false;
    } else {
        validation.set_issuer(&[issuer_url]);
        validation.set_audience(&[expected_client_id]);
    }

    // Decode and verify signature + standard claims
    let token_data = decode::<serde_json::Value>(id_token, &decoding_key, &validation)
        .map_err(IdTokenError::Signature)?;

    let payload = token_data.claims;

    // Extract claims
    let issuer = payload["iss"]
        .as_str()
        .ok_or(IdTokenError::MissingClaim("issuer"))?
        .to_string();
    let subject = payload["sub"]
        .as_str()
        .ok_or(IdTokenError::MissingClaim("subject"))?
        .to_string();

    // For multi-tenant: manually validate issuer against pattern
    if idp.oidc_multi_tenant && !is_valid_issuer_for_idp(idp, &issuer) {
        return Err(IdTokenError::InvalidIssuer(issuer));
    }
    // ...and the audience against our registered client id: it is OUR
    // client_id at every tenant, so skipping it would accept an ID token
    // minted for a different relying party at the same IdP (Go
    // `VerifyIDToken`, bridge/oidc.go:246-260).
    if idp.oidc_multi_tenant && !audience_contains(&payload["aud"], expected_client_id) {
        return Err(IdTokenError::InvalidAudience(payload["aud"].clone()));
    }

    // Validate nonce
    let nonce = payload["nonce"].as_str();
    if nonce != Some(expected_nonce) {
        return Err(IdTokenError::NonceMismatch);
    }

    // Extract email
    let email = payload["email"]
        .as_str()
        .or_else(|| payload["preferred_username"].as_str())
        .ok_or(IdTokenError::MissingEmail)?
        .to_lowercase();

    // Reject Entra external/guest users whose UPN contains #EXT#
    // (e.g. "user_domain.co.za#EXT#@tenant.onmicrosoft.com").
    // Guest accounts are managed by a different organization and bypass
    // our email domain trust boundary. Users should sign in via their
    // home organization's IDP instead.
    if email.contains("#ext#") {
        return Err(IdTokenError::ExternalGuest);
    }

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

    // Build raw claims for the event, stripping OIDC protocol artifacts
    let mut raw_claims = payload.clone();
    if let Some(obj) = raw_claims.as_object_mut() {
        for key in &["nonce", "at_hash", "c_hash"] {
            obj.remove(*key);
        }
    }

    Ok(IdTokenClaims {
        issuer,
        subject,
        email,
        name,
        tenant_id,
        roles,
        raw_claims,
    })
}

/// Validate issuer against IDP configuration (exact match or pattern)
/// Whether an `aud` claim, in either RFC 7519 form (a string or an array
/// of strings), names `client_id` (Go `audienceContains`).
fn audience_contains(aud: &serde_json::Value, client_id: &str) -> bool {
    match aud {
        serde_json::Value::String(a) => a == client_id,
        serde_json::Value::Array(auds) => auds.iter().any(|a| a.as_str() == Some(client_id)),
        _ => false,
    }
}

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
        let mut url = format!(
            "{}/oauth/authorize?response_type=code&client_id={}",
            base_url,
            urlencoding::encode(client_id)
        );

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
            url.push_str(&format!(
                "&code_challenge={}",
                urlencoding::encode(challenge)
            ));
        }
        if let Some(ref method) = login_state.oauth_code_challenge_method {
            url.push_str(&format!(
                "&code_challenge_method={}",
                urlencoding::encode(method)
            ));
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
    (StatusCode::SEE_OTHER, [(header::LOCATION, error_url)]).into_response()
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
            )
                .into_response();
        }
    };

    let login_state = login_states
        .into_iter()
        .find(|s| s.interaction_uid.as_deref() == Some(&uid) && s.is_valid());

    let login_state = match login_state {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Interaction not found or expired"})),
            )
                .into_response();
        }
    };

    // requires_oidc reflects whether the interaction's email domain has a
    // federated IdP mapping. Internal-auth domains (no mapping) tell the SPA
    // to use `/auth/login` or `/auth/webauthn/*` instead of redirecting to an
    // external IdP.
    let requires_oidc = matches!(
        state
            .email_domain_mapping_repo
            .find_by_email_domain(&login_state.email_domain)
            .await,
        Ok(Some(_)),
    );

    (
        StatusCode::OK,
        Json(InteractionDetailsResponse {
            uid,
            prompt: "login".to_string(),
            client_id: login_state.oauth_client_id.clone(),
            scope: login_state.oauth_scope.clone(),
            email_domain: login_state.email_domain.clone(),
            requires_oidc,
        }),
    )
        .into_response()
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
            )
                .into_response();
        }
    };

    let login_state = login_states
        .into_iter()
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
            )
                .into_response();
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
        )
            .into_response();
    }

    // Branch on whether the email's domain is federated. Internal-auth
    // domains don't have an OIDC IdP — the SPA should drive login via
    // `/auth/login` or `/auth/webauthn/*` and bounce back to /oauth/authorize
    // once a session cookie is set.
    if matches!(
        state
            .email_domain_mapping_repo
            .find_by_email_domain(&email_domain)
            .await,
        Ok(None),
    ) {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": false,
                "internalAuth": true,
                "loginUrl": "/auth/login",
                "passkeyBeginUrl": "/auth/webauthn/authenticate/begin",
                "error": "domain uses internal authentication; complete login via /auth/login or /auth/webauthn/* and revisit /oauth/authorize",
            })),
        ).into_response();
    }

    // Load the IDP for this interaction — it requires OIDC federation
    let idp = match state
        .identity_provider_repo
        .find_by_id(&login_state.identity_provider_id)
        .await
    {
        Ok(Some(idp)) if idp.r#type == IdentityProviderType::Oidc => idp,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(InteractionLoginResponse {
                    success: false,
                    redirect_url: None,
                    error: Some("Identity provider not found or not OIDC type".to_string()),
                }),
            )
                .into_response();
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
    )
        .into_response()
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
    /// The relying party's client_id, naming the client whose registered
    /// URIs to check when there is no (usable) `id_token_hint`
    #[serde(default)]
    pub client_id: Option<String>,
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
    let jar = jar.add(state.session_cookie.clear_cookie());

    // If id_token_hint is provided, validate it to extract the subject
    // (best-effort — don't fail logout if token is invalid/expired)
    if let Some(ref token_hint) = params.id_token_hint {
        // Attempt to decode without validation (just for logging)
        if let Ok(header) = jsonwebtoken::decode_header(token_hint) {
            debug!(alg = ?header.alg, "Session end with id_token_hint");
        }
    }

    // Redirect to post-logout URI if provided.
    //
    // Per OIDC RP-Initiated Logout 1.0 §2, "If a post_logout_redirect_uri
    // parameter value is supplied, the OP MUST verify the supplied URI is in
    // the list registered for that Client." We identify the client via the
    // `aud` claim of `id_token_hint`. If we can't identify the client, we
    // can't verify the URI — refuse to redirect rather than fall back to a
    // heuristic that allows attacker-owned HTTPS subdomains (CWE-601).
    if let Some(ref redirect_uri) = params.post_logout_redirect_uri {
        let reject = |reason: &str| -> Response {
            warn!(redirect_uri = %redirect_uri, reason, "Rejected post_logout_redirect_uri");
            (
                jar.clone(),
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid_request",
                        "error_description": format!("Invalid post_logout_redirect_uri: {}", reason)
                    })),
                ),
            )
                .into_response()
        };

        // The client is the hint's audience, else the `client_id` parameter
        // (OIDC RP-Initiated Logout 1.0 §2; Go handleSessionEnd,
        // auth/bridge/login_endpoint.go:174-230). AgentPlanner (oidc.py:
        // 248-258) and the Laravel SDK (OidcAuthController.php:215-247) send
        // `client_id` when they hold no id_token.
        let client_id = params
            .id_token_hint
            .as_deref()
            .and_then(extract_aud_from_id_token_hint)
            .filter(|aud| !aud.is_empty())
            .or_else(|| params.client_id.clone().filter(|c| !c.is_empty()));
        let Some(client_id) = client_id else {
            return reject(
                "id_token_hint or client_id is required to verify post_logout_redirect_uri",
            );
        };

        let client = match state.oauth_client_repo.find_by_client_id(&client_id).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                return reject("id_token_hint audience does not match any registered client")
            }
            Err(e) => {
                error!(error = %e, "Failed to look up client for post_logout_redirect_uri check");
                return reject("internal error verifying client");
            }
        };

        if !crate::auth::oauth_api::matches_redirect_uri(
            redirect_uri,
            &client.post_logout_redirect_uris,
        ) {
            return reject("not in the client's registered post_logout_redirect_uris");
        }

        let mut url = redirect_uri.clone();
        if let Some(ref s) = params.state {
            let separator = if url.contains('?') { "&" } else { "?" };
            url.push_str(&format!("{}state={}", separator, urlencoding::encode(s)));
        }

        return (jar, (StatusCode::SEE_OTHER, [(header::LOCATION, url)])).into_response();
    }

    // No redirect — return a simple response
    (
        jar,
        (
            StatusCode::OK,
            Json(serde_json::json!({"message": "Session ended"})),
        ),
    )
        .into_response()
}

/// Best-effort extraction of the `aud` (client_id) claim from an
/// `id_token_hint`. The hint is used only to identify which client's
/// `post_logout_redirect_uris` whitelist to consult — the whitelist itself
/// is the security boundary, so we deliberately do **not** verify the JWT
/// signature here. Returns `None` for any structural malformation; the
/// caller treats `None` as "cannot identify client" and refuses the
/// redirect.
fn extract_aud_from_id_token_hint(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;
    // OIDC Core §2: `aud` is a string OR array of strings. When an array, the
    // first entry is the audience this token was minted for.
    match payload.get("aud") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(arr)) => {
            arr.first().and_then(|v| v.as_str()).map(String::from)
        }
        _ => None,
    }
}

/// Create the OIDC login router
pub fn oidc_login_router(state: OidcLoginApiState) -> Router {
    Router::new()
        .route("/check-domain", post(check_domain))
        .route("/oidc/login", get(oidc_login))
        .route("/oidc/callback", get(oidc_callback))
        .route("/oidc/interaction/{uid}", get(get_interaction))
        .route(
            "/oidc/interaction/{uid}/login",
            post(post_interaction_login),
        )
        .route("/oidc/session/end", get(session_end))
        .with_state(state)
}

// ==================== Portal identity plane hooks ====================
//
// Go's bridge serves the portal SSO start (`/portal/auth/oidc/login`) and the
// portal branch of this callback through the same IdP machinery. The portal
// flow itself lives in `crate::portal::oidc`; these two functions expose the
// IdP plumbing it needs, so this file only gains this section.

/// The secrets and IdP authorize URL of a portal-plane handshake.
pub(crate) struct PortalHandshake {
    pub state: String,
    pub nonce: String,
    pub code_verifier: String,
    pub authorize_url: String,
}

/// Start a provider-direct handshake with `idp` (Go `handlePortalOIDCLogin`
/// after the flow is consumed): fresh state, nonce and PKCE verifier, and
/// the IdP's authorize URL for this platform's callback.
pub(crate) fn portal_handshake(
    state: &OidcLoginApiState,
    host: &str,
    uri: &Uri,
    idp: &IdentityProvider,
) -> PortalHandshake {
    let oidc_state = generate_random_string(32);
    let nonce = generate_random_string(32);
    let code_verifier = generate_code_verifier();
    let challenge = generate_code_challenge(&code_verifier);
    let callback_url = get_callback_url(state, host, uri);
    let authorize_url =
        build_authorization_url_from_idp(idp, &oidc_state, &nonce, &challenge, &callback_url);
    PortalHandshake {
        state: oidc_state,
        nonce,
        code_verifier,
        authorize_url,
    }
}

/// Exchange a portal handshake's code and verify the ID token (signature,
/// issuer, audience, nonce, guest accounts). Returns the verified email and
/// name, or Go's error code and message.
pub(crate) async fn portal_verify_callback(
    state: &OidcLoginApiState,
    host: &str,
    uri: &Uri,
    idp: &IdentityProvider,
    code: &str,
    code_verifier: &str,
    nonce: &str,
) -> Result<(String, Option<String>), (StatusCode, &'static str, String)> {
    let callback_url = get_callback_url(state, host, uri);
    let tokens = exchange_code_for_tokens_from_idp(
        idp,
        code,
        code_verifier,
        &callback_url,
        state.encryption_service.as_deref(),
    )
    .await
    .map_err(|e| {
        error!(error = %e, "portal OIDC code exchange failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "OIDC_EXCHANGE",
            "code exchange failed".to_string(),
        )
    })?;
    let claims = validate_id_token_with_jwks(&tokens.id_token, idp, nonce, &state.jwks_cache)
        .await
        .map_err(|e| match e {
            IdTokenError::NonceMismatch => (
                StatusCode::FORBIDDEN,
                "NONCE_MISMATCH",
                "nonce did not match".to_string(),
            ),
            IdTokenError::MissingEmail => (
                StatusCode::FORBIDDEN,
                "NO_EMAIL",
                "id_token has no email / preferred_username claim".to_string(),
            ),
            IdTokenError::ExternalGuest => (
                StatusCode::FORBIDDEN,
                "EXTERNAL_GUEST",
                "external guest accounts are not supported".to_string(),
            ),
            other => (
                StatusCode::FORBIDDEN,
                "OIDC_VERIFY",
                format!("id_token verification failed: {other}"),
            ),
        })?;
    Ok((claims.email, claims.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::jwks_cache::{JwkKey, Jwks};
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde_json::json;

    const TENANT_ISSUER: &str = "https://login.microsoftonline.com/tenant-a/v2.0";

    /// A multi-tenant Entra-style IdP whose client id is `our-client`, and
    /// the key its JWKS (seeded, no network) publishes.
    async fn multi_tenant_idp() -> (IdentityProvider, JwksCache, EncodingKey) {
        use rsa::{pkcs8::DecodePublicKey, traits::PublicKeyParts, RsaPublicKey};
        let (private_pem, public_pem) =
            crate::auth::auth_service::AuthConfig::generate_rsa_keys(None).unwrap();
        let public = RsaPublicKey::from_public_key_pem(&public_pem).unwrap();
        let jwk = JwkKey {
            kty: "RSA".to_string(),
            key_use: Some("sig".to_string()),
            kid: Some("k1".to_string()),
            alg: Some("RS256".to_string()),
            n: Some(URL_SAFE_NO_PAD.encode(public.n().to_bytes_be())),
            e: Some(URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())),
            x: None,
            y: None,
            crv: None,
        };
        let mut idp = IdentityProvider::new("entra", "Entra", IdentityProviderType::Oidc);
        idp.oidc_issuer_url = Some("https://login.microsoftonline.com/common/v2.0".to_string());
        idp.oidc_client_id = Some("our-client".to_string());
        idp.oidc_multi_tenant = true;
        idp.oidc_issuer_pattern =
            Some(r"^https://login\.microsoftonline\.com/[^/]+/v2\.0$".to_string());
        let cache = JwksCache::new(900);
        cache
            .seed(
                idp.oidc_issuer_url.as_deref().unwrap(),
                Jwks { keys: vec![jwk] },
            )
            .await;
        let key = EncodingKey::from_rsa_pem(private_pem.as_bytes()).unwrap();
        (idp, cache, key)
    }

    fn id_token(key: &EncodingKey, aud: serde_json::Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("k1".to_string());
        let now = chrono::Utc::now().timestamp();
        encode(
            &header,
            &json!({
                "iss": TENANT_ISSUER,
                "sub": "ext-sub",
                "aud": aud,
                "exp": now + 300,
                "iat": now,
                "nonce": "n-1",
                "email": "ada@acme.test",
                "tid": "tenant-a"
            }),
            key,
        )
        .unwrap()
    }

    /// Go `VerifyIDToken` (bridge/oidc.go:246-260): a multi-tenant IdP
    /// re-applies the audience check, so an ID token minted for another
    /// relying party at the same IdP is refused.
    #[tokio::test]
    async fn multi_tenant_id_token_must_name_our_client() {
        let (idp, cache, key) = multi_tenant_idp().await;

        for aud in [json!("our-client"), json!(["other", "our-client"])] {
            let claims =
                validate_id_token_with_jwks(&id_token(&key, aud.clone()), &idp, "n-1", &cache)
                    .await
                    .unwrap_or_else(|e| panic!("{aud}: {e}"));
            assert_eq!(claims.tenant_id.as_deref(), Some("tenant-a"));
        }

        for aud in [json!("someone-else"), json!(["someone-else"]), json!(null)] {
            let err =
                validate_id_token_with_jwks(&id_token(&key, aud.clone()), &idp, "n-1", &cache)
                    .await
                    .unwrap_err();
            assert!(
                matches!(err, IdTokenError::InvalidAudience(_)),
                "{aud}: {err}"
            );
        }
    }

    #[test]
    fn audience_contains_reads_both_forms() {
        assert!(audience_contains(&json!("c"), "c"));
        assert!(audience_contains(&json!(["a", "c"]), "c"));
        assert!(!audience_contains(&json!(["a"]), "c"));
        assert!(!audience_contains(&json!("a"), "c"));
        assert!(!audience_contains(&json!(null), "c"));
    }
}
