//! The redaction rule for an audit row that is already stored.
//!
//! Shared by every audit read (rows are redacted on the way out, Java
//! b4a15fd8) and by the temporary sweep that rewrites them
//! (`operations::redact_existing`), so the rule outlives the sweep.

use serde_json::Value;

use crate::platform_config::operations::SetPlatformConfigPropertyCommand;

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
