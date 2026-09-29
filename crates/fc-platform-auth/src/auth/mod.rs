//! Authentication Aggregate: the sign-in flows.
//!
//! The token endpoint and OAuth protocol routes, OIDC login, sessions,
//! refresh tokens, authorization codes, the password-reset endpoints and
//! login backoff. The IAM model they work on (OAuth clients, auth configs,
//! the token service, password hashing) is fc-platform-iam's `auth`,
//! re-exported here.

pub use fc_platform_iam::auth::*;

// Core auth
pub mod auth_api;
pub mod login_backoff;
pub mod session_cookie;

// OAuth
pub mod oauth_api;

// OIDC
pub mod jwks_cache;
pub mod oidc_login_api;
pub mod oidc_login_state;
pub mod oidc_login_state_repository;
pub mod oidc_payload_repository;
pub mod oidc_sync_service;

// Authorization codes
pub mod authorization_code;
pub mod authorization_code_repository;

// Password reset API
pub mod password_reset_api;

// Pending auth state (OAuth authorize flow)
pub mod pending_auth_repository;

// Refresh tokens
pub mod refresh_rotation;
pub mod refresh_token;
pub mod refresh_token_repository;

// Re-export main types
pub use oauth_api::OAuthState;
