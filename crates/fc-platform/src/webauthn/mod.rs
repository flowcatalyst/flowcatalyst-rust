//! The `webauthn` aggregate (fc-platform-iam), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_iam::webauthn::*;

pub mod routes;
pub use routes::{routes, webauthn_router};
