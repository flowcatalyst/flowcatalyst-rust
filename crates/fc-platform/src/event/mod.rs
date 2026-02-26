//! Event Aggregate
//!
//! Platform events.

pub mod entity;
pub mod repository;
pub mod api;

// Re-export main types
pub use entity::Event;
pub use repository::EventRepository;
pub use api::{events_router};
