//! Application Aggregate
//!
//! Platform applications and integrations.

pub mod entity;
pub mod client_config;
pub mod repository;
pub mod client_config_repository;
pub mod api;
pub mod operations;

// Re-export main types
pub use entity::{Application, ApplicationType};
pub use client_config::ApplicationClientConfig;
pub use repository::ApplicationRepository;
pub use client_config_repository::ApplicationClientConfigRepository;
pub use api::{ApplicationsState, applications_router};
