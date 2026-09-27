//! Event Type management operations.

use super::applications::CreatedResponse;
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
    /// Events of this type are carried per client: the subscription editor
    /// offers client-scoped types only to client-scoped subscriptions.
    /// Distinct from `client_id`, which scopes the type itself. `None` (not
    /// sent) is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_scoped: Option<bool>,
}

/// Request to update an event type. Go's platform replaces the record, so
/// `name` is required and always sent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEventTypeRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Events of this type are per-client; `None` leaves it unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_scoped: Option<bool>,
}

/// Request to add a schema version to an event type (Go's
/// `AddSchemaRequest`: both members required).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSchemaVersionRequest {
    /// Schema version (typically semver, e.g. `1.0`).
    pub version: String,
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
    /// Where the event type came from (e.g. `UI`, `API`, `CODE`).
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub application: String,
    #[serde(default)]
    pub subdomain: String,
    #[serde(default)]
    pub aggregate: String,
    /// Go's `eventName`; the Rust platform's older `event` is still read.
    #[serde(default, alias = "event")]
    pub event_name: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
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
    #[serde(default)]
    pub created_at: Option<String>,
}

/// One event type in a sync (Go's strict `SyncEventTypeInputRequest`:
/// `code`, `name` and `description` only).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncEventTypeItem {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request body for the per-resource sync endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEventTypesRequest {
    pub event_types: Vec<SyncEventTypeItem>,
}

/// Event types resource accessor — created via [`FlowCatalystClient::event_types`].
pub struct EventTypes<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl EventTypes<'_> {
    /// Create a new event type.
    ///
    /// Returns `{ id }` only. Call `get(&id)` if you need the full record.
    pub async fn create(
        &self,
        req: &CreateEventTypeRequest,
    ) -> Result<CreatedResponse, ClientError> {
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
            params.push(("application", app.to_string()));
        }
        if let Some(cid) = client_id {
            params.push(("clientId", cid.to_string()));
        }
        if let Some(s) = status {
            params.push(("status", s.to_string()));
        }
        let query = FlowCatalystClient::query_string(&params);
        self.client.get(&format!("/api/event-types{}", query)).await
    }

    /// Update an event type (204). Call `get(id)` for the updated record.
    pub async fn update(&self, id: &str, req: &UpdateEventTypeRequest) -> Result<(), ClientError> {
        self.client
            .put_empty(&format!("/api/event-types/{}", id), req)
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

    /// Delete an event type (`DELETE /api/event-types/{id}`, 204).
    pub async fn delete(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/event-types/{}", id))
            .await
    }

    /// Archive an event type.
    ///
    /// Go's platform has no archive route for event types: this sends
    /// `DELETE /api/event-types/{id}`, which Go treats as a delete. Use
    /// [`Self::delete`] to say so, or archive through a sync.
    #[deprecated(
        note = "Go's platform has no event-type archive route; this sends DELETE, which deletes. Use `delete`."
    )]
    pub async fn archive(&self, id: &str) -> Result<(), ClientError> {
        self.delete(id).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::MockPlatform;

    const ET: &str = r#"{"id":"et_1","code":"orders:f:shipment:shipped","name":"Shipped",
        "application":"orders","subdomain":"f","aggregate":"shipment","eventName":"shipped",
        "status":"CURRENT","source":"API","clientId":"clt_1",
        "specVersions":[{"version":"1.0","status":"CURRENT","schema":{},"createdAt":"t"}],
        "createdAt":"t","updatedAt":"t"}"#;

    #[tokio::test]
    async fn get_reads_gos_field_names() {
        let stub = MockPlatform::start(&[("GET", "/api/event-types/et_1", 200, ET)]).await;
        let et = stub.client().event_types().get("et_1").await.unwrap();
        assert_eq!(et.event_name, "shipped");
        assert_eq!(et.source, "API");
        assert_eq!(et.client_id.as_deref(), Some("clt_1"));
        assert_eq!(et.spec_versions[0].created_at.as_deref(), Some("t"));
    }

    #[tokio::test]
    async fn create_reads_the_id_update_sends_name_and_accepts_204() {
        let stub = MockPlatform::start(&[
            ("POST", "/api/event-types", 201, r#"{"id":"et_1"}"#),
            ("PUT", "/api/event-types/et_1", 204, ""),
        ])
        .await;
        let c = stub.client();
        let created = c
            .event_types()
            .create(&CreateEventTypeRequest {
                code: "orders:f:shipment:shipped".into(),
                name: "Shipped".into(),
                client_scoped: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(created.id, "et_1");
        assert_eq!(
            stub.requests()[0].json(),
            serde_json::json!({
                "code": "orders:f:shipment:shipped",
                "name": "Shipped",
                "clientScoped": true
            })
        );
        c.event_types()
            .update(
                "et_1",
                &UpdateEventTypeRequest {
                    name: "Shipped".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let req = &stub.requests()[1];
        assert_eq!(req.method, "PUT");
        assert_eq!(req.json(), serde_json::json!({"name": "Shipped"}));
    }

    #[tokio::test]
    async fn list_sends_client_id_in_camel_case() {
        let stub =
            MockPlatform::start(&[("GET", "/api/event-types", 200, r#"{"items":[]}"#)]).await;
        stub.client()
            .event_types()
            .list(Some("orders"), None, Some("clt_1"))
            .await
            .unwrap();
        assert_eq!(
            stub.single().query_pairs(),
            vec![
                ("application".to_string(), "orders".to_string()),
                ("clientId".to_string(), "clt_1".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn add_schema_version_sends_version() {
        let stub =
            MockPlatform::start(&[("POST", "/api/event-types/et_1/versions", 200, ET)]).await;
        stub.client()
            .event_types()
            .add_schema_version(
                "et_1",
                &AddSchemaVersionRequest {
                    version: "1.1".into(),
                    schema: serde_json::json!({"type": "object"}),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            stub.single().json(),
            serde_json::json!({"version": "1.1", "schema": {"type": "object"}})
        );
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn archive_is_the_delete_route() {
        let stub = MockPlatform::start(&[("DELETE", "/api/event-types/et_1", 204, "")]).await;
        stub.client().event_types().archive("et_1").await.unwrap();
        assert_eq!(stub.single().method, "DELETE");
    }

    #[tokio::test]
    async fn sync_items_carry_only_gos_members() {
        let stub = MockPlatform::start(&[(
            "POST",
            "/api/applications/orders/event-types/sync",
            200,
            r#"{"applicationCode":"orders","created":1,"updated":0,"deleted":0,"syncedCodes":["c"]}"#,
        )])
        .await;
        stub.client()
            .event_types()
            .sync(
                "orders",
                &SyncEventTypesRequest {
                    event_types: vec![SyncEventTypeItem {
                        code: "orders:f:shipment:shipped".into(),
                        name: "Shipped".into(),
                        description: None,
                    }],
                },
                true,
            )
            .await
            .unwrap();
        let req = stub.single();
        assert_eq!(req.query, "removeUnlisted=true");
        assert_eq!(
            req.json(),
            serde_json::json!({"eventTypes": [{"code": "orders:f:shipment:shipped", "name": "Shipped"}]})
        );
    }
}
