//! Identity Provider Aggregate
//!
//! OAuth/OIDC identity provider management.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

pub use entity::{IdentityProvider, IdentityProviderType};
pub use repository::IdentityProviderRepository;
pub use api::{IdentityProvidersState, identity_providers_router};
