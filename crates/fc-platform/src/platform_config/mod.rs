//! Platform Config Aggregate
//!
//! Hierarchical configuration with RBAC access control.

pub mod access_api;
pub mod access_entity;
pub mod access_repository;
pub mod api;
pub mod entity;
pub mod go_api;
pub mod operations;
pub mod repository;
pub mod routes;

pub use access_api::ConfigAccessState;
pub use access_entity::PlatformConfigAccess;
pub use access_repository::PlatformConfigAccessRepository;
pub use api::PlatformConfigState;
pub use entity::{ConfigScope, ConfigValueType, PlatformConfig};
pub use repository::PlatformConfigRepository;
pub use routes::{admin_platform_config_router, config_access_router, routes};
