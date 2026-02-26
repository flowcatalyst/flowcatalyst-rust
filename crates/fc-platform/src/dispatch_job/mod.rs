//! Dispatch Job Aggregate
//!
//! Individual message dispatch job tracking.

pub mod entity;
pub mod repository;
pub mod api;

// Re-export main types
pub use entity::{DispatchJob, DispatchStatus};
pub use repository::DispatchJobRepository;
pub use api::{dispatch_jobs_router};
