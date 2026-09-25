//! Role Aggregate
//!
//! Role and permission management.

pub mod api;
pub mod ceiling;
pub mod entity;
pub mod operations;
pub mod permission_api;
pub mod permission_catalog;
pub mod permission_repository;
pub mod repository;

// Re-export main types
pub use api::{roles_router, RolesState};
pub use entity::{AuthRole, Permission, RoleSource};
pub use repository::RoleRepository;
