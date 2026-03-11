//! Connection Aggregate
//!
//! Named endpoint connections for dispatch.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

pub use entity::{Connection, ConnectionStatus};
pub use repository::ConnectionRepository;
pub use api::{ConnectionsState, connections_router};
