//! Process documentation management operations.
//!
//! Processes store free-form workflow documentation (typically Mermaid
//! diagram source). Codes follow `{application}:{subdomain}:{process-name}`,
//! mirroring EventType.

use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

/// List of processes returned by `GET /api/processes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessListResponse {
    pub items: Vec<ProcessResponse>,
}

/// Request to create a process.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateProcessRequest {
    /// Code in format `{application}:{subdomain}:{process-name}`.
    pub code: String,
    /// Human-readable name.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Diagram body (typically Mermaid source). Stored verbatim.
    #[serde(default)]
    pub body: String,
    /// Diagram type. Defaults to `mermaid` when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Request to update a process. All fields optional — only set fields
/// change. The platform rejects update requests with no changes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProcessRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Process response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: String,
    pub source: String,
    #[serde(default)]
    pub application: String,
    #[serde(default)]
    pub subdomain: String,
    #[serde(default)]
    pub process_name: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub diagram_type: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// One process in the sync payload.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncProcessInput {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Request body for the per-resource sync endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProcessesRequest {
    pub processes: Vec<SyncProcessInput>,
}

/// Processes resource accessor — created via [`FlowCatalystClient::processes`].
pub struct Processes<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl Processes<'_> {
    /// Create a new process.
    pub async fn create(
        &self,
        req: &CreateProcessRequest,
    ) -> Result<ProcessResponse, ClientError> {
        self.client.post("/api/processes", req).await
    }

    /// Get a process by ID.
    pub async fn get(&self, id: &str) -> Result<ProcessResponse, ClientError> {
        self.client.get(&format!("/api/processes/{}", id)).await
    }

    /// Get a process by code.
    pub async fn get_by_code(&self, code: &str) -> Result<ProcessResponse, ClientError> {
        self.client
            .get(&format!("/api/processes/by-code/{}", code))
            .await
    }

    /// List processes with optional filters.
    pub async fn list(
        &self,
        application: Option<&str>,
        subdomain: Option<&str>,
        status: Option<&str>,
        search: Option<&str>,
    ) -> Result<ProcessListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(app) = application {
            params.push(format!("application={}", app));
        }
        if let Some(sub) = subdomain {
            params.push(format!("subdomain={}", sub));
        }
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }
        if let Some(term) = search {
            params.push(format!("search={}", term));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.client.get(&format!("/api/processes{}", query)).await
    }

    /// Update a process. The platform returns 204 No Content on success.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdateProcessRequest,
    ) -> Result<(), ClientError> {
        self.client
            .put_empty(&format!("/api/processes/{}", id), req)
            .await
    }

    /// Archive (soft-delete) a process. Distinct from `delete`, which
    /// hard-deletes — and which the platform only permits on already-archived
    /// processes.
    pub async fn archive(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!("/api/processes/{}/archive", id))
            .await
    }

    /// Hard-delete an archived process.
    pub async fn delete(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/processes/{}", id))
            .await
    }

    /// Sync processes for an application — declarative reconciliation
    /// against `POST /api/applications/{app_code}/processes/sync`.
    ///
    /// `remove_unlisted` removes API/CODE-sourced processes not in the
    /// list. UI-sourced processes are never touched.
    pub async fn sync(
        &self,
        app_code: &str,
        processes: Vec<SyncProcessInput>,
        remove_unlisted: bool,
    ) -> Result<crate::client::SyncResult, ClientError> {
        let query = if remove_unlisted {
            "?removeUnlisted=true"
        } else {
            ""
        };
        let req = SyncProcessesRequest { processes };
        self.client
            .post(
                &format!("/api/applications/{}/processes/sync{}", app_code, query),
                &req,
            )
            .await
    }
}
