//! Dispatch Job Aggregate
//!
//! Individual message dispatch job tracking.

pub mod api;
pub mod delivery_credentials;
pub mod entity;
pub mod repository;

// Re-export main types
pub use api::dispatch_jobs_router;
pub use entity::{DispatchJob, DispatchStatus};
pub use repository::DispatchJobRepository;
