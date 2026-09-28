//! Client Aggregate
//!
//! Client management - tenants in the platform.

pub mod access;
pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;

// Re-export main types
pub use api::ClientsState;
pub use entity::{Client, ClientStatus};
pub use repository::ClientRepository;
