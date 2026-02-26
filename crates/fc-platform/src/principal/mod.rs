//! Principal Aggregate
//!
//! User and service account identity management.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

// Re-export main types
pub use entity::{Principal, PrincipalType, UserScope, UserIdentity};
pub use repository::PrincipalRepository;
pub use api::{PrincipalsState, principals_router};
