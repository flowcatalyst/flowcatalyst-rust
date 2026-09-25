//! Connection Aggregate
//!
//! Named endpoint connections for dispatch.

pub mod access;
pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;
pub mod sync_plan;

pub use api::{connections_router, ConnectionsState};
pub use entity::{Connection, ConnectionStatus};
pub use repository::ConnectionRepository;
