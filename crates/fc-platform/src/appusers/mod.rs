//! The `appusers` aggregate (fc-platform-iam): an application's users and
//! invitations (Go F20), and its routes.
//!
//! `routes.rs` is wiring (it builds the state and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_iam::appusers::*;

pub mod routes;
pub use routes::{app_users_router, app_users_state, routes};
