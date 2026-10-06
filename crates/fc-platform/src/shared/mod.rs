//! Shared Module
//!
//! Cross-cutting concerns and shared utilities. The kernel's modules are
//! fc-platform-core's and the domain services are the domain crates',
//! re-exported here at their old paths; `database` adds the startup
//! seeding and the platform's code migrations to core's.

pub use fc_platform_core::shared::{
    api_common, caller_reach, capped_body, email_service, encryption_service, enum_str, error, id,
    jsonb_text, log_throttle, rate_limit_middleware, rate_limit_store, rejection, secret_backfill,
    secret_ref, tsid, webhook_signer,
};

// fc-platform-iam's.
pub use fc_platform_iam::shared::{authorization_service, branding, middleware, role_sync_service};
// fc-platform-auth's.
pub use fc_platform_auth::shared::{client_selection_api, me_api};
// fc-platform-messaging's.
pub use fc_platform_messaging::shared::{
    batch_api, dispatch_process_api, dispatch_queue, projections_service, sdk_dispatch_jobs_api,
};

// The assembly's: cross-aggregate endpoints, the context and server setup,
// the OpenAPI documents, bootstrap.
pub mod application_roles_sdk_api;
pub mod bff_dashboard_api;
pub mod bff_developer_api;
pub mod bootstrap_admin;
pub mod database;
pub mod debug_api;
pub mod default_processes;
pub mod filter_options_api;
pub mod go_read_aliases_api;
pub mod health_api;
pub mod integrity_scan;
pub mod monitoring_api;
pub mod openapi_api;
pub mod openapi_contract;
pub mod platform_config_api;
pub mod platform_context;
pub mod profile_only;
pub mod public_api;
pub mod router_config_api;
pub mod routes;
pub mod sdk_audit_batch_api;
pub mod sdk_sync_api;
pub mod sdk_sync_go_api;
pub mod server_setup;
pub mod well_known_api;

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
