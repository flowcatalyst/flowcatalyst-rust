//! Principal (user/service) management operations.

use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

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

/// Role assignment returned by GET /api/principals/{id}/roles.
///
/// Matches the platform's `RoleAssignmentDto` — uses `roleName` and
/// `assignmentSource`, not `name` and `source`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalRoleResponse {
    pub id: String,
    pub role_name: String,
    pub assignment_source: String,
    pub assigned_at: String,
}

/// Client access grant for a principal.
///
/// Matches the platform's `ClientAccessGrantResponse` — `id`, `clientId`,
/// `grantedAt`, optional `expiresAt`. No client name / identifier fields
/// (those were in an older SDK version that didn't reflect the backend).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessGrantResponse {
    pub id: String,
    pub client_id: String,
    pub granted_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// Request to assign a single role (additive — keeps existing roles).
///
/// The backend expects `{ "role": "..." }`, not `{ "roleName": "..." }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignRoleRequest {
    pub role: String,
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

/// Paginated list of principals — `GET /api/principals`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalListResponse {
    pub principals: Vec<PrincipalResponse>,
    #[serde(default)]
    pub total: u64,
}

/// List of role assignments for a principal — `GET /api/principals/{id}/roles`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalRoleListResponse {
    pub roles: Vec<PrincipalRoleResponse>,
}

/// List of client access grants — `GET /api/principals/{id}/client-access`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessListResponse {
    pub grants: Vec<ClientAccessGrantResponse>,
}

/// Request body for the per-resource sync endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPrincipalsRequest {
    pub principals: Vec<SyncPrincipalItem>,
}

/// A principal item for sync. Matched by email.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPrincipalItem {
    pub email: String,
    pub name: String,
    /// Role short names (without the `<app>:` prefix — the platform adds it).
    #[serde(default)]
    pub roles: Vec<String>,
    /// Defaults to `true` server-side when omitted.
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool {
    true
}

/// Principals resource accessor — created via [`FlowCatalystClient::principals`].
pub struct Principals<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl Principals<'_> {
    /// Create a new user principal.
    pub async fn create_user(
        &self,
        req: &CreateUserRequest,
    ) -> Result<PrincipalResponse, ClientError> {
        self.client.post("/api/principals/users", req).await
    }

    /// List principals with optional filters.
    pub async fn list(
        &self,
        filters: &PrincipalFilters,
    ) -> Result<PrincipalListResponse, ClientError> {
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
        self.client.get(&format!("/api/principals{}", query)).await
    }

    /// Get a principal by ID.
    pub async fn get(&self, id: &str) -> Result<PrincipalResponse, ClientError> {
        self.client.get(&format!("/api/principals/{}", id)).await
    }

    /// Find principals by email.
    ///
    /// The result still contains every principal the caller is authorised to
    /// see whose email matches exactly (case-insensitive) — callers should
    /// pick the expected one by `email` rather than assuming index 0.
    pub async fn find_by_email(
        &self,
        email: &str,
    ) -> Result<PrincipalListResponse, ClientError> {
        self.list(&PrincipalFilters {
            email: Some(email.to_string()),
            ..Default::default()
        })
        .await
    }

    /// Update a principal.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdatePrincipalRequest,
    ) -> Result<PrincipalResponse, ClientError> {
        self.client
            .put(&format!("/api/principals/{}", id), req)
            .await
    }

    /// Activate a principal.
    ///
    /// The platform returns `{ "message": "..." }` only. Call `get(id)` if
    /// you need the refreshed record.
    pub async fn activate(&self, id: &str) -> Result<(), ClientError> {
        let _: serde_json::Value = self
            .client
            .post_action(&format!("/api/principals/{}/activate", id))
            .await?;
        Ok(())
    }

    /// Deactivate a principal. The platform returns a message only.
    pub async fn deactivate(&self, id: &str) -> Result<(), ClientError> {
        let _: serde_json::Value = self
            .client
            .post_action(&format!("/api/principals/{}/deactivate", id))
            .await?;
        Ok(())
    }

    /// Get roles assigned to a principal.
    pub async fn roles(
        &self,
        id: &str,
    ) -> Result<PrincipalRoleListResponse, ClientError> {
        self.client
            .get(&format!("/api/principals/{}/roles", id))
            .await
    }

    /// Add a single role to a principal (additive — keeps existing roles).
    ///
    /// Distinct from [`set_roles`] which replaces the full set. Renamed from
    /// the original `assign_role` / `assign_roles` pair to make the
    /// additive-vs-replace distinction visible at the call site.
    pub async fn add_role(&self, id: &str, role_name: &str) -> Result<(), ClientError> {
        let body = AssignRoleRequest {
            role: role_name.to_string(),
        };
        let _: serde_json::Value = self
            .client
            .post(&format!("/api/principals/{}/roles", id), &body)
            .await?;
        Ok(())
    }

    /// Remove a role from a principal.
    pub async fn remove_role(&self, id: &str, role_name: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/principals/{}/roles/{}", id, role_name))
            .await
    }

    /// Replace all roles on a principal with the given set.
    pub async fn set_roles(&self, id: &str, roles: Vec<String>) -> Result<(), ClientError> {
        let body = ReplaceRolesRequest { roles };
        let _: serde_json::Value = self
            .client
            .put(&format!("/api/principals/{}/roles", id), &body)
            .await?;
        Ok(())
    }

    /// Get client access grants for a principal.
    pub async fn client_access_grants(
        &self,
        id: &str,
    ) -> Result<ClientAccessListResponse, ClientError> {
        self.client
            .get(&format!("/api/principals/{}/client-access", id))
            .await
    }

    /// Grant client access to a principal.
    pub async fn grant_client_access(
        &self,
        principal_id: &str,
        client_id: &str,
    ) -> Result<(), ClientError> {
        let body = GrantClientAccessRequest {
            client_id: client_id.to_string(),
        };
        let _: serde_json::Value = self
            .client
            .post(
                &format!("/api/principals/{}/client-access", principal_id),
                &body,
            )
            .await?;
        Ok(())
    }

    /// Revoke client access from a principal.
    pub async fn revoke_client_access(
        &self,
        principal_id: &str,
        client_id: &str,
    ) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!(
                "/api/principals/{}/client-access/{}",
                principal_id, client_id
            ))
            .await
    }

    /// Reset a principal's password via the admin API.
    pub async fn reset_password(
        &self,
        principal_id: &str,
        req: &ResetPasswordRequest,
    ) -> Result<(), ClientError> {
        let _: serde_json::Value = self
            .client
            .post(
                &format!("/api/principals/{}/reset-password", principal_id),
                req,
            )
            .await?;
        Ok(())
    }

    /// Sync principals for an application — declarative reconciliation
    /// against `POST /api/applications/{app_code}/principals/sync`.
    ///
    /// When `remove_unlisted` is true the platform strips SDK-sourced role
    /// assignments from principals not in the list (principals themselves
    /// are never deleted by sync).
    pub async fn sync(
        &self,
        app_code: &str,
        req: &SyncPrincipalsRequest,
        remove_unlisted: bool,
    ) -> Result<crate::client::SyncResult, ClientError> {
        let query = if remove_unlisted {
            "?removeUnlisted=true"
        } else {
            ""
        };
        self.client
            .post(
                &format!("/api/applications/{}/principals/sync{}", app_code, query),
                req,
            )
            .await
    }
}
