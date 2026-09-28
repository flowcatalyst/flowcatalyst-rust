//! The `mfa` aggregate (fc-platform-iam), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_iam::mfa::*;

pub mod routes;
pub use routes::{
    account_router, routes, two_factor_admin_router, two_factor_login_router,
    two_factor_self_service_router,
};
