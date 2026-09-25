//! Email Domain Mapping Aggregate
//!
//! Maps email domains to identity providers and client access.

pub mod api;
pub mod entity;
pub mod lookup_api;
pub mod operations;
pub mod provider_move_repository;
pub mod repository;

pub use api::{email_domain_mappings_router, EmailDomainMappingsState};
pub use entity::{EmailDomainMapping, ScopeType};
pub use repository::EmailDomainMappingRepository;
