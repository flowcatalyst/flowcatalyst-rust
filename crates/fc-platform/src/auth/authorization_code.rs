//! Authorization Code Domain Model
//!
//! OAuth2 authorization codes for the authorization code flow.
//! Codes are short-lived (10 minutes), single-use, and bound to PKCE.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;

/// Authorization code for OAuth2 authorization code flow.
///
/// Matches Java AuthorizationCode entity for cross-platform compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationCode {
    /// The authorization code value (64 char random string).
    /// This is the MongoDB _id field.
    #[serde(rename = "_id")]
    pub code: String,

    /// OAuth client that initiated this authorization.
    pub client_id: String,

    /// The authenticated principal.
    pub principal_id: String,

    /// Redirect URI used in the authorization request.
    /// Must match exactly during token exchange.
    pub redirect_uri: String,

    /// Requested scopes.
    pub scope: Option<String>,

    /// PKCE code challenge (required for public clients).
    pub code_challenge: Option<String>,

    /// PKCE challenge method (S256 or plain).
    pub code_challenge_method: Option<String>,

    /// OIDC nonce for replay protection.
    pub nonce: Option<String>,

    /// Client-provided state for CSRF protection.
    pub state: Option<String>,

    /// Client context for the authorization.
    pub context_client_id: Option<String>,

    /// When this code was created.
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,

    /// When this code expires.
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub expires_at: DateTime<Utc>,

    /// Whether this code has been used (single-use enforcement).
    pub used: bool,
}

impl AuthorizationCode {
    /// Default expiration time for authorization codes (10 minutes)
    const DEFAULT_EXPIRY_MINUTES: i64 = 10;

    /// Create a new authorization code.
    pub fn new(
        code: String,
        client_id: String,
        principal_id: String,
        redirect_uri: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            code,
            client_id,
            principal_id,
            redirect_uri,
            scope: None,
            code_challenge: None,
            code_challenge_method: None,
            nonce: None,
            state: None,
            context_client_id: None,
            created_at: now,
            expires_at: now + Duration::minutes(Self::DEFAULT_EXPIRY_MINUTES),
            used: false,
        }
    }

    /// Set the scope.
    pub fn with_scope(mut self, scope: Option<String>) -> Self {
        self.scope = scope;
        self
    }

    /// Set PKCE challenge.
    pub fn with_pkce(mut self, challenge: String, method: String) -> Self {
        self.code_challenge = Some(challenge);
        self.code_challenge_method = Some(method);
        self
    }

    /// Set OIDC nonce.
    pub fn with_nonce(mut self, nonce: Option<String>) -> Self {
        self.nonce = nonce;
        self
    }

    /// Set state.
    pub fn with_state(mut self, state: Option<String>) -> Self {
        self.state = state;
        self
    }

    /// Set context client ID.
    pub fn with_context_client(mut self, client_id: Option<String>) -> Self {
        self.context_client_id = client_id;
        self
    }

    /// Check if this code is expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if this code is valid (not used and not expired).
    pub fn is_valid(&self) -> bool {
        !self.used && !self.is_expired()
    }

    /// Mark this code as used.
    pub fn mark_used(&mut self) {
        self.used = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_code() {
        let code = AuthorizationCode::new(
            "test-code".to_string(),
            "client-123".to_string(),
            "principal-456".to_string(),
            "https://example.com/callback".to_string(),
        );

        assert_eq!(code.code, "test-code");
        assert_eq!(code.client_id, "client-123");
        assert!(!code.used);
        assert!(code.is_valid());
    }

    #[test]
    fn test_code_with_pkce() {
        let code = AuthorizationCode::new(
            "test-code".to_string(),
            "client-123".to_string(),
            "principal-456".to_string(),
            "https://example.com/callback".to_string(),
        )
        .with_pkce("challenge".to_string(), "S256".to_string());

        assert_eq!(code.code_challenge, Some("challenge".to_string()));
        assert_eq!(code.code_challenge_method, Some("S256".to_string()));
    }

    #[test]
    fn test_mark_used() {
        let mut code = AuthorizationCode::new(
            "test-code".to_string(),
            "client-123".to_string(),
            "principal-456".to_string(),
            "https://example.com/callback".to_string(),
        );

        assert!(code.is_valid());
        code.mark_used();
        assert!(!code.is_valid());
    }
}
