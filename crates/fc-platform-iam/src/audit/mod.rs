//! Audit Log Aggregate
//!
//! Audit logging for platform operations.

pub mod api;
pub mod bff;
pub mod entity;
pub mod operations;
pub mod repository;
pub mod service;
pub mod stored_redaction;

// Re-export main types
pub use entity::AuditLog;
pub use repository::AuditLogRepository;
pub use service::AuditService;
