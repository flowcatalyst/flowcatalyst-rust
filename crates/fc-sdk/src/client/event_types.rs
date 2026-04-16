//! Event Type management operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError, ListResponse};

/// Request to create an event type.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventTypeRequest {
    /// Code in format `{app}:{domain}:{aggregate}:{event}` (e.g., "orders:fulfillment:shipment:shipped")
    pub code: String,
    /// Human-readable name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional initial JSON schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
    /// Client ID for multi-tenant scoping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// Request to update an event type.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEventTypeRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request to add a schema version to an event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSchemaVersionRequest {
    /// JSON schema for this version
    pub schema: serde_json::Value,
}

/// Event type response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: String,
    #[serde(default)]
    pub application: String,
    #[serde(default)]
    pub subdomain: String,
    #[serde(default)]
    pub aggregate: String,
    #[serde(default, rename = "event")]
    pub event_name: String,
    #[serde(default)]
    pub spec_versions: Vec<SpecVersionResponse>,
    pub created_at: String,
    pub updated_at: String,
}

/// Schema version response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecVersionResponse {
    pub version: String,
    pub status: String,
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
}

impl FlowCatalystClient {
    /// Create a new event type.
    pub async fn create_event_type(
        &self,
        req: &CreateEventTypeRequest,
    ) -> Result<EventTypeResponse, ClientError> {
        self.post("/api/event-types", req).await
    }

    /// Get an event type by ID.
    pub async fn get_event_type(&self, id: &str) -> Result<EventTypeResponse, ClientError> {
        self.get(&format!("/api/event-types/{}", id)).await
    }

    /// Get an event type by code.
    pub async fn get_event_type_by_code(
        &self,
        code: &str,
    ) -> Result<EventTypeResponse, ClientError> {
        self.get(&format!("/api/event-types/by-code/{}", code)).await
    }

    /// List event types with optional filters.
    pub async fn list_event_types(
        &self,
        application: Option<&str>,
        status: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<ListResponse<EventTypeResponse>, ClientError> {
        let mut params = Vec::new();
        if let Some(app) = application {
            params.push(format!("application={}", app));
        }
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }
        if let Some(cid) = client_id {
            params.push(format!("client_id={}", cid));
        }

        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };

        self.get(&format!("/api/event-types{}", query)).await
    }

    /// Update an event type.
    pub async fn update_event_type(
        &self,
        id: &str,
        req: &UpdateEventTypeRequest,
    ) -> Result<EventTypeResponse, ClientError> {
        self.put(&format!("/api/event-types/{}", id), req).await
    }

    /// Add a schema version to an event type.
    pub async fn add_schema_version(
        &self,
        id: &str,
        req: &AddSchemaVersionRequest,
    ) -> Result<EventTypeResponse, ClientError> {
        self.post(&format!("/api/event-types/{}/versions", id), req)
            .await
    }

    /// Archive (soft-delete) an event type.
    pub async fn archive_event_type(&self, id: &str) -> Result<(), ClientError> {
        self.delete_req(&format!("/api/event-types/{}", id)).await
    }
}
