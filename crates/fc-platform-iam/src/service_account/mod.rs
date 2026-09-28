//! Service Account Aggregate
//!
//! Machine-to-machine identity management.

pub mod api;
pub mod entity;
pub mod operations;
pub mod outbound_credentials;
pub mod repository;
pub mod signing_account;

// Re-export main types
pub use api::ServiceAccountsState;
pub use entity::{RoleAssignment, ServiceAccount};
pub use repository::ServiceAccountRepository;
