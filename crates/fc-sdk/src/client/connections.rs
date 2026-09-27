//! Connection management operations.

use super::applications::CreatedResponse;
use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

/// Paginated list of connections — `GET /api/connections`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsListResponse {
    pub connections: Vec<ConnectionResponse>,
    #[serde(default)]
    pub total: u64,
}

/// Request to create a connection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnectionRequest {
    /// Unique code for this connection
    pub code: String,
    /// Display name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Service account for authentication credentials
    pub service_account_id: String,
    /// External system reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    /// Client ID for multi-tenant scoping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// Request to update a connection. Go's platform replaces the record, so
/// `name` is required and always sent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnectionRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_code: Option<String>,
}

/// Connection response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    pub status: String,
    pub service_account_id: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_identifier: Option<String>,
    #[serde(default)]
    pub application_code: Option<String>,
    /// Where the connection came from (e.g. `UI`, `API`, `CODE`).
    #[serde(default)]
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Connections resource accessor — created via [`FlowCatalystClient::connections`].
pub struct Connections<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl Connections<'_> {
    /// Create a new connection.
    ///
    /// Returns `{ id }` only. Call `get(&id)` if you need the full record.
    pub async fn create(
        &self,
        req: &CreateConnectionRequest,
    ) -> Result<CreatedResponse, ClientError> {
        self.client.post("/api/connections", req).await
    }

    /// Get a connection by ID.
    pub async fn get(&self, id: &str) -> Result<ConnectionResponse, ClientError> {
        self.client.get(&format!("/api/connections/{}", id)).await
    }

    /// List connections, optionally filtered by client and status.
    pub async fn list(
        &self,
        client_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<ConnectionsListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(v) = status {
            params.push(("status", v.to_string()));
        }
        if let Some(v) = client_id {
            params.push(("clientId", v.to_string()));
        }
        let query = FlowCatalystClient::query_string(&params);
        self.client.get(&format!("/api/connections{}", query)).await
    }

    /// Update a connection (204).
    pub async fn update(&self, id: &str, req: &UpdateConnectionRequest) -> Result<(), ClientError> {
        self.client
            .put_empty(&format!("/api/connections/{}", id), req)
            .await
    }

    /// Delete a connection.
    pub async fn delete(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/connections/{}", id))
            .await
    }

    /// Pause a connection.
    pub async fn pause(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!("/api/connections/{}/pause", id))
            .await
    }

    /// Activate a connection.
    pub async fn activate(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!("/api/connections/{}/activate", id))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::MockPlatform;

    #[tokio::test]
    async fn update_always_sends_name_and_accepts_204() {
        let stub = MockPlatform::start(&[("PUT", "/api/connections/con_1", 204, "")]).await;
        stub.client()
            .connections()
            .update(
                "con_1",
                &UpdateConnectionRequest {
                    name: "Billing".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let req = stub.single();
        assert_eq!(req.method, "PUT");
        assert_eq!(req.json(), serde_json::json!({"name": "Billing"}));
    }

    #[tokio::test]
    async fn create_reads_the_id_and_list_sends_gos_filters() {
        let conn = r#"{"id":"con_1","code":"billing","name":"Billing","status":"ACTIVE",
            "serviceAccountId":"sa_1","source":"API","createdAt":"t","updatedAt":"t"}"#;
        let list = format!(r#"{{"connections":[{conn}],"total":1}}"#);
        let stub = MockPlatform::start(&[
            ("POST", "/api/connections", 201, conn),
            ("GET", "/api/connections", 200, &list),
        ])
        .await;
        let c = stub.client();
        let created = c
            .connections()
            .create(&CreateConnectionRequest {
                code: "billing".into(),
                name: "Billing".into(),
                service_account_id: "sa_1".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(created.id, "con_1");
        let listed = c
            .connections()
            .list(Some("clt_1"), Some("ACTIVE"))
            .await
            .unwrap();
        assert_eq!(listed.connections[0].source.as_deref(), Some("API"));
        assert_eq!(
            stub.requests()[1].query_pairs(),
            vec![
                ("status".to_string(), "ACTIVE".to_string()),
                ("clientId".to_string(), "clt_1".to_string())
            ]
        );
    }
}
