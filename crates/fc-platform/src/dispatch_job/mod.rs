//! The `dispatch_job` aggregate (fc-platform-messaging), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_messaging::dispatch_job::*;

pub mod routes;
pub use routes::{dispatch_jobs_router, routes};
