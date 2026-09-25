//! Audit Log Domain Events

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

/// Emitted once per run of the **temporary** dashboard sweep that redacts
/// existing audit rows (owner spec `docs/spec/audit-redaction.md`, Java
/// repo): how many candidate rows it read and how many it rewrote.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogsRedacted {
    #[serde(skip)]
    pub metadata: EventMetadata,

    pub scanned: u64,
    pub redacted: u64,
}

impl_domain_event!(AuditLogsRedacted);

impl AuditLogsRedacted {
    // Type, source, subject and group as the Java platform emits them.
    const EVENT_TYPE: &'static str = "platform:admin:audit-log:redacted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, scanned: u64, redacted: u64) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                "platform.audit-logs".to_string(),
                "platform:audit-logs".to_string(),
            ),
            scanned,
            redacted,
        }
    }
}
