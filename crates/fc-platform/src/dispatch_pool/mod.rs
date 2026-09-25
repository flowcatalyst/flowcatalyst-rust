//! Dispatch Pool Aggregate
//!
//! Message dispatch pool management.

pub mod access;
pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;
pub mod router_config_repository;

// Re-export main types
pub use api::{dispatch_pools_router, DispatchPoolsState};
pub use entity::{DispatchPool, DispatchPoolStatus};
pub use repository::DispatchPoolRepository;
