//! Client (tenant) management operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError};
use super::applications::{CreatedResponse, SuccessResponse};

/// Request to create a client (tenant).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateClientRequest {
    /// Unique identifier for the client (e.g., slug or domain)
    pub identifier: String,
    /// Human-readable name
    pub name: String,
}

/// Request to update a client.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Request for status change operations (suspend, deactivate).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusChangeRequest {
    /// Reason for the status change
    pub reason: String,
}

/// Request to add a note to a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddNoteRequest {
    /// Note category
    pub category: String,
    /// Note text
    pub text: String,
}

/// Request to update client applications (bulk enable/disable).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientApplicationsRequest {
    /// Application IDs to enable for this client
    pub enabled_application_ids: Vec<String>,
}

/// Client response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientResponse {
    pub id: String,
    pub name: String,
    pub identifier: String,
    pub status: String,
    #[serde(default)]
    pub status_reason: Option<String>,
    #[serde(default)]
    pub status_changed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Client list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientListResponse {
    pub clients: Vec<ClientResponse>,
    #[serde(default)]
    pub total: Option<u64>,
}

/// Status change response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusChangeResponse {
    pub message: String,
}

/// Add note response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddNoteResponse {
    pub message: String,
}

/// Client application status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientApplicationResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub enabled_for_client: bool,
}

/// Client applications list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientApplicationsResponse {
    pub applications: Vec<ClientApplicationResponse>,
    #[serde(default)]
    pub total: Option<u64>,
}

impl FlowCatalystClient {
    // ── Clients ──────────────────────────────────────────────────

    /// Create a new client (tenant).
    pub async fn create_client(
        &self,
        req: &CreateClientRequest,
    ) -> Result<CreatedResponse, ClientError> {
        self.post("/api/clients", req).await
    }

    /// List clients with optional pagination and status filter.
    pub async fn list_clients(
        &self,
        status: Option<&str>,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ClientListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }
        if let Some(p) = page {
            params.push(format!("page={}", p));
        }
        if let Some(ps) = page_size {
            params.push(format!("pageSize={}", ps));
        }

        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };

        self.get(&format!("/api/clients{}", query)).await
    }

    /// Get a client by ID.
    pub async fn get_client(&self, id: &str) -> Result<ClientResponse, ClientError> {
        self.get(&format!("/api/clients/{}", id)).await
    }

    /// Get a client by identifier (slug/domain).
    pub async fn get_client_by_identifier(
        &self,
        identifier: &str,
    ) -> Result<ClientResponse, ClientError> {
        self.get(&format!("/api/clients/by-identifier/{}", identifier))
            .await
    }

    /// Search clients by name or identifier.
    pub async fn search_clients(&self, query: &str) -> Result<ClientListResponse, ClientError> {
        self.get(&format!("/api/clients/search?q={}", query)).await
    }

    /// Update a client.
    pub async fn update_client(
        &self,
        id: &str,
        req: &UpdateClientRequest,
    ) -> Result<ClientResponse, ClientError> {
        self.put(&format!("/api/clients/{}", id), req).await
    }

    /// Delete (deactivate) a client.
    pub async fn delete_client(&self, id: &str) -> Result<(), ClientError> {
        self.delete_req(&format!("/api/clients/{}", id)).await
    }

    /// Activate a client.
    pub async fn activate_client(&self, id: &str) -> Result<StatusChangeResponse, ClientError> {
        self.post_action(&format!("/api/clients/{}/activate", id))
            .await
    }

    /// Suspend a client.
    pub async fn suspend_client(
        &self,
        id: &str,
        req: &StatusChangeRequest,
    ) -> Result<StatusChangeResponse, ClientError> {
        self.post(&format!("/api/clients/{}/suspend", id), req)
            .await
    }

    /// Deactivate a client.
    pub async fn deactivate_client(
        &self,
        id: &str,
        req: &StatusChangeRequest,
    ) -> Result<StatusChangeResponse, ClientError> {
        self.post(&format!("/api/clients/{}/deactivate", id), req)
            .await
    }

    /// Add a note to a client.
    pub async fn add_client_note(
        &self,
        id: &str,
        req: &AddNoteRequest,
    ) -> Result<AddNoteResponse, ClientError> {
        self.post(&format!("/api/clients/{}/notes", id), req).await
    }

    /// List applications for a client (with enabled status).
    pub async fn list_client_applications(
        &self,
        client_id: &str,
    ) -> Result<ClientApplicationsResponse, ClientError> {
        self.get(&format!("/api/clients/{}/applications", client_id))
            .await
    }

    /// Enable an application for a client.
    pub async fn enable_client_application(
        &self,
        client_id: &str,
        application_id: &str,
    ) -> Result<SuccessResponse, ClientError> {
        self.post_action(&format!(
            "/api/clients/{}/applications/{}/enable",
            client_id, application_id
        ))
        .await
    }

    /// Disable an application for a client.
    pub async fn disable_client_application(
        &self,
        client_id: &str,
        application_id: &str,
    ) -> Result<SuccessResponse, ClientError> {
        self.post_action(&format!(
            "/api/clients/{}/applications/{}/disable",
            client_id, application_id
        ))
        .await
    }

    /// Bulk update which applications are enabled for a client.
    pub async fn update_client_applications(
        &self,
        client_id: &str,
        req: &UpdateClientApplicationsRequest,
    ) -> Result<SuccessResponse, ClientError> {
        self.put(
            &format!("/api/clients/{}/applications", client_id),
            req,
        )
        .await
    }
}
