//! Identity Provider Aggregate
//!
//! OAuth/OIDC identity provider management.

pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;
pub mod routes;

pub use api::IdentityProvidersState;
pub use entity::{IdentityProvider, IdentityProviderType};
pub use repository::IdentityProviderRepository;
pub use routes::{identity_providers_router, routes};
