//! CORS Allowed Origins
//!
//! CORS origin management for platform.

pub mod entity;
pub mod repository;
pub mod api;
pub mod operations;

pub use entity::CorsAllowedOrigin;
pub use repository::CorsOriginRepository;
pub use api::{CorsState, cors_router};
