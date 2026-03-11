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
//! // 4. Logout
//! let logout_url = oauth.logout_url(Some("https://myapp.example.com"), None);
//! ```

pub mod claims;
pub mod jwks;
pub mod oauth;

pub use claims::{AccessTokenClaims, AuthContext};
pub use jwks::{TokenValidator, TokenValidatorConfig, HmacTokenValidator, JwksCache};
pub use oauth::{
    OAuthClient, OAuthConfig, PkceChallenge, AuthorizeParams,
    TokenResponse, IntrospectionResponse, UserInfoResponse,
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
}
