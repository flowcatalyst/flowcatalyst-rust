//! Identity Provider Aggregate
//!
//! OAuth/OIDC identity provider management.

pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;

pub use api::IdentityProvidersState;
pub use entity::{IdentityProvider, IdentityProviderType};
pub use repository::IdentityProviderRepository;
