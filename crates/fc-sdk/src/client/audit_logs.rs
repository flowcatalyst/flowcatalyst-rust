//! Audit log query operations.

use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

/// One page of audit logs — `GET /api/audit-logs`. Paging is by cursor:
/// pass `next_cursor` back as [`AuditLogFilters::after`] while `has_more`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogListResponse {
    pub audit_logs: Vec<AuditLogResponse>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// Filters for listing audit logs (Go's query parameters).
#[derive(Debug, Clone, Default)]
pub struct AuditLogFilters {
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub operation: Option<String>,
    pub principal_id: Option<String>,
    /// Sent as the CSV `applicationIds`.
    pub application_ids: Vec<String>,
    /// Sent as the CSV `clientIds`.
    pub client_ids: Vec<String>,
    /// Opaque cursor from a previous page's `next_cursor`.
    pub after: Option<String>,
    /// Page size (platform default 50, capped at 200).
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
    /// The operation's payload, as a JSON string.
    #[serde(default)]
    pub operation_json: Option<String>,
    pub performed_at: String,
}

/// Audit logs resource accessor — created via [`FlowCatalystClient::audit_logs`].
pub struct AuditLogs<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl AuditLogs<'_> {
    /// List one page of audit logs with optional filters.
    pub async fn list(
        &self,
        filters: &AuditLogFilters,
    ) -> Result<AuditLogListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(ref v) = filters.after {
            params.push(("after", v.clone()));
        }
        if let Some(v) = filters.page_size {
            params.push(("pageSize", v.to_string()));
        }
        if let Some(ref v) = filters.entity_type {
            params.push(("entityType", v.clone()));
        }
        if let Some(ref v) = filters.entity_id {
            params.push(("entityId", v.clone()));
        }
        if let Some(ref v) = filters.principal_id {
            params.push(("principalId", v.clone()));
        }
        if let Some(ref v) = filters.operation {
            params.push(("operation", v.clone()));
        }
        if !filters.application_ids.is_empty() {
            params.push(("applicationIds", filters.application_ids.join(",")));
        }
        if !filters.client_ids.is_empty() {
            params.push(("clientIds", filters.client_ids.join(",")));
        }
        let query = FlowCatalystClient::query_string(&params);
        self.client.get(&format!("/api/audit-logs{}", query)).await
    }

    /// Get a single audit log entry by ID.
    pub async fn get(&self, id: &str) -> Result<AuditLogResponse, ClientError> {
        self.client.get(&format!("/api/audit-logs/{}", id)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::MockPlatform;

    #[tokio::test]
    async fn list_uses_cursor_paging_and_csv_filters() {
        let stub = MockPlatform::start(&[(
            "GET",
            "/api/audit-logs",
            200,
            r#"{"auditLogs":[{"id":"al_1","operation":"CreateRole","entityType":"Role",
                "entityId":"rol_1","operationJson":"{}","performedAt":"t"}],
                "hasMore":true,"nextCursor":"cur_2"}"#,
        )])
        .await;
        let page = stub
            .client()
            .audit_logs()
            .list(&AuditLogFilters {
                after: Some("cur_1".into()),
                page_size: Some(20),
                entity_type: Some("Role".into()),
                client_ids: vec!["clt_1".into(), "clt_2".into()],
                application_ids: vec!["app_1".into()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(page.has_more);
        assert_eq!(page.next_cursor.as_deref(), Some("cur_2"));
        assert_eq!(page.audit_logs[0].operation_json.as_deref(), Some("{}"));

        let req = stub.single();
        assert_eq!(req.method, "GET");
        assert_eq!(
            req.query_pairs(),
            vec![
                ("after".to_string(), "cur_1".to_string()),
                ("pageSize".to_string(), "20".to_string()),
                ("entityType".to_string(), "Role".to_string()),
                ("applicationIds".to_string(), "app_1".to_string()),
                ("clientIds".to_string(), "clt_1,clt_2".to_string()),
            ]
        );
    }
}
