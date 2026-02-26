//! OIDC Authorization Code Flow
//!
//! Implements the full OIDC authorization code flow with:
//! - PKCE (Proof Key for Code Exchange) for security
//! - In-memory session store with TTL-based cleanup
//! - Login, callback, and logout handlers
//! - Cookie-based session management
//!
//! This module is gated behind the `oidc-flow` feature flag.

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL, Engine};
use dashmap::DashMap;
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use super::auth::{OidcValidator, TokenClaims};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the OIDC Authorization Code Flow.
#[derive(Debug, Clone)]
pub struct OidcFlowConfig {
    /// OIDC issuer URL (e.g., `https://login.microsoftonline.com/{tenant}/v2.0`)
    pub issuer_url: String,
    /// OAuth2 client ID
    pub client_id: String,
    /// OAuth2 client secret (required for confidential clients)
    pub client_secret: Option<String>,
    /// Redirect URI registered with the identity provider
    pub redirect_uri: String,
    /// Scopes to request (e.g., `["openid", "profile", "email"]`)
    pub scopes: Vec<String>,
    /// Session TTL in seconds (default: 3600)
    pub session_ttl_seconds: u64,
}

// ---------------------------------------------------------------------------
// Session Store
// ---------------------------------------------------------------------------

/// In-memory session data.
struct SessionData {
    claims: TokenClaims,
    created_at: Instant,
}

/// Thread-safe in-memory session store with TTL-based expiration.
pub struct SessionStore {
    sessions: DashMap<String, SessionData>,
    ttl: Duration,
}

impl SessionStore {
    /// Create a new session store with the given TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            sessions: DashMap::new(),
            ttl,
        }
    }

    /// Insert a session.
    pub fn insert(&self, session_id: String, claims: TokenClaims) {
        self.sessions.insert(session_id, SessionData {
            claims,
            created_at: Instant::now(),
        });
    }

    /// Get claims for a session, returning `None` if expired or absent.
    pub fn get(&self, session_id: &str) -> Option<TokenClaims> {
        let entry = self.sessions.get(session_id)?;
        if entry.created_at.elapsed() > self.ttl {
            // Expired - drop the ref before removing
            drop(entry);
            self.sessions.remove(session_id);
            None
        } else {
            Some(entry.claims.clone())
        }
    }

    /// Remove a session.
    pub fn remove(&self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    /// Remove all expired sessions.
    pub fn cleanup(&self) {
        let ttl = self.ttl;
        self.sessions.retain(|_, v| v.created_at.elapsed() < ttl);
    }
}

// ---------------------------------------------------------------------------
// Pending OIDC State Store
// ---------------------------------------------------------------------------

/// State stored while the user is redirected to the IdP.
struct PendingState {
    pkce_verifier: String,
    nonce: String,
    original_url: String,
    created_at: Instant,
}

/// Store for pending OIDC authorization requests (5-minute TTL).
pub struct PendingOidcStateStore {
    states: DashMap<String, PendingState>,
}

impl PendingOidcStateStore {
    const TTL: Duration = Duration::from_secs(300); // 5 minutes

    pub fn new() -> Self {
        Self {
            states: DashMap::new(),
        }
    }

    fn insert(&self, state: String, pkce_verifier: String, nonce: String, original_url: String) {
        self.states.insert(state, PendingState {
            pkce_verifier,
            nonce,
            original_url,
            created_at: Instant::now(),
        });
    }

    fn take(&self, state: &str) -> Option<(String, String, String)> {
        let (_, pending) = self.states.remove(state)?;
        if pending.created_at.elapsed() > Self::TTL {
            None
        } else {
            Some((pending.pkce_verifier, pending.nonce, pending.original_url))
        }
    }

    /// Remove all expired pending states.
    pub fn cleanup(&self) {
        self.states.retain(|_, v| v.created_at.elapsed() < Self::TTL);
    }
}

// ---------------------------------------------------------------------------
// Shared State
// ---------------------------------------------------------------------------

/// Shared state for the OIDC flow handlers.
pub struct OidcFlowState {
    pub config: OidcFlowConfig,
    pub session_store: Arc<SessionStore>,
    pub pending_states: Arc<PendingOidcStateStore>,
    pub http_client: reqwest::Client,
    /// Re-use the existing OIDC validator for token validation
    pub oidc_validator: Option<Arc<OidcValidator>>,
}

// ---------------------------------------------------------------------------
// Token Exchange Types
// ---------------------------------------------------------------------------

/// Token endpoint response.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
    access_token: Option<String>,
    #[allow(dead_code)]
    token_type: Option<String>,
    #[allow(dead_code)]
    expires_in: Option<u64>,
}

/// OIDC Discovery document (subset).
#[derive(Debug, Deserialize)]
struct OidcDiscoveryDoc {
    authorization_endpoint: String,
    token_endpoint: String,
}

// ---------------------------------------------------------------------------
// Query Parameters
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LoginQuery {
    redirect_to: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

// ---------------------------------------------------------------------------
// Helper Functions
// ---------------------------------------------------------------------------

/// Generate a cryptographically random alphanumeric string.
fn generate_random_string(len: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..62);
            match idx {
                0..=9 => (b'0' + idx) as char,
                10..=35 => (b'a' + idx - 10) as char,
                36..=61 => (b'A' + idx - 36) as char,
                _ => unreachable!(),
            }
        })
        .collect()
}

/// Compute the S256 PKCE code challenge from a verifier.
fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    BASE64URL.encode(digest)
}

/// Extract the `fc_session` cookie value from request headers.
pub fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .map(|cookie| cookie.trim())
        .find_map(|cookie| {
            if let Some(val) = cookie.strip_prefix("fc_session=") {
                Some(val.to_string())
            } else {
                None
            }
        })
}

/// Fetch the OIDC discovery document from the issuer.
async fn fetch_discovery(
    http_client: &reqwest::Client,
    issuer_url: &str,
) -> Result<OidcDiscoveryDoc, String> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer_url.trim_end_matches('/')
    );

    debug!(url = %url, "Fetching OIDC discovery document for flow");

    let response = http_client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch OIDC discovery: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "OIDC discovery returned status: {}",
            response.status()
        ));
    }

    response
        .json::<OidcDiscoveryDoc>()
        .await
        .map_err(|e| format!("Failed to parse OIDC discovery: {}", e))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /auth/login` -- Initiate the OIDC authorization code flow.
///
/// Generates PKCE challenge, state, and nonce, then redirects the user
/// to the identity provider's authorization endpoint.
async fn login_handler(
    State(state): State<Arc<OidcFlowState>>,
    Query(query): Query<LoginQuery>,
) -> Response {
    let original_url = query.redirect_to.unwrap_or_else(|| "/".to_string());

    // Generate PKCE verifier and challenge
    let pkce_verifier = generate_random_string(64);
    let code_challenge = pkce_challenge(&pkce_verifier);

    // Generate state and nonce
    let state_param = generate_random_string(32);
    let nonce = generate_random_string(32);

    // Store pending state
    state.pending_states.insert(
        state_param.clone(),
        pkce_verifier,
        nonce.clone(),
        original_url,
    );

    // Fetch discovery document to get the authorization endpoint
    let discovery = match fetch_discovery(&state.http_client, &state.config.issuer_url).await {
        Ok(doc) => doc,
        Err(e) => {
            error!(error = %e, "Failed to fetch OIDC discovery for login");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "oidc_discovery_failed",
                    "message": e,
                })),
            )
                .into_response();
        }
    };

    // Build scopes string
    let scopes = state.config.scopes.join(" ");

    // Build authorization URL
    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        discovery.authorization_endpoint,
        urlencoding::encode(&state.config.client_id),
        urlencoding::encode(&state.config.redirect_uri),
        urlencoding::encode(&scopes),
        urlencoding::encode(&state_param),
        urlencoding::encode(&nonce),
        urlencoding::encode(&code_challenge),
    );

    debug!(
        auth_url = %auth_url,
        "Redirecting to authorization endpoint"
    );

    Redirect::temporary(&auth_url).into_response()
}

/// `GET /auth/callback` -- Handle the OIDC callback after user authenticates.
///
/// Exchanges the authorization code for tokens, validates the ID token,
/// creates a session, and redirects the user to the original URL.
async fn callback_handler(
    State(state): State<Arc<OidcFlowState>>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    // Look up pending state
    let (pkce_verifier, expected_nonce, original_url) =
        match state.pending_states.take(&query.state) {
            Some(pending) => pending,
            None => {
                warn!(state = %query.state, "Unknown or expired OIDC state parameter");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid_state",
                        "message": "Unknown or expired state parameter. Please try logging in again.",
                    })),
                )
                    .into_response();
            }
        };

    // Fetch discovery document to get the token endpoint
    let discovery = match fetch_discovery(&state.http_client, &state.config.issuer_url).await {
        Ok(doc) => doc,
        Err(e) => {
            error!(error = %e, "Failed to fetch OIDC discovery for callback");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "oidc_discovery_failed",
                    "message": e,
                })),
            )
                .into_response();
        }
    };

    // Exchange authorization code for tokens
    let mut token_params = vec![
        ("grant_type", "authorization_code"),
        ("code", &query.code),
        ("redirect_uri", &state.config.redirect_uri),
        ("client_id", &state.config.client_id),
        ("code_verifier", &pkce_verifier),
    ];

    // Include client_secret if present (confidential client)
    let client_secret_ref;
    if let Some(ref secret) = state.config.client_secret {
        client_secret_ref = secret.clone();
        token_params.push(("client_secret", &client_secret_ref));
    }

    let token_response = match state
        .http_client
        .post(&discovery.token_endpoint)
        .form(&token_params)
        .send()
        .await
    {
        Ok(resp) => {
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                error!(
                    status = %status,
                    body = %body,
                    "Token exchange failed"
                );
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": "token_exchange_failed",
                        "message": format!("Token endpoint returned {}", status),
                    })),
                )
                    .into_response();
            }

            match resp.json::<TokenResponse>().await {
                Ok(token_resp) => token_resp,
                Err(e) => {
                    error!(error = %e, "Failed to parse token response");
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({
                            "error": "token_parse_failed",
                            "message": format!("Failed to parse token response: {}", e),
                        })),
                    )
                        .into_response();
                }
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to exchange authorization code");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "token_exchange_error",
                    "message": format!("Failed to contact token endpoint: {}", e),
                })),
            )
                .into_response();
        }
    };

    // Get the ID token (prefer id_token, fall back to access_token)
    let id_token = match token_response.id_token.or(token_response.access_token) {
        Some(token) => token,
        None => {
            error!("Token response contained neither id_token nor access_token");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "no_token",
                    "message": "Token response did not contain an id_token or access_token",
                })),
            )
                .into_response();
        }
    };

    // Validate the ID token using our existing OidcValidator
    let claims = if let Some(ref validator) = state.oidc_validator {
        match validator.validate_token(&id_token).await {
            Ok(claims) => claims,
            Err(e) => {
                // Try refreshing JWKS once (key rotation)
                if e.contains("signature") || e.contains("key") {
                    debug!("Attempting JWKS refresh for callback token validation");
                    if validator.refresh_jwks().await.is_ok() {
                        match validator.validate_token(&id_token).await {
                            Ok(claims) => claims,
                            Err(e2) => {
                                warn!(error = %e2, "ID token validation failed after JWKS refresh");
                                return (
                                    StatusCode::UNAUTHORIZED,
                                    Json(serde_json::json!({
                                        "error": "token_validation_failed",
                                        "message": e2,
                                    })),
                                )
                                    .into_response();
                            }
                        }
                    } else {
                        warn!(error = %e, "ID token validation failed and JWKS refresh also failed");
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(serde_json::json!({
                                "error": "token_validation_failed",
                                "message": e,
                            })),
                        )
                            .into_response();
                    }
                } else {
                    warn!(error = %e, "ID token validation failed");
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({
                            "error": "token_validation_failed",
                            "message": e,
                        })),
                    )
                        .into_response();
                }
            }
        }
    } else {
        // No validator configured - decode without signature verification
        // This should not happen in production but allows for testing
        warn!("No OIDC validator configured; decoding ID token without signature verification");
        match decode_claims_insecure(&id_token) {
            Ok(claims) => claims,
            Err(e) => {
                error!(error = %e, "Failed to decode ID token claims");
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": "token_decode_failed",
                        "message": e,
                    })),
                )
                    .into_response();
            }
        }
    };

    // Validate nonce if present in claims (extra security check)
    // Note: The nonce claim is not in our standard TokenClaims struct, so we
    // do a best-effort check by re-decoding the JWT payload.
    if let Err(e) = validate_nonce(&id_token, &expected_nonce) {
        warn!(error = %e, "Nonce validation failed");
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "nonce_mismatch",
                "message": e,
            })),
        )
            .into_response();
    }

    // Create session
    let session_id = generate_random_string(48);
    state.session_store.insert(session_id.clone(), claims.clone());

    info!(
        sub = %claims.sub,
        email = ?claims.email,
        "OIDC flow: session created"
    );

    // Build redirect response with session cookie
    let cookie_value = format!(
        "fc_session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        session_id, state.config.session_ttl_seconds
    );

    let mut response = Redirect::temporary(&original_url).into_response();
    if let Ok(cookie_header) = cookie_value.parse() {
        response
            .headers_mut()
            .insert(header::SET_COOKIE, cookie_header);
    }

    response
}

/// `GET /auth/logout` -- Destroy the session and clear the cookie.
async fn logout_handler(
    State(state): State<Arc<OidcFlowState>>,
    headers: HeaderMap,
) -> Response {
    if let Some(session_id) = extract_session_cookie(&headers) {
        state.session_store.remove(&session_id);
        debug!(session_id = %session_id, "Session removed on logout");
    }

    // Clear the session cookie
    let clear_cookie = "fc_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0";
    let mut response = (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "logged_out" })),
    )
        .into_response();

    if let Ok(cookie_header) = clear_cookie.parse() {
        response
            .headers_mut()
            .insert(header::SET_COOKIE, cookie_header);
    }

    response
}

// ---------------------------------------------------------------------------
// Nonce Validation
// ---------------------------------------------------------------------------

/// Validate the nonce in the ID token matches what we sent.
/// Decodes the JWT payload (without signature verification) to extract the nonce claim.
fn validate_nonce(id_token: &str, expected_nonce: &str) -> Result<(), String> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        return Err("ID token is not a valid JWT (expected 3 parts)".to_string());
    }

    let payload_bytes = BASE64URL
        .decode(parts[1])
        .or_else(|_| {
            // Try with padding
            base64::engine::general_purpose::URL_SAFE.decode(parts[1])
        })
        .map_err(|e| format!("Failed to base64url-decode ID token payload: {}", e))?;

    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes)
        .map_err(|e| format!("Failed to parse ID token payload as JSON: {}", e))?;

    if let Some(nonce) = payload.get("nonce").and_then(|v| v.as_str()) {
        if nonce != expected_nonce {
            return Err(format!(
                "Nonce mismatch: expected {}, got {}",
                expected_nonce, nonce
            ));
        }
    }
    // If no nonce claim in the token, that's acceptable for some providers

    Ok(())
}

/// Decode token claims without signature verification (fallback).
fn decode_claims_insecure(token: &str) -> Result<TokenClaims, String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("Token is not a valid JWT (expected 3 parts)".to_string());
    }

    let payload_bytes = BASE64URL
        .decode(parts[1])
        .or_else(|_| {
            base64::engine::general_purpose::URL_SAFE.decode(parts[1])
        })
        .map_err(|e| format!("Failed to base64url-decode token payload: {}", e))?;

    serde_json::from_slice::<TokenClaims>(&payload_bytes)
        .map_err(|e| format!("Failed to parse token claims: {}", e))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Create the OIDC flow routes.
///
/// Returns a `Router` with:
/// - `GET /auth/login` - Initiate login
/// - `GET /auth/callback` - Handle callback
/// - `GET /auth/logout` - Logout
pub fn oidc_flow_routes(state: Arc<OidcFlowState>) -> Router {
    Router::new()
        .route("/auth/login", get(login_handler))
        .route("/auth/callback", get(callback_handler))
        .route("/auth/logout", get(logout_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_random_string() {
        let s1 = generate_random_string(32);
        let s2 = generate_random_string(32);
        assert_eq!(s1.len(), 32);
        assert_eq!(s2.len(), 32);
        assert_ne!(s1, s2);
        // All characters should be alphanumeric
        assert!(s1.chars().all(|c| c.is_alphanumeric()));
    }

    #[test]
    fn test_pkce_challenge() {
        // Known test vector: the challenge for "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk" should be
        // "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM" (from RFC 7636)
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = pkce_challenge(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn test_extract_session_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "other=value; fc_session=abc123; another=test".parse().unwrap(),
        );
        assert_eq!(extract_session_cookie(&headers), Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_session_cookie_missing() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "other=value; another=test".parse().unwrap(),
        );
        assert_eq!(extract_session_cookie(&headers), None);
    }

    #[test]
    fn test_extract_session_cookie_empty() {
        let headers = HeaderMap::new();
        assert_eq!(extract_session_cookie(&headers), None);
    }

    #[test]
    fn test_session_store_insert_and_get() {
        let store = SessionStore::new(Duration::from_secs(60));
        let claims = TokenClaims {
            sub: "user-1".to_string(),
            iss: "https://example.com".to_string(),
            aud: serde_json::Value::Null,
            exp: 9999999999,
            iat: 0,
            nbf: 0,
            jti: None,
            email: Some("user@example.com".to_string()),
            name: Some("Test User".to_string()),
            preferred_username: None,
            oid: None,
            tid: None,
            roles: vec![],
            scp: None,
        };

        store.insert("session-1".to_string(), claims.clone());
        let retrieved = store.get("session-1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().sub, "user-1");
    }

    #[test]
    fn test_session_store_remove() {
        let store = SessionStore::new(Duration::from_secs(60));
        let claims = TokenClaims {
            sub: "user-1".to_string(),
            iss: "https://example.com".to_string(),
            aud: serde_json::Value::Null,
            exp: 9999999999,
            iat: 0,
            nbf: 0,
            jti: None,
            email: None,
            name: None,
            preferred_username: None,
            oid: None,
            tid: None,
            roles: vec![],
            scp: None,
        };

        store.insert("session-1".to_string(), claims);
        store.remove("session-1");
        assert!(store.get("session-1").is_none());
    }

    #[test]
    fn test_session_store_expired() {
        let store = SessionStore::new(Duration::from_secs(0)); // 0 TTL = immediately expired
        let claims = TokenClaims {
            sub: "user-1".to_string(),
            iss: "https://example.com".to_string(),
            aud: serde_json::Value::Null,
            exp: 9999999999,
            iat: 0,
            nbf: 0,
            jti: None,
            email: None,
            name: None,
            preferred_username: None,
            oid: None,
            tid: None,
            roles: vec![],
            scp: None,
        };

        store.insert("session-1".to_string(), claims);
        // Session should be expired immediately
        std::thread::sleep(Duration::from_millis(10));
        assert!(store.get("session-1").is_none());
    }

    #[test]
    fn test_pending_state_store() {
        let store = PendingOidcStateStore::new();
        store.insert(
            "state-1".to_string(),
            "verifier".to_string(),
            "nonce".to_string(),
            "/dashboard".to_string(),
        );

        let result = store.take("state-1");
        assert!(result.is_some());
        let (verifier, nonce, url) = result.unwrap();
        assert_eq!(verifier, "verifier");
        assert_eq!(nonce, "nonce");
        assert_eq!(url, "/dashboard");

        // Should be consumed (not available again)
        assert!(store.take("state-1").is_none());
    }

    #[test]
    fn test_validate_nonce_valid() {
        // Create a fake JWT with a nonce in the payload
        let header = BASE64URL.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = BASE64URL.encode(r#"{"sub":"user","nonce":"test-nonce-123"}"#);
        let signature = BASE64URL.encode("fake-signature");
        let token = format!("{}.{}.{}", header, payload, signature);

        assert!(validate_nonce(&token, "test-nonce-123").is_ok());
    }

    #[test]
    fn test_validate_nonce_mismatch() {
        let header = BASE64URL.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = BASE64URL.encode(r#"{"sub":"user","nonce":"actual-nonce"}"#);
        let signature = BASE64URL.encode("fake-signature");
        let token = format!("{}.{}.{}", header, payload, signature);

        assert!(validate_nonce(&token, "expected-nonce").is_err());
    }

    #[test]
    fn test_validate_nonce_no_nonce_claim() {
        // No nonce claim in payload - should be OK (some providers don't include it)
        let header = BASE64URL.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = BASE64URL.encode(r#"{"sub":"user"}"#);
        let signature = BASE64URL.encode("fake-signature");
        let token = format!("{}.{}.{}", header, payload, signature);

        assert!(validate_nonce(&token, "any-nonce").is_ok());
    }
}
