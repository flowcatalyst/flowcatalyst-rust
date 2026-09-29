//! Shared Module
//!
//! Cross-cutting concerns and shared utilities every domain uses.

pub mod api_common;
pub mod authorization_service;
pub mod caller_reach;
pub mod capped_body;
pub mod database;
pub mod email_service;
pub mod encryption_service;
pub mod enum_str;
pub mod error;
pub mod jsonb_text;
pub mod log_throttle;
pub mod middleware;
pub mod rate_limit_middleware;
pub mod rate_limit_store;
pub mod rejection;
pub mod secret_backfill;
pub mod secret_ref;
pub mod tsid;
pub mod webhook_signer;

// Re-export commonly used items
pub use api_common::{PaginatedResponse, PaginationParams};
pub use error::{NotFoundExt, PlatformError, Result};
pub use middleware::{Authenticated, ClientIp};
pub use tsid::EntityType;
