//! Service Account Aggregate
//!
//! Machine-to-machine identity management.

pub mod api;
pub mod entity;
pub mod operations;
pub mod outbound_credentials;
pub mod repository;

// Re-export main types
pub use api::{service_accounts_router, ServiceAccountsState};
pub use entity::{RoleAssignment, ServiceAccount};
pub use repository::ServiceAccountRepository;
