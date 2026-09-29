//! The `portal` aggregate (fc-platform-auth), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_auth::portal::*;

pub mod routes;
pub use routes::routes;
