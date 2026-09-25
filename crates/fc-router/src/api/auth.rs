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
use tracing::{debug, error, info, warn};

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
    pub fn oidc(
        issuer: impl Into<String>,
        client_id: impl Into<String>,
        audience: impl Into<String>,
    ) -> Self {
        Self {
            mode: AuthMode::Oidc,
            oidc_issuer: Some(issuer.into()),
            oidc_client_id: Some(client_id.into()),
            oidc_audience: Some(audience.into()),
            ..Default::default()
        }
    }

    /// Create config from environment variables.
    ///
    /// BasicAuth credentials accept `FC_ROUTER_AUTH_USER`/`FC_ROUTER_AUTH_PASS`
    /// (the canonical Go-dialect names — `internal/router/api` `resolveRouterAuth`)
    /// first, falling back to the historical `AUTH_BASIC_USERNAME`/`AUTH_BASIC_PASSWORD`.
    /// `AUTH_MODE=NONE` (case-insensitive) always disables auth, matching Go.
    /// When `AUTH_MODE` is unset (or unrecognized) and credentials are present,
    /// mode is inferred as `Basic` — a Go-dialect ECS task definition sets the
    /// credentials but never sets `AUTH_MODE=BASIC` explicitly, so requiring it
    /// here would silently leave auth off for a drop-in deployment. An explicit
    /// `AUTH_MODE=OIDC`/`OIDC_FLOW` still wins over inferred Basic.
    pub fn from_env() -> Self {
        let basic_username =
            fc_common::config::env_first_opt(&["FC_ROUTER_AUTH_USER", "AUTH_BASIC_USERNAME"]);
        let basic_password =
            fc_common::config::env_first_opt(&["FC_ROUTER_AUTH_PASS", "AUTH_BASIC_PASSWORD"]);

        // Trimmed and case-insensitive, as Go's `resolveRouterAuth` reads it.
        let mode = match std::env::var("AUTH_MODE")
            .ok()
            .as_deref()
            .map(|m| m.trim().to_uppercase())
        {
            Some(ref m) if m == "NONE" => AuthMode::None,
            Some(ref m) if m == "BASIC" => AuthMode::Basic,
            Some(ref m) if m == "OIDC" => AuthMode::Oidc,
            Some(ref m) if m == "OIDC_FLOW" => AuthMode::OidcFlow,
            _ => {
                // AUTH_MODE unset or unrecognized: infer Basic when credentials
                // are present (Go-dialect drop-in), else stay fully open.
                if basic_username.as_deref().is_some_and(|u| !u.is_empty()) {
                    AuthMode::Basic
                } else {
                    AuthMode::None
                }
            }
        };

        Self {
            mode,
            basic_username,
            basic_password,
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
    n: Option<String>, // RSA modulus
    e: Option<String>, // RSA exponent
    x: Option<String>, // EC x coordinate
    y: Option<String>, // EC y coordinate
}

/// Cached JWKS with expiration
struct CachedJwks {
    jwks: Jwks,
    fetched_at: Instant,
}

/// Why [`OidcValidator::validate_token`] (or a JWKS fetch) failed.
///
/// The `Display` text is returned verbatim as the `message` of the 401
/// body, so each variant's message is part of the HTTP contract.
#[derive(Debug, thiserror::Error)]
pub enum TokenValidationError {
    #[error("Failed to fetch OIDC discovery: {0}")]
    DiscoveryFetch(#[source] reqwest::Error),
    #[error("OIDC discovery returned status: {0}")]
    DiscoveryStatus(reqwest::StatusCode),
    #[error("Failed to parse OIDC discovery: {0}")]
    DiscoveryParse(#[source] reqwest::Error),
    #[error("Failed to fetch JWKS: {0}")]
    JwksFetch(#[source] reqwest::Error),
    #[error("JWKS fetch returned status: {0}")]
    JwksStatus(reqwest::StatusCode),
    #[error("Failed to parse JWKS: {0}")]
    JwksParse(#[source] reqwest::Error),
    #[error("{kty} key missing '{component}' component")]
    MissingKeyComponent {
        kty: &'static str,
        component: &'static str,
    },
    #[error("Failed to create {kty} decoding key: {source}")]
    DecodingKey {
        kty: &'static str,
        #[source]
        source: jsonwebtoken::errors::Error,
    },
    #[error("Unsupported key type: {0}")]
    UnsupportedKeyType(String),
    #[error("Failed to decode token header: {0}")]
    Header(#[source] jsonwebtoken::errors::Error),
    #[error("No matching key found for kid: {0:?}")]
    NoMatchingKey(Option<String>),
    #[error("Token validation failed: {0}")]
    Invalid(#[source] jsonwebtoken::errors::Error),
}

impl TokenValidationError {
    /// Whether the failure concerns the signing key, so a JWKS refresh and
    /// one retry might succeed (key rotation).
    pub fn is_key_error(&self) -> bool {
        use jsonwebtoken::errors::ErrorKind;
        match self {
            Self::MissingKeyComponent { .. }
            | Self::DecodingKey { .. }
            | Self::UnsupportedKeyType(_)
            | Self::NoMatchingKey(_) => true,
            Self::Header(e) | Self::Invalid(e) => matches!(e.kind(), ErrorKind::InvalidRsaKey(_)),
            _ => false,
        }
    }
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
    /// Create a new OIDC validator.
    ///
    /// Stores `issuer` with any trailing `/` stripped so validation accepts
    /// both forms (`https://idp.example.com` and `https://idp.example.com/`).
    /// IdPs are inconsistent about which form they emit in `iss` claims;
    /// normalizing on the consumer side avoids config-drift 401s.
    pub fn new(issuer: String, audience: String) -> Self {
        Self {
            issuer: issuer.trim_end_matches('/').to_string(),
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
    async fn fetch_discovery(&self) -> Result<OidcDiscovery, TokenValidationError> {
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            self.issuer.trim_end_matches('/')
        );

        debug!(url = %discovery_url, "Fetching OIDC discovery document");

        let response = self
            .http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(TokenValidationError::DiscoveryFetch)?;

        if !response.status().is_success() {
            return Err(TokenValidationError::DiscoveryStatus(response.status()));
        }

        response
            .json::<OidcDiscovery>()
            .await
            .map_err(TokenValidationError::DiscoveryParse)
    }

    /// Fetch JWKS from the issuer
    async fn fetch_jwks(&self) -> Result<Jwks, TokenValidationError> {
        let discovery = self.fetch_discovery().await?;

        debug!(jwks_uri = %discovery.jwks_uri, "Fetching JWKS");

        let response = self
            .http_client
            .get(&discovery.jwks_uri)
            .send()
            .await
            .map_err(TokenValidationError::JwksFetch)?;

        if !response.status().is_success() {
            return Err(TokenValidationError::JwksStatus(response.status()));
        }

        response
            .json::<Jwks>()
            .await
            .map_err(TokenValidationError::JwksParse)
    }

    /// Get JWKS, using cache if valid
    async fn get_jwks(&self) -> Result<Jwks, TokenValidationError> {
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
    fn jwk_to_decoding_key(&self, jwk: &Jwk) -> Result<DecodingKey, TokenValidationError> {
        fn component<'a>(
            value: &'a Option<String>,
            kty: &'static str,
            component: &'static str,
        ) -> Result<&'a str, TokenValidationError> {
            value
                .as_deref()
                .ok_or(TokenValidationError::MissingKeyComponent { kty, component })
        }
        fn decoding_key(
            kty: &'static str,
        ) -> impl FnOnce(jsonwebtoken::errors::Error) -> TokenValidationError {
            move |source| TokenValidationError::DecodingKey { kty, source }
        }

        match jwk.kty.as_str() {
            "RSA" => {
                let n = component(&jwk.n, "RSA", "n")?;
                let e = component(&jwk.e, "RSA", "e")?;
                DecodingKey::from_rsa_components(n, e).map_err(decoding_key("RSA"))
            }
            "EC" => {
                let x = component(&jwk.x, "EC", "x")?;
                let y = component(&jwk.y, "EC", "y")?;
                DecodingKey::from_ec_components(x, y).map_err(decoding_key("EC"))
            }
            other => Err(TokenValidationError::UnsupportedKeyType(other.to_string())),
        }
    }

    /// Validate a JWT token
    pub async fn validate_token(&self, token: &str) -> Result<TokenClaims, TokenValidationError> {
        // Decode the header to get the key ID
        let header = decode_header(token).map_err(TokenValidationError::Header)?;

        // Get JWKS and find the matching key. On `kid`-miss, force-refresh
        // the cache once and retry — this is the standard pattern for key
        // rotation: the IdP advertises a new key, but our 1h cache still
        // holds only the old one. Without the refetch, validation 401s
        // for up to an hour after each rotation.
        let jwks = self.get_jwks().await?;
        let jwk = match self.find_key(&jwks, header.kid.as_deref()) {
            Some(k) => k.clone(),
            None => {
                warn!(
                    kid = ?header.kid,
                    "kid not in cached JWKS — forcing refresh and retrying"
                );
                self.refresh_jwks().await?;
                let jwks = self.get_jwks().await?;
                self.find_key(&jwks, header.kid.as_deref())
                    .ok_or_else(|| TokenValidationError::NoMatchingKey(header.kid.clone()))?
                    .clone()
            }
        };

        // Create decoding key
        let decoding_key = self.jwk_to_decoding_key(&jwk)?;

        // Determine algorithm - use from header, or infer from JWK
        let algorithm = header.alg;

        // Set up validation. Pass both trailing-slash forms of the issuer
        // so a config / IdP-emit mismatch doesn't 401 — see `Self::new`.
        let issuer_with_slash = format!("{}/", self.issuer);
        let mut validation = Validation::new(algorithm);
        validation.set_issuer(&[&self.issuer, &issuer_with_slash]);
        validation.set_audience(&[&self.audience]);
        validation.validate_exp = true;
        validation.validate_nbf = true;

        // Decode and validate
        let token_data = decode::<TokenClaims>(token, &decoding_key, &validation)
            .map_err(TokenValidationError::Invalid)?;

        debug!(
            sub = %token_data.claims.sub,
            "Token validated successfully"
        );

        Ok(token_data.claims)
    }

    /// Force refresh the JWKS cache (e.g., on signature verification failure)
    pub async fn refresh_jwks(&self) -> Result<(), TokenValidationError> {
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
        // Java: OidcDiagnostics — log auth/OIDC configuration at startup
        info!(
            mode = ?config.mode,
            oidc_issuer = config.oidc_issuer.as_deref().unwrap_or("<not set>"),
            oidc_client_id = config.oidc_client_id.as_deref().unwrap_or("<not set>"),
            oidc_client_secret = if config.oidc_client_secret.is_some() { "****" } else { "<not set>" },
            oidc_audience = config.oidc_audience.as_deref().unwrap_or("<not set>"),
            "OIDC diagnostics: authentication configuration"
        );

        let oidc_validator = if config.mode == AuthMode::Oidc || config.mode == AuthMode::OidcFlow {
            if let (Some(issuer), Some(audience)) = (&config.oidc_issuer, &config.oidc_audience) {
                Some(Arc::new(OidcValidator::new(
                    issuer.clone(),
                    audience.clone(),
                )))
            } else {
                warn!("OIDC mode enabled but missing issuer or audience configuration");
                None
            }
        } else {
            None
        };

        #[cfg(feature = "oidc-flow")]
        let oidc_flow_state = if config.mode == AuthMode::OidcFlow {
            use crate::api::oidc_flow::{
                OidcFlowConfig, OidcFlowState, PendingOidcStateStore, SessionStore,
            };

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

                let session_store = Arc::new(SessionStore::new(std::time::Duration::from_secs(
                    session_ttl_seconds,
                )));

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
                            if e.is_key_error() {
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

                            return unauthorized_response(&e.to_string());
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
            if let Some(session_id) =
                crate::api::oidc_flow::extract_session_cookie(request.headers())
            {
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
        if let Some(token) = auth.strip_prefix("Bearer ") {
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
                            if e.is_key_error() && validator.refresh_jwks().await.is_ok() {
                                if let Ok(claims) = validator.validate_token(token).await {
                                    debug!(
                                        sub = %claims.sub,
                                        "OIDC flow: authenticated via Bearer token after JWKS refresh"
                                    );
                                    return next.run(request).await;
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
        let path = request
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");
        let login_url = format!("/auth/login?redirect_to={}", urlencoding::encode(path));
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
        })),
    )
        .into_response();

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
        })),
    )
        .into_response();

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
            "api://client-id",
        );
        assert_eq!(config.mode, AuthMode::Oidc);
        assert_eq!(
            config.oidc_issuer,
            Some("https://login.microsoftonline.com/tenant/v2.0".to_string())
        );
        assert_eq!(config.oidc_client_id, Some("client-id".to_string()));
        assert_eq!(config.oidc_audience, Some("api://client-id".to_string()));
    }

    #[test]
    fn test_oidc_validator_normalizes_trailing_slash() {
        let with_slash = OidcValidator::new(
            "https://idp.example.com/realms/foo/".to_string(),
            "fc-router".to_string(),
        );
        let without_slash = OidcValidator::new(
            "https://idp.example.com/realms/foo".to_string(),
            "fc-router".to_string(),
        );
        assert_eq!(with_slash.issuer, without_slash.issuer);
        assert_eq!(with_slash.issuer, "https://idp.example.com/realms/foo");
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
