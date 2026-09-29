//! Dispatch pool management operations.

use super::applications::CreatedResponse;
use super::{ClientError, FlowCatalystClient};
use crate::client::SyncResult;
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
    #[serde(default)]
    pub client_identifier: Option<String>,
    pub status: String,
    #[serde(default)]
    pub rate_limit: Option<u32>,
    #[serde(default)]
    pub concurrency: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
}

/// Dispatch pool list — `GET /api/dispatch-pools` answers `{pools, total}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolListResponse {
    pub pools: Vec<DispatchPoolResponse>,
    #[serde(default)]
    pub total: u64,
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
    /// Required by Go's platform.
    pub name: String,
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
    pub async fn list(
        &self,
        filters: &DispatchPoolFilters,
    ) -> Result<DispatchPoolListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(ref s) = filters.status {
            params.push(("status", s.clone()));
        }
        if let Some(ref cid) = filters.client_id {
            params.push(("clientId", cid.clone()));
        }
        let query = FlowCatalystClient::query_string(&params);
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
    ///
    /// Returns `{ id }` only. Call `get(&id)` if you need the full record.
    pub async fn create(
        &self,
        req: &CreateDispatchPoolRequest,
    ) -> Result<CreatedResponse, ClientError> {
        self.client.post("/api/dispatch-pools", req).await
    }

    /// Update a dispatch pool (204). Call `get(id)` for the updated record.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdateDispatchPoolRequest,
    ) -> Result<(), ClientError> {
        self.client
            .put_empty(&format!("/api/dispatch-pools/{}", id), req)
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
    ///
    /// The platform answers 204 (as Go); the pool is read back.
    pub async fn archive(&self, id: &str) -> Result<DispatchPoolResponse, ClientError> {
        self.client
            .post_empty(&format!("/api/dispatch-pools/{}/archive", id))
            .await?;
        self.get(id).await
    }

    /// Suspend a dispatch pool.
    ///
    /// The platform answers 204 (as Go); the pool is read back.
    pub async fn suspend(&self, id: &str) -> Result<DispatchPoolResponse, ClientError> {
        self.client
            .post_empty(&format!("/api/dispatch-pools/{}/suspend", id))
            .await?;
        self.get(id).await
    }

    /// Activate a dispatch pool.
    ///
    /// The platform answers 204 (as Go); the pool is read back.
    pub async fn activate(&self, id: &str) -> Result<DispatchPoolResponse, ClientError> {
        self.client
            .post_empty(&format!("/api/dispatch-pools/{}/activate", id))
            .await?;
        self.get(id).await
    }

    /// Sync dispatch pools for an application — declarative reconciliation
    /// against `POST /api/applications/{appCode}/dispatch-pools/sync`.
    pub async fn sync(
        &self,
        app_code: &str,
        req: &SyncDispatchPoolsRequest,
        remove_unlisted: bool,
    ) -> Result<SyncResult, ClientError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::MockPlatform;

    const POOL: &str = r#"{"id":"dpl_1","code":"fast","name":"Fast","status":"ARCHIVED",
        "concurrency":10,"createdAt":"t","updatedAt":"t"}"#;

    #[tokio::test]
    async fn list_reads_pools() {
        let list = format!(r#"{{"pools":[{POOL}],"total":1}}"#);
        let stub = MockPlatform::start(&[("GET", "/api/dispatch-pools", 200, &list)]).await;
        let resp = stub
            .client()
            .dispatch_pools()
            .list(&DispatchPoolFilters {
                status: Some("ACTIVE".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.pools.len(), 1);
        assert_eq!(resp.total, 1);
        assert_eq!(
            stub.single().query_pairs(),
            vec![("status".to_string(), "ACTIVE".to_string())]
        );
    }

    #[tokio::test]
    async fn create_reads_the_id_and_update_accepts_204() {
        let stub = MockPlatform::start(&[
            ("POST", "/api/dispatch-pools", 201, r#"{"id":"dpl_1"}"#),
            ("PUT", "/api/dispatch-pools/dpl_1", 204, ""),
        ])
        .await;
        let c = stub.client();
        let created = c
            .dispatch_pools()
            .create(&CreateDispatchPoolRequest {
                code: "fast".into(),
                name: "Fast".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(created.id, "dpl_1");
        c.dispatch_pools()
            .update(
                "dpl_1",
                &UpdateDispatchPoolRequest {
                    concurrency: Some(5),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            stub.requests()[1].json(),
            serde_json::json!({"concurrency": 5})
        );
    }

    #[tokio::test]
    async fn archive_posts_to_the_archive_route() {
        let stub = MockPlatform::start(&[
            ("POST", "/api/dispatch-pools/dpl_1/archive", 204, ""),
            ("GET", "/api/dispatch-pools/dpl_1", 200, POOL),
        ])
        .await;
        let pool = stub
            .client()
            .dispatch_pools()
            .archive("dpl_1")
            .await
            .unwrap();
        assert_eq!(pool.status, "ARCHIVED");
        let reqs = stub.requests();
        assert_eq!(reqs[0].method, "POST");
        assert_eq!(reqs[0].path, "/api/dispatch-pools/dpl_1/archive");
    }

    #[test]
    fn sync_item_always_sends_name() {
        let item = SyncDispatchPoolItem {
            code: "fast".into(),
            name: "Fast".into(),
            concurrency: None,
            rate_limit: None,
            description: None,
        };
        assert_eq!(
            serde_json::to_value(&item).unwrap(),
            serde_json::json!({"code": "fast", "name": "Fast"})
        );
    }
}
