//! Client Aggregate
//!
//! Client management - tenants in the platform.

pub mod access;
pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;
pub mod search_api;

// Re-export main types
pub use api::{clients_router, ClientsState};
pub use entity::{Client, ClientStatus};
pub use repository::ClientRepository;
