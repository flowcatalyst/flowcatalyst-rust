//! Shared Module
//!
//! Cross-cutting concerns and shared utilities.

pub mod error;
pub mod tsid;
pub mod middleware;
pub mod api_common;
pub mod database;
pub mod indexes;

// APIs
pub mod health_api;
pub mod well_known_api;
pub mod platform_config_api;
pub mod debug_api;
pub mod monitoring_api;
pub mod filter_options_api;
pub mod client_selection_api;
pub mod application_roles_sdk_api;

// Services
pub mod authorization_service;
pub mod dispatch_service;
pub mod projections_service;
pub mod role_sync_service;

// Re-export commonly used items
pub use error::{PlatformError, Result};
pub use tsid::TsidGenerator;
pub use middleware::{Authenticated, AppState};
pub use api_common::{PaginationParams, PaginatedResponse};
pub use health_api::health_router;
pub use well_known_api::well_known_router;
pub use platform_config_api::platform_config_router;
pub use monitoring_api::monitoring_router;
pub use filter_options_api::filter_options_router;
pub use client_selection_api::client_selection_router;
pub use application_roles_sdk_api::application_roles_sdk_router;
pub use authorization_service::AuthorizationService;
pub use dispatch_service::{DispatchScheduler, DispatchSchedulerConfig, EventDispatcher};
