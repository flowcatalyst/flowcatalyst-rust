//! Dispatch Pool Aggregate
//!
//! Message dispatch pool management.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

// Re-export main types
pub use entity::{DispatchPool, DispatchPoolStatus};
pub use repository::DispatchPoolRepository;
pub use api::{DispatchPoolsState, dispatch_pools_router};
