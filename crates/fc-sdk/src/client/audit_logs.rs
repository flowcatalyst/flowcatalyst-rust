//! Audit log query operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError, ListResponse};

/// Filters for listing audit logs.
#[derive(Debug, Clone, Default)]
pub struct AuditLogFilters {
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub operation: Option<String>,
    pub principal_id: Option<String>,
    pub client_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// Audit log entry from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogResponse {
    pub id: String,
    pub operation: String,
    pub entity_type: String,
    #[serde(default)]
    pub entity_id: Option<String>,
    #[serde(default)]
    pub principal_id: Option<String>,
    #[serde(default)]
    pub principal_name: Option<String>,
    #[serde(default)]
    pub application_id: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    pub performed_at: String,
}

impl FlowCatalystClient {
    /// List audit logs with optional filters.
    pub async fn list_audit_logs(
        &self,
        filters: &AuditLogFilters,
    ) -> Result<ListResponse<AuditLogResponse>, ClientError> {
        let mut params = Vec::new();
        if let Some(ref v) = filters.entity_type {
            params.push(format!("entityType={}", v));
        }
        if let Some(ref v) = filters.entity_id {
            params.push(format!("entityId={}", v));
        }
        if let Some(ref v) = filters.operation {
            params.push(format!("operation={}", v));
        }
        if let Some(ref v) = filters.principal_id {
            params.push(format!("principalId={}", v));
        }
        if let Some(ref v) = filters.client_id {
            params.push(format!("clientId={}", v));
        }
        if let Some(ref v) = filters.from {
            params.push(format!("from={}", v));
        }
        if let Some(ref v) = filters.to {
            params.push(format!("to={}", v));
        }
        if let Some(v) = filters.page {
            params.push(format!("page={}", v));
        }
        if let Some(v) = filters.page_size {
            params.push(format!("pageSize={}", v));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.get(&format!("/api/audit-logs{}", query)).await
    }

    /// Get a single audit log entry by ID.
    pub async fn get_audit_log(&self, id: &str) -> Result<AuditLogResponse, ClientError> {
        self.get(&format!("/api/audit-logs/{}", id)).await
    }
}
