//! Role and permission management operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError};
use super::applications::CreatedResponse;

/// Paginated list of roles — `GET /api/roles`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleListResponse {
    pub roles: Vec<RoleResponse>,
    #[serde(default)]
    pub total: u64,
}

/// Paginated list of permissions — `GET /api/roles/permissions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionListResponse {
    pub permissions: Vec<PermissionResponse>,
    #[serde(default)]
    pub total: u64,
}

/// Request to create a role.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleRequest {
    /// Application code this role belongs to
    pub application_code: String,
    /// Role name (will be combined with app code to form code)
    pub role_name: String,
    /// Display name
    pub display_name: String,
    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Initial permissions
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Whether clients can manage this role
    #[serde(default)]
    pub client_managed: bool,
}

/// Request to update a role.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoleRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_managed: Option<bool>,
}

/// Request body for `grant_permission` — `{ "permission": "..." }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantPermissionRequest {
    pub permission: String,
}

/// Role response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleResponse {
    pub id: String,
    pub name: String,
    pub short_name: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub application_code: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub source: String,
    #[serde(default)]
    pub client_managed: bool,
    pub created_at: String,
    pub updated_at: String,
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

impl FlowCatalystClient {
    // ── Roles ────────────────────────────────────────────────────────────────

    /// List all roles.
    pub async fn list_roles(&self) -> Result<RoleListResponse, ClientError> {
        self.get("/api/roles").await
    }

    /// Get a role by name.
    pub async fn get_role(&self, name: &str) -> Result<RoleResponse, ClientError> {
        self.get(&format!("/api/roles/{}", name)).await
    }

    /// Get a role by code (`application:role-name`).
    pub async fn get_role_by_code(&self, code: &str) -> Result<RoleResponse, ClientError> {
        self.get(&format!("/api/roles/by-code/{}", code)).await
    }

    /// Create a new role.
    ///
    /// Returns `{ id }` only. Call `get_role(name)` if you need the full record.
    pub async fn create_role(
        &self,
        req: &CreateRoleRequest,
    ) -> Result<CreatedResponse, ClientError> {
        self.post("/api/roles", req).await
    }

    /// Update an existing role by name. The platform responds with 204.
    pub async fn update_role(
        &self,
        name: &str,
        req: &UpdateRoleRequest,
    ) -> Result<(), ClientError> {
        let _: serde_json::Value =
            self.put(&format!("/api/roles/{}", name), req).await?;
        Ok(())
    }

    /// Delete a role by name.
    pub async fn delete_role(&self, name: &str) -> Result<(), ClientError> {
        self.delete_req(&format!("/api/roles/{}", name)).await
    }

    /// List roles scoped to an application.
    pub async fn list_roles_for_application(
        &self,
        application_id: &str,
    ) -> Result<RoleListResponse, ClientError> {
        self.get(&format!(
            "/api/roles/by-application/{}",
            application_id
        ))
        .await
    }

    /// Grant a permission to a role. Returns the updated role.
    pub async fn grant_permission(
        &self,
        role_name: &str,
        permission: &str,
    ) -> Result<RoleResponse, ClientError> {
        let body = GrantPermissionRequest {
            permission: permission.to_string(),
        };
        self.post(&format!("/api/roles/{}/permissions", role_name), &body)
            .await
    }

    /// Revoke a permission from a role. Returns the updated role.
    pub async fn revoke_permission(
        &self,
        role_name: &str,
        permission: &str,
    ) -> Result<RoleResponse, ClientError> {
        self.delete_with_response(&format!(
            "/api/roles/{}/permissions/{}",
            role_name, permission
        ))
        .await
    }

    // ── Permissions ──────────────────────────────────────────────────────────

    /// List all permissions.
    pub async fn list_permissions(
        &self,
    ) -> Result<PermissionListResponse, ClientError> {
        self.get("/api/roles/permissions").await
    }

    /// Get a permission by name.
    pub async fn get_permission(
        &self,
        name: &str,
    ) -> Result<PermissionResponse, ClientError> {
        self.get(&format!("/api/roles/permissions/{}", name))
            .await
    }
}
