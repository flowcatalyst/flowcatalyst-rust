//! Redact Existing Audit Logs Use Case — **temporary**.
//!
//! Owner spec `docs/spec/audit-redaction.md` in the Java repo, "Temporary:
//! redact existing rows from the dashboard" (2026-09-24; to be removed once
//! the rows written before source-side redaction have been swept). Walks
//! `aud_logs` in id order, in batches of [`BATCH_SIZE`], applies the same
//! redaction rule the unit of work applies to new rows, and rewrites only
//! the rows whose JSON changed. Idempotent: a second run rewrites nothing.
//!
//! ## Why the rewrite goes straight to the repository
//!
//! Rewriting stored audit rows is platform maintenance, like
//! `shared/secret_backfill.rs`: it changes how a recorded fact is stored,
//! not the fact, and audit rows have no aggregate. Routing each rewrite
//! through the unit of work would emit an event and an audit row per
//! redacted row. The run itself is audited once, through the unit of work:
//! one `RedactExistingAuditLogs` row carrying `{scanned, redacted}`.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

use super::events::AuditLogsRedacted;
use crate::audit::repository::AuditLogRepository;
use crate::platform_config::operations::SetPlatformConfigPropertyCommand;
use crate::usecase::{
    AuditMasked, ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Rows read per candidate query.
pub const BATCH_SIZE: i64 = 500;

/// The stored `operation` of a set-property command: this platform's type
/// name, and the name the Java and Go platforms record for the same command.
pub const SET_PROPERTY_OPERATIONS: &[&str] =
    &["SetPlatformConfigPropertyCommand", "SetPropertyCommand"];

/// The redaction rule applied to an already-stored row: the name rule,
/// plus — for a set-property row — `value` unless the row's own `valueType`
/// is exactly `PLAIN` (the same helper the live command declares its masked
/// fields with). A JSON-string document (SDK-ingested rows) is redacted
/// inside. Returns the document unchanged when nothing is secret.
pub fn redact_stored_document(operation: &str, document: &Value) -> Value {
    let masked = if SET_PROPERTY_OPERATIONS.contains(&operation) {
        SetPlatformConfigPropertyCommand::audit_masked_fields_for(
            document.get("valueType").and_then(Value::as_str),
        )
    } else {
        &[]
    };
    fc_common::audit_redaction::redact_document(document, masked)
}

/// The use case's input: the sweep takes none.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RedactExistingAuditLogsCommand {}

/// The command the run's own audit row records. Its type name is the audit
/// `operation` the spec names; it carries the sweep's result because the
/// spec's `operation_json` is `{scanned, redacted}`.
#[derive(Debug, Clone, Serialize)]
pub struct RedactExistingAuditLogs {
    pub scanned: u64,
    pub redacted: u64,
}

impl AuditMasked for RedactExistingAuditLogs {}

pub struct RedactExistingAuditLogsUseCase<U: UnitOfWork> {
    audit_log_repo: Arc<AuditLogRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RedactExistingAuditLogsUseCase<U> {
    pub fn new(audit_log_repo: Arc<AuditLogRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            audit_log_repo,
            unit_of_work,
        }
    }

    /// Sweep every candidate row; `(scanned, redacted)`.
    async fn sweep(&self) -> Result<(u64, u64), UseCaseError> {
        let mut scanned = 0u64;
        let mut redacted = 0u64;
        let mut after_id = String::new();
        loop {
            let batch = self
                .audit_log_repo
                .find_redaction_candidates(&after_id, SET_PROPERTY_OPERATIONS, BATCH_SIZE)
                .await?;
            let Some(last) = batch.last() else { break };
            after_id = last.id.clone();
            scanned += batch.len() as u64;

            let changed: Vec<(String, Value)> = batch
                .iter()
                .filter_map(|row| {
                    let stored = row.operation_json.as_ref()?;
                    let next = redact_stored_document(&row.operation, stored);
                    (&next != stored).then(|| (row.id.clone(), next))
                })
                .collect();
            // Platform maintenance write; see the module docs.
            self.audit_log_repo.rewrite_operation_json(&changed).await?;
            redacted += changed.len() as u64;

            if (batch.len() as i64) < BATCH_SIZE {
                break;
            }
        }
        Ok((scanned, redacted))
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RedactExistingAuditLogsUseCase<U> {
    type Command = RedactExistingAuditLogsCommand;
    type Event = AuditLogsRedacted;

    async fn validate(
        &self,
        _command: &RedactExistingAuditLogsCommand,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Anchor scope and the audit-log read permission are checked by the
    /// handler; the sweep has no resource to scope to.
    async fn authorize(
        &self,
        _command: &RedactExistingAuditLogsCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        _command: RedactExistingAuditLogsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AuditLogsRedacted> {
        let (scanned, redacted) = match self.sweep().await {
            Ok(counts) => counts,
            Err(e) => return UseCaseResult::failure(e),
        };
        let event = AuditLogsRedacted::new(&ctx, scanned, redacted);
        self.unit_of_work
            .emit_event(event, &RedactExistingAuditLogs { scanned, redacted })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_set_property_row_masks_value_unless_plain() {
        for operation in SET_PROPERTY_OPERATIONS {
            for doc in [
                json!({"property": "k", "value": "sk_live", "valueType": "SECRET"}),
                json!({"property": "k", "value": "sk_live"}),
                json!({"property": "k", "value": "sk_live", "valueType": null}),
            ] {
                assert_eq!(redact_stored_document(operation, &doc)["value"], "***");
            }
            let plain = json!({"property": "k", "value": "host", "valueType": "PLAIN"});
            assert_eq!(redact_stored_document(operation, &plain), plain);
        }
    }

    #[test]
    fn other_rows_get_the_name_rule_only() {
        let doc = json!({"value": "v", "newPassword": "p"});
        assert_eq!(
            redact_stored_document("ResetPasswordCommand", &doc),
            json!({"value": "v", "newPassword": "***"})
        );
    }

    /// The stored operation is the command's type name; a rename would
    /// silently stop the sweep masking set-property values.
    #[test]
    fn the_set_property_operation_is_the_commands_type_name() {
        let name = std::any::type_name::<SetPlatformConfigPropertyCommand>()
            .rsplit("::")
            .next()
            .unwrap();
        assert_eq!(name, SET_PROPERTY_OPERATIONS[0]);
    }
}
