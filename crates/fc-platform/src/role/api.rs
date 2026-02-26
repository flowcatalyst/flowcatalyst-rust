//! Roles Admin API
//!
//! REST endpoints for role management.

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::role::entity::{AuthRole, RoleSource};
use crate::role::repository::RoleRepository; use crate::application::repository::ApplicationRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::{PaginationParams, CreatedResponse, SuccessResponse};
use crate::shared::middleware::Authenticated;

/// Create role request
#[derive(Debug, Deserialize, ToSchema)]
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

/// Update role request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoleRequest {
    /// Display name
    pub display_name: Option<String>,

    /// Description
    pub description: Option<String>,

    /// Whether clients can manage this role
    pub client_managed: Option<bool>,
}

/// Grant permission request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrantPermissionRequest {
    /// Permission to grant
    pub permission: String,
}

/// Role response DTO (matches Java BffRoleResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleResponse {
    pub id: String,
    pub name: String,
    pub short_name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub application_code: String,
    pub permissions: Vec<String>,
    pub source: String,
    pub client_managed: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<AuthRole> for RoleResponse {
    fn from(r: AuthRole) -> Self {
        // Extract short name (part after colon, e.g., "platform:admin" -> "admin")
        let short_name = r.name.split(':').last().unwrap_or(&r.name).to_string();
        Self {
            id: r.id,
            name: r.name,
            short_name,
            display_name: r.display_name,
            description: r.description,
            application_code: r.application_code,
            permissions: r.permissions.into_iter().collect(),
            source: r.source.as_str().to_string(),
            client_managed: r.client_managed,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

/// Role list response (matches Java RoleListResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleListResponse {
    pub roles: Vec<RoleResponse>,
    pub total: usize,
}

/// Query parameters for roles list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct RolesQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by application code
    pub application_code: Option<String>,

    /// Filter by source
    pub source: Option<String>,

    /// Filter client-managed roles only
    pub client_managed: Option<bool>,
}

/// Roles service state
#[derive(Clone)]
pub struct RolesState {
    pub role_repo: Arc<RoleRepository>,
    pub application_repo: Option<Arc<ApplicationRepository>>,
}

/// Application option for filter dropdown
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationOption {
    pub id: String,
    pub code: String,
    pub name: String,
}

/// Application options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationOptionsResponse {
    pub options: Vec<ApplicationOption>,
}

/// Permission response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    pub permission: String,
    pub application: String,
    pub context: String,
    pub aggregate: String,
    pub action: String,
    pub description: String,
}

/// Permission list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionListResponse {
    pub permissions: Vec<PermissionResponse>,
    pub total: usize,
}

fn parse_source(s: &str) -> Result<RoleSource, PlatformError> {
    match s.to_uppercase().as_str() {
        "CODE" => Ok(RoleSource::Code),
        "DATABASE" => Ok(RoleSource::Database),
        "SDK" => Ok(RoleSource::Sdk),
        _ => Err(PlatformError::validation(format!("Invalid source: {}", s))),
    }
}

/// Create a new role
#[utoipa::path(
    post,
    path = "",
    tag = "roles",
    operation_id = "postApiAdminPlatformRoles",
    request_body = CreateRoleRequest,
    responses(
        (status = 201, description = "Role created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate role code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_role(
    State(state): State<RolesState>,
    auth: Authenticated,
    Json(req): Json<CreateRoleRequest>,
) -> Result<Json<CreatedResponse>, PlatformError> {
    // Only anchor users can create roles
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let role_name = format!("{}:{}", req.application_code, req.role_name);

    // Check for duplicate name
    if let Some(_) = state.role_repo.find_by_name(&role_name).await? {
        return Err(PlatformError::duplicate("Role", "name", &role_name));
    }

    let mut role = AuthRole::new(&req.application_code, &req.role_name, &req.display_name);

    if let Some(desc) = req.description {
        role = role.with_description(desc);
    }

    role = role.with_permissions(req.permissions);
    role = role.with_client_managed(req.client_managed);

    let id = role.id.clone();
    state.role_repo.insert(&role).await?;

    Ok(Json(CreatedResponse::new(id)))
}

/// Get role by ID or name (code)
///
/// The frontend calls this with the role name (e.g., "platform:super-admin"),
/// so we try by code first if it contains ":", otherwise by ID.
#[utoipa::path(
    get,
    path = "/{role_name}",
    tag = "roles",
    operation_id = "getApiAdminPlatformRolesByRoleName",
    params(
        ("role_name" = String, Path, description = "Role name (code) or ID")
    ),
    responses(
        (status = 200, description = "Role found", body = RoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_role(
    State(state): State<RolesState>,
    _auth: Authenticated,
    Path(role_name): Path<String>,
) -> Result<Json<RoleResponse>, PlatformError> {
    // Try by name first if it looks like a role name (contains ":")
    let role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        // Fall back to ID lookup
        state.role_repo.find_by_id(&role_name).await?
    };

    let role = role.ok_or_else(|| PlatformError::not_found("Role", &role_name))?;
    Ok(Json(role.into()))
}

/// Get role by code (name)
#[utoipa::path(
    get,
    path = "/by-code/{code}",
    tag = "roles",
    operation_id = "getApiAdminPlatformRolesByCodeByCode",
    params(
        ("code" = String, Path, description = "Role code")
    ),
    responses(
        (status = 200, description = "Role found", body = RoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_role_by_code(
    State(state): State<RolesState>,
    _auth: Authenticated,
    Path(code): Path<String>,
) -> Result<Json<RoleResponse>, PlatformError> {
    let role = state.role_repo.find_by_name(&code).await?
        .ok_or_else(|| PlatformError::not_found("Role", &code))?;

    Ok(Json(role.into()))
}

/// List roles
#[utoipa::path(
    get,
    path = "",
    tag = "roles",
    operation_id = "getApiAdminPlatformRoles",
    params(RolesQuery),
    responses(
        (status = 200, description = "List of roles", body = RoleListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_roles(
    State(state): State<RolesState>,
    _auth: Authenticated,
    Query(query): Query<RolesQuery>,
) -> Result<Json<RoleListResponse>, PlatformError> {
    let roles = if let Some(ref app) = query.application_code {
        state.role_repo.find_by_application(app).await?
    } else if let Some(ref source) = query.source {
        let s = parse_source(source)?;
        state.role_repo.find_by_source(s).await?
    } else if query.client_managed == Some(true) {
        state.role_repo.find_client_managed().await?
    } else {
        state.role_repo.find_all().await?
    };

    let roles: Vec<RoleResponse> = roles.into_iter()
        .map(|r| r.into())
        .collect();

    let total = roles.len();
    Ok(Json(RoleListResponse { roles, total }))
}

/// Update role
#[utoipa::path(
    put,
    path = "/{role_name}",
    tag = "roles",
    operation_id = "putApiAdminPlatformRolesByRoleName",
    params(
        ("role_name" = String, Path, description = "Role name (code) or ID")
    ),
    request_body = UpdateRoleRequest,
    responses(
        (status = 200, description = "Role updated", body = RoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_role(
    State(state): State<RolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    // Try by code first if it looks like a role code (contains ":")
    let mut role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    }.ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    // Check if role can be modified
    if !role.can_modify() {
        return Err(PlatformError::validation(format!(
            "Role {} is from source {} and cannot be modified",
            role.name, role.source.as_str()
        )));
    }

    if let Some(display_name) = req.display_name {
        role.display_name = display_name;
    }
    if let Some(desc) = req.description {
        role.description = Some(desc);
    }
    if let Some(client_managed) = req.client_managed {
        role.client_managed = client_managed;
    }

    role.updated_at = chrono::Utc::now();
    state.role_repo.update(&role).await?;

    Ok(Json(role.into()))
}

/// Grant permission to role
#[utoipa::path(
    post,
    path = "/{role_name}/permissions",
    tag = "roles",
    operation_id = "postApiAdminPlatformRolesByRoleNamePermissions",
    params(
        ("role_name" = String, Path, description = "Role name (code) or ID")
    ),
    request_body = GrantPermissionRequest,
    responses(
        (status = 200, description = "Permission granted", body = RoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn grant_permission(
    State(state): State<RolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
    Json(req): Json<GrantPermissionRequest>,
) -> Result<Json<RoleResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let mut role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    }.ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    if !role.can_modify() {
        return Err(PlatformError::validation("This role cannot be modified"));
    }

    role.grant_permission(req.permission);
    state.role_repo.update(&role).await?;

    Ok(Json(role.into()))
}

/// Revoke permission from role
#[utoipa::path(
    delete,
    path = "/{role_name}/permissions/{permission}",
    tag = "roles",
    operation_id = "deleteApiAdminPlatformRolesByRoleNamePermissionsByPermission",
    params(
        ("role_name" = String, Path, description = "Role name (code) or ID"),
        ("permission" = String, Path, description = "Permission to revoke")
    ),
    responses(
        (status = 200, description = "Permission revoked", body = RoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn revoke_permission(
    State(state): State<RolesState>,
    auth: Authenticated,
    Path((role_name, permission)): Path<(String, String)>,
) -> Result<Json<RoleResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let mut role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    }.ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    if !role.can_modify() {
        return Err(PlatformError::validation("This role cannot be modified"));
    }

    role.revoke_permission(&permission);
    state.role_repo.update(&role).await?;

    Ok(Json(role.into()))
}

/// Delete role
#[utoipa::path(
    delete,
    path = "/{role_name}",
    tag = "roles",
    operation_id = "deleteApiAdminPlatformRolesByRoleName",
    params(
        ("role_name" = String, Path, description = "Role name (code) or ID")
    ),
    responses(
        (status = 200, description = "Role deleted", body = SuccessResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_role(
    State(state): State<RolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    // Try by code first if it looks like a role code (contains ":")
    let role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    }.ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    if !role.can_modify() {
        return Err(PlatformError::validation("This role cannot be deleted"));
    }

    state.role_repo.delete(&role.id).await?;

    Ok(Json(SuccessResponse::ok()))
}

/// Get applications for role filter dropdown
#[utoipa::path(
    get,
    path = "/filters/applications",
    tag = "roles",
    operation_id = "getApiAdminPlatformRolesFiltersApplications",
    responses(
        (status = 200, description = "Application options", body = ApplicationOptionsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_applications(
    State(state): State<RolesState>,
    _auth: Authenticated,
) -> Result<Json<ApplicationOptionsResponse>, PlatformError> {
    let options = if let Some(ref app_repo) = state.application_repo {
        let apps = app_repo.find_active().await?;
        apps.into_iter()
            .map(|a| ApplicationOption {
                id: a.id,
                code: a.code,
                name: a.name,
            })
            .collect()
    } else {
        vec![]
    };

    Ok(Json(ApplicationOptionsResponse { options }))
}

/// List all permissions
#[utoipa::path(
    get,
    path = "/permissions",
    tag = "roles",
    operation_id = "getApiAdminPlatformRolesPermissions",
    responses(
        (status = 200, description = "List of permissions", body = PermissionListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_permissions(
    _auth: Authenticated,
) -> Result<Json<PermissionListResponse>, PlatformError> {
    // Return built-in platform permissions
    let permissions = get_builtin_permissions();
    let total = permissions.len();
    Ok(Json(PermissionListResponse { permissions, total }))
}

/// Get permission by string
#[utoipa::path(
    get,
    path = "/permissions/{permission}",
    tag = "roles",
    operation_id = "getApiAdminPlatformRolesPermissionsByPermission",
    params(
        ("permission" = String, Path, description = "Permission string")
    ),
    responses(
        (status = 200, description = "Permission found", body = PermissionResponse),
        (status = 404, description = "Permission not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_permission(
    _auth: Authenticated,
    Path(permission): Path<String>,
) -> Result<Json<PermissionResponse>, PlatformError> {
    let permissions = get_builtin_permissions();
    let found = permissions.into_iter()
        .find(|p| p.permission == permission)
        .ok_or_else(|| PlatformError::not_found("Permission", &permission))?;

    Ok(Json(found))
}

/// Get built-in platform permissions (matches Java PermissionRegistry)
fn get_builtin_permissions() -> Vec<PermissionResponse> {
    vec![
        // IAM Permissions
        perm("platform", "iam", "user", "view", "View users"),
        perm("platform", "iam", "user", "create", "Create users"),
        perm("platform", "iam", "user", "update", "Update users"),
        perm("platform", "iam", "user", "delete", "Delete users"),
        perm("platform", "iam", "role", "view", "View roles"),
        perm("platform", "iam", "role", "create", "Create roles"),
        perm("platform", "iam", "role", "update", "Update roles"),
        perm("platform", "iam", "role", "delete", "Delete roles"),
        perm("platform", "iam", "permission", "view", "View permissions"),
        perm("platform", "iam", "service-account", "view", "View service accounts"),
        perm("platform", "iam", "service-account", "create", "Create service accounts"),
        perm("platform", "iam", "service-account", "update", "Update service accounts"),
        perm("platform", "iam", "service-account", "delete", "Delete service accounts"),
        perm("platform", "iam", "idp", "manage", "Manage identity providers"),
        // Admin Permissions
        perm("platform", "admin", "client", "view", "View clients"),
        perm("platform", "admin", "client", "create", "Create clients"),
        perm("platform", "admin", "client", "update", "Update clients"),
        perm("platform", "admin", "client", "delete", "Delete clients"),
        perm("platform", "admin", "application", "view", "View applications"),
        perm("platform", "admin", "application", "create", "Create applications"),
        perm("platform", "admin", "application", "update", "Update applications"),
        perm("platform", "admin", "application", "delete", "Delete applications"),
        perm("platform", "admin", "config", "view", "View platform config"),
        perm("platform", "admin", "config", "update", "Update platform config"),
        // Messaging Permissions
        perm("platform", "messaging", "event", "view", "View events"),
        perm("platform", "messaging", "event", "view-raw", "View raw event data"),
        perm("platform", "messaging", "event-type", "view", "View event types"),
        perm("platform", "messaging", "event-type", "create", "Create event types"),
        perm("platform", "messaging", "event-type", "update", "Update event types"),
        perm("platform", "messaging", "event-type", "delete", "Delete event types"),
        perm("platform", "messaging", "subscription", "view", "View subscriptions"),
        perm("platform", "messaging", "subscription", "create", "Create subscriptions"),
        perm("platform", "messaging", "subscription", "update", "Update subscriptions"),
        perm("platform", "messaging", "subscription", "delete", "Delete subscriptions"),
        perm("platform", "messaging", "dispatch-job", "view", "View dispatch jobs"),
        perm("platform", "messaging", "dispatch-job", "view-raw", "View raw dispatch job data"),
        perm("platform", "messaging", "dispatch-job", "create", "Create dispatch jobs"),
        perm("platform", "messaging", "dispatch-job", "retry", "Retry dispatch jobs"),
        perm("platform", "messaging", "dispatch-pool", "view", "View dispatch pools"),
        perm("platform", "messaging", "dispatch-pool", "create", "Create dispatch pools"),
        perm("platform", "messaging", "dispatch-pool", "update", "Update dispatch pools"),
        perm("platform", "messaging", "dispatch-pool", "delete", "Delete dispatch pools"),
    ]
}

fn perm(app: &str, ctx: &str, agg: &str, action: &str, desc: &str) -> PermissionResponse {
    PermissionResponse {
        permission: format!("{}:{}:{}:{}", app, ctx, agg, action),
        application: app.to_string(),
        context: ctx.to_string(),
        aggregate: agg.to_string(),
        action: action.to_string(),
        description: desc.to_string(),
    }
}

/// Create roles router
pub fn roles_router(state: RolesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_role, list_roles))
        .routes(routes!(get_filter_applications))
        .routes(routes!(list_permissions))
        .routes(routes!(get_permission))
        .routes(routes!(get_role_by_code))
        .routes(routes!(get_role, update_role, delete_role))
        .routes(routes!(grant_permission))
        .routes(routes!(revoke_permission))
        .with_state(state)
}
