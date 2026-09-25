//! Authentication Aggregate
//!
//! Authentication, OAuth, and OIDC functionality.

// Auth config
pub mod config_api;
pub mod config_entity;
pub mod config_repository;
pub mod operations;

// Core auth
pub mod auth_api;
pub mod auth_service;
pub mod login_backoff;
pub mod password_service;
pub mod session_cookie;
pub mod signing_keys;

// OAuth
pub mod oauth_api;
pub mod oauth_client_repository;
pub mod oauth_clients_api;
pub mod oauth_entity;

// OIDC
pub mod jwks_cache;
pub mod oidc_login_api;
pub mod oidc_login_state;
pub mod oidc_login_state_repository;
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
pub use auth_api::auth_router;
pub use auth_service::AuthService;
pub use config_api::{
    anchor_domains_router, client_auth_configs_router, idp_role_mappings_router, AuthConfigState,
};
pub use config_entity::ClientAuthConfig;
pub use config_repository::ClientAuthConfigRepository;
pub use oauth_api::{oauth_router, OAuthState};
pub use oauth_clients_api::oauth_clients_router;
pub use oidc_login_api::oidc_login_router;
pub use password_service::PasswordService;
