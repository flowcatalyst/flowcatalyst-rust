//! Login Attempt Aggregate
//!
//! Tracks authentication attempts for auditing.

pub mod entity;
pub mod repository;
pub mod api;

pub use entity::{LoginAttempt, AttemptType, LoginOutcome};
pub use repository::LoginAttemptRepository;
pub use api::{LoginAttemptsState, login_attempts_router};
