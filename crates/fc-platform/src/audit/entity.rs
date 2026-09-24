//! Audit Log Entity — matches TypeScript AuditLog domain

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLog {
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub operation_json: Option<serde_json::Value>,
    pub principal_id: Option<String>,
    /// Enriched from principals table (not stored in audit log)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_name: Option<String>,
    pub application_id: Option<String>,
    pub client_id: Option<String>,
    pub performed_at: DateTime<Utc>,
}

impl AuditLog {
    pub fn new(
        entity_type: impl Into<String>,
        entity_id: impl Into<String>,
        operation: impl Into<String>,
        operation_json: Option<serde_json::Value>,
        principal_id: Option<String>,
    ) -> Self {
        Self {
            id: crate::shared::tsid::generate_untyped(),
            entity_type: entity_type.into(),
            entity_id: entity_id.into(),
            operation: operation.into(),
            operation_json,
            principal_id,
            principal_name: None,
            application_id: None,
            client_id: None,
            performed_at: Utc::now(),
        }
    }

    pub fn with_application_id(mut self, app_id: impl Into<String>) -> Self {
        self.application_id = Some(app_id.into());
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    pub fn from_command<C: serde::Serialize>(
        entity_type: impl Into<String>,
        entity_id: impl Into<String>,
        command: &C,
        principal_id: Option<String>,
    ) -> Self {
        let command_name = std::any::type_name::<C>()
            .rsplit("::")
            .next()
            .unwrap_or("Unknown")
            .to_string();
        let operation_json = serde_json::to_value(command).ok();
        Self::new(
            entity_type,
            entity_id,
            command_name,
            operation_json,
            principal_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_audit_log() {
        let log = AuditLog::new(
            "Client",
            "clt_0HZXEQ5Y8JY5Z",
            "CreateClient",
            Some(serde_json::json!({"name": "Test"})),
            Some("prn_ABCDEFGHIJKLM".to_string()),
        );

        assert!(!log.id.is_empty());
        assert_eq!(
            log.id.len(),
            13,
            "Untyped ID should be 13 chars, got: {}",
            log.id.len()
        );
        assert!(
            !log.id.contains('_'),
            "Untyped ID should not have prefix underscore"
        );
        assert_eq!(log.entity_type, "Client");
        assert_eq!(log.entity_id, "clt_0HZXEQ5Y8JY5Z");
        assert_eq!(log.operation, "CreateClient");
        assert!(log.operation_json.is_some());
        assert_eq!(log.principal_id, Some("prn_ABCDEFGHIJKLM".to_string()));
        assert!(log.principal_name.is_none());
        assert!(log.application_id.is_none());
        assert!(log.client_id.is_none());
    }

    #[test]
    fn test_audit_log_unique_ids() {
        let l1 = AuditLog::new("A", "1", "Op", None, None);
        let l2 = AuditLog::new("B", "2", "Op", None, None);
        assert_ne!(l1.id, l2.id);
    }

    #[test]
    fn test_audit_log_no_principal() {
        let log = AuditLog::new("Client", "c1", "Delete", None, None);
        assert!(log.principal_id.is_none());
        assert!(log.operation_json.is_none());
    }

    #[test]
    fn test_audit_log_with_application_id() {
        let log = AuditLog::new("EventType", "evt1", "Create", None, None)
            .with_application_id("app_ABCDEFGHIJKLM");
        assert_eq!(log.application_id, Some("app_ABCDEFGHIJKLM".to_string()));
    }

    #[test]
    fn test_audit_log_with_client_id() {
        let log = AuditLog::new("EventType", "evt1", "Create", None, None)
            .with_client_id("clt_ABCDEFGHIJKLM");
        assert_eq!(log.client_id, Some("clt_ABCDEFGHIJKLM".to_string()));
    }

    #[test]
    fn test_audit_log_from_command() {
        #[derive(serde::Serialize)]
        struct CreateClientCommand {
            name: String,
            identifier: String,
        }

        let cmd = CreateClientCommand {
            name: "Test".to_string(),
            identifier: "test".to_string(),
        };

        let log = AuditLog::from_command("Client", "clt_123", &cmd, Some("prn_456".to_string()));

        assert_eq!(log.entity_type, "Client");
        assert_eq!(log.entity_id, "clt_123");
        assert!(log.operation.contains("CreateClientCommand"));
        assert!(log.operation_json.is_some());
        let json = log.operation_json.unwrap();
        assert_eq!(json["name"], "Test");
        assert_eq!(json["identifier"], "test");
    }

    #[test]
    fn test_audit_log_builder_chain() {
        let log = AuditLog::new("Role", "rol1", "AssignPermission", None, None)
            .with_application_id("app1")
            .with_client_id("clt1");

        assert_eq!(log.application_id, Some("app1".to_string()));
        assert_eq!(log.client_id, Some("clt1".to_string()));
    }
}
