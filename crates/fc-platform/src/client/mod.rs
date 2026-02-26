//! Client Aggregate
//!
//! Client management - tenants in the platform.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

// Re-export main types
pub use entity::{Client, ClientStatus};
pub use repository::ClientRepository;
pub use api::{ClientsState, clients_router};
