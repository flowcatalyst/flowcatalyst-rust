//! The `function` aggregate (fc-platform-functions), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_functions::function::*;

pub mod routes;
pub use routes::routes;
