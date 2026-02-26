//! Authentication Service
//!
//! JWT token generation and validation.
//! Supports both RS256 (RSA) for production and HS256 (HMAC) for development.

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation, Algorithm};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::{info, warn};
use crate::{Principal, UserScope};
use crate::shared::error::{PlatformError, Result};

/// JWT Claims for access tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessTokenClaims {
    /// Subject (principal ID)
    pub sub: String,

    /// Issuer
    pub iss: String,

    /// Audience
    pub aud: String,

    /// Expiration time (Unix timestamp)
    pub exp: i64,

    /// Issued at (Unix timestamp)
    pub iat: i64,

    /// Not before (Unix timestamp)
    pub nbf: i64,

    /// JWT ID (unique identifier)
    pub jti: String,

    /// Principal type (USER or SERVICE)
    #[serde(rename = "type")]
    pub principal_type: String,

    /// User scope (ANCHOR, PARTNER, CLIENT)
    pub scope: String,

    /// User email (for USER type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Display name
    pub name: String,

    /// Client IDs this principal can access
    /// "*" for anchor users (access all)
    pub clients: Vec<String>,

    /// Roles assigned to this principal
    #[serde(default)]
    pub roles: Vec<String>,
}

/// Configuration for the auth service
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// RSA private key PEM content (for RS256)
    /// Takes precedence over secret_key if set
    pub rsa_private_key: Option<String>,

    /// RSA public key PEM content (for RS256)
    pub rsa_public_key: Option<String>,

    /// JWT secret key for HS256 (fallback for development)
    pub secret_key: String,

    /// Token issuer
    pub issuer: String,

    /// Token audience (used per-token in OIDC, but default for access tokens)
    pub audience: String,

    /// Access token expiration in seconds
    pub access_token_expiry_secs: i64,

    /// Session token expiration in seconds (for human users)
    pub session_token_expiry_secs: i64,

    /// Refresh token expiration in seconds
    pub refresh_token_expiry_secs: i64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            rsa_private_key: None,
            rsa_public_key: None,
            secret_key: String::new(),
            issuer: "flowcatalyst".to_string(),
            audience: "flowcatalyst".to_string(),
            access_token_expiry_secs: 3600,      // 1 hour (PT1H)
            session_token_expiry_secs: 28800,    // 8 hours (PT8H)
            refresh_token_expiry_secs: 86400 * 30, // 30 days (P30D)
        }
    }
}

impl AuthConfig {
    /// Load RSA keys from file paths
    /// Falls back to env vars if files not found
    pub fn load_rsa_keys(
        private_key_path: Option<&str>,
        public_key_path: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        let private_key = private_key_path
            .and_then(|p| Self::load_key_from_path_or_env(p, "FLOWCATALYST_JWT_PRIVATE_KEY"));

        let public_key = public_key_path
            .and_then(|p| Self::load_key_from_path_or_env(p, "FLOWCATALYST_JWT_PUBLIC_KEY"));

        (private_key, public_key)
    }

    /// Load key from file path, or from env var if path is empty/missing
    fn load_key_from_path_or_env(path: &str, env_var: &str) -> Option<String> {
        // Try file path first
        if !path.is_empty() {
            if let Ok(content) = fs::read_to_string(path) {
                info!("Loaded JWT key from file: {}", path);
                return Some(content);
            }
        }

        // Fall back to env var
        if let Ok(content) = std::env::var(env_var) {
            if !content.is_empty() {
                info!("Loaded JWT key from env: {}", env_var);
                return Some(content);
            }
        }

        None
    }

    /// Generate RSA key pair and optionally persist to directory
    /// Returns (private_key_pem, public_key_pem)
    pub fn generate_rsa_keys(persist_dir: Option<&Path>) -> Result<(String, String)> {
        use rsa::{RsaPrivateKey, RsaPublicKey, pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding}};

        info!("Generating RSA key pair (2048 bit)");

        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048)
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to generate RSA key: {}", e)
            })?;
        let public_key = RsaPublicKey::from(&private_key);

        let private_pem = private_key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to encode private key: {}", e)
            })?
            .to_string();

        let public_pem = public_key
            .to_public_key_pem(LineEnding::LF)
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to encode public key: {}", e)
            })?;

        // Persist if directory provided
        if let Some(dir) = persist_dir {
            if let Err(e) = fs::create_dir_all(dir) {
                warn!("Could not create key directory: {}", e);
            } else {
                let private_path = dir.join("private.key");
                let public_path = dir.join("public.key");

                if let Err(e) = fs::write(&private_path, &private_pem) {
                    warn!("Could not persist private key: {}", e);
                } else if let Err(e) = fs::write(&public_path, &public_pem) {
                    warn!("Could not persist public key: {}", e);
                } else {
                    info!("Persisted RSA keys to {}", dir.display());
                }
            }
        }

        Ok((private_pem, public_pem))
    }

    /// Load or generate RSA keys (like Java JwtKeyService)
    /// 1. Try loading from configured paths
    /// 2. Try loading from persisted .jwt-keys directory
    /// 3. Generate new keys and persist
    pub fn load_or_generate_rsa_keys(
        private_key_path: Option<&str>,
        public_key_path: Option<&str>,
    ) -> Result<(String, String)> {
        // 1. Try configured paths / env vars
        let (private, public) = Self::load_rsa_keys(private_key_path, public_key_path);
        if let (Some(priv_key), Some(pub_key)) = (private, public) {
            return Ok((priv_key, pub_key));
        }

        // 2. Try persisted keys
        let keys_dir = Path::new(".jwt-keys");
        let private_path = keys_dir.join("private.key");
        let public_path = keys_dir.join("public.key");

        if private_path.exists() && public_path.exists() {
            if let (Ok(priv_key), Ok(pub_key)) = (
                fs::read_to_string(&private_path),
                fs::read_to_string(&public_path),
            ) {
                info!("Loaded persisted RSA keys from .jwt-keys/");
                return Ok((priv_key, pub_key));
            }
        }

        // 3. Generate and persist
        Self::generate_rsa_keys(Some(keys_dir))
    }
}

/// RSA public key components for JWKS
#[derive(Debug, Clone)]
pub struct RsaPublicKeyComponents {
    /// Modulus (n) - base64url encoded
    pub n: String,
    /// Exponent (e) - base64url encoded
    pub e: String,
}

/// Authentication service for token management
pub struct AuthService {
    config: AuthConfig,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    algorithm: Algorithm,
    key_id: Option<String>,
    /// RSA public key components for JWKS (only set when using RS256)
    rsa_components: Option<RsaPublicKeyComponents>,
}

impl AuthService {
    /// Create auth service with RSA keys (RS256) - recommended for production
    pub fn new_with_rsa(config: AuthConfig, private_key_pem: &str, public_key_pem: &str) -> Result<Self> {
        let encoding_key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
            .map_err(|e| PlatformError::Internal {
                message: format!("Invalid RSA private key: {}", e)
            })?;

        let decoding_key = DecodingKey::from_rsa_pem(public_key_pem.as_bytes())
            .map_err(|e| PlatformError::Internal {
                message: format!("Invalid RSA public key: {}", e)
            })?;

        // Generate key ID from public key hash (like Java)
        let key_id = Self::generate_key_id(public_key_pem);

        // Extract RSA components for JWKS
        let rsa_components = Self::extract_rsa_components(public_key_pem)?;

        info!("AuthService initialized with RS256 (key_id: {})", key_id);

        Ok(Self {
            config,
            encoding_key,
            decoding_key,
            algorithm: Algorithm::RS256,
            key_id: Some(key_id),
            rsa_components: Some(rsa_components),
        })
    }

    /// Extract RSA public key components (n, e) for JWKS
    fn extract_rsa_components(public_key_pem: &str) -> Result<RsaPublicKeyComponents> {
        use rsa::{RsaPublicKey, pkcs8::DecodePublicKey, traits::PublicKeyParts};
        use base64::Engine;

        let public_key = RsaPublicKey::from_public_key_pem(public_key_pem)
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to parse RSA public key: {}", e)
            })?;

        // Get modulus and exponent as big-endian bytes
        let n_bytes = public_key.n().to_bytes_be();
        let e_bytes = public_key.e().to_bytes_be();

        // Base64url encode (no padding)
        let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&n_bytes);
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&e_bytes);

        Ok(RsaPublicKeyComponents { n, e })
    }

    /// Create auth service with HMAC secret (HS256) - for development/simple setups
    pub fn new_with_secret(config: AuthConfig) -> Self {
        let encoding_key = EncodingKey::from_secret(config.secret_key.as_bytes());
        let decoding_key = DecodingKey::from_secret(config.secret_key.as_bytes());

        info!("AuthService initialized with HS256");

        Self {
            config,
            encoding_key,
            decoding_key,
            algorithm: Algorithm::HS256,
            key_id: None,
            rsa_components: None,
        }
    }

    /// Create auth service - uses RSA if keys provided, falls back to HMAC
    pub fn new(config: AuthConfig) -> Self {
        if let (Some(ref private_key), Some(ref public_key)) =
            (&config.rsa_private_key, &config.rsa_public_key)
        {
            match Self::new_with_rsa(config.clone(), private_key, public_key) {
                Ok(service) => return service,
                Err(e) => {
                    warn!("Failed to initialize RSA keys, falling back to HMAC: {}", e);
                }
            }
        }

        Self::new_with_secret(config)
    }

    /// Generate key ID from public key (8 char SHA-256 hash, like Java)
    fn generate_key_id(public_key_pem: &str) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(public_key_pem.as_bytes());
        let hash = hasher.finalize();
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &hash[..6])
    }

    /// Get the key ID (for JWKS)
    pub fn key_id(&self) -> Option<&str> {
        self.key_id.as_deref()
    }

    /// Get the RSA public key components (for JWKS)
    pub fn rsa_components(&self) -> Option<&RsaPublicKeyComponents> {
        self.rsa_components.as_ref()
    }

    /// Get the algorithm being used
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// Generate an access token for a principal
    pub fn generate_access_token(&self, principal: &Principal) -> Result<String> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.config.access_token_expiry_secs);

        // Determine client access
        let clients = match principal.scope {
            UserScope::Anchor => vec!["*".to_string()],
            UserScope::Partner => principal.assigned_clients.clone(),
            UserScope::Client => principal.client_id.clone().into_iter().collect(),
        };

        let claims = AccessTokenClaims {
            sub: principal.id.clone(),
            iss: self.config.issuer.clone(),
            aud: self.config.audience.clone(),
            exp: exp.timestamp(),
            iat: now.timestamp(),
            nbf: now.timestamp(),
            jti: crate::TsidGenerator::generate(),
            principal_type: format!("{:?}", principal.principal_type).to_uppercase(),
            scope: format!("{:?}", principal.scope).to_uppercase(),
            email: principal.email().map(String::from),
            name: principal.name.clone(),
            clients,
            roles: principal.roles.iter().map(|r| r.role.clone()).collect(),
        };

        let header = Header::new(self.algorithm);
        encode(&header, &claims, &self.encoding_key)
            .map_err(|e| PlatformError::Internal { message: format!("Failed to encode JWT: {}", e) })
    }

    /// Validate an access token and extract claims
    pub fn validate_token(&self, token: &str) -> Result<AccessTokenClaims> {
        let mut validation = Validation::new(self.algorithm);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_audience(&[&self.config.audience]);

        decode::<AccessTokenClaims>(token, &self.decoding_key, &validation)
            .map(|data| data.claims)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => PlatformError::TokenExpired,
                _ => PlatformError::InvalidToken { message: format!("{}", e) },
            })
    }

    /// Check if claims grant access to a specific client
    pub fn has_client_access(&self, claims: &AccessTokenClaims, client_id: &str) -> bool {
        claims.clients.contains(&"*".to_string()) || claims.clients.contains(&client_id.to_string())
    }

    /// Check if claims have a specific role
    pub fn has_role(&self, claims: &AccessTokenClaims, role: &str) -> bool {
        claims.roles.contains(&role.to_string())
    }

    /// Check if claims are for an anchor user
    pub fn is_anchor(&self, claims: &AccessTokenClaims) -> bool {
        claims.scope == "ANCHOR"
    }

    /// Extract principal ID from claims
    pub fn principal_id<'a>(&self, claims: &'a AccessTokenClaims) -> &'a str {
        &claims.sub
    }
}

/// Extract bearer token from Authorization header
pub fn extract_bearer_token(auth_header: &str) -> Option<&str> {
    if auth_header.starts_with("Bearer ") {
        Some(&auth_header[7..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Principal, UserScope};

    #[test]
    fn test_generate_and_validate_token() {
        let config = AuthConfig::default();
        let service = AuthService::new(config);

        let principal = Principal::new_user("test@example.com", UserScope::Anchor);
        let token = service.generate_access_token(&principal).unwrap();

        let claims = service.validate_token(&token).unwrap();
        assert_eq!(claims.sub, principal.id);
        assert_eq!(claims.scope, "ANCHOR");
        assert!(claims.clients.contains(&"*".to_string()));
    }

    #[test]
    fn test_client_scope_token() {
        let config = AuthConfig::default();
        let service = AuthService::new(config);

        let principal = Principal::new_user("test@example.com", UserScope::Client)
            .with_client_id("client123");

        let token = service.generate_access_token(&principal).unwrap();
        let claims = service.validate_token(&token).unwrap();

        assert_eq!(claims.scope, "CLIENT");
        assert!(claims.clients.contains(&"client123".to_string()));
        assert!(!claims.clients.contains(&"*".to_string()));
    }

    #[test]
    fn test_extract_bearer_token() {
        assert_eq!(extract_bearer_token("Bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer_token("bearer abc123"), None);
        assert_eq!(extract_bearer_token("Basic abc123"), None);
    }
}
