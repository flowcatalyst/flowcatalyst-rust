//! CORS Allowed Origins
//!
//! CORS origin management for platform.

pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;

pub use api::CorsState;
pub use entity::CorsAllowedOrigin;
pub use repository::CorsOriginRepository;
