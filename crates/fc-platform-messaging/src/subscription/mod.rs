//! Subscription Aggregate
//!
//! Event subscription management.

pub mod access;
pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;

// Re-export main types
pub use api::SubscriptionsState;
pub use entity::{Subscription, SubscriptionStatus};
pub use repository::SubscriptionRepository;
