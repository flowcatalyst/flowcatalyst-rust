//! Application Roles SDK API
//!
//! REST endpoints for applications to manage their own roles.
//! Used by application SDKs to sync role definitions.

use axum::{
    routing::{get, delete},
    extract::{State, Path, Query},
    Json, Router,
};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashSet;

use crate::{AuthRole, RoleSource};
use crate::{ApplicationRepository, RoleRepository};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// Role DTO for SDK response
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleDto {
    /// Short name (without app prefix)
    pub name: String,
    /// Full role code (e.g., "myapp:admin")
    pub full_name: String,
    /// Human-readable display name
    pub display_name: String,
    /// Role description
    pub description: Option<String>,
    /// Permissions granted by this role
    pub permissions: Vec<String>,
    /// Role source (CODE, DATABASE, or SDK)
    pub source: String,
    /// Whether client can manage this role
    pub client_managed: bool,
}

impl RoleDto {
    fn from_role(role: AuthRole) -> Self {
        // Extract short name from full name (e.g., "myapp:admin" -> "admin")
        let short_name = role.name.split(':').nth(1)
            .unwrap_or(&role.name)
            .to_string();

        Self {
            name: short_name,
            full_name: role.name,
            display_name: role.display_name,
            description: role.description,
            permissions: role.permissions.into_iter().collect(),
            source: role.source.as_str().to_string(),
            client_managed: role.client_managed,
        }
    }
}

/// List roles response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListRolesResponse {
    pub roles: Vec<RoleDto>,
    pub total: usize,
}

/// Create role request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleRequest {
    /// Role name (will be auto-prefixed with app code)
    pub name: String,
    /// Human-readable display name
    pub display_name: Option<String>,
    /// Description
    pub description: Option<String>,
    /// Permission strings
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Whether client can manage this role
    #[serde(default)]
    pub client_managed: bool,
}

/// Sync roles request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncRolesRequest {
    /// Roles to sync
    pub roles: Vec<CreateRoleRequest>,
}

/// Sync roles response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncRolesResponse {
    /// Number of roles synced
    pub synced_count: usize,
    /// Updated SDK role list
    pub roles: Vec<RoleDto>,
}

/// Query parameters for listing roles
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRolesQuery {
    /// Filter by source (CODE, DATABASE, SDK)
    pub source: Option<String>,
}

/// Query parameters for sync
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRolesQuery {
    /// Remove SDK roles not in the sync list
    #[serde(default)]
    pub remove_unlisted: bool,
}

/// Application Roles SDK state
#[derive(Clone)]
pub struct ApplicationRolesSdkState {
    pub application_repo: Arc<ApplicationRepository>,
    pub role_repo: Arc<RoleRepository>,
}

/// List all roles for an application
#[utoipa::path(
    get,
    path = "/{app_code}/roles",
    tag = "application-roles-sdk",
    params(
        ("app_code" = String, Path, description = "Application code"),
        ("source" = Option<String>, Query, description = "Filter by source (CODE, DATABASE, SDK)")
    ),
    responses(
        (status = 200, description = "List of roles", body = ListRolesResponse),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_roles(
    State(state): State<ApplicationRolesSdkState>,
    _auth: Authenticated,
    Path(app_code): Path<String>,
    Query(query): Query<ListRolesQuery>,
) -> Result<Json<ListRolesResponse>, PlatformError> {
    // Verify application exists
    state.application_repo.find_by_code(&app_code).await?
        .ok_or_else(|| PlatformError::not_found("Application", &app_code))?;

    // Get roles for this application
    let mut roles = state.role_repo.find_by_application(&app_code).await?;

    // Filter by source if specified
    if let Some(ref source_filter) = query.source {
        let source = match source_filter.to_uppercase().as_str() {
            "CODE" => Some(RoleSource::Code),
            "DATABASE" => Some(RoleSource::Database),
            "SDK" => Some(RoleSource::Sdk),
            _ => None,
        };

        if let Some(s) = source {
            roles.retain(|r| r.source == s);
        }
    }

    let total = roles.len();
    let role_dtos: Vec<RoleDto> = roles.into_iter()
        .map(RoleDto::from_role)
        .collect();

    Ok(Json(ListRolesResponse {
        roles: role_dtos,
        total,
    }))
}

/// Create a single role
#[utoipa::path(
    post,
    path = "/{app_code}/roles",
    tag = "application-roles-sdk",
    params(
        ("app_code" = String, Path, description = "Application code")
    ),
    request_body = CreateRoleRequest,
    responses(
        (status = 201, description = "Role created", body = RoleDto),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Application not found"),
        (status = 409, description = "Role already exists")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_role(
    State(state): State<ApplicationRolesSdkState>,
    _auth: Authenticated,
    Path(app_code): Path<String>,
    Json(req): Json<CreateRoleRequest>,
) -> Result<Json<RoleDto>, PlatformError> {
    // Validate request
    if req.name.is_empty() {
        return Err(PlatformError::validation("Role name is required"));
    }

    // Verify application exists
    state.application_repo.find_by_code(&app_code).await?
        .ok_or_else(|| PlatformError::not_found("Application", &app_code))?;

    // Build full role code
    let role_code = format!("{}:{}", app_code, req.name);

    // Check if role already exists
    if state.role_repo.exists_by_code(&role_code).await? {
        return Err(PlatformError::duplicate("Role", "code", &role_code));
    }

    // Create the role
    let display_name = req.display_name.unwrap_or_else(|| req.name.clone());
    let mut role = AuthRole::new(&app_code, &req.name, &display_name)
        .with_source(RoleSource::Sdk)
        .with_client_managed(req.client_managed);

    if let Some(desc) = req.description {
        role = role.with_description(desc);
    }

    for perm in req.permissions {
        role.permissions.insert(perm);
    }

    state.role_repo.insert(&role).await?;

    Ok(Json(RoleDto::from_role(role)))
}

/// Bulk sync roles
#[utoipa::path(
    post,
    path = "/{app_code}/roles/sync",
    tag = "application-roles-sdk",
    params(
        ("app_code" = String, Path, description = "Application code"),
        ("remove_unlisted" = Option<bool>, Query, description = "Remove SDK roles not in list")
    ),
    request_body = SyncRolesRequest,
    responses(
        (status = 200, description = "Roles synced", body = SyncRolesResponse),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn sync_roles(
    State(state): State<ApplicationRolesSdkState>,
    _auth: Authenticated,
    Path(app_code): Path<String>,
    Query(query): Query<SyncRolesQuery>,
    Json(req): Json<SyncRolesRequest>,
) -> Result<Json<SyncRolesResponse>, PlatformError> {
    // Verify application exists
    state.application_repo.find_by_code(&app_code).await?
        .ok_or_else(|| PlatformError::not_found("Application", &app_code))?;

    // Get existing SDK roles
    let existing_roles = state.role_repo.find_by_application(&app_code).await?;
    let existing_sdk_roles: Vec<_> = existing_roles.into_iter()
        .filter(|r| r.source == RoleSource::Sdk)
        .collect();

    // Track synced role codes
    let mut synced_codes: HashSet<String> = HashSet::new();
    let mut synced_count = 0;

    // Process each role in the sync request
    for role_req in &req.roles {
        if role_req.name.is_empty() {
            continue;
        }

        let role_code = format!("{}:{}", app_code, role_req.name);
        synced_codes.insert(role_code.clone());

        // Check if role exists
        if let Some(mut existing) = state.role_repo.find_by_name(&role_code).await? {
            // Update existing role
            let display_name = role_req.display_name.as_ref()
                .unwrap_or(&role_req.name);
            existing.display_name = display_name.clone();
            existing.description = role_req.description.clone();
            existing.client_managed = role_req.client_managed;
            existing.permissions = role_req.permissions.iter().cloned().collect();
            existing.updated_at = chrono::Utc::now();

            state.role_repo.update(&existing).await?;
        } else {
            // Create new role
            let display_name = role_req.display_name.as_ref()
                .unwrap_or(&role_req.name);
            let mut role = AuthRole::new(&app_code, &role_req.name, display_name)
                .with_source(RoleSource::Sdk)
                .with_client_managed(role_req.client_managed);

            if let Some(ref desc) = role_req.description {
                role = role.with_description(desc);
            }

            for perm in &role_req.permissions {
                role.permissions.insert(perm.clone());
            }

            state.role_repo.insert(&role).await?;
        }

        synced_count += 1;
    }

    // Remove unlisted SDK roles if requested. Refuse if any still have
    // principal assignments — the junction has no DB-level FK, so silently
    // dropping it would orphan user role assignments. Caller must strip the
    // assignments first.
    if query.remove_unlisted {
        for existing in existing_sdk_roles {
            if synced_codes.contains(&existing.name) {
                continue;
            }
            let assignments = state.role_repo.count_assignments(&existing.name).await?;
            if assignments > 0 {
                return Err(PlatformError::validation(format!(
                    "Cannot remove role '{}' — {} principal(s) still hold it. \
                     Strip the assignments before syncing with remove_unlisted=true.",
                    existing.name, assignments,
                )));
            }
            state.role_repo.delete(&existing.id).await?;
        }
    }

    // Get updated roles
    let updated_roles = state.role_repo.find_by_application(&app_code).await?;
    let sdk_roles: Vec<RoleDto> = updated_roles.into_iter()
        .filter(|r| r.source == RoleSource::Sdk)
        .map(RoleDto::from_role)
        .collect();

    Ok(Json(SyncRolesResponse {
        synced_count,
        roles: sdk_roles,
    }))
}

/// Delete a role (SDK-sourced only)
#[utoipa::path(
    delete,
    path = "/{app_code}/roles/{role_name}",
    tag = "application-roles-sdk",
    params(
        ("app_code" = String, Path, description = "Application code"),
        ("role_name" = String, Path, description = "Role name (without app prefix)")
    ),
    responses(
        (status = 204, description = "Role deleted"),
        (status = 400, description = "Cannot delete non-SDK role"),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_role(
    State(state): State<ApplicationRolesSdkState>,
    _auth: Authenticated,
    Path((app_code, role_name)): Path<(String, String)>,
) -> Result<(), PlatformError> {
    let role_code = format!("{}:{}", app_code, role_name);

    // Get the role
    let role = state.role_repo.find_by_name(&role_code).await?
        .ok_or_else(|| PlatformError::not_found("Role", &role_code))?;

    // Only allow deleting SDK-sourced roles
    if role.source != RoleSource::Sdk {
        return Err(PlatformError::validation(
            "Cannot delete non-SDK role. Only SDK-sourced roles can be deleted via API."
        ));
    }

    // Refuse if principals still hold this role — enforced in code because
    // the junction (iam_principal_roles) has no DB-level FK.
    let assignments = state.role_repo.count_assignments(&role.name).await?;
    if assignments > 0 {
        return Err(PlatformError::validation(format!(
            "Cannot delete role '{}' — {} principal(s) still hold it. \
             Strip the assignments before deleting.",
            role.name, assignments,
        )));
    }

    state.role_repo.delete(&role.id).await?;

    Ok(())
}

/// Create application roles SDK router
pub fn application_roles_sdk_router(state: ApplicationRolesSdkState) -> Router {
    Router::new()
        .route("/{app_code}/roles", get(list_roles).post(create_role))
        .route("/{app_code}/roles/{role_name}", delete(delete_role))
        .with_state(state)
}
