//! Client (tenant) management operations.

use super::applications::CreatedResponse;
use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub notes: Vec<NoteResponse>,
    pub created_at: String,
    pub updated_at: String,
}

/// A note on a client (Go's `NoteResponse`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteResponse {
    pub category: String,
    pub text: String,
    #[serde(default)]
    pub added_by: Option<String>,
    pub added_at: String,
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

/// Clients resource accessor — created via [`FlowCatalystClient::clients`].
pub struct Clients<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl Clients<'_> {
    /// Create a new client (tenant).
    pub async fn create(&self, req: &CreateClientRequest) -> Result<CreatedResponse, ClientError> {
        self.client.post("/api/clients", req).await
    }

    /// List all clients. Go's platform takes no filters or paging here;
    /// use [`Self::search`] to narrow by name or identifier.
    pub async fn list(&self) -> Result<ClientListResponse, ClientError> {
        self.client.get("/api/clients").await
    }

    /// Get a client by ID.
    pub async fn get(&self, id: &str) -> Result<ClientResponse, ClientError> {
        self.client.get(&format!("/api/clients/{}", id)).await
    }

    /// Get a client by identifier (slug/domain).
    pub async fn get_by_identifier(&self, identifier: &str) -> Result<ClientResponse, ClientError> {
        self.client
            .get(&format!("/api/clients/by-identifier/{}", identifier))
            .await
    }

    /// Search clients by name or identifier.
    pub async fn search(&self, query: &str) -> Result<ClientListResponse, ClientError> {
        let query = FlowCatalystClient::query_string(&[("q", query.to_string())]);
        self.client
            .get(&format!("/api/clients/search{}", query))
            .await
    }

    /// Update a client. The platform answers 204; call `get(id)` for the
    /// updated record.
    pub async fn update(&self, id: &str, req: &UpdateClientRequest) -> Result<(), ClientError> {
        self.client
            .put_empty(&format!("/api/clients/{}", id), req)
            .await
    }

    /// Delete (deactivate) a client.
    pub async fn delete(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/clients/{}", id))
            .await
    }

    /// Activate a client.
    pub async fn activate(&self, id: &str) -> Result<StatusChangeResponse, ClientError> {
        self.client
            .post_action(&format!("/api/clients/{}/activate", id))
            .await
    }

    /// Suspend a client.
    pub async fn suspend(
        &self,
        id: &str,
        req: &StatusChangeRequest,
    ) -> Result<StatusChangeResponse, ClientError> {
        self.client
            .post(&format!("/api/clients/{}/suspend", id), req)
            .await
    }

    /// Deactivate a client.
    pub async fn deactivate(
        &self,
        id: &str,
        req: &StatusChangeRequest,
    ) -> Result<StatusChangeResponse, ClientError> {
        self.client
            .post(&format!("/api/clients/{}/deactivate", id), req)
            .await
    }

    /// Add a note to a client.
    pub async fn add_note(
        &self,
        id: &str,
        req: &AddNoteRequest,
    ) -> Result<AddNoteResponse, ClientError> {
        self.client
            .post(&format!("/api/clients/{}/notes", id), req)
            .await
    }

    /// List applications for a client (with enabled status).
    pub async fn list_applications(
        &self,
        client_id: &str,
    ) -> Result<ClientApplicationsResponse, ClientError> {
        self.client
            .get(&format!("/api/clients/{}/applications", client_id))
            .await
    }

    /// Enable an application for a client (204).
    pub async fn enable_application(
        &self,
        client_id: &str,
        application_id: &str,
    ) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!(
                "/api/clients/{}/applications/{}/enable",
                client_id, application_id
            ))
            .await
    }

    /// Disable an application for a client (204).
    pub async fn disable_application(
        &self,
        client_id: &str,
        application_id: &str,
    ) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!(
                "/api/clients/{}/applications/{}/disable",
                client_id, application_id
            ))
            .await
    }

    /// Bulk update which applications are enabled for a client (204).
    pub async fn update_applications(
        &self,
        client_id: &str,
        req: &UpdateClientApplicationsRequest,
    ) -> Result<(), ClientError> {
        self.client
            .put_empty(&format!("/api/clients/{}/applications", client_id), req)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::MockPlatform;

    #[tokio::test]
    async fn list_sends_no_filters_and_get_reads_notes() {
        let client = r#"{"id":"clt_1","name":"Acme","identifier":"acme","status":"ACTIVE",
            "notes":[{"category":"ops","text":"hi","addedAt":"t"}],
            "createdAt":"t","updatedAt":"t"}"#;
        let list = format!(r#"{{"clients":[{client}],"total":1}}"#);
        let stub = MockPlatform::start(&[
            ("GET", "/api/clients", 200, &list),
            ("GET", "/api/clients/clt_1", 200, client),
        ])
        .await;
        let c = stub.client();
        let listed = c.clients().list().await.unwrap();
        assert_eq!(listed.clients.len(), 1);
        let got = c.clients().get("clt_1").await.unwrap();
        assert_eq!(got.notes[0].text, "hi");
        assert_eq!(stub.requests()[0].query, "");
    }

    #[tokio::test]
    async fn search_encodes_q() {
        let stub = MockPlatform::start(&[(
            "GET",
            "/api/clients/search",
            200,
            r#"{"clients":[],"total":0}"#,
        )])
        .await;
        stub.client().clients().search("a&b c").await.unwrap();
        assert_eq!(
            stub.single().query_pairs(),
            vec![("q".to_string(), "a&b c".to_string())]
        );
    }

    #[tokio::test]
    async fn no_content_writes_accept_204() {
        let stub = MockPlatform::start(&[
            ("PUT", "/api/clients/clt_1", 204, ""),
            (
                "POST",
                "/api/clients/clt_1/applications/app_1/enable",
                204,
                "",
            ),
            (
                "POST",
                "/api/clients/clt_1/applications/app_1/disable",
                204,
                "",
            ),
            ("PUT", "/api/clients/clt_1/applications", 204, ""),
        ])
        .await;
        let c = stub.client();
        c.clients()
            .update(
                "clt_1",
                &UpdateClientRequest {
                    name: Some("New".into()),
                },
            )
            .await
            .unwrap();
        c.clients()
            .enable_application("clt_1", "app_1")
            .await
            .unwrap();
        c.clients()
            .disable_application("clt_1", "app_1")
            .await
            .unwrap();
        c.clients()
            .update_applications(
                "clt_1",
                &UpdateClientApplicationsRequest {
                    enabled_application_ids: vec!["app_1".into()],
                },
            )
            .await
            .unwrap();
        let reqs = stub.requests();
        assert_eq!(reqs.len(), 4);
        assert_eq!(reqs[0].json(), serde_json::json!({"name": "New"}));
        assert_eq!(
            reqs[3].json(),
            serde_json::json!({"enabledApplicationIds": ["app_1"]})
        );
    }
}
