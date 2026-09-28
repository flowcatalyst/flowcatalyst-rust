//! WebAuthn / Passkeys
//!
//! Public-key credential support for internal-auth users (those whose email
//! domain has no row in `email_domain_mapping`). Federated users never have
//! credentials here — see `project_passkeys_scope.md` for rationale.

pub mod api;
pub mod ceremony_repository;
pub mod entity;
pub mod gate;
pub mod operations;
pub mod repository;
pub mod routes;
pub mod webauthn_service;

pub use api::WebauthnApiState;
pub use routes::{routes, webauthn_router};
pub use ceremony_repository::WebauthnCeremonyRepository;
pub use webauthn_service::WebauthnService;
