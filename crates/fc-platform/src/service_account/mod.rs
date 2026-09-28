//! Service Account Aggregate
//!
//! Machine-to-machine identity management.

pub mod admin_api;
pub mod api;
pub mod entity;
pub mod operations;
pub mod outbound_credentials;
pub mod repository;
pub mod routes;
pub mod signing_reach;

// Re-export main types
pub use api::ServiceAccountsState;
pub use routes::{routes, service_accounts_router};
pub use entity::{RoleAssignment, ServiceAccount};
pub use repository::ServiceAccountRepository;
