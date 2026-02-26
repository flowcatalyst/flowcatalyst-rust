//! Authentication Aggregate
//!
//! Authentication, OAuth, and OIDC functionality.

// Auth config
pub mod config_entity;
pub mod config_repository;
pub mod config_api;

// Core auth
pub mod auth_service;
pub mod auth_api;
pub mod password_service;

// OAuth
pub mod oauth_entity;
pub mod oauth_api;
pub mod oauth_clients_api;
pub mod oauth_client_repository;

// OIDC
pub mod oidc_login_state;
pub mod oidc_login_state_repository;
pub mod oidc_login_api;
pub mod oidc_service;
pub mod oidc_sync_service;

// Authorization codes
pub mod authorization_code;
pub mod authorization_code_repository;

// Refresh tokens
pub mod refresh_token;
pub mod refresh_token_repository;

// Re-export main types
pub use config_entity::ClientAuthConfig;
pub use config_repository::ClientAuthConfigRepository;
pub use config_api::{anchor_domains_router, client_auth_configs_router, idp_role_mappings_router, AuthConfigState};
pub use auth_api::auth_router;
pub use auth_service::AuthService;
pub use oauth_api::{oauth_router, OAuthState};
pub use oauth_clients_api::oauth_clients_router;
pub use oidc_login_api::oidc_login_router;
pub use oidc_service::OidcService;
pub use password_service::PasswordService;
