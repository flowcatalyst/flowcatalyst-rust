//! Self-service developer API credentials (Go
//! `principal/operations/developer_credential.go`, `principal/api/api.go`):
//! a USER principal holding `platform:developer` can mint client_credentials
//! tokens as themselves (`client_id` = their principal id) with a dedicated,
//! rotatable secret — never their login password.
//!
//! - `GET /api/principals/developer-users` — the developer-role users
//! - `POST /api/principals/{id}/developer-credential` — set or rotate; the
//!   plaintext secret is answered exactly once
//! - `DELETE /api/principals/{id}/developer-credential` — revoke
//!
//! The secret is stored as the app key's keyed hash (`hashed:v1:`, the same
//! shape as OAuth client secrets) on the principal's row, written through
//! [`PrincipalRepository`](crate::PrincipalRepository) by the unit of work.
//! `/oauth/token` verifies it (`auth::oauth_api`).

pub mod api;
pub mod events;
pub mod operations;

pub use api::developer_credentials_router;

/// The role that may hold a developer credential (Go `developerRoleName`).
pub const DEVELOPER_ROLE: &str = "platform:developer";

/// A principal's developer secret, as the unit of work persists it: the
/// keyed hash, or none (revoked).
#[derive(Debug, Clone)]
pub struct DeveloperCredential {
    pub principal_id: String,
    pub secret_ref: Option<String>,
}
