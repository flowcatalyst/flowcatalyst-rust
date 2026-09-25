//! Authentication Service
//!
//! JWT token generation and validation.
//! Supports both RS256 (RSA) for production and HS256 (HMAC) for development.

use crate::shared::error::{PlatformError, Result};
use crate::{Principal, PrincipalType, UserScope};
use chrono::{Duration, Utc};
use dashmap::DashMap;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;
use tracing::{info, warn};

/// Cached token validation result
struct CachedClaims {
    claims: AccessTokenClaims,
    cached_at: Instant,
}

/// Cache TTL for validated tokens (30 seconds — short enough to respect expiry changes)
const TOKEN_CACHE_TTL_SECS: u64 = 30;

/// `token_use` of an access token that carries the principal's authority
/// and may be presented as a platform API bearer (Go
/// `authservice.TokenUseAPI`, authservice.go:49).
pub const TOKEN_USE_API: &str = "api";

/// `token_use` of an interactive-login access token: identity only, no
/// authority; the API middleware refuses it (Go
/// `authservice.TokenUseIdentity`, authservice.go:50).
pub const TOKEN_USE_IDENTITY: &str = "identity";

/// The `applications` entry meaning every application, present and future
/// (Go `allApplicationsSentinel`, authservice.go:758).
pub const ALL_APPLICATIONS_SENTINEL: &str = "*";

/// OIDC ID token lifetime. Go wires `IDTokenExpirySecs: 300`
/// (internal/server/wire_services.go:96): the ID token proves the login
/// once and is never a bearer.
pub const ID_TOKEN_EXPIRY_SECS: i64 = 300;

/// JWT Claims for ID tokens (OIDC Core 1.0), in Go's shape
/// (`authservice.IDTokenClaims`, authservice.go:144-181).
///
/// The ID token is a security token that contains claims about the authentication
/// of the end-user. Unlike the access token (used for API calls), the ID token
/// is consumed by the client application to establish the user's identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdTokenClaims {
    /// Subject (principal ID)
    pub sub: String,

    /// Issuer
    pub iss: String,

    /// Audience (client_id of the relying party)
    pub aud: String,

    /// Expiration time (Unix timestamp)
    pub exp: i64,

    /// Issued at (Unix timestamp)
    pub iat: i64,

    /// Authentication time (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_time: Option<i64>,

    /// Nonce from the authorization request (replay protection)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,

    // --- Standard OIDC claims ---
    /// User's display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// User's email address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Whether the email is verified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,

    /// The principal's last modification (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,

    // --- OIDC optional claims ---
    /// Authentication Context Class Reference (OIDC Core §2)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acr: Option<String>,

    /// Authentication Methods References (OIDC Core §2)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amr: Option<Vec<String>>,

    /// Authorized party — the client_id of the party the ID token was issued to (OIDC Core §2)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azp: Option<String>,

    // --- FlowCatalyst custom claims ---
    /// Principal type; on the wire `USER` or `SERVICE`
    #[serde(rename = "type")]
    pub principal_type: PrincipalType,

    /// Tenancy tier; on the wire `ANCHOR`, `PARTNER` or `CLIENT`
    pub tier: UserScope,

    /// Client ID this principal belongs to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Roles assigned to this principal
    pub roles: Vec<String>,

    /// Accessible applications: `["*"]`, or `id:code` pairs
    pub applications: Vec<String>,

    /// Deprecated companion of the `"*"` applications entry
    pub all_applications: bool,

    /// Client access list ("*" for anchor users, "id:identifier" pairs for others)
    pub clients: Vec<String>,
}

/// The session cookie's claims, in Go's shape (`sessiontoken.Mint` via
/// `provider.MintSessionToken`, auth/sessiontoken/sessiontoken.go:79-121,
/// auth/provider/provider.go:285-314): the subject and email only, plus an
/// empty `tier` and a false `all_applications`, which Go always writes. No
/// `aud`, `type`, `jti` or `token_use`. Every piece of authority is
/// reloaded from the database per request, so a role change or a
/// deactivation takes effect on the next request, and a cookie Go issued
/// before cutover (same key and issuer) stays valid.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTokenClaims {
    pub iss: String,
    pub sub: String,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    /// Always empty: Go mints the cookie without the tier.
    pub tier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Always false: Go mints the cookie without application access.
    pub all_applications: bool,
}

/// What a verified session cookie establishes: who signed in, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentity {
    /// The principal id (`sub`).
    pub principal_id: String,
    /// When the session was minted (`iat`) — the sign-in time, for OIDC
    /// `max_age`. `None` when the token carries no `iat`.
    pub issued_at: Option<i64>,
}

/// The claims a session cookie is read with. Every other token the
/// platform signs (access tokens, ID tokens) carries at least one of
/// `token_use`, `type` or `jti`; a session token carries none of them
/// (Java `TokenClaims.isSessionToken`, 6a06a7f0), so an API or identity
/// token replayed as the cookie is never a sign-in.
#[derive(Deserialize)]
struct SessionTokenWire {
    sub: String,
    #[serde(default)]
    iat: Option<i64>,
    #[serde(default)]
    token_use: Option<serde_json::Value>,
    #[serde(default, rename = "type")]
    principal_type: Option<serde_json::Value>,
    #[serde(default)]
    jti: Option<serde_json::Value>,
}

/// JWT Claims for access tokens, in Go's shape
/// (`authservice.AccessTokenClaims`, authservice.go:84-142).
///
/// Deserialization also accepts the shape Rust issued before it matched Go —
/// the tier on `scope` and no `tier` claim — so those tokens keep working
/// until they expire; see [`AccessTokenClaimsWire`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "AccessTokenClaimsWire")]
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

    /// Principal type; on the wire `USER` or `SERVICE`
    #[serde(rename = "type")]
    pub principal_type: PrincipalType,

    /// Tenancy tier; on the wire `ANCHOR`, `PARTNER` or `CLIENT`
    pub tier: UserScope,

    /// Granted permissions, space-delimited (the OAuth `scope`). Absent when
    /// the token carries none; permissions then derive from `roles`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// User email (for USER type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Display name
    pub name: String,

    /// Client access: `["*"]` for anchor, `id:identifier` pairs otherwise
    pub clients: Vec<String>,

    /// Roles assigned to this principal
    pub roles: Vec<String>,

    /// Accessible applications: `["*"]`, or `id:code` pairs
    pub applications: Vec<String>,

    /// Deprecated companion of the `"*"` applications entry
    pub all_applications: bool,

    /// The OAuth client the token was minted through, when there was one
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azp: Option<String>,

    /// [`TOKEN_USE_API`] or [`TOKEN_USE_IDENTITY`]; absent on older tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_use: Option<String>,
}

/// The access-token claims as they may arrive: Go's shape, or the shape
/// Rust issued before it (the tier on `scope`, no `tier`, no `token_use`).
#[derive(Deserialize)]
struct AccessTokenClaimsWire {
    sub: String,
    iss: String,
    aud: String,
    exp: i64,
    iat: i64,
    #[serde(default)]
    nbf: i64,
    #[serde(default)]
    jti: String,
    #[serde(rename = "type")]
    principal_type: PrincipalType,
    #[serde(default)]
    tier: Option<UserScope>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    clients: Vec<String>,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    applications: Vec<String>,
    #[serde(default)]
    all_applications: bool,
    #[serde(default)]
    azp: Option<String>,
    #[serde(default)]
    token_use: Option<String>,
}

impl TryFrom<AccessTokenClaimsWire> for AccessTokenClaims {
    type Error = String;

    fn try_from(w: AccessTokenClaimsWire) -> std::result::Result<Self, Self::Error> {
        // `tier` wins when present. Without it the token predates Go's shape
        // and its `scope` is the tier, not permissions.
        let (tier, scope) = match w.tier {
            Some(tier) => (tier, w.scope.filter(|s| !s.trim().is_empty())),
            None => match w.scope.as_deref().map(str::parse::<UserScope>) {
                Some(Ok(tier)) => (tier, None),
                _ => return Err("token carries no tier".to_string()),
            },
        };
        Ok(Self {
            sub: w.sub,
            iss: w.iss,
            aud: w.aud,
            exp: w.exp,
            iat: w.iat,
            nbf: w.nbf,
            jti: w.jti,
            principal_type: w.principal_type,
            tier,
            scope,
            email: w.email,
            name: w.name,
            clients: w.clients,
            roles: w.roles,
            applications: w.applications,
            all_applications: w.all_applications,
            azp: w.azp,
            token_use: w.token_use,
        })
    }
}

impl AccessTokenClaims {
    /// Check if the claims grant access to a specific client.
    /// Handles both plain IDs and "id:identifier" format.
    pub fn has_client_access(&self, client_id: &str) -> bool {
        self.clients.iter().any(|c| {
            c == "*"
                || c == client_id
                || c.strip_prefix(client_id)
                    .is_some_and(|rest| rest.starts_with(':'))
        })
    }

    /// Check if the claims carry a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if the claims are for an anchor user.
    pub fn is_anchor(&self) -> bool {
        self.tier.is_anchor()
    }

    /// The principal ID (the `sub` claim).
    pub fn principal_id(&self) -> &str {
        &self.sub
    }

    /// The permissions granted on the `scope` claim (Go's
    /// `strings.Fields(scope)`, sessiontoken.go:196). Empty when the token
    /// carries none.
    pub fn granted_permissions(&self) -> Vec<String> {
        self.scope
            .as_deref()
            .map(|s| s.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// Whether this is an identity-only (interactive-login) access token,
    /// which must not authorize API calls.
    pub fn is_identity_only(&self) -> bool {
        self.token_use.as_deref() == Some(TOKEN_USE_IDENTITY)
    }
}

/// The `clients` claim (Go `buildClients`, authservice.go:700-717): anchor →
/// `["*"]`; partner → the assigned clients; client → the home client; each
/// as `id:identifier` when the identifier is known, else the bare id.
pub fn clients_claim(principal: &Principal) -> Vec<String> {
    let pair = |id: &String| match principal.client_identifier_map.get(id) {
        Some(identifier) => format!("{id}:{identifier}"),
        None => id.clone(),
    };
    match principal.scope {
        UserScope::Anchor => vec!["*".to_string()],
        UserScope::Partner => principal.assigned_clients.iter().map(pair).collect(),
        UserScope::Client => principal.client_id.iter().map(pair).collect(),
    }
}

/// The `applications` claim (Go `appAccessOf`, authservice.go:738-755):
/// `["*"]` when the principal reaches every application, otherwise its
/// application grants as `id:code` pairs (the bare id when the code is
/// unknown).
pub fn applications_claim(principal: &Principal) -> Vec<String> {
    if principal.all_applications {
        return vec![ALL_APPLICATIONS_SENTINEL.to_string()];
    }
    principal
        .accessible_application_ids
        .iter()
        .map(|id| match principal.application_code_map.get(id) {
            Some(code) if !code.is_empty() => format!("{id}:{code}"),
            _ => id.clone(),
        })
        .collect()
}

/// The role names a principal holds, in assignment order (Go `roleNames`,
/// authservice.go:680-686).
pub fn role_names(principal: &Principal) -> Vec<String> {
    principal.roles.iter().map(|r| r.role.clone()).collect()
}

/// Configuration for the auth service
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// RSA private key PEM content (for RS256)
    /// Takes precedence over secret_key if set
    pub rsa_private_key: Option<String>,

    /// RSA public key PEM content (for RS256)
    pub rsa_public_key: Option<String>,

    /// Previous RSA public key PEM (for key rotation — validation only)
    pub rsa_public_key_previous: Option<String>,

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
            rsa_public_key_previous: None,
            secret_key: String::new(),
            issuer: "flowcatalyst".to_string(),
            audience: "flowcatalyst".to_string(),
            access_token_expiry_secs: 3600,        // 1 hour (PT1H)
            session_token_expiry_secs: 86400,      // 24 hours (PT24H)
            refresh_token_expiry_secs: 86400 * 30, // 30 days (P30D)
        }
    }
}

impl AuthConfig {
    /// Load RSA keys from file paths or environment variables.
    /// Priority: file path → env var (FLOWCATALYST_JWT_*) → None
    pub fn load_rsa_keys(
        private_key_path: Option<&str>,
        public_key_path: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        let private_key = private_key_path
            .and_then(|p| {
                if p.is_empty() {
                    None
                } else {
                    std::fs::read_to_string(p).ok()
                }
            })
            .or_else(|| {
                std::env::var("FLOWCATALYST_JWT_PRIVATE_KEY")
                    .ok()
                    .filter(|s| !s.is_empty())
            });

        let public_key = public_key_path
            .and_then(|p| {
                if p.is_empty() {
                    None
                } else {
                    std::fs::read_to_string(p).ok()
                }
            })
            .or_else(|| {
                std::env::var("FLOWCATALYST_JWT_PUBLIC_KEY")
                    .ok()
                    .filter(|s| !s.is_empty())
            });

        if private_key.is_some() && public_key.is_some() {
            info!("Loaded RSA keys from environment/file");
        }

        (private_key, public_key)
    }

    /// Generate RSA key pair and optionally persist to directory
    /// Returns (private_key_pem, public_key_pem)
    pub fn generate_rsa_keys(persist_dir: Option<&Path>) -> Result<(String, String)> {
        use rsa::{
            pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
            RsaPrivateKey, RsaPublicKey,
        };

        info!("Generating RSA key pair (2048 bit)");

        let mut rng = rsa::rand_core::OsRng;
        let private_key =
            RsaPrivateKey::new(&mut rng, 2048).map_err(|e| PlatformError::Internal {
                message: format!("Failed to generate RSA key: {}", e),
            })?;
        let public_key = RsaPublicKey::from(&private_key);

        let private_pem = private_key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to encode private key: {}", e),
            })?
            .to_string();

        let public_pem =
            public_key
                .to_public_key_pem(LineEnding::LF)
                .map_err(|e| PlatformError::Internal {
                    message: format!("Failed to encode public key: {}", e),
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

    /// Load or generate RSA keys
    /// 1. Try loading from configured paths / env vars
    /// 2. If both paths are configured (but the files don't yet exist),
    ///    generate a keypair and persist to those paths — keeps keys
    ///    anchored to an absolute location across restarts.
    /// 3. Try loading from the relative `.jwt-keys/` fallback
    /// 4. Generate new keys and persist to `.jwt-keys/`
    pub fn load_or_generate_rsa_keys(
        private_key_path: Option<&str>,
        public_key_path: Option<&str>,
    ) -> Result<(String, String)> {
        // 1. Try configured paths / env vars
        let (private, public) = Self::load_rsa_keys(private_key_path, public_key_path);
        if let (Some(priv_key), Some(pub_key)) = (private, public) {
            return Ok((priv_key, pub_key));
        }

        // 2. Both paths configured but files missing → generate into those paths
        //    so the next launch reads the same keys regardless of CWD.
        if let (Some(priv_p), Some(pub_p)) = (private_key_path, public_key_path) {
            if !priv_p.is_empty() && !pub_p.is_empty() {
                return Self::generate_rsa_keys_at(Path::new(priv_p), Path::new(pub_p));
            }
        }

        // 3. Try persisted keys at the legacy relative location
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

        // 4. Generate and persist to the legacy relative location
        Self::generate_rsa_keys(Some(keys_dir))
    }

    /// Generate a fresh RSA keypair and write it to the supplied absolute paths.
    fn generate_rsa_keys_at(private_path: &Path, public_path: &Path) -> Result<(String, String)> {
        use rsa::{
            pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
            RsaPrivateKey, RsaPublicKey,
        };

        info!(
            private = %private_path.display(),
            public = %public_path.display(),
            "Generating RSA key pair (2048 bit) at configured paths"
        );

        let mut rng = rsa::rand_core::OsRng;
        let private_key =
            RsaPrivateKey::new(&mut rng, 2048).map_err(|e| PlatformError::Internal {
                message: format!("Failed to generate RSA key: {}", e),
            })?;
        let public_key = RsaPublicKey::from(&private_key);

        let private_pem = private_key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to encode private key: {}", e),
            })?
            .to_string();
        let public_pem =
            public_key
                .to_public_key_pem(LineEnding::LF)
                .map_err(|e| PlatformError::Internal {
                    message: format!("Failed to encode public key: {}", e),
                })?;

        if let Some(parent) = private_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warn!(
                    "Could not create private key directory {}: {}",
                    parent.display(),
                    e
                );
            }
        }
        if let Some(parent) = public_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warn!(
                    "Could not create public key directory {}: {}",
                    parent.display(),
                    e
                );
            }
        }

        if let Err(e) = fs::write(private_path, &private_pem) {
            warn!(
                "Could not persist private key to {}: {}",
                private_path.display(),
                e
            );
        }
        if let Err(e) = fs::write(public_path, &public_pem) {
            warn!(
                "Could not persist public key to {}: {}",
                public_path.display(),
                e
            );
        }

        Ok((private_pem, public_pem))
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

/// A single signing/verification key with its metadata
#[derive(Clone)]
struct KeyEntry {
    decoding_key: DecodingKey,
    key_id: String,
    rsa_components: Option<RsaPublicKeyComponents>,
}

/// Authentication service for token management.
///
/// Supports JWT key rotation: signs with the current key, validates against
/// both current and previous keys. The JWKS endpoint exposes all active public keys
/// so clients can verify tokens signed by either key during rotation.
///
/// ## Key Rotation Procedure
/// 1. Set `FC_JWT_PRIVATE_KEY_PATH_PREVIOUS` / `FLOWCATALYST_JWT_PRIVATE_KEY_PREVIOUS`
///    and `FC_JWT_PUBLIC_KEY_PATH_PREVIOUS` / `FLOWCATALYST_JWT_PUBLIC_KEY_PREVIOUS`
///    to the current keys
/// 2. Set the primary key paths/env vars to the new keys
/// 3. Restart — new tokens signed with new key, old tokens still validate
/// 4. After max token TTL passes (e.g., 30 days for refresh tokens), remove previous keys
pub struct AuthService {
    config: AuthConfig,
    /// Current key for signing new tokens
    encoding_key: EncodingKey,
    /// Current key for validation
    decoding_key: DecodingKey,
    algorithm: Algorithm,
    key_id: Option<String>,
    /// RSA public key components for JWKS (only set when using RS256)
    rsa_components: Option<RsaPublicKeyComponents>,
    /// Previous keys — used for validation only (not signing), exposed in JWKS
    previous_keys: Vec<KeyEntry>,
    /// Cache of validated tokens: token string → claims (avoids repeated RSA verification)
    token_cache: DashMap<String, CachedClaims>,
}

impl AuthService {
    /// Create auth service with RSA keys (RS256) - recommended for production
    pub fn new_with_rsa(
        config: AuthConfig,
        private_key_pem: &str,
        public_key_pem: &str,
    ) -> Result<Self> {
        let encoding_key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes()).map_err(|e| {
            PlatformError::Internal {
                message: format!("Invalid RSA private key: {}", e),
            }
        })?;

        let decoding_key = DecodingKey::from_rsa_pem(public_key_pem.as_bytes()).map_err(|e| {
            PlatformError::Internal {
                message: format!("Invalid RSA public key: {}", e),
            }
        })?;

        // Generate key ID from public key hash
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
            previous_keys: Vec::new(),
            token_cache: DashMap::new(),
        })
    }

    /// Add a previous RSA key pair for validation-only (key rotation).
    /// The previous key will be used to validate existing tokens and exposed in JWKS.
    pub fn add_previous_rsa_key(&mut self, public_key_pem: &str) -> Result<()> {
        let decoding_key = DecodingKey::from_rsa_pem(public_key_pem.as_bytes()).map_err(|e| {
            PlatformError::Internal {
                message: format!("Invalid previous RSA public key: {}", e),
            }
        })?;
        let key_id = Self::generate_key_id(public_key_pem);
        let rsa_components = Self::extract_rsa_components(public_key_pem)?;

        info!("Added previous RSA key for rotation (key_id: {})", key_id);

        self.previous_keys.push(KeyEntry {
            decoding_key,
            key_id,
            rsa_components: Some(rsa_components),
        });
        Ok(())
    }

    /// Extract RSA public key components (n, e) for JWKS
    fn extract_rsa_components(public_key_pem: &str) -> Result<RsaPublicKeyComponents> {
        use base64::Engine;
        use rsa::{pkcs8::DecodePublicKey, traits::PublicKeyParts, RsaPublicKey};

        let public_key = RsaPublicKey::from_public_key_pem(public_key_pem).map_err(|e| {
            PlatformError::Internal {
                message: format!("Failed to parse RSA public key: {}", e),
            }
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
            previous_keys: Vec::new(),
            token_cache: DashMap::new(),
        }
    }

    /// Create auth service - uses RSA if keys provided, falls back to HMAC.
    /// Automatically loads previous key for rotation if configured.
    pub fn new(config: AuthConfig) -> Self {
        if let (Some(ref private_key), Some(ref public_key)) =
            (&config.rsa_private_key, &config.rsa_public_key)
        {
            match Self::new_with_rsa(config.clone(), private_key, public_key) {
                Ok(mut service) => {
                    // Load previous key for rotation if configured
                    if let Some(ref prev_pub) = config.rsa_public_key_previous {
                        if let Err(e) = service.add_previous_rsa_key(prev_pub) {
                            warn!("Failed to load previous RSA key for rotation: {}", e);
                        }
                    }
                    return service;
                }
                Err(e) => {
                    warn!("Failed to initialize RSA keys, falling back to HMAC: {}", e);
                }
            }
        }

        Self::new_with_secret(config)
    }

    /// Generate key ID from public key (22 char base64url SHA-256 hash)
    fn generate_key_id(public_key_pem: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(public_key_pem.as_bytes());
        let hash = hasher.finalize();
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            &hash[..16],
        )
    }

    /// Get the key ID (for JWKS)
    pub fn key_id(&self) -> Option<&str> {
        self.key_id.as_deref()
    }

    /// Get the RSA public key components (for JWKS) — current key only
    pub fn rsa_components(&self) -> Option<&RsaPublicKeyComponents> {
        self.rsa_components.as_ref()
    }

    /// Get all JWKS entries (current + previous keys for rotation).
    /// Returns Vec of (key_id, rsa_components) pairs.
    pub fn all_jwks_keys(&self) -> Vec<(&str, &RsaPublicKeyComponents)> {
        let mut keys = Vec::new();
        if let (Some(kid), Some(components)) = (&self.key_id, &self.rsa_components) {
            keys.push((kid.as_str(), components));
        }
        for prev in &self.previous_keys {
            if let Some(ref components) = prev.rsa_components {
                keys.push((&prev.key_id, components));
            }
        }
        keys
    }

    /// Get the algorithm being used
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// The access-token lifetime, which token responses advertise as
    /// `expires_in` (Go `AccessTokenTTLSecs`, authservice.go:388).
    pub fn access_token_expiry_secs(&self) -> i64 {
        self.config.access_token_expiry_secs
    }

    /// A short-lived, authority-bearing access token (`token_use: api`, no
    /// `scope`). Go `GenerateAccessToken` (authservice.go:416).
    pub fn generate_access_token(&self, principal: &Principal) -> Result<String> {
        self.sign_access(self.access_token_claims(
            principal,
            self.config.access_token_expiry_secs,
            &[],
            true,
            None,
            Utc::now(),
        ))
    }

    /// An authority-bearing access token whose `scope` carries the granted
    /// permissions, stamped with `azp` when minted through an OAuth client
    /// that is not the principal itself. Go `GenerateAccessTokenWithScope`
    /// / `GenerateAccessTokenWithScopeFor` (authservice.go:426-435).
    pub fn generate_access_token_with_scope(
        &self,
        principal: &Principal,
        granted: &[String],
        azp: Option<&str>,
    ) -> Result<String> {
        self.sign_access(self.access_token_claims(
            principal,
            self.config.access_token_expiry_secs,
            granted,
            true,
            azp,
            Utc::now(),
        ))
    }

    /// The access token an interactive login returns: `token_use: identity`
    /// and no authority (empty `roles`, `clients`, `applications`, no
    /// `scope`). The API middleware refuses it. Go
    /// `GenerateIdentityAccessToken[For]` (authservice.go:445-455).
    pub fn generate_identity_access_token(
        &self,
        principal: &Principal,
        azp: Option<&str>,
    ) -> Result<String> {
        self.sign_access(self.access_token_claims(
            principal,
            self.config.access_token_expiry_secs,
            &[],
            false,
            azp,
            Utc::now(),
        ))
    }

    /// The session cookie's token: the subject only, in Go's shape (see
    /// [`SessionTokenClaims`]), valid for the session lifetime. Go
    /// `provider.MintSessionToken` (auth/provider/provider.go:285-314).
    pub fn generate_session_token(&self, principal: &Principal) -> Result<String> {
        let claims = self.session_token_claims(principal, Utc::now());
        let mut header = Header::new(self.algorithm);
        header.kid = self.key_id.clone();
        encode(&header, &claims, &self.encoding_key).map_err(|e| PlatformError::Internal {
            message: format!("Failed to encode session token: {}", e),
        })
    }

    /// The session-token claim set, unsigned.
    pub fn session_token_claims(
        &self,
        principal: &Principal,
        now: chrono::DateTime<Utc>,
    ) -> SessionTokenClaims {
        SessionTokenClaims {
            iss: self.config.issuer.clone(),
            sub: principal.id.clone(),
            iat: now.timestamp(),
            nbf: now.timestamp(),
            exp: (now + Duration::seconds(self.config.session_token_expiry_secs)).timestamp(),
            tier: String::new(),
            email: principal
                .email()
                .filter(|e| !e.is_empty())
                .map(String::from),
            all_applications: false,
        }
    }

    /// Verify a session cookie (Go `sessiontoken.Validate` with the
    /// platform's issuer and audience, sessiontoken.go:151-209): signature
    /// under the current or a previous key, `exp`, the issuer, a non-empty
    /// subject, and an `aud` — when present — that names the platform. On
    /// top of Go, the token must be the session kind: one carrying
    /// `token_use`, `type` or `jti` (an access or ID token) is refused.
    pub fn validate_session_token(&self, token: &str) -> Result<SessionIdentity> {
        let mut validation = Validation::new(self.algorithm);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_audience(&[&self.config.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "sub"]);

        let mut decoded = decode::<SessionTokenWire>(token, &self.decoding_key, &validation);
        if let Err(e) = &decoded {
            if !matches!(e.kind(), jsonwebtoken::errors::ErrorKind::ExpiredSignature) {
                for prev in &self.previous_keys {
                    if let Ok(data) =
                        decode::<SessionTokenWire>(token, &prev.decoding_key, &validation)
                    {
                        decoded = Ok(data);
                        break;
                    }
                }
            }
        }
        let claims = match decoded {
            Ok(data) => data.claims,
            Err(e) if matches!(e.kind(), jsonwebtoken::errors::ErrorKind::ExpiredSignature) => {
                return Err(PlatformError::TokenExpired)
            }
            Err(e) => {
                return Err(PlatformError::InvalidToken {
                    message: e.to_string(),
                })
            }
        };
        if claims.token_use.is_some() || claims.principal_type.is_some() || claims.jti.is_some() {
            return Err(PlatformError::InvalidToken {
                message: "not a session token".to_string(),
            });
        }
        if claims.sub.is_empty() {
            return Err(PlatformError::InvalidToken {
                message: "session token has no subject".to_string(),
            });
        }
        Ok(SessionIdentity {
            principal_id: claims.sub,
            issued_at: claims.iat,
        })
    }

    /// Generate an OIDC ID token for a principal with its full role list.
    /// `client_id` becomes the `aud` claim; `nonce` is echoed from the
    /// authorization request. Go `GenerateIDToken` (authservice.go:508).
    pub fn generate_id_token(
        &self,
        principal: &Principal,
        client_id: &str,
        nonce: Option<String>,
    ) -> Result<String> {
        self.generate_id_token_with_roles(principal, client_id, nonce, role_names(principal))
    }

    /// [`Self::generate_id_token`] with the `roles` claim overridden — the
    /// roles narrowed to an app-scoped client's applications. Go
    /// `GenerateIDTokenWithRoles` (authservice.go:521).
    pub fn generate_id_token_with_roles(
        &self,
        principal: &Principal,
        client_id: &str,
        nonce: Option<String>,
        roles: Vec<String>,
    ) -> Result<String> {
        let claims = self.id_token_claims(principal, client_id, nonce, roles, Utc::now());
        let mut header = Header::new(self.algorithm);
        header.kid = self.key_id.clone();
        encode(&header, &claims, &self.encoding_key).map_err(|e| PlatformError::Internal {
            message: format!("Failed to encode ID token: {}", e),
        })
    }

    /// The ID token of a PORTAL identity login (Go `GeneratePortalIDToken`,
    /// authservice.go:542-552): the identity's own claims (sub = its `ptu_`
    /// id) with no authority — empty `roles`, `applications` and `clients`,
    /// an empty `tier`, no `client_id` — plus `portal_client_id` and, for an
    /// app-linked portal OAuth client, `portal_app_id` / `portal_app_code`.
    /// `identity` is a transient principal-shaped view of the portal
    /// identity; it never touches the principal store.
    pub fn generate_portal_id_token(
        &self,
        identity: &Principal,
        client_id: &str,
        nonce: Option<String>,
        portal_client_id: &str,
        portal_app: Option<(&str, &str)>,
    ) -> Result<String> {
        let claims = self.id_token_claims(identity, client_id, nonce, Vec::new(), Utc::now());
        let mut value = serde_json::to_value(&claims).map_err(|e| PlatformError::Internal {
            message: format!("Failed to encode ID token: {}", e),
        })?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("tier".into(), serde_json::json!(""));
            obj.insert("roles".into(), serde_json::json!([]));
            obj.insert("applications".into(), serde_json::json!([]));
            obj.insert("all_applications".into(), serde_json::json!(false));
            obj.insert("clients".into(), serde_json::json!([]));
            obj.remove("client_id");
            if !portal_client_id.is_empty() {
                obj.insert(
                    "portal_client_id".into(),
                    serde_json::json!(portal_client_id),
                );
            }
            if let Some((app_id, app_code)) = portal_app.filter(|(_, code)| !code.is_empty()) {
                obj.insert("portal_app_code".into(), serde_json::json!(app_code));
                obj.insert("portal_app_id".into(), serde_json::json!(app_id));
            }
        }
        let mut header = Header::new(self.algorithm);
        header.kid = self.key_id.clone();
        encode(&header, &value, &self.encoding_key).map_err(|e| PlatformError::Internal {
            message: format!("Failed to encode ID token: {}", e),
        })
    }

    /// The access-token claim set, unsigned. Go `generateTokenWithExpiry`
    /// (authservice.go:467-501): an authoritative token carries the
    /// principal's authority and `token_use: api`; an identity token emits
    /// empty authority arrays and `token_use: identity`.
    pub fn access_token_claims(
        &self,
        principal: &Principal,
        expiry_secs: i64,
        granted: &[String],
        authoritative: bool,
        azp: Option<&str>,
        now: chrono::DateTime<Utc>,
    ) -> AccessTokenClaims {
        let exp = now + Duration::seconds(expiry_secs);
        let mut claims = AccessTokenClaims {
            sub: principal.id.clone(),
            iss: self.config.issuer.clone(),
            aud: self.config.audience.clone(),
            exp: exp.timestamp(),
            iat: now.timestamp(),
            nbf: now.timestamp(),
            jti: crate::shared::tsid::generate_untyped(),
            principal_type: principal.principal_type,
            tier: principal.scope,
            scope: None,
            email: principal
                .email()
                .filter(|e| !e.is_empty())
                .map(String::from),
            name: principal.name.clone(),
            clients: Vec::new(),
            roles: Vec::new(),
            applications: Vec::new(),
            all_applications: false,
            azp: azp.filter(|a| !a.is_empty()).map(String::from),
            token_use: None,
        };
        if authoritative {
            claims.token_use = Some(TOKEN_USE_API.to_string());
            claims.scope = Some(granted.join(" ")).filter(|s| !s.is_empty());
            claims.clients = clients_claim(principal);
            claims.roles = role_names(principal);
            claims.applications = applications_claim(principal);
            claims.all_applications = principal.all_applications;
        } else {
            claims.token_use = Some(TOKEN_USE_IDENTITY.to_string());
        }
        claims
    }

    /// The ID-token claim set, unsigned. Go `idTokenClaims`
    /// (authservice.go:555-602). `auth_time` is `now`: Rust does not yet
    /// carry the login time on its authorization codes (Go's zero-time
    /// fallback).
    pub fn id_token_claims(
        &self,
        principal: &Principal,
        client_id: &str,
        nonce: Option<String>,
        roles: Vec<String>,
        now: chrono::DateTime<Utc>,
    ) -> IdTokenClaims {
        let email = principal
            .email()
            .filter(|e| !e.is_empty())
            .map(String::from);
        IdTokenClaims {
            sub: principal.id.clone(),
            iss: self.config.issuer.clone(),
            aud: client_id.to_string(),
            exp: (now + Duration::seconds(ID_TOKEN_EXPIRY_SECS)).timestamp(),
            iat: now.timestamp(),
            auth_time: Some(now.timestamp()),
            nonce,
            name: Some(principal.name.clone()),
            email_verified: email.as_ref().map(|_| true),
            email,
            updated_at: Some(principal.updated_at.timestamp()),
            acr: None,
            amr: None,
            azp: Some(client_id.to_string()),
            principal_type: principal.principal_type,
            tier: principal.scope,
            client_id: principal.client_id.clone(),
            roles,
            applications: applications_claim(principal),
            all_applications: principal.all_applications,
            clients: clients_claim(principal),
        }
    }

    fn sign_access(&self, claims: AccessTokenClaims) -> Result<String> {
        let mut header = Header::new(self.algorithm);
        header.kid = self.key_id.clone();
        encode(&header, &claims, &self.encoding_key).map_err(|e| PlatformError::Internal {
            message: format!("Failed to encode JWT: {}", e),
        })
    }

    /// Validate an access token and extract claims.
    /// Uses an in-memory cache to avoid repeated RSA signature verification.
    /// Tries the current key first, then falls back to previous keys (for key rotation).
    pub fn validate_token(&self, token: &str) -> Result<AccessTokenClaims> {
        // Check cache first
        if let Some(entry) = self.token_cache.get(token) {
            if entry.cached_at.elapsed().as_secs() < TOKEN_CACHE_TTL_SECS {
                // Still need to check expiry even for cached tokens
                let now = Utc::now().timestamp();
                if entry.claims.exp > now {
                    return Ok(entry.claims.clone());
                } else {
                    // Token expired since caching — remove and return error
                    drop(entry);
                    self.token_cache.remove(token);
                    return Err(PlatformError::TokenExpired);
                }
            }
            // Cache entry expired — remove it
            drop(entry);
            self.token_cache.remove(token);
        }

        // Cache miss — do full validation
        let claims = self.validate_token_uncached(token)?;

        // Store in cache
        self.token_cache.insert(
            token.to_string(),
            CachedClaims {
                claims: claims.clone(),
                cached_at: Instant::now(),
            },
        );

        Ok(claims)
    }

    /// Perform full JWT validation without cache
    fn validate_token_uncached(&self, token: &str) -> Result<AccessTokenClaims> {
        let mut validation = Validation::new(self.algorithm);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_audience(&[&self.config.audience]);

        // Try current key first
        match decode::<AccessTokenClaims>(token, &self.decoding_key, &validation) {
            Ok(data) => return Ok(data.claims),
            Err(e) => {
                // If expired, don't bother trying other keys
                if matches!(e.kind(), jsonwebtoken::errors::ErrorKind::ExpiredSignature) {
                    return Err(PlatformError::TokenExpired);
                }
                // If no previous keys, fail immediately
                if self.previous_keys.is_empty() {
                    return Err(PlatformError::InvalidToken {
                        message: format!("{}", e),
                    });
                }
            }
        }

        // Try previous keys (rotation fallback)
        for prev in &self.previous_keys {
            if let Ok(data) = decode::<AccessTokenClaims>(token, &prev.decoding_key, &validation) {
                return Ok(data.claims);
            }
        }

        Err(PlatformError::InvalidToken {
            message: "Token signature invalid with all available keys".to_string(),
        })
    }
}

/// Extract bearer token from Authorization header
pub fn extract_bearer_token(auth_header: &str) -> Option<&str> {
    auth_header.strip_prefix("Bearer ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Principal, PrincipalType, UserScope};

    use serde_json::json;

    fn service() -> AuthService {
        AuthService::new(AuthConfig {
            secret_key: "golden-claims-test-secret-at-least-32-bytes!".to_string(),
            issuer: "https://fc.example.test".to_string(),
            audience: "https://fc.example.test".to_string(),
            ..AuthConfig::default()
        })
    }

    fn now() -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(1_760_000_000, 0).unwrap()
    }

    /// A CLIENT-tier user with a known client identifier, two roles, and two
    /// application grants — one whose code is known and one whose isn't.
    fn client_user() -> Principal {
        let mut p = Principal::new_user("ada@acme.test", UserScope::Client).with_client_id("clt_A");
        p.id = "prn_ADA".to_string();
        p.name = "Ada Lovelace".to_string();
        p.client_identifier_map
            .insert("clt_A".to_string(), "acme".to_string());
        p.assign_role("hr:manager");
        p.assign_role("platform:viewer");
        p.all_applications = false;
        p.accessible_application_ids = vec!["app_1".to_string(), "app_2".to_string()];
        p.application_code_map
            .insert("app_1".to_string(), "hr".to_string());
        p.updated_at = chrono::DateTime::from_timestamp(1_750_000_000, 0).unwrap();
        p
    }

    /// The claims as JSON, with the random `jti` pinned.
    fn wire(claims: &impl Serialize) -> serde_json::Value {
        let mut v = serde_json::to_value(claims).unwrap();
        if v.get("jti").is_some() {
            v["jti"] = json!("JTI");
        }
        v
    }

    fn payload_json(token: &str) -> serde_json::Value {
        use base64::Engine;
        let payload = token.split('.').nth(1).unwrap();
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Go `generateTokenWithExpiry(p, 3600, nil, true, "")`
    /// (authservice.go:467-501) for the same principal: `tier` carries the
    /// tier, `scope` is omitted (no granted permissions), `clients` and
    /// `applications` are `id:identifier` / `id:code` pairs, `token_use` is
    /// `api`, no `azp`.
    #[test]
    fn access_token_claims_match_go_for_a_client_user() {
        let s = service();
        let claims = s.access_token_claims(&client_user(), 3600, &[], true, None, now());
        assert_eq!(
            wire(&claims),
            json!({
                "iss": "https://fc.example.test",
                "sub": "prn_ADA",
                "aud": "https://fc.example.test",
                "exp": 1_760_003_600,
                "iat": 1_760_000_000,
                "nbf": 1_760_000_000,
                "jti": "JTI",
                "type": "USER",
                "tier": "CLIENT",
                "email": "ada@acme.test",
                "name": "Ada Lovelace",
                "clients": ["clt_A:acme"],
                "roles": ["hr:manager", "platform:viewer"],
                "applications": ["app_1:hr", "app_2"],
                "all_applications": false,
                "token_use": "api"
            })
        );
    }

    /// Go `GenerateAccessTokenWithScopeFor(p, granted, clientID)`: `scope`
    /// is the space-joined granted permissions and `azp` the OAuth client.
    #[test]
    fn scoped_access_token_carries_permissions_and_azp() {
        let s = service();
        let granted = vec![
            "platform:iam:user:view".to_string(),
            "hr:staff:record:view".to_string(),
        ];
        let v = wire(&s.access_token_claims(
            &client_user(),
            3600,
            &granted,
            true,
            Some("oc_hr"),
            now(),
        ));
        assert_eq!(v["scope"], "platform:iam:user:view hr:staff:record:view");
        assert_eq!(v["azp"], "oc_hr");
        assert_eq!(v["tier"], "CLIENT");
        assert_eq!(v["token_use"], "api");
    }

    /// Go `generateTokenWithExpiry(..., authoritative=false, ...)`: identity
    /// only, empty authority arrays, `token_use: identity`.
    #[test]
    fn identity_access_token_matches_go() {
        let s = service();
        let v =
            wire(&s.access_token_claims(&client_user(), 3600, &[], false, Some("oc_hr"), now()));
        assert_eq!(v["token_use"], "identity");
        assert_eq!(v["clients"], json!([]));
        assert_eq!(v["roles"], json!([]));
        assert_eq!(v["applications"], json!([]));
        assert_eq!(v["all_applications"], false);
        assert!(v.get("scope").is_none());
        assert_eq!(v["tier"], "CLIENT");
        assert_eq!(v["azp"], "oc_hr");
    }

    /// Go `buildClients` / `appAccessOf` for the other tiers: an anchor
    /// reaches `*` clients; all-applications is the `*` entry plus the
    /// deprecated flag; a partner's grants are paired where known.
    #[test]
    fn anchor_and_partner_authority_claims_match_go() {
        let s = service();
        let mut anchor = Principal::new_service("svc", "Svc", UserScope::Anchor);
        anchor.all_applications = true;
        let v = wire(&s.access_token_claims(&anchor, 3600, &[], true, None, now()));
        assert_eq!(v["type"], "SERVICE");
        assert_eq!(v["tier"], "ANCHOR");
        assert_eq!(v["clients"], json!(["*"]));
        assert_eq!(v["applications"], json!(["*"]));
        assert_eq!(v["all_applications"], true);
        assert!(v.get("email").is_none());

        let mut partner = Principal::new_user("p@x.test", UserScope::Partner);
        partner.all_applications = false;
        partner.assigned_clients = vec!["clt_B".to_string(), "clt_C".to_string()];
        partner
            .client_identifier_map
            .insert("clt_B".to_string(), "beta".to_string());
        let v = wire(&s.access_token_claims(&partner, 3600, &[], true, None, now()));
        assert_eq!(v["tier"], "PARTNER");
        assert_eq!(v["clients"], json!(["clt_B:beta", "clt_C"]));
        assert_eq!(v["applications"], json!([]));
    }

    /// Go `idTokenClaims` (authservice.go:555-602) for the same principal:
    /// 300-second lifetime, `tier` (not `scope`), `updated_at` = the
    /// principal's, `azp` = `aud` = the relying party, no `nbf`/`jti`.
    #[test]
    fn id_token_claims_match_go() {
        let s = service();
        let p = client_user();
        let claims = s.id_token_claims(&p, "oc_hr", Some("n-1".to_string()), role_names(&p), now());
        assert_eq!(
            wire(&claims),
            json!({
                "iss": "https://fc.example.test",
                "sub": "prn_ADA",
                "aud": "oc_hr",
                "exp": 1_760_000_300,
                "iat": 1_760_000_000,
                "auth_time": 1_760_000_000,
                "nonce": "n-1",
                "name": "Ada Lovelace",
                "email": "ada@acme.test",
                "email_verified": true,
                "updated_at": 1_750_000_000,
                "azp": "oc_hr",
                "type": "USER",
                "tier": "CLIENT",
                "client_id": "clt_A",
                "roles": ["hr:manager", "platform:viewer"],
                "applications": ["app_1:hr", "app_2"],
                "all_applications": false,
                "clients": ["clt_A:acme"]
            })
        );
    }

    #[test]
    fn issued_tokens_round_trip_through_validation() {
        let s = service();
        let p = client_user();
        let claims = s
            .validate_token(&s.generate_access_token(&p).unwrap())
            .unwrap();
        assert_eq!(claims.sub, "prn_ADA");
        assert_eq!(claims.tier, UserScope::Client);
        assert_eq!(claims.scope, None);
        assert_eq!(claims.token_use.as_deref(), Some(TOKEN_USE_API));
        assert_eq!(claims.applications, vec!["app_1:hr", "app_2"]);
        assert!(claims.has_client_access("clt_A"));

        let granted = vec!["platform:iam:user:view".to_string()];
        let claims = s
            .validate_token(
                &s.generate_access_token_with_scope(&p, &granted, None)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(claims.granted_permissions(), granted);
    }

    /// Go `provider.MintSessionToken` → `sessiontoken.Mint`
    /// (provider.go:285-314, sessiontoken.go:79-121) for the same principal:
    /// the subject and email only, an empty `tier`, a false
    /// `all_applications`, the session lifetime; no `aud`, `type`, `jti`,
    /// `token_use` or authority.
    #[test]
    fn session_token_claims_match_go() {
        let s = service();
        assert_eq!(
            serde_json::to_value(s.session_token_claims(&client_user(), now())).unwrap(),
            json!({
                "iss": "https://fc.example.test",
                "sub": "prn_ADA",
                "iat": 1_760_000_000,
                "nbf": 1_760_000_000,
                "exp": 1_760_086_400,
                "tier": "",
                "email": "ada@acme.test",
                "all_applications": false
            })
        );
        let mut no_email = client_user();
        no_email.user_identity = None;
        let v = serde_json::to_value(s.session_token_claims(&no_email, now())).unwrap();
        assert!(v.get("email").is_none(), "{v}");
    }

    #[test]
    fn a_session_token_round_trips_and_is_no_bearer() {
        let s = service();
        let token = s.generate_session_token(&client_user()).unwrap();
        let identity = s.validate_session_token(&token).unwrap();
        assert_eq!(identity.principal_id, "prn_ADA");
        assert!(identity.issued_at.is_some());
        // It carries no `type`: as a bearer it is no access token at all.
        assert!(s.validate_token(&token).is_err());
    }

    /// An RS256 cookie exactly as Go mints it (no `kid`, no `aud`, claims
    /// as `sessiontoken.Mint` writes them) validates under the same key and
    /// issuer: Go-issued sessions survive the cutover.
    #[test]
    fn a_go_issued_session_cookie_is_accepted() {
        let (private_pem, public_pem) = AuthConfig::generate_rsa_keys(None).unwrap();
        let s = AuthService::new(AuthConfig {
            rsa_private_key: Some(private_pem.clone()),
            rsa_public_key: Some(public_pem),
            issuer: "https://fc.example.test".to_string(),
            audience: "https://fc.example.test".to_string(),
            ..AuthConfig::default()
        });
        let now = Utc::now().timestamp();
        let go_cookie = encode(
            &Header::new(Algorithm::RS256),
            &json!({
                "iss": "https://fc.example.test",
                "sub": "prn_GO",
                "iat": now,
                "nbf": now,
                "tier": "",
                "exp": now + 86_400,
                "email": "go@acme.test",
                "all_applications": false
            }),
            &EncodingKey::from_rsa_pem(private_pem.as_bytes()).unwrap(),
        )
        .unwrap();
        let identity = s.validate_session_token(&go_cookie).unwrap();
        assert_eq!(identity.principal_id, "prn_GO");
        assert_eq!(identity.issued_at, Some(now));
    }

    /// Only a session token is a sign-in: an access token (authority or
    /// identity), an ID token for a relying party, a foreign issuer, an
    /// expired or a subject-less token are all refused.
    #[test]
    fn only_a_session_token_validates_as_the_cookie() {
        let s = service();
        let p = client_user();
        for (what, token) in [
            ("access token", s.generate_access_token(&p).unwrap()),
            (
                "identity token",
                s.generate_identity_access_token(&p, Some("oc_hr")).unwrap(),
            ),
            ("id token", s.generate_id_token(&p, "oc_hr", None).unwrap()),
        ] {
            assert!(s.validate_session_token(&token).is_err(), "{what}");
        }

        let now = Utc::now().timestamp();
        let sign = |claims: serde_json::Value| {
            encode(&Header::new(s.algorithm), &claims, &s.encoding_key).unwrap()
        };
        let base = json!({
            "iss": "https://fc.example.test", "sub": "prn_1",
            "iat": now, "exp": now + 600
        });
        assert!(s.validate_session_token(&sign(base.clone())).is_ok());
        let mut with_aud = base.clone();
        with_aud["aud"] = json!("https://fc.example.test");
        assert!(s.validate_session_token(&sign(with_aud)).is_ok());
        for (what, patch) in [
            ("foreign aud", json!({"aud": "oc_hr"})),
            ("foreign iss", json!({"iss": "https://evil.test"})),
            ("expired", json!({"exp": now - 3600})),
            ("empty sub", json!({"sub": ""})),
            ("jti", json!({"jti": "x"})),
            ("type", json!({"type": "USER"})),
            ("token_use", json!({"token_use": "api"})),
        ] {
            let mut claims = base.clone();
            for (k, v) in patch.as_object().unwrap() {
                claims[k] = v.clone();
            }
            assert!(
                s.validate_session_token(&sign(claims)).is_err(),
                "{what} accepted"
            );
        }
    }

    /// AgentPlanner's bearer check (central_agent/flowcatalyst/oidc.py:
    /// 159-174, 270-282): RS256, `aud` = the issuer, `iss` = the issuer,
    /// `exp`/`iat`/`sub` present, `token_use == "api"`, and a `tier` it
    /// recognises (principal.py:81-106).
    #[test]
    fn agentplanner_accepts_a_client_credentials_token() {
        let (private_pem, public_pem) = AuthConfig::generate_rsa_keys(None).unwrap();
        let s = AuthService::new(AuthConfig {
            rsa_private_key: Some(private_pem),
            rsa_public_key: Some(public_pem),
            issuer: "https://fc.example.test".to_string(),
            audience: "https://fc.example.test".to_string(),
            ..AuthConfig::default()
        });
        let mut sa = Principal::new_service("agent-planner", "Agent Planner", UserScope::Client);
        sa.client_id = Some("clt_A".to_string());
        let token = s
            .generate_access_token_with_scope(
                &sa,
                &["agent-planner:planning:run:read".to_string()],
                None,
            )
            .unwrap();
        let header = jsonwebtoken::decode_header(&token).unwrap();
        assert_eq!(header.alg, Algorithm::RS256);
        assert!(header.kid.is_some());
        let v = payload_json(&token);
        assert_eq!(v["aud"], v["iss"]);
        assert_eq!(v["token_use"], "api");
        assert!(["ANCHOR", "PARTNER", "CLIENT"].contains(&v["tier"].as_str().unwrap()));
        for claim in [
            "exp",
            "iat",
            "sub",
            "name",
            "type",
            "roles",
            "clients",
            "applications",
        ] {
            assert!(v.get(claim).is_some(), "{claim} missing");
        }
        assert!(v["all_applications"].is_boolean());
    }

    /// The access-token shape Rust issued before it matched Go: the tier on
    /// `scope`, no `tier`, no `token_use`, applications as codes.
    #[derive(Serialize)]
    struct PreGoAccessClaims {
        sub: String,
        iss: String,
        aud: String,
        exp: i64,
        iat: i64,
        nbf: i64,
        jti: String,
        #[serde(rename = "type")]
        principal_type: String,
        scope: String,
        email: Option<String>,
        name: String,
        clients: Vec<String>,
        roles: Vec<String>,
        applications: Vec<String>,
    }

    #[test]
    fn tokens_issued_in_the_old_shape_still_validate() {
        let s = service();
        let now = Utc::now().timestamp();
        for (ty, scope, want_ty, want_tier) in [
            ("USER", "ANCHOR", PrincipalType::User, UserScope::Anchor),
            (
                "SERVICE",
                "CLIENT",
                PrincipalType::Service,
                UserScope::Client,
            ),
            ("USER", "PARTNER", PrincipalType::User, UserScope::Partner),
        ] {
            let old = PreGoAccessClaims {
                sub: "prn_1".to_string(),
                iss: s.config.issuer.clone(),
                aud: s.config.audience.clone(),
                exp: now + 600,
                iat: now,
                nbf: now,
                jti: "jti_1".to_string(),
                principal_type: ty.to_string(),
                scope: scope.to_string(),
                email: None,
                name: "Old Token".to_string(),
                clients: vec!["*".to_string()],
                roles: vec!["platform:super-admin".to_string()],
                applications: vec!["platform".to_string()],
            };
            let mut header = Header::new(s.algorithm);
            header.kid = s.key_id.clone();
            let token = encode(&header, &old, &s.encoding_key).unwrap();
            let claims = s.validate_token(&token).unwrap();
            assert_eq!(claims.principal_type, want_ty);
            assert_eq!(claims.tier, want_tier);
            // The old scope was the tier, never permissions: they derive
            // from the roles, as before.
            assert!(claims.granted_permissions().is_empty());
            assert_eq!(claims.token_use, None);
            assert!(!claims.is_identity_only());
        }
    }

    /// When both are present `tier` wins and `scope` is permissions.
    #[test]
    fn tier_is_preferred_over_scope() {
        let claims: AccessTokenClaims = serde_json::from_value(json!({
            "sub": "prn_1", "iss": "i", "aud": "a", "exp": 2, "iat": 1,
            "type": "USER", "tier": "PARTNER", "scope": "ANCHOR"
        }))
        .unwrap();
        assert_eq!(claims.tier, UserScope::Partner);
        assert_eq!(claims.granted_permissions(), vec!["ANCHOR"]);

        let untiered: std::result::Result<AccessTokenClaims, _> = serde_json::from_value(json!({
            "sub": "prn_1", "iss": "i", "aud": "a", "exp": 2, "iat": 1,
            "type": "USER", "scope": "platform:iam:user:view"
        }));
        assert!(untiered.is_err(), "a token with no tier is refused");
    }

    #[test]
    fn test_extract_bearer_token() {
        assert_eq!(extract_bearer_token("Bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer_token("bearer abc123"), None);
        assert_eq!(extract_bearer_token("Basic abc123"), None);
    }
}
