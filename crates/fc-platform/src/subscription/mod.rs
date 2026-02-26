//! Subscription Aggregate
//!
//! Event subscription management.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

// Re-export main types
pub use entity::{Subscription, SubscriptionStatus};
pub use repository::SubscriptionRepository;
pub use api::{SubscriptionsState, subscriptions_router};
