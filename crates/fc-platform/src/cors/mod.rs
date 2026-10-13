//! The `cors` aggregate (fc-platform-iam), its routes, and fc-server's CORS
//! layer over the table's origins.
//!
//! `routes.rs` is wiring (it builds the states and use cases from the
//! `PlatformContext`), and `layer.rs` is the HTTP policy the binary
//! applies, so both live in the assembly crate.

pub use fc_platform_iam::cors::*;

pub mod layer;
pub mod routes;
pub use layer::{cors_layer, AllowedOrigins};
pub use routes::{cors_router, routes};
