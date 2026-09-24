//! Shared server setup helpers.
//!
//! Extracts duplicated binary startup code (shutdown handling, auth init,
//! platform route state construction) so fc-server, fc-platform-server,
//! and fc-dev share the same implementation.

pub mod auth_init;
pub mod housekeeping;
pub mod platform_routes;
pub mod shutdown;

pub use auth_init::{init_auth_services, AuthInitConfig, AuthServices};
pub use housekeeping::spawn_lapsed_previous_secret_purge;
pub use platform_routes::{build_platform_routes, PlatformRoutesConfig};
pub use shutdown::wait_for_shutdown_signal;
