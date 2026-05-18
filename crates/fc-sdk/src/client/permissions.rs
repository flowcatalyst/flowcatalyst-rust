//! Permission catalogue queries — `/api/roles/permissions/*`.
//!
//! Permissions are immutable platform constants; the only operations are
//! list + get. Mutation of permission grants happens via the role itself
//! (see [`crate::client::Roles::grant_permission`]).

use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

/// Paginated list of permissions — `GET /api/roles/permissions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionListResponse {
    pub permissions: Vec<PermissionResponse>,
    #[serde(default)]
    pub total: u64,
}

/// Permission response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    pub permission: String,
    pub application: String,
    pub context: String,
    pub aggregate: String,
    pub action: String,
    pub description: String,
}

/// Permissions resource accessor — created via [`FlowCatalystClient::permissions`].
pub struct Permissions<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl Permissions<'_> {
    /// List all permissions.
    pub async fn list(&self) -> Result<PermissionListResponse, ClientError> {
        self.client.get("/api/roles/permissions").await
    }

    /// Get a permission by name.
    pub async fn get(&self, name: &str) -> Result<PermissionResponse, ClientError> {
        self.client
            .get(&format!("/api/roles/permissions/{}", name))
            .await
    }
}
