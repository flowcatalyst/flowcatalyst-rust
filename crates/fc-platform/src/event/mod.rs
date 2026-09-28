//! Event Aggregate
//!
//! Platform events.

pub mod api;
pub mod entity;
pub mod repository;
pub mod routes;

// Re-export main types
pub use routes::{events_router, routes};
pub use entity::Event;
pub use repository::EventRepository;
