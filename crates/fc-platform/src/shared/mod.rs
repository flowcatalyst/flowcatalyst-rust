//! Shared Module
//!
//! Cross-cutting concerns and shared utilities.

pub mod api_common;
pub mod bootstrap_admin;
pub mod database;
pub mod default_processes;
pub mod error;
pub mod middleware;
pub mod tsid;
// APIs
pub mod application_roles_sdk_api;
pub mod batch_api;
pub mod bff_dashboard_api;
pub mod bff_developer_api;
pub mod bff_event_types_api;
pub mod bff_roles_api;
pub mod bff_scheduled_jobs_api;
pub mod client_selection_api;
pub mod debug_api;
pub mod dispatch_process_api;
pub mod filter_options_api;
pub mod health_api;
pub mod me_api;
pub mod monitoring_api;
pub mod platform_config_api;
pub mod public_api;
pub mod sdk_audit_batch_api;
pub mod sdk_dispatch_jobs_api;
pub mod sdk_sync_api;
pub mod well_known_api;

// Server setup helpers (shared across fc-server, fc-platform-server, fc-dev)
pub mod server_setup;

// Per-IP rate limit middleware
pub mod rate_limit_middleware;

// Services
pub mod authorization_service;
pub mod email_service;
pub mod encryption_service;
pub mod integrity_scan;
pub mod projections_service;
pub mod role_sync_service;

// Re-export commonly used items
pub use api_common::{PaginatedResponse, PaginationParams};
pub use application_roles_sdk_api::application_roles_sdk_router;
pub use authorization_service::AuthorizationService;
pub use client_selection_api::client_selection_router;
pub use error::{NotFoundExt, PlatformError, Result};
pub use filter_options_api::filter_options_router;
pub use health_api::health_router;
pub use middleware::{AppState, Authenticated, ClientIp};
pub use monitoring_api::monitoring_router;
pub use platform_config_api::platform_config_router;
pub use tsid::{EntityType, TsidGenerator};
pub use well_known_api::well_known_router;
