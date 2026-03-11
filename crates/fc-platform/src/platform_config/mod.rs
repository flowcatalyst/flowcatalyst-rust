//! Platform Config Aggregate
//!
//! Hierarchical configuration with RBAC access control.

pub mod entity;
pub mod repository;
pub mod api;
pub mod access_entity;
pub mod access_repository;
pub mod access_api;

pub use entity::{PlatformConfig, ConfigScope, ConfigValueType};
pub use repository::PlatformConfigRepository;
pub use access_entity::PlatformConfigAccess;
pub use access_repository::PlatformConfigAccessRepository;
pub use api::{PlatformConfigState, admin_platform_config_router};
pub use access_api::{ConfigAccessState, config_access_router};
