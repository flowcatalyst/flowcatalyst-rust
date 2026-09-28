//! Platform Config Aggregate
//!
//! Hierarchical configuration with RBAC access control.

pub mod access;
pub mod access_api;
pub mod access_entity;
pub mod access_repository;
pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;

pub use access_api::ConfigAccessState;
pub use access_entity::PlatformConfigAccess;
pub use access_repository::PlatformConfigAccessRepository;
pub use api::PlatformConfigState;
pub use entity::{ConfigScope, ConfigValueType, PlatformConfig};
pub use repository::PlatformConfigRepository;
