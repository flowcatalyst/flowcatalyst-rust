//! The `platform_config` aggregate (fc-platform-iam), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_iam::platform_config::*;

pub mod routes;
pub use routes::{admin_platform_config_router, config_access_router, routes};
