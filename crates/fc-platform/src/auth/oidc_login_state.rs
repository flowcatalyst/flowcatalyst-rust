//! OIDC Login State Entity
//!
//! Stores OIDC login state for the authorization code flow.
//! Used to correlate the callback with the original login request
//! and prevent CSRF attacks.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, Duration};

/// OIDC login state for authorization code flow
///
/// This entity stores the state needed to:
/// 1. Validate the callback is legitimate (CSRF protection via state)
/// 2. Prevent replay attacks (nonce validation)
/// 3. Exchange the code securely (PKCE code_verifier)
/// 4. Resume OAuth flows (when login was triggered by /oauth/authorize)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OidcLoginState {
    /// Random state parameter - primary key and CSRF token
    pub state: String,

    /// The email domain that initiated this login
    pub email_domain: String,

    /// The IdentityProvider ID used for this login
    pub identity_provider_id: String,

    /// The EmailDomainMapping ID that matched this login
    pub email_domain_mapping_id: String,

    /// Nonce for ID token validation (prevents replay attacks)
    pub nonce: String,

    /// PKCE code verifier (we store it locally, send challenge to IDP)
    pub code_verifier: String,

    /// Where to redirect after successful login (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_url: Option<String>,

    // ==================== OAuth Flow Chaining ====================
    // These fields are populated when login is triggered from /oauth/authorize
    // After OIDC login completes, we resume the original OAuth flow

    /// Original OAuth client ID (if login was triggered by /oauth/authorize)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,

    /// Original OAuth redirect URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_redirect_uri: Option<String>,

    /// Original OAuth scope
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_scope: Option<String>,

    /// Original OAuth state (from client app)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_state: Option<String>,

    /// Original OAuth PKCE code challenge
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_code_challenge: Option<String>,

    /// Original OAuth PKCE code challenge method
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_code_challenge_method: Option<String>,

    /// Original OAuth nonce (from client app)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_nonce: Option<String>,

    /// OIDC interaction UID (optional, for interaction-based flows)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_uid: Option<String>,

    // ==================== Timestamps ====================

    /// When this state was created
    pub created_at: DateTime<Utc>,

    /// When this state expires (10 minutes from creation)
    pub expires_at: DateTime<Utc>,
}

/// Default expiry duration: 10 minutes
const STATE_EXPIRY_SECONDS: i64 = 600;

impl OidcLoginState {
    /// Create a new OIDC login state
    pub fn new(
        state: impl Into<String>,
        email_domain: impl Into<String>,
        identity_provider_id: impl Into<String>,
        email_domain_mapping_id: impl Into<String>,
        nonce: impl Into<String>,
        code_verifier: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            state: state.into(),
            email_domain: email_domain.into().to_lowercase(),
            identity_provider_id: identity_provider_id.into(),
            email_domain_mapping_id: email_domain_mapping_id.into(),
            nonce: nonce.into(),
            code_verifier: code_verifier.into(),
            return_url: None,
            oauth_client_id: None,
            oauth_redirect_uri: None,
            oauth_scope: None,
            oauth_state: None,
            oauth_code_challenge: None,
            oauth_code_challenge_method: None,
            oauth_nonce: None,
            interaction_uid: None,
            created_at: now,
            expires_at: now + Duration::seconds(STATE_EXPIRY_SECONDS),
        }
    }

    /// Set the return URL
    pub fn with_return_url(mut self, return_url: impl Into<String>) -> Self {
        self.return_url = Some(return_url.into());
        self
    }

    /// Set OAuth flow chaining parameters
    pub fn with_oauth_params(
        mut self,
        client_id: Option<String>,
        redirect_uri: Option<String>,
        scope: Option<String>,
        state: Option<String>,
        code_challenge: Option<String>,
        code_challenge_method: Option<String>,
        nonce: Option<String>,
    ) -> Self {
        self.oauth_client_id = client_id;
        self.oauth_redirect_uri = redirect_uri;
        self.oauth_scope = scope;
        self.oauth_state = state;
        self.oauth_code_challenge = code_challenge;
        self.oauth_code_challenge_method = code_challenge_method;
        self.oauth_nonce = nonce;
        self
    }

    /// Check if this state has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if this state is valid (not expired)
    pub fn is_valid(&self) -> bool {
        !self.is_expired()
    }

    /// Check if this login is part of an OAuth flow
    /// (i.e., was triggered from /oauth/authorize)
    pub fn is_oauth_flow(&self) -> bool {
        self.oauth_client_id.is_some()
    }
}

/// Conversion from SeaORM model
impl From<crate::entities::iam_oidc_login_states::Model> for OidcLoginState {
    fn from(m: crate::entities::iam_oidc_login_states::Model) -> Self {
        Self {
            state: m.state,
            email_domain: m.email_domain,
            identity_provider_id: m.identity_provider_id,
            email_domain_mapping_id: m.email_domain_mapping_id,
            nonce: m.nonce,
            code_verifier: m.code_verifier,
            return_url: m.return_url,
            oauth_client_id: m.oauth_client_id,
            oauth_redirect_uri: m.oauth_redirect_uri,
            oauth_scope: m.oauth_scope,
            oauth_state: m.oauth_state,
            oauth_code_challenge: m.oauth_code_challenge,
            oauth_code_challenge_method: m.oauth_code_challenge_method,
            oauth_nonce: m.oauth_nonce,
            interaction_uid: m.interaction_uid,
            created_at: m.created_at.with_timezone(&Utc),
            expires_at: m.expires_at.with_timezone(&Utc),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_state() {
        let state = OidcLoginState::new(
            "random-state-123",
            "example.com",
            "idp-456",
            "edm-789",
            "nonce-789",
            "verifier-abc",
        );

        assert_eq!(state.state, "random-state-123");
        assert_eq!(state.email_domain, "example.com");
        assert_eq!(state.identity_provider_id, "idp-456");
        assert_eq!(state.email_domain_mapping_id, "edm-789");
        assert_eq!(state.nonce, "nonce-789");
        assert_eq!(state.code_verifier, "verifier-abc");
        assert!(!state.is_expired());
        assert!(state.is_valid());
        assert!(!state.is_oauth_flow());
    }

    #[test]
    fn test_with_oauth_params() {
        let state = OidcLoginState::new(
            "state",
            "example.com",
            "idp-id",
            "edm-id",
            "nonce",
            "verifier",
        ).with_oauth_params(
            Some("client123".to_string()),
            Some("https://app.example.com/callback".to_string()),
            Some("openid profile".to_string()),
            Some("client-state".to_string()),
            None,
            None,
            None,
        );

        assert!(state.is_oauth_flow());
        assert_eq!(state.oauth_client_id, Some("client123".to_string()));
    }

    #[test]
    fn test_email_domain_lowercase() {
        let state = OidcLoginState::new(
            "state",
            "EXAMPLE.COM",
            "idp-id",
            "edm-id",
            "nonce",
            "verifier",
        );

        assert_eq!(state.email_domain, "example.com");
    }
}
