//! Audit Log Aggregate
//!
//! Audit logging for platform operations.

pub mod entity;
pub mod repository;
pub mod api;
pub mod service;

// Re-export main types
pub use entity::{AuditLog, AuditAction};
pub use repository::AuditLogRepository;
pub use api::{audit_logs_router};
pub use service::AuditService;
