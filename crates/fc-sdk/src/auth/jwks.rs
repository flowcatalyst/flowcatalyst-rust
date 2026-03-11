//! JWKS Cache and JWT Token Validation
//!
//! Fetches FlowCatalyst's public keys via OIDC discovery and validates
//! JWT access tokens using RS256 signature verification.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tracing::{debug, warn};

use super::claims::{AccessTokenClaims, AuthContext};
use super::AuthError;

/// JWKS response from the provider.
#[derive(Debug, Clone, Deserialize)]
pub struct Jwks {
    pub keys: Vec<JwkKey>,
}

/// Individual JWK key.
#[derive(Debug, Clone, Deserialize)]
pub struct JwkKey {
    /// Key type (e.g., "RSA")
    pub kty: String,
    /// Key usage (e.g., "sig")
    #[serde(rename = "use")]
    pub key_use: Option<String>,
    /// Key ID
    pub kid: Option<String>,
    /// Algorithm (e.g., "RS256")
    pub alg: Option<String>,
    /// RSA modulus (base64url)
    pub n: Option<String>,
    /// RSA exponent (base64url)
    pub e: Option<String>,
}

/// Partial OIDC discovery document.
#[derive(Debug, Deserialize)]
struct DiscoveryDoc {
    jwks_uri: String,
    #[serde(default)]
    #[allow(dead_code)]
    issuer: Option<String>,
}

/// Cached JWKS entry.
struct CachedJwks {
    jwks: Jwks,
    fetched_at: DateTime<Utc>,
}

/// JWKS cache with per-issuer TTL.
///
/// Automatically discovers and caches FlowCatalyst's public keys
/// via the `.well-known/openid-configuration` endpoint.
pub struct JwksCache {
    cache: Arc<RwLock<HashMap<String, CachedJwks>>>,
    http_client: reqwest::Client,
    ttl_secs: i64,
}

impl JwksCache {
    /// Create a new JWKS cache with the given TTL (in seconds).
    pub fn new(ttl_secs: i64) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            ttl_secs,
        }
    }

    /// Get JWKS for an issuer, fetching from network if not cached or expired.
    pub async fn get_jwks(&self, issuer_url: &str) -> Result<Jwks, AuthError> {
        // Check cache
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(issuer_url) {
                let age = (Utc::now() - entry.fetched_at).num_seconds();
                if age < self.ttl_secs {
                    debug!(issuer = %issuer_url, age_secs = age, "JWKS cache hit");
                    return Ok(entry.jwks.clone());
                }
            }
        }

        // Fetch fresh
        let jwks = self.fetch_jwks(issuer_url).await?;

        // Store
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                issuer_url.to_string(),
                CachedJwks {
                    jwks: jwks.clone(),
                    fetched_at: Utc::now(),
                },
            );
        }

        Ok(jwks)
    }

    /// Fetch JWKS via OIDC discovery.
    async fn fetch_jwks(&self, issuer_url: &str) -> Result<Jwks, AuthError> {
        let base = issuer_url.trim_end_matches('/');
        let discovery_url = format!("{}/.well-known/openid-configuration", base);

        debug!(url = %discovery_url, "Fetching OIDC discovery document");

        let discovery: DiscoveryDoc = self
            .http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| AuthError::Discovery(format!("Failed to fetch discovery from {}: {}", discovery_url, e)))?
            .json()
            .await
            .map_err(|e| AuthError::Discovery(format!("Failed to parse discovery: {}", e)))?;

        debug!(jwks_uri = %discovery.jwks_uri, "Fetching JWKS");

        let jwks: Jwks = self
            .http_client
            .get(&discovery.jwks_uri)
            .send()
            .await
            .map_err(|e| AuthError::Discovery(format!("Failed to fetch JWKS from {}: {}", discovery.jwks_uri, e)))?
            .json()
            .await
            .map_err(|e| AuthError::Discovery(format!("Failed to parse JWKS: {}", e)))?;

        if jwks.keys.is_empty() {
            warn!(issuer = %issuer_url, "JWKS contains no keys");
        }

        debug!(issuer = %issuer_url, key_count = jwks.keys.len(), "JWKS fetched successfully");
        Ok(jwks)
    }

    /// Invalidate cached JWKS for a specific issuer (forces re-fetch on next use).
    pub async fn invalidate(&self, issuer_url: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(issuer_url);
    }
}

impl Default for JwksCache {
    fn default() -> Self {
        Self::new(3600) // 1 hour
    }
}

/// Validates JWT access tokens issued by a FlowCatalyst OIDC server.
///
/// Uses JWKS auto-discovery to fetch and cache the server's public keys,
/// then validates token signatures, expiry, issuer, and audience.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::auth::{TokenValidator, TokenValidatorConfig};
///
/// let validator = TokenValidator::new(TokenValidatorConfig {
///     issuer_url: "https://auth.flowcatalyst.io".to_string(),
///     audience: "my-app".to_string(),
///     ..Default::default()
/// });
///
/// // Validate a Bearer token
/// let auth_ctx = validator.validate("eyJ...").await?;
/// println!("Hello, {}", auth_ctx.name());
///
/// if auth_ctx.has_role("admin") {
///     // Authorized
/// }
/// ```
pub struct TokenValidator {
    config: TokenValidatorConfig,
    jwks_cache: JwksCache,
}

/// Configuration for the token validator.
#[derive(Debug, Clone)]
pub struct TokenValidatorConfig {
    /// FlowCatalyst OIDC server URL (e.g., `"https://auth.flowcatalyst.io"`)
    pub issuer_url: String,

    /// Expected audience claim (your application identifier)
    pub audience: String,

    /// JWKS cache TTL in seconds (default: 3600 = 1 hour)
    pub jwks_ttl_secs: i64,

    /// Allowed clock skew in seconds for exp/nbf validation (default: 60)
    pub clock_skew_secs: u64,
}

impl Default for TokenValidatorConfig {
    fn default() -> Self {
        Self {
            issuer_url: String::new(),
            audience: "flowcatalyst".to_string(),
            jwks_ttl_secs: 3600,
            clock_skew_secs: 60,
        }
    }
}

impl TokenValidator {
    /// Create a new token validator.
    pub fn new(config: TokenValidatorConfig) -> Self {
        let jwks_cache = JwksCache::new(config.jwks_ttl_secs);
        Self { config, jwks_cache }
    }

    /// Validate a JWT access token and return an [`AuthContext`].
    ///
    /// Performs:
    /// 1. Decode JWT header to find the key ID (`kid`)
    /// 2. Fetch JWKS from the issuer (cached)
    /// 3. Find matching RSA public key
    /// 4. Verify RS256 signature
    /// 5. Validate claims: `iss`, `aud`, `exp`, `nbf`
    /// 6. Return parsed [`AuthContext`] with claims
    pub async fn validate(&self, token: &str) -> Result<AuthContext, AuthError> {
        // Decode header to get kid
        let header = jsonwebtoken::decode_header(token)
            .map_err(|e| AuthError::InvalidToken(format!("Invalid JWT header: {}", e)))?;

        let kid = header.kid.as_deref();

        // Fetch JWKS
        let jwks = self.jwks_cache.get_jwks(&self.config.issuer_url).await?;

        // Find matching key
        let key = self.find_key(&jwks, kid)?;

        // Build decoding key from RSA components
        let n = key.n.as_deref().ok_or_else(|| {
            AuthError::InvalidToken("JWK missing RSA modulus (n)".to_string())
        })?;
        let e = key.e.as_deref().ok_or_else(|| {
            AuthError::InvalidToken("JWK missing RSA exponent (e)".to_string())
        })?;

        let decoding_key = DecodingKey::from_rsa_components(n, e)
            .map_err(|e| AuthError::InvalidToken(format!("Invalid RSA components: {}", e)))?;

        // Validate
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.config.issuer_url]);
        validation.set_audience(&[&self.config.audience]);
        validation.leeway = self.config.clock_skew_secs;

        let token_data = decode::<AccessTokenClaims>(token, &decoding_key, &validation)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                jsonwebtoken::errors::ErrorKind::InvalidAudience => {
                    AuthError::InvalidToken(format!("Invalid audience: {}", e))
                }
                jsonwebtoken::errors::ErrorKind::InvalidIssuer => {
                    AuthError::InvalidToken(format!("Invalid issuer: {}", e))
                }
                _ => AuthError::InvalidToken(format!("{}", e)),
            })?;

        Ok(AuthContext::new(token_data.claims, token.to_string()))
    }

    /// Validate a token from an `Authorization: Bearer <token>` header value.
    pub async fn validate_bearer(&self, auth_header: &str) -> Result<AuthContext, AuthError> {
        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AuthError::InvalidToken("Missing 'Bearer ' prefix".to_string()))?;

        self.validate(token).await
    }

    /// Find a matching JWK key by `kid`, or return the first RSA signing key.
    fn find_key<'a>(&self, jwks: &'a Jwks, kid: Option<&str>) -> Result<&'a JwkKey, AuthError> {
        // If kid is specified, find exact match
        if let Some(kid) = kid {
            if let Some(key) = jwks.keys.iter().find(|k| k.kid.as_deref() == Some(kid)) {
                return Ok(key);
            }
            // Fall through to try any RSA key
            warn!(kid = %kid, "No JWK found with matching kid, trying first RSA key");
        }

        // Find first RSA signing key
        jwks.keys
            .iter()
            .find(|k| {
                k.kty == "RSA"
                    && k.key_use.as_deref() != Some("enc") // not encryption-only
                    && k.n.is_some()
                    && k.e.is_some()
            })
            .ok_or_else(|| AuthError::InvalidToken("No suitable RSA key found in JWKS".to_string()))
    }

    /// Force refresh of cached JWKS (e.g., after key rotation).
    pub async fn refresh_jwks(&self) {
        self.jwks_cache.invalidate(&self.config.issuer_url).await;
    }
}

/// Validates tokens using a shared HMAC secret (HS256).
///
/// Use this for development or when your app shares a secret with FlowCatalyst
/// instead of using JWKS-based RS256 validation.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::auth::HmacTokenValidator;
///
/// let validator = HmacTokenValidator::new(
///     "your-shared-secret",
///     "flowcatalyst",  // issuer
///     "flowcatalyst",  // audience
/// );
///
/// let ctx = validator.validate("eyJ...")?;
/// ```
pub struct HmacTokenValidator {
    decoding_key: DecodingKey,
    issuer: String,
    audience: String,
}

impl HmacTokenValidator {
    /// Create a new HMAC token validator.
    pub fn new(secret: &str, issuer: &str, audience: &str) -> Self {
        Self {
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            issuer: issuer.to_string(),
            audience: audience.to_string(),
        }
    }

    /// Validate a JWT token signed with HS256.
    pub fn validate(&self, token: &str) -> Result<AuthContext, AuthError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);

        let token_data = decode::<AccessTokenClaims>(token, &self.decoding_key, &validation)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                _ => AuthError::InvalidToken(format!("{}", e)),
            })?;

        Ok(AuthContext::new(token_data.claims, token.to_string()))
    }

    /// Validate a token from an `Authorization: Bearer <token>` header value.
    pub fn validate_bearer(&self, auth_header: &str) -> Result<AuthContext, AuthError> {
        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AuthError::InvalidToken("Missing 'Bearer ' prefix".to_string()))?;

        self.validate(token)
    }
}
