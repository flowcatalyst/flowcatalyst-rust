//! Service Account Aggregate
//!
//! Machine-to-machine identity management.

pub mod admin_api;
pub mod api;
pub mod entity;
pub mod operations;
pub mod outbound_credentials;
pub mod repository;
pub mod signing_reach;

// Re-export main types
pub use api::{service_accounts_router, ServiceAccountsState};
pub use entity::{RoleAssignment, ServiceAccount};
pub use repository::ServiceAccountRepository;
