//! Dispatch pool management operations.

use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

/// Request to create a dispatch pool.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateDispatchPoolRequest {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
}

/// Request to update a dispatch pool.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDispatchPoolRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
}

/// Filters for listing dispatch pools.
#[derive(Debug, Clone, Default)]
pub struct DispatchPoolFilters {
    pub client_id: Option<String>,
    pub status: Option<String>,
}

/// Dispatch pool response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub rate_limit: Option<u32>,
    #[serde(default)]
    pub concurrency: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request body for the per-resource sync endpoint.
///
/// The platform's app-scoped endpoint expects `{ pools: [...] }`, NOT
/// `{ dispatchPools: [...] }` (see `shared/sdk_sync_api.rs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDispatchPoolsRequest {
    pub pools: Vec<SyncDispatchPoolItem>,
}

/// A dispatch pool item for sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDispatchPoolItem {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
    /// Messages per minute. The backend's camelCase field is `rateLimit`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Dispatch pools resource accessor — created via [`FlowCatalystClient::dispatch_pools`].
pub struct DispatchPools<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl DispatchPools<'_> {
    /// List dispatch pools with optional filters.
    ///
    /// The platform returns a bare JSON array — no `{ pools, total }` envelope.
    pub async fn list(
        &self,
        filters: &DispatchPoolFilters,
    ) -> Result<Vec<DispatchPoolResponse>, ClientError> {
        let mut params = Vec::new();
        if let Some(ref cid) = filters.client_id {
            params.push(format!("clientId={}", cid));
        }
        if let Some(ref s) = filters.status {
            params.push(format!("status={}", s));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.client
            .get(&format!("/api/dispatch-pools{}", query))
            .await
    }

    /// Get a dispatch pool by ID.
    pub async fn get(&self, id: &str) -> Result<DispatchPoolResponse, ClientError> {
        self.client
            .get(&format!("/api/dispatch-pools/{}", id))
            .await
    }

    /// Create a new dispatch pool.
    pub async fn create(
        &self,
        req: &CreateDispatchPoolRequest,
    ) -> Result<DispatchPoolResponse, ClientError> {
        self.client.post("/api/dispatch-pools", req).await
    }

    /// Update a dispatch pool.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdateDispatchPoolRequest,
    ) -> Result<DispatchPoolResponse, ClientError> {
        self.client
            .put(&format!("/api/dispatch-pools/{}", id), req)
            .await
    }

    /// Hard-delete a dispatch pool.
    pub async fn delete(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/dispatch-pools/{}", id))
            .await
    }

    /// Archive (soft-delete) a dispatch pool. The row is kept; status flips
    /// to ARCHIVED.
    pub async fn archive(&self, id: &str) -> Result<DispatchPoolResponse, ClientError> {
        self.client
            .post_action(&format!("/api/dispatch-pools/{}/archive", id))
            .await
    }

    /// Suspend a dispatch pool.
    pub async fn suspend(&self, id: &str) -> Result<DispatchPoolResponse, ClientError> {
        self.client
            .post_action(&format!("/api/dispatch-pools/{}/suspend", id))
            .await
    }

    /// Activate a dispatch pool.
    pub async fn activate(&self, id: &str) -> Result<DispatchPoolResponse, ClientError> {
        self.client
            .post_action(&format!("/api/dispatch-pools/{}/activate", id))
            .await
    }

    /// Sync dispatch pools for an application — declarative reconciliation
    /// against `POST /api/applications/{app_code}/dispatch-pools/sync`.
    pub async fn sync(
        &self,
        app_code: &str,
        req: &SyncDispatchPoolsRequest,
        remove_unlisted: bool,
    ) -> Result<crate::client::SyncResult, ClientError> {
        let query = if remove_unlisted {
            "?removeUnlisted=true"
        } else {
            ""
        };
        self.client
            .post(
                &format!(
                    "/api/applications/{}/dispatch-pools/sync{}",
                    app_code, query
                ),
                req,
            )
            .await
    }
}
