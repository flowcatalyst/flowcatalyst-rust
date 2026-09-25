//! JWKS Cache and JWT Token Validation
//!
//! Fetches FlowCatalyst's public keys via OIDC discovery and validates
//! JWT access tokens using RS256 signature verification.

use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
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
            .map_err(|e| {
                AuthError::Discovery(format!(
                    "Failed to fetch discovery from {}: {}",
                    discovery_url, e
                ))
            })?
            .json()
            .await
            .map_err(|e| AuthError::Discovery(format!("Failed to parse discovery: {}", e)))?;

        debug!(jwks_uri = %discovery.jwks_uri, "Fetching JWKS");

        let jwks: Jwks = self
            .http_client
            .get(&discovery.jwks_uri)
            .send()
            .await
            .map_err(|e| {
                AuthError::Discovery(format!(
                    "Failed to fetch JWKS from {}: {}",
                    discovery.jwks_uri, e
                ))
            })?
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
        let n = key
            .n
            .as_deref()
            .ok_or_else(|| AuthError::InvalidToken("JWK missing RSA modulus (n)".to_string()))?;
        let e = key
            .e
            .as_deref()
            .ok_or_else(|| AuthError::InvalidToken("JWK missing RSA exponent (e)".to_string()))?;

        let decoding_key = DecodingKey::from_rsa_components(n, e)
            .map_err(|e| AuthError::InvalidToken(format!("Invalid RSA components: {}", e)))?;

        // Validate
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.config.issuer_url]);
        validation.set_audience(&[&self.config.audience]);
        validation.leeway = self.config.clock_skew_secs;

        let token_data =
            decode::<AccessTokenClaims>(token, &decoding_key, &validation).map_err(|e| match e
                .kind()
            {
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

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    // ─── TokenValidatorConfig ───────────────────────────────────────────

    #[test]
    fn token_validator_config_default() {
        let config = TokenValidatorConfig::default();
        assert!(config.issuer_url.is_empty());
        assert_eq!(config.audience, "flowcatalyst");
        assert_eq!(config.jwks_ttl_secs, 3600);
        assert_eq!(config.clock_skew_secs, 60);
    }

    #[test]
    fn token_validator_config_clone() {
        let config = TokenValidatorConfig {
            issuer_url: "https://auth.example.com".to_string(),
            audience: "my-app".to_string(),
            jwks_ttl_secs: 120,
            clock_skew_secs: 30,
        };
        let cloned = config.clone();
        assert_eq!(cloned.issuer_url, "https://auth.example.com");
        assert_eq!(cloned.audience, "my-app");
        assert_eq!(cloned.jwks_ttl_secs, 120);
        assert_eq!(cloned.clock_skew_secs, 30);
    }

    // ─── JwksCache ──────────────────────────────────────────────────────

    #[test]
    fn jwks_cache_default_ttl() {
        let cache = JwksCache::default();
        assert_eq!(cache.ttl_secs, 3600);
    }

    #[test]
    fn jwks_cache_custom_ttl() {
        let cache = JwksCache::new(120);
        assert_eq!(cache.ttl_secs, 120);
    }

    #[tokio::test]
    async fn jwks_cache_invalidate_is_noop_when_empty() {
        let cache = JwksCache::new(60);
        // Should not panic
        cache.invalidate("https://not-cached.example.com").await;
    }

    // ─── JwkKey deserialization ─────────────────────────────────────────

    #[test]
    fn jwk_key_deserialization() {
        let json = r#"{
            "kty": "RSA",
            "use": "sig",
            "kid": "key-1",
            "alg": "RS256",
            "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM",
            "e": "AQAB"
        }"#;
        let key: JwkKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.kty, "RSA");
        assert_eq!(key.key_use.as_deref(), Some("sig"));
        assert_eq!(key.kid.as_deref(), Some("key-1"));
        assert_eq!(key.alg.as_deref(), Some("RS256"));
        assert_eq!(
            key.n.as_deref(),
            Some("0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM")
        );
        assert_eq!(key.e.as_deref(), Some("AQAB"));
    }

    #[test]
    fn jwk_key_deserialization_minimal() {
        let json = r#"{"kty": "RSA"}"#;
        let key: JwkKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.kty, "RSA");
        assert!(key.key_use.is_none());
        assert!(key.kid.is_none());
        assert!(key.alg.is_none());
        assert!(key.n.is_none());
        assert!(key.e.is_none());
    }

    #[test]
    fn jwks_deserialization() {
        let json = r#"{
            "keys": [
                {"kty": "RSA", "kid": "k1", "n": "abc", "e": "AQAB"},
                {"kty": "RSA", "kid": "k2", "n": "def", "e": "AQAB"}
            ]
        }"#;
        let jwks: Jwks = serde_json::from_str(json).unwrap();
        assert_eq!(jwks.keys.len(), 2);
        assert_eq!(jwks.keys[0].kid.as_deref(), Some("k1"));
        assert_eq!(jwks.keys[1].kid.as_deref(), Some("k2"));
    }

    #[test]
    fn jwks_empty_keys() {
        let json = r#"{"keys": []}"#;
        let jwks: Jwks = serde_json::from_str(json).unwrap();
        assert!(jwks.keys.is_empty());
    }

    // ─── TokenValidator::find_key ───────────────────────────────────────

    fn make_rsa_key(kid: &str) -> JwkKey {
        JwkKey {
            kty: "RSA".to_string(),
            key_use: Some("sig".to_string()),
            kid: Some(kid.to_string()),
            alg: Some("RS256".to_string()),
            n: Some("modulus".to_string()),
            e: Some("AQAB".to_string()),
        }
    }

    fn make_validator() -> TokenValidator {
        TokenValidator::new(TokenValidatorConfig {
            issuer_url: "https://auth.example.com".to_string(),
            audience: "test".to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn find_key_by_kid_match() {
        let validator = make_validator();
        let jwks = Jwks {
            keys: vec![make_rsa_key("k1"), make_rsa_key("k2")],
        };
        let key = validator.find_key(&jwks, Some("k2")).unwrap();
        assert_eq!(key.kid.as_deref(), Some("k2"));
    }

    #[test]
    fn find_key_falls_back_to_first_rsa_when_kid_not_found() {
        let validator = make_validator();
        let jwks = Jwks {
            keys: vec![make_rsa_key("k1")],
        };
        // kid "k999" doesn't exist, but falls back to first RSA key
        let key = validator.find_key(&jwks, Some("k999")).unwrap();
        assert_eq!(key.kid.as_deref(), Some("k1"));
    }

    #[test]
    fn find_key_no_kid_returns_first_rsa() {
        let validator = make_validator();
        let jwks = Jwks {
            keys: vec![make_rsa_key("k1")],
        };
        let key = validator.find_key(&jwks, None).unwrap();
        assert_eq!(key.kid.as_deref(), Some("k1"));
    }

    #[test]
    fn find_key_skips_enc_key() {
        let validator = make_validator();
        let mut enc_key = make_rsa_key("enc");
        enc_key.key_use = Some("enc".to_string());

        let jwks = Jwks {
            keys: vec![enc_key, make_rsa_key("sig")],
        };
        let key = validator.find_key(&jwks, None).unwrap();
        assert_eq!(key.kid.as_deref(), Some("sig"));
    }

    #[test]
    fn find_key_skips_key_without_n() {
        let validator = make_validator();
        let mut incomplete = make_rsa_key("inc");
        incomplete.n = None;

        let jwks = Jwks {
            keys: vec![incomplete, make_rsa_key("complete")],
        };
        let key = validator.find_key(&jwks, None).unwrap();
        assert_eq!(key.kid.as_deref(), Some("complete"));
    }

    #[test]
    fn find_key_skips_key_without_e() {
        let validator = make_validator();
        let mut incomplete = make_rsa_key("inc");
        incomplete.e = None;

        let jwks = Jwks {
            keys: vec![incomplete, make_rsa_key("complete")],
        };
        let key = validator.find_key(&jwks, None).unwrap();
        assert_eq!(key.kid.as_deref(), Some("complete"));
    }

    #[test]
    fn find_key_error_when_no_suitable_key() {
        let validator = make_validator();
        let jwks = Jwks { keys: vec![] };
        let err = validator.find_key(&jwks, None).unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[test]
    fn find_key_error_when_only_non_rsa_keys() {
        let validator = make_validator();
        let ec_key = JwkKey {
            kty: "EC".to_string(),
            key_use: Some("sig".to_string()),
            kid: Some("ec1".to_string()),
            alg: Some("ES256".to_string()),
            n: None,
            e: None,
        };
        let jwks = Jwks { keys: vec![ec_key] };
        let err = validator.find_key(&jwks, None).unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    // ─── HmacTokenValidator ─────────────────────────────────────────────

    fn make_hs256_token(sub: &str, iss: &str, aud: &str, secret: &str) -> String {
        let now = chrono::Utc::now().timestamp();
        let claims = AccessTokenClaims {
            sub: sub.to_string(),
            iss: iss.to_string(),
            aud: aud.to_string(),
            exp: now + 3600,
            iat: now,
            nbf: now,
            jti: "jti_test".to_string(),
            principal_type: "USER".to_string(),
            tier: "ANCHOR".to_string(),
            scope: String::new(),
            email: Some("test@example.com".to_string()),
            name: "Test".to_string(),
            clients: vec!["*".to_string()],
            roles: vec!["admin".to_string()],
            applications: vec![],
            all_applications: false,
            token_use: None,
        };

        let header = Header::new(Algorithm::HS256);
        encode(
            &header,
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn hmac_validator_accepts_valid_token() {
        let secret = "my-test-secret-at-least-32-chars!";
        let token = make_hs256_token("prn_1", "fc", "fc", secret);
        let validator = HmacTokenValidator::new(secret, "fc", "fc");

        let ctx = validator.validate(&token).unwrap();
        assert_eq!(ctx.principal_id(), "prn_1");
        assert!(ctx.is_anchor());
        assert!(ctx.has_role("admin"));
        assert_eq!(ctx.email(), Some("test@example.com"));
    }

    #[test]
    fn hmac_validator_rejects_wrong_secret() {
        let token = make_hs256_token("prn_1", "fc", "fc", "secret-a");
        let validator = HmacTokenValidator::new("secret-b", "fc", "fc");

        let err = validator.validate(&token).unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[test]
    fn hmac_validator_rejects_wrong_issuer() {
        let secret = "shared-secret-for-testing-12345!";
        let token = make_hs256_token("prn_1", "wrong-issuer", "fc", secret);
        let validator = HmacTokenValidator::new(secret, "fc", "fc");

        let err = validator.validate(&token).unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[test]
    fn hmac_validator_rejects_wrong_audience() {
        let secret = "shared-secret-for-testing-12345!";
        let token = make_hs256_token("prn_1", "fc", "wrong-aud", secret);
        let validator = HmacTokenValidator::new(secret, "fc", "fc");

        let err = validator.validate(&token).unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[test]
    fn hmac_validator_rejects_expired_token() {
        let secret = "shared-secret-for-testing-12345!";
        let now = chrono::Utc::now().timestamp();
        let claims = AccessTokenClaims {
            sub: "prn_1".to_string(),
            iss: "fc".to_string(),
            aud: "fc".to_string(),
            exp: now - 100, // expired
            iat: now - 200,
            nbf: now - 200,
            jti: "j".to_string(),
            principal_type: "USER".to_string(),
            tier: "CLIENT".to_string(),
            scope: String::new(),
            email: None,
            name: "t".to_string(),
            clients: vec![],
            roles: vec![],
            applications: vec![],
            all_applications: false,
            token_use: None,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        let validator = HmacTokenValidator::new(secret, "fc", "fc");
        let err = validator.validate(&token).unwrap_err();
        assert!(matches!(err, AuthError::TokenExpired));
    }

    #[test]
    fn hmac_validator_validate_bearer_strips_prefix() {
        let secret = "shared-secret-for-testing-12345!";
        let token = make_hs256_token("prn_1", "fc", "fc", secret);
        let validator = HmacTokenValidator::new(secret, "fc", "fc");

        let ctx = validator
            .validate_bearer(&format!("Bearer {}", token))
            .unwrap();
        assert_eq!(ctx.principal_id(), "prn_1");
    }

    #[test]
    fn hmac_validator_validate_bearer_rejects_missing_prefix() {
        let secret = "shared-secret-for-testing-12345!";
        let token = make_hs256_token("prn_1", "fc", "fc", secret);
        let validator = HmacTokenValidator::new(secret, "fc", "fc");

        let err = validator.validate_bearer(&token).unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[test]
    fn hmac_validator_returns_bearer_token_in_context() {
        let secret = "shared-secret-for-testing-12345!";
        let token = make_hs256_token("prn_1", "fc", "fc", secret);
        let validator = HmacTokenValidator::new(secret, "fc", "fc");

        let ctx = validator.validate(&token).unwrap();
        assert_eq!(ctx.bearer_token(), token);
    }
}
