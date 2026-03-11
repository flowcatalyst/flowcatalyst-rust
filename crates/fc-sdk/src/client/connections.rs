//! Connection management operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError, ListResponse};

/// Request to create a connection (webhook endpoint).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateConnectionRequest {
    /// Unique code for this connection
    pub code: String,
    /// Webhook endpoint URL
    pub endpoint: String,
    /// Service account for authentication credentials
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
    /// External system reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    /// Client ID for multi-tenant scoping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// Request to update a connection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateConnectionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

/// Connection response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionResponse {
    pub id: String,
    pub code: String,
    pub endpoint: String,
    pub status: String,
    #[serde(default)]
    pub service_account_id: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
}

impl FlowCatalystClient {
    /// Create a new connection.
    pub async fn create_connection(
        &self,
        req: &CreateConnectionRequest,
    ) -> Result<ConnectionResponse, ClientError> {
        self.post("/api/connections", req).await
    }

    /// Get a connection by ID.
    pub async fn get_connection(&self, id: &str) -> Result<ConnectionResponse, ClientError> {
        self.get(&format!("/api/connections/{}", id)).await
    }

    /// List connections with optional filters.
    pub async fn list_connections(
        &self,
        client_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<ListResponse<ConnectionResponse>, ClientError> {
        let mut params = Vec::new();
        if let Some(cid) = client_id {
            params.push(format!("client_id={}", cid));
        }
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }

        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };

        self.get(&format!("/api/connections{}", query)).await
    }

    /// Update a connection.
    pub async fn update_connection(
        &self,
        id: &str,
        req: &UpdateConnectionRequest,
    ) -> Result<ConnectionResponse, ClientError> {
        self.put(&format!("/api/connections/{}", id), req).await
    }

    /// Delete a connection.
    pub async fn delete_connection(&self, id: &str) -> Result<(), ClientError> {
        self.delete_req(&format!("/api/connections/{}", id)).await
    }

    /// Pause a connection.
    pub async fn pause_connection(&self, id: &str) -> Result<(), ClientError> {
        self.post_empty(&format!("/api/connections/{}/pause", id))
            .await
    }

    /// Activate a connection.
    pub async fn activate_connection(&self, id: &str) -> Result<(), ClientError> {
        self.post_empty(&format!("/api/connections/{}/activate", id))
            .await
    }
}
