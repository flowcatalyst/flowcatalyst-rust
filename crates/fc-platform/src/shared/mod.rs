//! Shared Module
//!
//! Cross-cutting concerns and shared utilities. The kernel's modules are
//! fc-platform-core's, re-exported here at their old paths;
//! `authorization_service`, `database` and `middleware` add the platform's
//! halves (the repository-backed services, the seeding, `AppState`) to
//! core's.

pub use fc_platform_core::shared::{
    api_common, caller_reach, capped_body, email_service, encryption_service, enum_str, error,
    jsonb_text, log_throttle, rate_limit_middleware, rate_limit_store, rejection, secret_backfill,
    secret_ref, tsid, webhook_signer,
};

pub mod bootstrap_admin;
pub mod database;
pub mod default_processes;
pub mod middleware;
pub mod profile_only;
// APIs
pub mod application_roles_sdk_api;
pub mod batch_api;
pub mod bff_dashboard_api;
pub mod bff_developer_api;
pub mod client_selection_api;
pub mod debug_api;
pub mod dispatch_process_api;
pub mod dispatch_queue;
pub mod filter_options_api;
pub mod go_read_aliases_api;
pub mod health_api;
pub mod me_api;
pub mod monitoring_api;
pub mod openapi_api;
pub mod openapi_contract;
pub mod platform_config_api;
pub mod platform_context;
pub mod public_api;
pub mod router_config_api;
pub mod routes;
pub mod sdk_audit_batch_api;
pub mod sdk_dispatch_jobs_api;
pub mod sdk_sync_api;
pub mod sdk_sync_go_api;
pub mod well_known_api;

// Server setup helpers (shared across fc-server and fc-dev)
pub mod server_setup;

// Per-IP rate limit middleware (in-memory, per-instance)

// Distributed rate-limit store (Redis when available, Postgres fallback)

// Services
pub mod authorization_service;
pub mod branding;
pub mod integrity_scan;
pub mod projections_service;
pub mod role_sync_service;

// Re-export commonly used items
pub use api_common::{PaginatedResponse, PaginationParams};
pub use authorization_service::AuthorizationService;
pub use error::{NotFoundExt, PlatformError, Result};
pub use middleware::{AppState, Authenticated, ClientIp};
pub use routes::{
    client_selection_router, filter_options_router, health_router, monitoring_router,
    platform_config_router, routes, well_known_router,
};
pub use tsid::EntityType;
