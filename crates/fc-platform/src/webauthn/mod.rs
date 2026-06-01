//! WebAuthn / Passkeys
//!
//! Public-key credential support for internal-auth principals: a principal
//! qualifies iff `user_identity.password_hash.is_some()` AND
//! `user_identity.external_id.is_none()`. Federated principals (managed by
//! an external IdP) never have credentials here — the IdP owns identity.
//! The gate is per-principal, not per-domain; a single email domain can
//! mix local and federated accounts. See `project_passkeys_scope.md`.

pub mod api;
pub mod ceremony_repository;
pub mod entity;
pub mod gate;
pub mod operations;
pub mod repository;
pub mod webauthn_service;

pub use api::{webauthn_router, WebauthnApiState};
pub use ceremony_repository::WebauthnCeremonyRepository;
pub use webauthn_service::WebauthnService;
