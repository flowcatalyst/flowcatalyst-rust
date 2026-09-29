//! The `event` aggregate (fc-platform-messaging), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_messaging::event::*;

pub mod routes;
pub use routes::{events_router, routes};
