//! Event Type management operations.

use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

/// List of event types returned by `GET /api/event-types`.
///
/// The platform uses `{ items: [...] }` — not `{ data, total }`. There is
/// no separate total count.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeListResponse {
    pub items: Vec<EventTypeResponse>,
}

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

/// Request body for the per-resource sync endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEventTypesRequest {
    pub event_types: Vec<CreateEventTypeRequest>,
}

/// Event types resource accessor — created via [`FlowCatalystClient::event_types`].
pub struct EventTypes<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl EventTypes<'_> {
    /// Create a new event type.
    pub async fn create(
        &self,
        req: &CreateEventTypeRequest,
    ) -> Result<EventTypeResponse, ClientError> {
        self.client.post("/api/event-types", req).await
    }

    /// Get an event type by ID.
    pub async fn get(&self, id: &str) -> Result<EventTypeResponse, ClientError> {
        self.client.get(&format!("/api/event-types/{}", id)).await
    }

    /// Get an event type by code.
    pub async fn get_by_code(&self, code: &str) -> Result<EventTypeResponse, ClientError> {
        self.client
            .get(&format!("/api/event-types/by-code/{}", code))
            .await
    }

    /// List event types with optional filters.
    pub async fn list(
        &self,
        application: Option<&str>,
        status: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<EventTypeListResponse, ClientError> {
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

        self.client
            .get(&format!("/api/event-types{}", query))
            .await
    }

    /// Update an event type.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdateEventTypeRequest,
    ) -> Result<EventTypeResponse, ClientError> {
        self.client
            .put(&format!("/api/event-types/{}", id), req)
            .await
    }

    /// Add a schema version to an event type.
    pub async fn add_schema_version(
        &self,
        id: &str,
        req: &AddSchemaVersionRequest,
    ) -> Result<EventTypeResponse, ClientError> {
        self.client
            .post(&format!("/api/event-types/{}/versions", id), req)
            .await
    }

    /// Archive (soft-delete) an event type.
    ///
    /// The server's DELETE on this resource is a soft archive — the row is
    /// retained with status flipped to ARCHIVED. We name it `archive` rather
    /// than `delete` to make the semantics visible.
    pub async fn archive(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/event-types/{}", id))
            .await
    }

    /// Sync event types for an application — declarative reconciliation
    /// against `POST /api/applications/{appCode}/event-types/sync`.
    pub async fn sync(
        &self,
        app_code: &str,
        req: &SyncEventTypesRequest,
        remove_unlisted: bool,
    ) -> Result<crate::client::SyncResult, ClientError> {
        let query = if remove_unlisted {
            "?removeUnlisted=true"
        } else {
            ""
        };
        self.client
            .post(
                &format!("/api/applications/{}/event-types/sync{}", app_code, query),
                req,
            )
            .await
    }
}
