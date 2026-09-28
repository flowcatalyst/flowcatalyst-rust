//! Shared server setup helpers.
//!
//! Extracts duplicated binary startup code (shutdown handling, auth init,
//! the platform context the routes are built from) so fc-server and fc-dev
//! share the same implementation.

pub mod auth_init;
pub mod housekeeping;
pub mod shutdown;

pub use crate::shared::platform_context::{PlatformContext, PlatformRoutesConfig};
pub use auth_init::{init_auth_services, AuthInitConfig, AuthServices};
pub use housekeeping::spawn_auth_purger;
pub use shutdown::wait_for_shutdown_signal;
