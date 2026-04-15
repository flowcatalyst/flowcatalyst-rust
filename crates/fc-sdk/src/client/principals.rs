//! Principal (user/service) management operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError, ListResponse};

/// Request to create a user principal.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// When set to `false`, the platform skips its password complexity rules
    /// (uppercase/lowercase/digit/special) and only enforces a 2-character minimum.
    /// Use when your application enforces its own password policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforce_password_complexity: Option<bool>,
}

/// Request to reset a principal's password via the admin API.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequest {
    pub new_password: String,
    /// When set to `false`, the platform skips its password complexity rules
    /// (uppercase/lowercase/digit/special) and only enforces a 2-character minimum.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforce_password_complexity: Option<bool>,
}

/// Request to update a principal.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePrincipalRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

/// Filters for listing principals.
#[derive(Debug, Clone, Default)]
pub struct PrincipalFilters {
    pub client_id: Option<String>,
    pub r#type: Option<String>,
    pub active: Option<String>,
    pub email: Option<String>,
}

/// Principal response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub principal_type: String,
    pub scope: String,
    #[serde(default)]
    pub client_id: Option<String>,
    pub name: String,
    pub active: bool,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub idp_type: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub is_anchor_user: bool,
    #[serde(default)]
    pub granted_client_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Role reference returned by principal role queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalRoleResponse {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

/// Client access grant for a principal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessGrantResponse {
    pub client_id: String,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub client_identifier: Option<String>,
    #[serde(default)]
    pub granted_at: Option<String>,
}

/// Request to assign a single role.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignRoleRequest {
    pub role_name: String,
}

/// Request to replace all roles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceRolesRequest {
    pub roles: Vec<String>,
}

/// Request to grant client access.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantClientAccessRequest {
    pub client_id: String,
}

impl FlowCatalystClient {
    /// Create a new user principal.
    pub async fn create_user(
        &self,
        req: &CreateUserRequest,
    ) -> Result<PrincipalResponse, ClientError> {
        self.post("/api/admin/principals/users", req).await
    }

    /// List principals with optional filters.
    pub async fn list_principals(
        &self,
        filters: &PrincipalFilters,
    ) -> Result<ListResponse<PrincipalResponse>, ClientError> {
        let mut params = Vec::new();
        if let Some(ref cid) = filters.client_id {
            params.push(format!("clientId={}", cid));
        }
        if let Some(ref t) = filters.r#type {
            params.push(format!("type={}", t));
        }
        if let Some(ref a) = filters.active {
            params.push(format!("active={}", a));
        }
        if let Some(ref e) = filters.email {
            params.push(format!("email={}", e));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.get(&format!("/api/admin/principals{}", query)).await
    }

    /// Get a principal by ID.
    pub async fn get_principal(&self, id: &str) -> Result<PrincipalResponse, ClientError> {
        self.get(&format!("/api/admin/principals/{}", id)).await
    }

    /// Find principals by email.
    pub async fn find_principal_by_email(
        &self,
        email: &str,
    ) -> Result<ListResponse<PrincipalResponse>, ClientError> {
        self.list_principals(&PrincipalFilters {
            email: Some(email.to_string()),
            ..Default::default()
        })
        .await
    }

    /// Update a principal.
    pub async fn update_principal(
        &self,
        id: &str,
        req: &UpdatePrincipalRequest,
    ) -> Result<PrincipalResponse, ClientError> {
        self.put(&format!("/api/admin/principals/{}", id), req).await
    }

    /// Activate a principal.
    pub async fn activate_principal(
        &self,
        id: &str,
    ) -> Result<PrincipalResponse, ClientError> {
        self.post_action(&format!("/api/admin/principals/{}/activate", id))
            .await
    }

    /// Deactivate a principal.
    pub async fn deactivate_principal(
        &self,
        id: &str,
    ) -> Result<PrincipalResponse, ClientError> {
        self.post_action(&format!("/api/admin/principals/{}/deactivate", id))
            .await
    }

    /// Get roles assigned to a principal.
    pub async fn get_principal_roles(
        &self,
        id: &str,
    ) -> Result<ListResponse<PrincipalRoleResponse>, ClientError> {
        self.get(&format!("/api/admin/principals/{}/roles", id))
            .await
    }

    /// Assign a single role to a principal.
    pub async fn assign_principal_role(
        &self,
        id: &str,
        role_name: &str,
    ) -> Result<(), ClientError> {
        let body = AssignRoleRequest {
            role_name: role_name.to_string(),
        };
        let _: serde_json::Value = self
            .post(&format!("/api/admin/principals/{}/roles", id), &body)
            .await?;
        Ok(())
    }

    /// Remove a role from a principal.
    pub async fn remove_principal_role(
        &self,
        id: &str,
        role_name: &str,
    ) -> Result<(), ClientError> {
        self.delete_req(&format!(
            "/api/admin/principals/{}/roles/{}",
            id, role_name
        ))
        .await
    }

    /// Replace all roles on a principal.
    pub async fn assign_principal_roles(
        &self,
        id: &str,
        roles: Vec<String>,
    ) -> Result<(), ClientError> {
        let body = ReplaceRolesRequest { roles };
        let _: serde_json::Value = self
            .put(&format!("/api/admin/principals/{}/roles", id), &body)
            .await?;
        Ok(())
    }

    /// Get client access grants for a principal.
    pub async fn get_principal_client_access(
        &self,
        id: &str,
    ) -> Result<ListResponse<ClientAccessGrantResponse>, ClientError> {
        self.get(&format!("/api/admin/principals/{}/client-access", id))
            .await
    }

    /// Grant client access to a principal.
    pub async fn grant_principal_client_access(
        &self,
        principal_id: &str,
        client_id: &str,
    ) -> Result<(), ClientError> {
        let body = GrantClientAccessRequest {
            client_id: client_id.to_string(),
        };
        let _: serde_json::Value = self
            .post(
                &format!("/api/admin/principals/{}/client-access", principal_id),
                &body,
            )
            .await?;
        Ok(())
    }

    /// Revoke client access from a principal.
    pub async fn revoke_principal_client_access(
        &self,
        principal_id: &str,
        client_id: &str,
    ) -> Result<(), ClientError> {
        self.delete_req(&format!(
            "/api/admin/principals/{}/client-access/{}",
            principal_id, client_id
        ))
        .await
    }

    /// Reset a principal's password via the admin API.
    pub async fn reset_principal_password(
        &self,
        principal_id: &str,
        req: &ResetPasswordRequest,
    ) -> Result<(), ClientError> {
        let _: serde_json::Value = self
            .post(
                &format!("/api/admin/principals/{}/reset-password", principal_id),
                req,
            )
            .await?;
        Ok(())
    }
}
