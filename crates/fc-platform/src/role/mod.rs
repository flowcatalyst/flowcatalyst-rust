//! Role Aggregate
//!
//! Role and permission management.

pub mod api;
pub mod bff;
pub mod ceiling;
pub mod entity;
pub mod operations;
pub mod permission_api;
pub mod permission_catalog;
pub mod permission_repository;
pub mod repository;
pub mod routes;

// Re-export main types
pub use api::RolesState;
pub use routes::{roles_router, routes};
pub use entity::{AuthRole, Permission, RoleSource};
pub use repository::RoleRepository;
