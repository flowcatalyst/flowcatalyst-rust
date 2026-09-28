//! The `event_type` aggregate (fc-platform-iam), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_iam::event_type::*;

pub mod routes;
pub use routes::{event_types_router, routes};
