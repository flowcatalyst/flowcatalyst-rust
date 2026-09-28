//! The `scheduled_job` aggregate (fc-platform-scheduled-jobs), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_scheduled_jobs::scheduled_job::*;

pub mod routes;
pub use routes::routes;
