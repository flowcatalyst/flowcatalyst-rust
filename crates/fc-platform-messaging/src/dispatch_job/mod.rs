//! Dispatch Job Aggregate
//!
//! Individual message dispatch job tracking.

pub mod api;
pub mod delivery_credentials;
pub mod entity;
pub mod lifecycle;
pub mod reaper;
pub mod repository;
pub mod requeue_rules;
pub mod signing_guard;
pub mod window;

// Re-export main types
pub use entity::{DispatchJob, DispatchStatus};
pub use repository::DispatchJobRepository;
