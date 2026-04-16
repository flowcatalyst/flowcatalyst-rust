//! Connection management operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError, ListResponse};

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

impl FlowCatalystClient {
    /// Create a new connection
    pub async fn create_connection(
        &self,
        req: &CreateConnectionRequest,
    ) -> Result<serde_json::Value, ClientError> {
        self.post("/api/connections", req).await
    }

    /// Get a connection by ID
    pub async fn get_connection(&self, id: &str) -> Result<ConnectionResponse, ClientError> {
        self.get(&format!("/api/connections/{}", id)).await
    }

    /// List connections with optional filters
    pub async fn list_connections(
        &self,
        client_id: Option<&str>,
        status: Option<&str>,
        service_account_id: Option<&str>,
    ) -> Result<ListResponse<ConnectionResponse>, ClientError> {
        let mut query = String::new();
        let mut params = Vec::new();
        if let Some(v) = client_id { params.push(format!("clientId={}", v)); }
        if let Some(v) = status { params.push(format!("status={}", v)); }
        if let Some(v) = service_account_id { params.push(format!("serviceAccountId={}", v)); }
        if !params.is_empty() {
            query = format!("?{}", params.join("&"));
        }
        self.get(&format!("/api/connections{}", query)).await
    }

    /// Update a connection
    pub async fn update_connection(
        &self,
        id: &str,
        req: &UpdateConnectionRequest,
    ) -> Result<(), ClientError> {
        self.put(&format!("/api/connections/{}", id), req).await
    }

    /// Delete a connection
    pub async fn delete_connection(&self, id: &str) -> Result<(), ClientError> {
        self.delete_req(&format!("/api/connections/{}", id)).await
    }

    /// Pause a connection
    pub async fn pause_connection(&self, id: &str) -> Result<(), ClientError> {
        self.post_empty(&format!("/api/connections/{}/pause", id))
            .await
    }

    /// Activate a connection
    pub async fn activate_connection(&self, id: &str) -> Result<(), ClientError> {
        self.post_empty(&format!("/api/connections/{}/activate", id))
            .await
    }
}
