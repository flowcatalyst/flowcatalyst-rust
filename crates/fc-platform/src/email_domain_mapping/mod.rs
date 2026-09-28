//! The `email_domain_mapping` aggregate (fc-platform-iam), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_iam::email_domain_mapping::*;

pub mod routes;
pub use routes::{email_domain_mappings_router, routes};
