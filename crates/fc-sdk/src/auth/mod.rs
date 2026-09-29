//! Authentication & Authorization
//!
//! OIDC/OAuth2 integration with FlowCatalyst's authentication server.
//!
//! This module provides everything an SDK application needs to authenticate
//! users via FlowCatalyst's OIDC server:
//!
//! - **Token validation** — Validate JWTs using JWKS auto-discovery (RS256)
//!   or shared secret (HS256)
//! - **OAuth2 flows** — Authorization code grant with PKCE, token refresh,
//!   revocation, and introspection
//! - **Auth context** — Rich context with principal info, roles, and
//!   client access for authorization checks
//!
//! # Token Validation (Resource Server)
//!
//! If your app receives tokens and needs to validate them:
//!
//! ```ignore
//! use fc_sdk::auth::{TokenValidator, TokenValidatorConfig};
//!
//! let validator = TokenValidator::new(TokenValidatorConfig {
//!     issuer_url: "https://auth.flowcatalyst.io".to_string(),
//!     audience: "my-app".to_string(),
//!     ..Default::default()
//! });
//!
//! // In your request handler
//! let ctx = validator.validate_bearer("Bearer eyJ...").await?;
//! if ctx.has_role("admin") && ctx.has_client_access("clt_123") {
//!     // Authorized
//! }
//! ```
//!
//! # OAuth2 Authorization Code Flow (Web App)
//!
//! If your app needs to log users in via FlowCatalyst:
//!
//! ```ignore
//! use fc_sdk::auth::{OAuthClient, OAuthConfig};
//!
//! let oauth = OAuthClient::new(OAuthConfig {
//!     issuer_url: "https://auth.flowcatalyst.io".to_string(),
//!     client_id: "my-app".to_string(),
//!     client_secret: Some("secret".to_string()),
//!     redirect_uri: "https://myapp.example.com/callback".to_string(),
//!     ..Default::default()
//! });
//!
//! // 1. Redirect user to FlowCatalyst for login
//! let (url, params) = oauth.authorize_url();
//! // Store params.pkce.code_verifier + params.state in session
//!
//! // 2. Handle callback
//! let tokens = oauth.exchange_code(&code, &stored_verifier).await?;
//!
//! // 3. Refresh when needed
//! let new_tokens = oauth.refresh_token(&tokens.refresh_token.unwrap()).await?;
//!
//! // 4. Logout — id_token_hint is required when post_logout_redirect_uri
//! // is supplied (FlowCatalyst uses it to verify the redirect against the
//! // client's registered postLogoutRedirectUris).
//! let logout_url = oauth.logout_url(
//!     Some("https://myapp.example.com"),
//!     Some(&tokens.id_token.unwrap()),
//!     None,
//! );
//! ```

pub mod claims;
pub mod jwks;
pub mod oauth;

#[cfg(feature = "axum")]
pub mod axum;

pub use claims::{AccessTokenClaims, AuthContext};
pub use jwks::{HmacTokenValidator, JwksCache, TokenValidator, TokenValidatorConfig};
pub use oauth::{
    AuthorizeParams, IntrospectionResponse, OAuthClient, OAuthConfig, PkceChallenge, TokenResponse,
    UserInfoResponse,
};

/// Authentication errors.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Token has expired.
    #[error("Token has expired")]
    TokenExpired,

    /// Token is invalid (bad signature, wrong issuer/audience, malformed).
    #[error("Invalid token: {0}")]
    InvalidToken(String),

    /// OIDC discovery or JWKS fetch failed.
    #[error("Discovery error: {0}")]
    Discovery(String),

    /// Token exchange or OAuth2 flow error.
    #[error("Token exchange error: {0}")]
    TokenExchange(String),

    /// Configuration error (bad key length, missing required field, etc.).
    #[error("Config error: {0}")]
    Config(String),

    /// Cryptographic operation failed (encrypt/decrypt/sign/verify).
    #[error("Crypto error: {0}")]
    Crypto(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error;

    #[test]
    fn auth_error_token_expired_display() {
        let err = AuthError::TokenExpired;
        assert_eq!(format!("{}", err), "Token has expired");
    }

    #[test]
    fn auth_error_invalid_token_display() {
        let err = AuthError::InvalidToken("bad signature".to_string());
        assert_eq!(format!("{}", err), "Invalid token: bad signature");
    }

    #[test]
    fn auth_error_discovery_display() {
        let err = AuthError::Discovery("connection refused".to_string());
        assert_eq!(format!("{}", err), "Discovery error: connection refused");
    }

    #[test]
    fn auth_error_token_exchange_display() {
        let err = AuthError::TokenExchange("HTTP 500".to_string());
        assert_eq!(format!("{}", err), "Token exchange error: HTTP 500");
    }

    #[test]
    fn auth_error_is_std_error() {
        let err = AuthError::TokenExpired;
        let _: &dyn error::Error = &err;
    }

    #[test]
    fn auth_error_debug() {
        let err = AuthError::InvalidToken("test".to_string());
        let debug = format!("{:?}", err);
        assert!(debug.contains("InvalidToken"));
        assert!(debug.contains("test"));
    }
}
