//! Event Type Aggregate
//!
//! Event type definitions and schemas.

pub mod access;
pub mod api;
pub mod entity;
pub mod go_api;
pub mod operations;
pub mod repository;
pub mod routes;

// Re-export main types
pub use routes::{event_types_router, routes};
pub use entity::{EventType, EventTypeCode, EventTypeCodeError, EventTypeStatus};
pub use repository::EventTypeRepository;
