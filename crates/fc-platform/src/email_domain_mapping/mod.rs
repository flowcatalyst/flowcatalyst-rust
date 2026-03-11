//! Email Domain Mapping Aggregate
//!
//! Maps email domains to identity providers and client access.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

pub use entity::{EmailDomainMapping, ScopeType};
pub use repository::EmailDomainMappingRepository;
pub use api::{EmailDomainMappingsState, email_domain_mappings_router};
