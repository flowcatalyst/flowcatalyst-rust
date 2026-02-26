//! Refresh Token Entity
//!
//! Stores refresh tokens for session renewal.
//! Refresh tokens are long-lived and can be used to obtain new access tokens.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, Duration};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;
use crate::TsidGenerator;

/// Default refresh token expiry: 30 days
const REFRESH_TOKEN_EXPIRY_DAYS: i64 = 30;

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
    #[serde(rename = "_id")]
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
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
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
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,

    /// When the token expires
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub expires_at: DateTime<Utc>,

    /// When the token was last used (for monitoring/security)
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
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
    pub fn new(
        token_hash: impl Into<String>,
        principal_id: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: TsidGenerator::generate(),
            token_hash: token_hash.into(),
            principal_id: principal_id.into(),
            oauth_client_id: None,
            scopes: vec![],
            accessible_clients: vec![],
            revoked: false,
            revoked_at: None,
            token_family: None,
            replaced_by: None,
            created_at: now,
            expires_at: now + Duration::days(REFRESH_TOKEN_EXPIRY_DAYS),
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

    /// Set client info (IP and user agent)
    pub fn with_client_info(
        mut self,
        ip: Option<String>,
        user_agent: Option<String>,
    ) -> Self {
        self.created_from_ip = ip;
        self.user_agent = user_agent;
        self
    }

    /// Set the token family ID.
    /// All tokens in a rotation chain share the same family ID.
    pub fn with_token_family(mut self, family: impl Into<String>) -> Self {
        self.token_family = Some(family.into());
        self
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
        use rand::Rng;
        use base64::Engine;

        let mut bytes = [0u8; 32];
        rand::thread_rng().fill(&mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    /// Hash a raw token for storage
    pub fn hash_token(raw_token: &str) -> String {
        use sha2::{Sha256, Digest};
        use base64::Engine;

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
    fn test_with_scopes() {
        let (_, token) = RefreshToken::generate_token_pair("principal-123");
        let token = token.with_scopes(vec!["openid".to_string(), "profile".to_string()]);

        assert_eq!(token.scopes.len(), 2);
        assert!(token.scopes.contains(&"openid".to_string()));
    }
}
