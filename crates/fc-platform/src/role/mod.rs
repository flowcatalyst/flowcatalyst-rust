//! Role Aggregate
//!
//! Role and permission management.

pub mod api;
pub mod ceiling;
pub mod entity;
pub mod operations;
pub mod repository;

// Re-export main types
pub use api::{roles_router, RolesState};
pub use entity::{AuthRole, Permission, RoleSource};
pub use repository::RoleRepository;
