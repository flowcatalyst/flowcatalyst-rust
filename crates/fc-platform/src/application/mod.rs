//! Application Aggregate
//!
//! Platform applications and integrations.

pub mod api;
pub mod client_config;
pub mod client_config_repository;
pub mod entity;
pub mod go_api;
pub mod operations;
pub mod repository;

// Re-export main types
pub use api::{applications_router, ApplicationsState};
pub use client_config::ApplicationClientConfig;
pub use client_config_repository::ApplicationClientConfigRepository;
pub use entity::{Application, ApplicationType};
pub use repository::ApplicationRepository;
