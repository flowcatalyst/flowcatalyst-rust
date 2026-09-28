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
pub mod routes;

// Re-export main types
pub use api::ApplicationsState;
pub use routes::{applications_router, routes};
pub use client_config::ApplicationClientConfig;
pub use client_config_repository::ApplicationClientConfigRepository;
pub use entity::{Application, ApplicationType};
pub use repository::ApplicationRepository;
