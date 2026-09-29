//! Audit Log Operations
//!
//! **Temporary** (owner spec `docs/spec/audit-redaction.md` in the Java repo,
//! "Temporary: redact existing rows from the dashboard"): the one-off sweep
//! that redacts passwords and secrets from `aud_logs` rows written before
//! the unit of work redacted at the source. Remove with the dashboard card.

pub mod events;
pub mod redact_existing;

pub use events::AuditLogsRedacted;
pub use redact_existing::{
    redact_stored_document, RedactExistingAuditLogs, RedactExistingAuditLogsCommand,
    RedactExistingAuditLogsUseCase,
};
