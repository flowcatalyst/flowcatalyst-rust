//! Refresh Token Entity
//!
//! Stores refresh tokens for session renewal.
//! Refresh tokens are long-lived and can be used to obtain new access tokens.

use crate::shared::tsid;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// The lifetime stamped on a freshly issued refresh token, in seconds, and so
/// the family's absolute cap: rotation carries the first deadline forward.
/// Go's `grantstore.RefreshTokenTTL` — a process-wide setting, one week by
/// default, set once at startup from `OIDC_REFRESH_TOKEN_TTL` (see
/// `server_setup::auth_init`).
static REFRESH_TOKEN_TTL_SECS: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(7 * 24 * 60 * 60);

/// Set the refresh-token lifetime; a non-positive value is ignored.
pub fn set_refresh_token_ttl_secs(secs: i64) {
    if secs > 0 {
        REFRESH_TOKEN_TTL_SECS.store(secs, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The configured refresh-token lifetime.
pub fn refresh_token_ttl() -> Duration {
    Duration::seconds(REFRESH_TOKEN_TTL_SECS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Refresh token entity
///
/// Stored in the database to enable:
/// 1. Token validation and exchange for new access tokens
/// 2. Token revocation (logout, security events)
/// 3. Token rotation (issue new refresh token on use)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshToken {
    /// TSID as primary key
    pub id: String,

    /// The actual token value (cryptographically random, hashed for storage)
    /// Only the hash is stored; the raw token is returned to the client once
    pub token_hash: String,

    /// Principal ID (user or service account)
    pub principal_id: String,

    /// OAuth client ID (optional - set for OAuth flows)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,

    /// Scopes granted with this token
    #[serde(default)]
    pub scopes: Vec<String>,

    /// Client IDs this token grants access to
    #[serde(default)]
    pub accessible_clients: Vec<String>,

    /// Whether this token has been revoked
    #[serde(default)]
    pub revoked: bool,

    /// When the token was revoked (if revoked)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,

    /// Token family ID for rotation tracking.
    /// All tokens in a rotation chain share the same family ID.
    /// Used to detect token reuse attacks and revoke entire families.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_family: Option<String>,

    /// Hash of the token that replaced this one during rotation.
    /// Set when a new token is issued using this refresh token.
    /// If a token with replaced_by is used again, it indicates a reuse attack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<String>,

    /// When the token was created
    pub created_at: DateTime<Utc>,

    /// When the token expires
    pub expires_at: DateTime<Utc>,

    /// When the token was last used (for monitoring/security)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,

    /// IP address of the client that created this token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_from_ip: Option<String>,

    /// User agent of the client that created this token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

impl RefreshToken {
    /// Create a new refresh token
    ///
    /// Note: The raw token should be generated separately and hashed before storage.
    /// Use `generate_token_pair()` to create both the raw token and entity.
    pub fn new(token_hash: impl Into<String>, principal_id: impl Into<String>) -> Self {
        let now = Utc::now();
        let id = tsid::generate_untyped();
        Self {
            // A fresh token roots its own rotation family, so even the
            // first token of a login is caught if replayed after rotation.
            token_family: Some(id.clone()),
            id,
            token_hash: token_hash.into(),
            principal_id: principal_id.into(),
            oauth_client_id: None,
            scopes: vec![],
            accessible_clients: vec![],
            revoked: false,
            revoked_at: None,
            replaced_by: None,
            created_at: now,
            expires_at: now + refresh_token_ttl(),
            last_used_at: None,
            created_from_ip: None,
            user_agent: None,
        }
    }

    /// Create with custom expiry duration
    pub fn with_expiry(mut self, expiry: Duration) -> Self {
        self.expires_at = self.created_at + expiry;
        self
    }

    /// Set OAuth client ID
    pub fn with_oauth_client(mut self, client_id: impl Into<String>) -> Self {
        self.oauth_client_id = Some(client_id.into());
        self
    }

    /// Set scopes
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Set accessible clients
    pub fn with_accessible_clients(mut self, clients: Vec<String>) -> Self {
        self.accessible_clients = clients;
        self
    }

    /// Set the token family ID.
    /// All tokens in a rotation chain share the same family ID.
    pub fn with_token_family(mut self, family: impl Into<String>) -> Self {
        self.token_family = Some(family.into());
        self
    }

    /// The token that replaces this one on rotation, and its raw value
    /// (handed to the caller once). It keeps this token's lineage — OAuth
    /// client binding, scopes, accessible clients — stays in its family (a
    /// legacy token without one roots the family at its own id), and
    /// inherits its expiry: the family's absolute cap, never extended by
    /// rotating (Go `grantstore.Rotate`, Java `RefreshRotation.successorOf`).
    pub fn successor(&self) -> (String, Self) {
        let (raw, mut next) = Self::generate_token_pair(&self.principal_id);
        next.oauth_client_id = self.oauth_client_id.clone();
        next.scopes = self.scopes.clone();
        next.accessible_clients = self.accessible_clients.clone();
        next.expires_at = self.expires_at;
        next.token_family = Some(self.family());
        (raw, next)
    }

    /// The rotation family this token belongs to: its recorded family, or
    /// for a legacy token that predates tracking, its own id.
    pub fn family(&self) -> String {
        self.token_family.clone().unwrap_or_else(|| self.id.clone())
    }

    /// Check if the token is valid (not expired and not revoked)
    pub fn is_valid(&self) -> bool {
        !self.revoked && Utc::now() < self.expires_at
    }

    /// Check if the token has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Check if this token has been replaced (used in rotation).
    /// A replaced token being used again indicates a token reuse attack.
    pub fn was_replaced(&self) -> bool {
        self.replaced_by.is_some()
    }

    /// Revoke the token
    pub fn revoke(&mut self) {
        self.revoked = true;
        self.revoked_at = Some(Utc::now());
    }

    /// Mark this token as replaced during token rotation.
    /// Records the hash of the new token that replaced this one.
    pub fn mark_replaced(&mut self, new_token_hash: impl Into<String>) {
        self.replaced_by = Some(new_token_hash.into());
    }

    /// Update last used timestamp
    pub fn mark_used(&mut self) {
        self.last_used_at = Some(Utc::now());
    }

    /// Generate a cryptographically random token string
    pub fn generate_raw_token() -> String {
        use base64::Engine;
        use rand::Rng;

        let mut bytes = [0u8; 32];
        rand::rng().fill(&mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    /// Hash a raw token for storage
    pub fn hash_token(raw_token: &str) -> String {
        use base64::Engine;
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(raw_token.as_bytes());
        let hash = hasher.finalize();
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
    }

    /// Generate a token pair (raw token for client, entity for storage)
    pub fn generate_token_pair(principal_id: impl Into<String>) -> (String, Self) {
        let raw_token = Self::generate_raw_token();
        let token_hash = Self::hash_token(&raw_token);
        let entity = Self::new(token_hash, principal_id);
        (raw_token, entity)
    }
}

// Note: Conversion from oauth_oidc_payloads is handled in RefreshTokenRepository::from_model

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_token() {
        let (raw, token) = RefreshToken::generate_token_pair("principal-123");

        assert!(!raw.is_empty());
        assert_eq!(token.principal_id, "principal-123");
        assert!(!token.revoked);
        assert!(token.is_valid());
        assert!(!token.is_expired());
    }

    #[test]
    fn test_token_hashing() {
        let raw = RefreshToken::generate_raw_token();
        let hash1 = RefreshToken::hash_token(&raw);
        let hash2 = RefreshToken::hash_token(&raw);

        // Same input produces same hash
        assert_eq!(hash1, hash2);

        // Different input produces different hash
        let raw2 = RefreshToken::generate_raw_token();
        let hash3 = RefreshToken::hash_token(&raw2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_revoke_token() {
        let (_, mut token) = RefreshToken::generate_token_pair("principal-123");
        assert!(token.is_valid());

        token.revoke();
        assert!(!token.is_valid());
        assert!(token.revoked);
    }

    #[test]
    fn test_with_oauth_client() {
        let (_, token) = RefreshToken::generate_token_pair("principal-123");
        let token = token.with_oauth_client("oauth-client-456");

        assert_eq!(token.oauth_client_id, Some("oauth-client-456".to_string()));
    }

    #[test]
    fn a_new_token_roots_its_family() {
        let (_, token) = RefreshToken::generate_token_pair("principal-123");
        assert_eq!(token.token_family.as_deref(), Some(token.id.as_str()));
    }

    /// The successor keeps the lineage and the family, and inherits the
    /// expiry instead of a fresh 30 days.
    #[test]
    fn the_successor_inherits_lineage_family_and_expiry() {
        let (_, stored) = RefreshToken::generate_token_pair("prn_1");
        let mut stored = stored
            .with_oauth_client("oc_planner")
            .with_scopes(vec!["openid".to_string(), "offline_access".to_string()])
            .with_accessible_clients(vec!["clt_A".to_string()]);
        stored.expires_at = Utc::now() + Duration::days(2);

        let (raw, next) = stored.successor();
        assert_eq!(RefreshToken::hash_token(&raw), next.token_hash);
        assert_ne!(next.id, stored.id);
        assert_eq!(next.principal_id, "prn_1");
        assert_eq!(next.oauth_client_id.as_deref(), Some("oc_planner"));
        assert_eq!(next.scopes, stored.scopes);
        assert_eq!(next.accessible_clients, stored.accessible_clients);
        assert_eq!(next.expires_at, stored.expires_at);
        assert_eq!(next.token_family, stored.token_family);

        // A legacy token (no family) roots the family at its own id.
        stored.token_family = None;
        let (_, next) = stored.successor();
        assert_eq!(next.token_family.as_deref(), Some(stored.id.as_str()));
    }

    #[test]
    fn test_with_scopes() {
        let (_, token) = RefreshToken::generate_token_pair("principal-123");
        let token = token.with_scopes(vec!["openid".to_string(), "profile".to_string()]);

        assert_eq!(token.scopes.len(), 2);
        assert!(token.scopes.contains(&"openid".to_string()));
    }
}
