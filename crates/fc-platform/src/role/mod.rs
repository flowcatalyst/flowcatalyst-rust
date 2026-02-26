//! Role Aggregate
//!
//! Role and permission management.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

// Re-export main types
pub use entity::{AuthRole, RoleSource, Permission};
pub use repository::RoleRepository;
pub use api::{RolesState, roles_router};
