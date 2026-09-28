//! Email Domain Mapping Aggregate
//!
//! Maps email domains to identity providers and client access.

pub mod api;
pub mod entity;
pub mod lookup_api;
pub mod operations;
pub mod provider_move_repository;
pub mod repository;
pub mod routes;

pub use api::EmailDomainMappingsState;
pub use routes::{email_domain_mappings_router, routes};
pub use entity::{EmailDomainMapping, ScopeType};
pub use repository::EmailDomainMappingRepository;
