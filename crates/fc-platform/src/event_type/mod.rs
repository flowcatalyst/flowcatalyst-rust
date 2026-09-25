//! Event Type Aggregate
//!
//! Event type definitions and schemas.

pub mod access;
pub mod api;
pub mod entity;
pub mod go_api;
pub mod operations;
pub mod repository;

// Re-export main types
pub use api::event_types_router;
pub use entity::{EventType, EventTypeStatus};
pub use repository::EventTypeRepository;
