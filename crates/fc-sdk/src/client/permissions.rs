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

/// A permission catalogue row (Go's `PermissionResponse`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    /// The permission string, e.g. `platform:iam:role:read`.
    pub permission: String,
    /// Human-readable name.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
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

#[cfg(test)]
mod tests {
    use crate::client::test_support::MockPlatform;

    #[tokio::test]
    async fn list_and_get_read_gos_permission_shape() {
        let row = r#"{"permission":"platform:iam:role:read","name":"Read roles",
            "description":"Read roles","category":"IAM"}"#;
        let list = format!(r#"{{"permissions":[{row}],"total":1}}"#);
        let stub = MockPlatform::start(&[
            ("GET", "/api/roles/permissions", 200, &list),
            (
                "GET",
                "/api/roles/permissions/platform:iam:role:read",
                200,
                row,
            ),
        ])
        .await;
        let c = stub.client();
        let all = c.permissions().list().await.unwrap();
        assert_eq!(all.permissions[0].name, "Read roles");
        assert_eq!(all.permissions[0].category.as_deref(), Some("IAM"));
        let one = c.permissions().get("platform:iam:role:read").await.unwrap();
        assert_eq!(one.permission, "platform:iam:role:read");
    }
}
