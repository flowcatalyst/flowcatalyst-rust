//! Event Type Aggregate
//!
//! Event type definitions and schemas.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

// Re-export main types
pub use entity::{EventType, EventTypeStatus};
pub use repository::EventTypeRepository;
pub use api::{event_types_router};
