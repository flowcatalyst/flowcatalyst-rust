//! Dispatch pool management operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError, ListResponse};

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

impl FlowCatalystClient {
    /// List dispatch pools with optional filters.
    pub async fn list_dispatch_pools(
        &self,
        filters: &DispatchPoolFilters,
    ) -> Result<ListResponse<DispatchPoolResponse>, ClientError> {
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
        self.get(&format!("/api/dispatch-pools{}", query))
            .await
    }

    /// Get a dispatch pool by ID.
    pub async fn get_dispatch_pool(
        &self,
        id: &str,
    ) -> Result<DispatchPoolResponse, ClientError> {
        self.get(&format!("/api/dispatch-pools/{}", id)).await
    }

    /// Create a new dispatch pool.
    pub async fn create_dispatch_pool(
        &self,
        req: &CreateDispatchPoolRequest,
    ) -> Result<DispatchPoolResponse, ClientError> {
        self.post("/api/dispatch-pools", req).await
    }

    /// Update a dispatch pool.
    pub async fn update_dispatch_pool(
        &self,
        id: &str,
        req: &UpdateDispatchPoolRequest,
    ) -> Result<DispatchPoolResponse, ClientError> {
        self.put(&format!("/api/dispatch-pools/{}", id), req)
            .await
    }

    /// Delete a dispatch pool.
    pub async fn delete_dispatch_pool(&self, id: &str) -> Result<(), ClientError> {
        self.delete_req(&format!("/api/dispatch-pools/{}", id))
            .await
    }

    /// Suspend a dispatch pool.
    pub async fn suspend_dispatch_pool(
        &self,
        id: &str,
    ) -> Result<DispatchPoolResponse, ClientError> {
        self.post_action(&format!("/api/dispatch-pools/{}/suspend", id))
            .await
    }

    /// Activate a dispatch pool.
    pub async fn activate_dispatch_pool(
        &self,
        id: &str,
    ) -> Result<DispatchPoolResponse, ClientError> {
        self.post_action(&format!("/api/dispatch-pools/{}/activate", id))
            .await
    }
}
