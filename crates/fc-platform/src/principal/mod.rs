//! Principal Aggregate
//!
//! User and service account identity management.

pub mod admin;
pub mod api;
pub mod entity;
pub mod go_api;
pub mod operations;
pub mod repository;

// Re-export main types
pub use api::{principals_router, PrincipalsState};
pub use entity::{Principal, PrincipalType, UserIdentity, UserScope};
pub use repository::PrincipalRepository;
