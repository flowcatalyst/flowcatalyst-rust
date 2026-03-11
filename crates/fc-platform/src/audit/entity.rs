//! Audit Log Entity — matches TypeScript AuditLog domain

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

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
            id: crate::TsidGenerator::generate(crate::EntityType::AuditLog),
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
        Self::new(entity_type, entity_id, command_name, operation_json, principal_id)
    }
}

impl From<crate::entities::aud_logs::Model> for AuditLog {
    fn from(m: crate::entities::aud_logs::Model) -> Self {
        Self {
            id: m.id,
            entity_type: m.entity_type,
            entity_id: m.entity_id,
            operation: m.operation,
            operation_json: m.operation_json.map(Into::into),
            principal_id: m.principal_id,
            principal_name: None,
            application_id: m.application_id,
            client_id: m.client_id,
            performed_at: m.performed_at.with_timezone(&Utc),
        }
    }
}
