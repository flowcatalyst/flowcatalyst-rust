//! The `login_attempt` aggregate (fc-platform-iam), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_iam::login_attempt::*;

pub mod routes;
pub use routes::{login_attempts_router, routes};
