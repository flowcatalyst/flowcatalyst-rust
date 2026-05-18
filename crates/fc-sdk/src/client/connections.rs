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

/// Request to update a connection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnectionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
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

    /// List connections with optional filters.
    pub async fn list(
        &self,
        client_id: Option<&str>,
        status: Option<&str>,
        service_account_id: Option<&str>,
    ) -> Result<ConnectionsListResponse, ClientError> {
        let mut query = String::new();
        let mut params = Vec::new();
        if let Some(v) = client_id {
            params.push(format!("clientId={}", v));
        }
        if let Some(v) = status {
            params.push(format!("status={}", v));
        }
        if let Some(v) = service_account_id {
            params.push(format!("serviceAccountId={}", v));
        }
        if !params.is_empty() {
            query = format!("?{}", params.join("&"));
        }
        self.client
            .get(&format!("/api/connections{}", query))
            .await
    }

    /// Update a connection.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdateConnectionRequest,
    ) -> Result<(), ClientError> {
        self.client
            .put(&format!("/api/connections/{}", id), req)
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
