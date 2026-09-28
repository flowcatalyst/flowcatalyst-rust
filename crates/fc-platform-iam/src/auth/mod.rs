//! Authentication Aggregate: the IAM model.
//!
//! OAuth clients, anchor domains, auth configs and IdP role mappings (their
//! entities, repositories, operations and admin APIs), the token service
//! (`auth_service`), password hashing and the signing keys. The sign-in
//! flows (token endpoint, OIDC login, sessions, refresh tokens,
//! authorization codes, password reset) are fc-platform-auth's, whose
//! `auth` module re-exports this one.

// Auth config
pub mod config_api;
pub mod config_entity;
pub mod config_repository;
pub mod operations;

// Tokens, passwords, keys
pub mod auth_service;
pub mod password_reset_emailer;
pub mod password_service;
pub mod signing_keys;

// OAuth clients
pub mod oauth_client_repository;
pub mod oauth_clients_api;
pub mod oauth_entity;

// Re-export main types
pub use auth_service::AuthService;
pub use config_api::AuthConfigState;
pub use config_entity::ClientAuthConfig;
pub use config_repository::ClientAuthConfigRepository;
pub use password_service::PasswordService;
