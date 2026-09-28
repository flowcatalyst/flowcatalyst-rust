//! Event Type Aggregate
//!
//! Event type definitions and schemas.

pub mod access;
pub mod api;
pub mod bff;
pub mod entity;
pub mod operations;
pub mod repository;

// Re-export main types
pub use entity::{EventType, EventTypeCode, EventTypeCodeError, EventTypeStatus};
pub use repository::EventTypeRepository;
