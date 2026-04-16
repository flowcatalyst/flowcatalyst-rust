//! Application management operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError};

/// Request to create an application.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationRequest {
    /// Unique code for the application
    pub code: String,
    /// Human-readable name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Application type (e.g., "APPLICATION", "INTEGRATION")
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub application_type: Option<String>,
    /// Default base URL for the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url: Option<String>,
    /// Icon URL for the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Request to update an application.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Client config for an application (per-client overrides).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigRequest {
    /// Whether the application is enabled for this client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Client-specific base URL override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    /// Additional config key-value pairs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// Application response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub application_type: String,
    #[serde(default)]
    pub default_base_url: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub service_account_id: Option<String>,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Application list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationListResponse {
    pub applications: Vec<ApplicationResponse>,
    #[serde(default)]
    pub total: Option<u64>,
}

/// Service account response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub active: bool,
    #[serde(default)]
    pub application_id: Option<String>,
    pub created_at: String,
}

/// Application role response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRoleResponse {
    pub id: String,
    pub code: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub application_code: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub source: String,
    #[serde(default)]
    pub client_managed: bool,
}

/// Client config for an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigResponse {
    pub id: String,
    pub application_id: String,
    pub client_id: String,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub client_identifier: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub base_url_override: Option<String>,
    #[serde(default)]
    pub effective_base_url: Option<String>,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

/// Client configs list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigsResponse {
    pub client_configs: Vec<ClientConfigResponse>,
    #[serde(default)]
    pub total: Option<u64>,
}

/// Created response (returns the new entity's ID).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedResponse {
    pub id: String,
    #[serde(default)]
    pub message: Option<String>,
}

/// Generic success response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessResponse {
    #[serde(default)]
    pub message: Option<String>,
}

impl FlowCatalystClient {
    // ── Applications ─────────────────────────────────────────────

    /// Create a new application.
    pub async fn create_application(
        &self,
        req: &CreateApplicationRequest,
    ) -> Result<CreatedResponse, ClientError> {
        self.post("/api/applications", req).await
    }

    /// List applications with optional pagination and filters.
    pub async fn list_applications(
        &self,
        active: Option<bool>,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ApplicationListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(a) = active {
            params.push(format!("active={}", a));
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

        self.get(&format!("/api/applications{}", query)).await
    }

    /// Get an application by ID.
    pub async fn get_application(&self, id: &str) -> Result<ApplicationResponse, ClientError> {
        self.get(&format!("/api/applications/{}", id)).await
    }

    /// Get an application by code.
    pub async fn get_application_by_code(
        &self,
        code: &str,
    ) -> Result<ApplicationResponse, ClientError> {
        self.get(&format!("/api/applications/by-code/{}", code))
            .await
    }

    /// Update an application.
    pub async fn update_application(
        &self,
        id: &str,
        req: &UpdateApplicationRequest,
    ) -> Result<ApplicationResponse, ClientError> {
        self.put(&format!("/api/applications/{}", id), req).await
    }

    /// Delete (deactivate) an application.
    pub async fn delete_application(&self, id: &str) -> Result<(), ClientError> {
        self.delete_req(&format!("/api/applications/{}", id)).await
    }

    /// Activate an application.
    pub async fn activate_application(&self, id: &str) -> Result<ApplicationResponse, ClientError> {
        self.post_action(&format!("/api/applications/{}/activate", id))
            .await
    }

    /// Deactivate an application.
    pub async fn deactivate_application(
        &self,
        id: &str,
    ) -> Result<ApplicationResponse, ClientError> {
        self.post_action(&format!("/api/applications/{}/deactivate", id))
            .await
    }

    /// Provision a service account for an application.
    pub async fn provision_service_account(
        &self,
        application_id: &str,
    ) -> Result<ServiceAccountResponse, ClientError> {
        self.post_action(&format!(
            "/api/applications/{}/provision-service-account",
            application_id
        ))
        .await
    }

    /// Get the service account for an application.
    pub async fn get_service_account(
        &self,
        application_id: &str,
    ) -> Result<ServiceAccountResponse, ClientError> {
        self.get(&format!(
            "/api/applications/{}/service-account",
            application_id
        ))
        .await
    }

    /// List roles for an application.
    pub async fn list_application_roles(
        &self,
        application_id: &str,
    ) -> Result<Vec<ApplicationRoleResponse>, ClientError> {
        self.get(&format!("/api/applications/{}/roles", application_id))
            .await
    }

    /// List client configs for an application.
    pub async fn list_application_clients(
        &self,
        application_id: &str,
    ) -> Result<ClientConfigsResponse, ClientError> {
        self.get(&format!("/api/applications/{}/clients", application_id))
            .await
    }

    /// Update client config for an application.
    pub async fn update_application_client_config(
        &self,
        application_id: &str,
        client_id: &str,
        req: &ClientConfigRequest,
    ) -> Result<ClientConfigResponse, ClientError> {
        self.put(
            &format!(
                "/api/applications/{}/clients/{}",
                application_id, client_id
            ),
            req,
        )
        .await
    }

    /// Enable an application for a specific client.
    pub async fn enable_application_for_client(
        &self,
        application_id: &str,
        client_id: &str,
    ) -> Result<ClientConfigResponse, ClientError> {
        self.post_action(&format!(
            "/api/applications/{}/clients/{}/enable",
            application_id, client_id
        ))
        .await
    }

    /// Disable an application for a specific client.
    pub async fn disable_application_for_client(
        &self,
        application_id: &str,
        client_id: &str,
    ) -> Result<ClientConfigResponse, ClientError> {
        self.post_action(&format!(
            "/api/applications/{}/clients/{}/disable",
            application_id, client_id
        ))
        .await
    }
}
