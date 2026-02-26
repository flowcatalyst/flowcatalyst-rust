//! Authentication middleware for FlowCatalyst Router API
//!
//! Supports:
//! - BasicAuth with configurable username/password
//! - OIDC with full JWT validation (signature, issuer, audience, expiration)
//! - No authentication (for development)

use axum::{
    extract::Request,
    http::{header, HeaderName, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn, error, info};

/// Authentication mode
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AuthMode {
    /// No authentication required
    #[default]
    None,
    /// HTTP Basic Authentication
    Basic,
    /// OpenID Connect authentication with full JWT validation
    Oidc,
    /// Full OIDC authorization code flow with browser redirects
    #[serde(rename = "OIDC_FLOW")]
    OidcFlow,
}

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Authentication mode
    pub mode: AuthMode,
    /// BasicAuth username (required if mode is Basic)
    pub basic_username: Option<String>,
    /// BasicAuth password (required if mode is Basic)
    pub basic_password: Option<String>,
    /// OIDC issuer URL (required if mode is Oidc or OidcFlow)
    pub oidc_issuer: Option<String>,
    /// OIDC client ID
    pub oidc_client_id: Option<String>,
    /// OIDC audience for token validation
    pub oidc_audience: Option<String>,
    /// OIDC Flow: client secret (for token exchange)
    pub oidc_client_secret: Option<String>,
    /// OIDC Flow: redirect URI (callback URL)
    pub oidc_redirect_uri: Option<String>,
    /// OIDC Flow: scopes to request (space-separated)
    pub oidc_scopes: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: AuthMode::None,
            basic_username: None,
            basic_password: None,
            oidc_issuer: None,
            oidc_client_id: None,
            oidc_audience: None,
            oidc_client_secret: None,
            oidc_redirect_uri: None,
            oidc_scopes: None,
        }
    }
}

impl AuthConfig {
    /// Create config for BasicAuth
    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            mode: AuthMode::Basic,
            basic_username: Some(username.into()),
            basic_password: Some(password.into()),
            ..Default::default()
        }
    }

    /// Create config for OIDC
    pub fn oidc(issuer: impl Into<String>, client_id: impl Into<String>, audience: impl Into<String>) -> Self {
        Self {
            mode: AuthMode::Oidc,
            oidc_issuer: Some(issuer.into()),
            oidc_client_id: Some(client_id.into()),
            oidc_audience: Some(audience.into()),
            ..Default::default()
        }
    }

    /// Create config from environment variables
    pub fn from_env() -> Self {
        let mode = std::env::var("AUTH_MODE")
            .ok()
            .and_then(|m| match m.to_uppercase().as_str() {
                "BASIC" => Some(AuthMode::Basic),
                "OIDC" => Some(AuthMode::Oidc),
                "OIDC_FLOW" => Some(AuthMode::OidcFlow),
                "NONE" | "" => Some(AuthMode::None),
                _ => None,
            })
            .unwrap_or(AuthMode::None);

        Self {
            mode,
            basic_username: std::env::var("AUTH_BASIC_USERNAME").ok(),
            basic_password: std::env::var("AUTH_BASIC_PASSWORD").ok(),
            oidc_issuer: std::env::var("OIDC_ISSUER").ok(),
            oidc_client_id: std::env::var("OIDC_CLIENT_ID").ok(),
            oidc_audience: std::env::var("OIDC_AUDIENCE").ok(),
            oidc_client_secret: std::env::var("OIDC_CLIENT_SECRET").ok(),
            oidc_redirect_uri: std::env::var("OIDC_REDIRECT_URI").ok(),
            oidc_scopes: std::env::var("OIDC_SCOPES").ok(),
        }
    }
}

/// OIDC Discovery document
#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    jwks_uri: String,
}

/// JWKS (JSON Web Key Set)
#[derive(Debug, Clone, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

/// Individual JWK (JSON Web Key)
#[derive(Debug, Clone, Deserialize)]
struct Jwk {
    kty: String,
    kid: Option<String>,
    n: Option<String>,  // RSA modulus
    e: Option<String>,  // RSA exponent
    x: Option<String>,  // EC x coordinate
    y: Option<String>,  // EC y coordinate
}

/// Cached JWKS with expiration
struct CachedJwks {
    jwks: Jwks,
    fetched_at: Instant,
}

/// OIDC validator with JWKS caching
pub struct OidcValidator {
    issuer: String,
    audience: String,
    jwks_cache: RwLock<Option<CachedJwks>>,
    jwks_cache_ttl: Duration,
    http_client: reqwest::Client,
}

impl OidcValidator {
    /// Create a new OIDC validator
    pub fn new(issuer: String, audience: String) -> Self {
        Self {
            issuer,
            audience,
            jwks_cache: RwLock::new(None),
            jwks_cache_ttl: Duration::from_secs(3600), // 1 hour cache
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Fetch OIDC discovery document
    async fn fetch_discovery(&self) -> Result<OidcDiscovery, String> {
        let discovery_url = format!("{}/.well-known/openid-configuration", self.issuer.trim_end_matches('/'));

        debug!(url = %discovery_url, "Fetching OIDC discovery document");

        let response = self.http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch OIDC discovery: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("OIDC discovery returned status: {}", response.status()));
        }

        response
            .json::<OidcDiscovery>()
            .await
            .map_err(|e| format!("Failed to parse OIDC discovery: {}", e))
    }

    /// Fetch JWKS from the issuer
    async fn fetch_jwks(&self) -> Result<Jwks, String> {
        let discovery = self.fetch_discovery().await?;

        debug!(jwks_uri = %discovery.jwks_uri, "Fetching JWKS");

        let response = self.http_client
            .get(&discovery.jwks_uri)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch JWKS: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("JWKS fetch returned status: {}", response.status()));
        }

        response
            .json::<Jwks>()
            .await
            .map_err(|e| format!("Failed to parse JWKS: {}", e))
    }

    /// Get JWKS, using cache if valid
    async fn get_jwks(&self) -> Result<Jwks, String> {
        // Check cache first
        {
            let cache = self.jwks_cache.read().await;
            if let Some(ref cached) = *cache {
                if cached.fetched_at.elapsed() < self.jwks_cache_ttl {
                    return Ok(cached.jwks.clone());
                }
            }
        }

        // Cache miss or expired, fetch new JWKS
        let jwks = self.fetch_jwks().await?;

        // Update cache
        {
            let mut cache = self.jwks_cache.write().await;
            *cache = Some(CachedJwks {
                jwks: jwks.clone(),
                fetched_at: Instant::now(),
            });
        }

        info!("JWKS cache refreshed with {} keys", jwks.keys.len());
        Ok(jwks)
    }

    /// Find a key by kid (key ID)
    fn find_key<'a>(&self, jwks: &'a Jwks, kid: Option<&str>) -> Option<&'a Jwk> {
        match kid {
            Some(kid) => jwks.keys.iter().find(|k| k.kid.as_deref() == Some(kid)),
            None => jwks.keys.first(), // If no kid in token, use first key
        }
    }

    /// Create a DecodingKey from a JWK
    fn jwk_to_decoding_key(&self, jwk: &Jwk) -> Result<DecodingKey, String> {
        match jwk.kty.as_str() {
            "RSA" => {
                let n = jwk.n.as_ref().ok_or("RSA key missing 'n' component")?;
                let e = jwk.e.as_ref().ok_or("RSA key missing 'e' component")?;
                DecodingKey::from_rsa_components(n, e)
                    .map_err(|e| format!("Failed to create RSA decoding key: {}", e))
            }
            "EC" => {
                let x = jwk.x.as_ref().ok_or("EC key missing 'x' component")?;
                let y = jwk.y.as_ref().ok_or("EC key missing 'y' component")?;
                DecodingKey::from_ec_components(x, y)
                    .map_err(|e| format!("Failed to create EC decoding key: {}", e))
            }
            other => Err(format!("Unsupported key type: {}", other)),
        }
    }

    /// Validate a JWT token
    pub async fn validate_token(&self, token: &str) -> Result<TokenClaims, String> {
        // Decode the header to get the key ID
        let header = decode_header(token)
            .map_err(|e| format!("Failed to decode token header: {}", e))?;

        // Get JWKS
        let jwks = self.get_jwks().await?;

        // Find the key
        let jwk = self.find_key(&jwks, header.kid.as_deref())
            .ok_or_else(|| format!("No matching key found for kid: {:?}", header.kid))?;

        // Create decoding key
        let decoding_key = self.jwk_to_decoding_key(jwk)?;

        // Determine algorithm - use from header, or infer from JWK
        let algorithm = header.alg;

        // Set up validation
        let mut validation = Validation::new(algorithm);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        validation.validate_exp = true;
        validation.validate_nbf = true;

        // Decode and validate
        let token_data = decode::<TokenClaims>(token, &decoding_key, &validation)
            .map_err(|e| format!("Token validation failed: {}", e))?;

        debug!(
            sub = %token_data.claims.sub,
            "Token validated successfully"
        );

        Ok(token_data.claims)
    }

    /// Force refresh the JWKS cache (e.g., on signature verification failure)
    pub async fn refresh_jwks(&self) -> Result<(), String> {
        let jwks = self.fetch_jwks().await?;

        let mut cache = self.jwks_cache.write().await;
        *cache = Some(CachedJwks {
            jwks,
            fetched_at: Instant::now(),
        });

        info!("JWKS cache force refreshed");
        Ok(())
    }
}

/// JWT token claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    /// Subject (user ID)
    pub sub: String,
    /// Issuer
    pub iss: String,
    /// Audience (can be string or array)
    #[serde(default)]
    pub aud: serde_json::Value,
    /// Expiration time
    pub exp: i64,
    /// Issued at
    #[serde(default)]
    pub iat: i64,
    /// Not before
    #[serde(default)]
    pub nbf: i64,
    /// JWT ID
    #[serde(default)]
    pub jti: Option<String>,
    /// Email (optional)
    #[serde(default)]
    pub email: Option<String>,
    /// Name (optional)
    #[serde(default)]
    pub name: Option<String>,
    /// Azure AD specific: preferred_username
    #[serde(default)]
    pub preferred_username: Option<String>,
    /// Azure AD specific: oid (object ID)
    #[serde(default)]
    pub oid: Option<String>,
    /// Azure AD specific: tid (tenant ID)
    #[serde(default)]
    pub tid: Option<String>,
    /// Roles (optional)
    #[serde(default)]
    pub roles: Vec<String>,
    /// Scope (optional)
    #[serde(default)]
    pub scp: Option<String>,
}

/// Authentication state for middleware
#[derive(Clone)]
pub struct AuthState {
    pub config: Arc<AuthConfig>,
    pub oidc_validator: Option<Arc<OidcValidator>>,
    /// OIDC flow state (only present when `oidc-flow` feature is enabled and mode is OidcFlow)
    #[cfg(feature = "oidc-flow")]
    pub oidc_flow_state: Option<Arc<crate::api::oidc_flow::OidcFlowState>>,
}

impl AuthState {
    pub fn new(config: AuthConfig) -> Self {
        let oidc_validator = if config.mode == AuthMode::Oidc || config.mode == AuthMode::OidcFlow {
            if let (Some(issuer), Some(audience)) = (&config.oidc_issuer, &config.oidc_audience) {
                Some(Arc::new(OidcValidator::new(issuer.clone(), audience.clone())))
            } else {
                warn!("OIDC mode enabled but missing issuer or audience configuration");
                None
            }
        } else {
            None
        };

        #[cfg(feature = "oidc-flow")]
        let oidc_flow_state = if config.mode == AuthMode::OidcFlow {
            use crate::api::oidc_flow::{OidcFlowConfig, OidcFlowState, SessionStore, PendingOidcStateStore};

            if let (Some(issuer), Some(client_id), Some(redirect_uri)) = (
                &config.oidc_issuer,
                &config.oidc_client_id,
                &config.oidc_redirect_uri,
            ) {
                let scopes = config
                    .oidc_scopes
                    .as_deref()
                    .unwrap_or("openid profile email")
                    .split_whitespace()
                    .map(String::from)
                    .collect();

                let session_ttl_seconds = 3600u64;

                let flow_config = OidcFlowConfig {
                    issuer_url: issuer.clone(),
                    client_id: client_id.clone(),
                    client_secret: config.oidc_client_secret.clone(),
                    redirect_uri: redirect_uri.clone(),
                    scopes,
                    session_ttl_seconds,
                };

                let session_store = Arc::new(SessionStore::new(
                    std::time::Duration::from_secs(session_ttl_seconds),
                ));

                let pending_states = Arc::new(PendingOidcStateStore::new());

                let http_client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .expect("Failed to create OIDC flow HTTP client");

                info!(
                    issuer = %issuer,
                    client_id = %client_id,
                    redirect_uri = %redirect_uri,
                    "OIDC flow state initialized"
                );

                Some(Arc::new(OidcFlowState {
                    config: flow_config,
                    session_store,
                    pending_states,
                    http_client,
                    oidc_validator: oidc_validator.clone(),
                }))
            } else {
                warn!(
                    "OIDC_FLOW mode enabled but missing required configuration \
                     (OIDC_ISSUER, OIDC_CLIENT_ID, OIDC_REDIRECT_URI)"
                );
                None
            }
        } else {
            None
        };

        Self {
            config: Arc::new(config),
            oidc_validator,
            #[cfg(feature = "oidc-flow")]
            oidc_flow_state,
        }
    }
}

/// Authentication middleware
pub async fn auth_middleware(
    state: axum::extract::State<AuthState>,
    request: Request,
    next: Next,
) -> Response {
    match state.config.mode {
        AuthMode::None => {
            // No authentication required
            next.run(request).await
        }
        AuthMode::Basic => basic_auth(&state.config, request, next).await,
        AuthMode::Oidc => oidc_auth(&state, request, next).await,
        AuthMode::OidcFlow => oidc_flow_auth(&state, request, next).await,
    }
}

/// HTTP Basic Authentication
async fn basic_auth(config: &AuthConfig, request: Request, next: Next) -> Response {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(auth) if auth.starts_with("Basic ") => {
            let encoded = &auth[6..];
            match BASE64.decode(encoded) {
                Ok(decoded) => {
                    if let Ok(credentials) = String::from_utf8(decoded) {
                        if let Some((username, password)) = credentials.split_once(':') {
                            let expected_username = config.basic_username.as_deref().unwrap_or("");
                            let expected_password = config.basic_password.as_deref().unwrap_or("");

                            if username == expected_username && password == expected_password {
                                debug!(username = %username, "BasicAuth successful");
                                return next.run(request).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Invalid base64 in Authorization header");
                }
            }
        }
        _ => {}
    }

    // Authentication failed
    warn!("BasicAuth failed");
    let mut response = (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"FlowCatalyst\""),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-auth-mode"),
        HeaderValue::from_static("BASIC"),
    );
    response
}

/// OIDC Authentication with full JWT validation
async fn oidc_auth(state: &AuthState, request: Request, next: Next) -> Response {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(auth) if auth.starts_with("Bearer ") => {
            let token = &auth[7..];

            if token.is_empty() {
                warn!("Empty Bearer token");
                return unauthorized_response("Empty token");
            }

            // Validate token
            match &state.oidc_validator {
                Some(validator) => {
                    match validator.validate_token(token).await {
                        Ok(claims) => {
                            debug!(
                                sub = %claims.sub,
                                email = ?claims.email,
                                "OIDC token validated"
                            );
                            // Token is valid, proceed with request
                            // TODO: Could inject claims into request extensions for handlers to use
                            return next.run(request).await;
                        }
                        Err(e) => {
                            warn!(error = %e, "OIDC token validation failed");

                            // If signature verification failed, try refreshing JWKS once
                            if e.contains("signature") || e.contains("key") {
                                debug!("Attempting JWKS refresh due to potential key rotation");
                                if validator.refresh_jwks().await.is_ok() {
                                    // Retry validation with fresh keys
                                    if let Ok(claims) = validator.validate_token(token).await {
                                        debug!(
                                            sub = %claims.sub,
                                            "OIDC token validated after JWKS refresh"
                                        );
                                        return next.run(request).await;
                                    }
                                }
                            }

                            return unauthorized_response(&e);
                        }
                    }
                }
                None => {
                    error!("OIDC validator not configured");
                    return unauthorized_response("OIDC not configured");
                }
            }
        }
        _ => {
            warn!("No Bearer token in Authorization header");
        }
    }

    unauthorized_response("No valid Bearer token")
}

/// OIDC Authorization Code Flow authentication.
///
/// Checks in order:
/// 1. Session cookie (fc_session) -- for browser sessions
/// 2. Bearer token -- for API client fallback
/// 3. If browser request (Accept: text/html) -- redirect to login
/// 4. If API request -- return 401 with X-Auth-Mode header
async fn oidc_flow_auth(state: &AuthState, request: Request, next: Next) -> Response {
    // 1. Check session cookie
    #[cfg(feature = "oidc-flow")]
    {
        if let Some(ref flow_state) = state.oidc_flow_state {
            if let Some(session_id) = crate::api::oidc_flow::extract_session_cookie(request.headers()) {
                if let Some(claims) = flow_state.session_store.get(&session_id) {
                    debug!(
                        sub = %claims.sub,
                        "OIDC flow: authenticated via session cookie"
                    );
                    return next.run(request).await;
                }
                debug!("OIDC flow: session cookie present but session not found or expired");
            }
        }
    }

    // 2. Check Bearer token (API client fallback)
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    if let Some(ref auth) = auth_header {
        if auth.starts_with("Bearer ") {
            let token = &auth[7..];
            if !token.is_empty() {
                if let Some(ref validator) = state.oidc_validator {
                    match validator.validate_token(token).await {
                        Ok(claims) => {
                            debug!(
                                sub = %claims.sub,
                                "OIDC flow: authenticated via Bearer token"
                            );
                            return next.run(request).await;
                        }
                        Err(e) => {
                            // Try JWKS refresh on signature/key errors
                            if e.contains("signature") || e.contains("key") {
                                if validator.refresh_jwks().await.is_ok() {
                                    if let Ok(claims) = validator.validate_token(token).await {
                                        debug!(
                                            sub = %claims.sub,
                                            "OIDC flow: authenticated via Bearer token after JWKS refresh"
                                        );
                                        return next.run(request).await;
                                    }
                                }
                            }
                            debug!(error = %e, "OIDC flow: Bearer token validation failed");
                        }
                    }
                }
            }
        }
    }

    // 3. Determine if this is a browser request
    let accept_header = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let is_browser = accept_header.contains("text/html");

    if is_browser {
        // Redirect to login with the original URL
        let path = request.uri().path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");
        let login_url = format!(
            "/auth/login?redirect_to={}",
            urlencoding::encode(path)
        );
        debug!(
            path = %path,
            "OIDC flow: browser request without session, redirecting to login"
        );
        return axum::response::Redirect::temporary(&login_url).into_response();
    }

    // 4. API request without valid credentials
    warn!("OIDC flow: API request without valid credentials");
    let mut response = (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "unauthorized",
            "message": "Authentication required. Use Bearer token or authenticate via /auth/login."
        }))
    ).into_response();

    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"FlowCatalyst\""),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-auth-mode"),
        HeaderValue::from_static("OIDC_FLOW"),
    );
    response
}

/// Create an unauthorized response
fn unauthorized_response(message: &str) -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "unauthorized",
            "message": message
        }))
    ).into_response();

    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"FlowCatalyst\""),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-auth-mode"),
        HeaderValue::from_static("OIDC"),
    );
    response
}

/// Create authentication state for use with middleware
pub fn create_auth_state(config: AuthConfig) -> AuthState {
    AuthState::new(config)
}

/// List of paths that should be public (no authentication)
pub fn is_public_path(path: &str) -> bool {
    matches!(
        path,
        "/health"
            | "/health/live"
            | "/health/ready"
            | "/health/startup"
            | "/q/health"
            | "/q/health/live"
            | "/q/health/ready"
            | "/metrics"
            | "/q/metrics"
            | "/swagger-ui"
            | "/swagger-ui/"
            | "/api-doc/openapi.json"
            | "/auth/login"
            | "/auth/callback"
            | "/auth/logout"
    ) || path.starts_with("/swagger-ui/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_mode_default() {
        let config = AuthConfig::default();
        assert_eq!(config.mode, AuthMode::None);
    }

    #[test]
    fn test_basic_auth_config() {
        let config = AuthConfig::basic("admin", "secret");
        assert_eq!(config.mode, AuthMode::Basic);
        assert_eq!(config.basic_username, Some("admin".to_string()));
        assert_eq!(config.basic_password, Some("secret".to_string()));
    }

    #[test]
    fn test_oidc_config() {
        let config = AuthConfig::oidc(
            "https://login.microsoftonline.com/tenant/v2.0",
            "client-id",
            "api://client-id"
        );
        assert_eq!(config.mode, AuthMode::Oidc);
        assert_eq!(config.oidc_issuer, Some("https://login.microsoftonline.com/tenant/v2.0".to_string()));
        assert_eq!(config.oidc_client_id, Some("client-id".to_string()));
        assert_eq!(config.oidc_audience, Some("api://client-id".to_string()));
    }

    #[test]
    fn test_public_paths() {
        assert!(is_public_path("/health"));
        assert!(is_public_path("/health/live"));
        assert!(is_public_path("/health/ready"));
        assert!(is_public_path("/metrics"));
        assert!(!is_public_path("/monitoring/health"));
        assert!(!is_public_path("/warnings"));
    }
}
