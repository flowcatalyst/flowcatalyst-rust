//! Service Account Aggregate
//!
//! Machine-to-machine identity management.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

// Re-export main types
pub use entity::{ServiceAccount, RoleAssignment};
pub use repository::ServiceAccountRepository;
pub use api::{ServiceAccountsState, service_accounts_router};
