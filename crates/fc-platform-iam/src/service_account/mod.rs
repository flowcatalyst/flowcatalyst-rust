//! Service Account Aggregate
//!
//! Machine-to-machine identity management.

pub mod api;
pub mod entity;
pub mod operations;
pub mod outbound_credentials;
pub mod repository;

use fc_platform_core::shared::id::PrincipalId;

/// A service account's id as a client names it, in a path or a body: the
/// SERVICE principal's `prn_` id, or the account's own `sac_` id (the id the
/// API answers with). The repository finds the account by either, so one id
/// kind travels in a `PrincipalId` here: a known seam of two kinds sharing one
/// parameter, not a checked principal id. The value is wrapped as sent
/// ([`PrincipalId::from_wire`]), which is what the plain strings did.
pub fn account_or_principal_id(raw: impl Into<String>) -> PrincipalId {
    PrincipalId::from_wire(raw)
}

// Re-export main types
pub use api::ServiceAccountsState;
pub use entity::{RoleAssignment, ServiceAccount};
pub use repository::ServiceAccountRepository;
