//! Event Aggregate
//!
//! Platform events.

pub mod api;
pub mod entity;
pub mod repository;

// Re-export main types
pub use entity::Event;
pub use repository::EventRepository;
