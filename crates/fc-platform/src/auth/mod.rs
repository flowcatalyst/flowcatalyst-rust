//! The `auth` aggregate (fc-platform-iam), and its routes.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), so it lives in the assembly crate.

pub use fc_platform_iam::auth::*;

pub mod routes;
pub use routes::{
    anchor_domains_router, auth_router, client_auth_configs_router, idp_role_mappings_router,
    oauth_clients_router, oauth_router, oidc_login_router, routes,
};
