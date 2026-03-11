//! BFF Roles API
//!
//! Backend-For-Frontend endpoints for role management.
//! Provides a UI-friendly view of roles at `/bff/roles`.

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::role::entity::{AuthRole, RoleSource};
use crate::role::repository::RoleRepository;
use crate::application::repository::ApplicationRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::{CreatedResponse, SuccessResponse};
use crate::shared::middleware::Authenticated;

// ── Response DTOs ──────────────────────────────────────────────────────────

/// BFF role response — UI-friendly view of a role
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffRoleResponse {
    pub id: String,
    /// Full name e.g. "myapp:admin"
    pub name: String,
    /// Short name e.g. "admin"
    pub short_name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub permissions: Vec<String>,
    pub application_code: String,
    /// "CODE", "DATABASE", "SDK"
    pub source: String,
    pub client_managed: bool,
    /// ISO8601
    pub created_at: String,
    /// ISO8601
    pub updated_at: String,
}

impl From<AuthRole> for BffRoleResponse {
    fn from(r: AuthRole) -> Self {
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

/// BFF role list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffRoleListResponse {
    pub roles: Vec<BffRoleResponse>,
    pub total: usize,
}

/// Application option for filter dropdown
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffApplicationOption {
    pub id: String,
    pub code: String,
    pub name: String,
}

/// Application options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffApplicationOptionsResponse {
    pub options: Vec<BffApplicationOption>,
}

/// Permission response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffPermissionResponse {
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
pub struct BffPermissionListResponse {
    pub permissions: Vec<BffPermissionResponse>,
    pub total: usize,
}

// ── Request DTOs ──────────────────────────────────────────────────────────

/// Create role request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffCreateRoleRequest {
    /// Application code this role belongs to
    pub application_code: String,
    /// Role name (will be combined with app code to form full name)
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
pub struct BffUpdateRoleRequest {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub client_managed: Option<bool>,
    pub permissions: Option<Vec<String>>,
}

// ── Query parameters ──────────────────────────────────────────────────────

/// Query parameters for BFF roles list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct BffRolesQuery {
    /// Filter by application code
    pub application: Option<String>,
    /// Filter by source (CODE, DATABASE, SDK)
    pub source: Option<String>,
}

// ── State ─────────────────────────────────────────────────────────────────

/// BFF roles service state
#[derive(Clone)]
pub struct BffRolesState {
    pub role_repo: Arc<RoleRepository>,
    pub application_repo: Option<Arc<ApplicationRepository>>,
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn parse_source(s: &str) -> Result<RoleSource, PlatformError> {
    match s.to_uppercase().as_str() {
        "CODE" => Ok(RoleSource::Code),
        "DATABASE" => Ok(RoleSource::Database),
        "SDK" => Ok(RoleSource::Sdk),
        _ => Err(PlatformError::validation(format!("Invalid source: {}", s))),
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────

/// List roles with optional filters
#[utoipa::path(
    get,
    path = "",
    tag = "bff-roles",
    operation_id = "getBffRoles",
    params(BffRolesQuery),
    responses(
        (status = 200, description = "List of roles", body = BffRoleListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_roles(
    State(state): State<BffRolesState>,
    _auth: Authenticated,
    Query(query): Query<BffRolesQuery>,
) -> Result<Json<BffRoleListResponse>, PlatformError> {
    let roles = match (&query.application, &query.source) {
        (Some(app), _) => state.role_repo.find_by_application(app).await?,
        (_, Some(source)) => {
            let s = parse_source(source)?;
            state.role_repo.find_by_source(s).await?
        }
        _ => state.role_repo.find_all().await?,
    };

    let roles: Vec<BffRoleResponse> = roles.into_iter().map(|r| r.into()).collect();
    let total = roles.len();
    Ok(Json(BffRoleListResponse { roles, total }))
}

/// Get applications for role filter dropdown
#[utoipa::path(
    get,
    path = "/filters/applications",
    tag = "bff-roles",
    operation_id = "getBffRolesFiltersApplications",
    responses(
        (status = 200, description = "Application options", body = BffApplicationOptionsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_applications(
    State(state): State<BffRolesState>,
    _auth: Authenticated,
) -> Result<Json<BffApplicationOptionsResponse>, PlatformError> {
    let options = if let Some(ref app_repo) = state.application_repo {
        let apps = app_repo.find_active().await?;
        apps.into_iter()
            .map(|a| BffApplicationOption {
                id: a.id,
                code: a.code,
                name: a.name,
            })
            .collect()
    } else {
        vec![]
    };

    Ok(Json(BffApplicationOptionsResponse { options }))
}

/// List all permissions
#[utoipa::path(
    get,
    path = "/permissions",
    tag = "bff-roles",
    operation_id = "getBffRolesPermissions",
    responses(
        (status = 200, description = "List of permissions", body = BffPermissionListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_permissions(
    _auth: Authenticated,
) -> Result<Json<BffPermissionListResponse>, PlatformError> {
    let permissions = get_builtin_permissions();
    let total = permissions.len();
    Ok(Json(BffPermissionListResponse { permissions, total }))
}

/// Get single permission by string
#[utoipa::path(
    get,
    path = "/permissions/{permission}",
    tag = "bff-roles",
    operation_id = "getBffRolesPermissionsByPermission",
    params(
        ("permission" = String, Path, description = "Permission string")
    ),
    responses(
        (status = 200, description = "Permission found", body = BffPermissionResponse),
        (status = 404, description = "Permission not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_permission(
    _auth: Authenticated,
    Path(permission): Path<String>,
) -> Result<Json<BffPermissionResponse>, PlatformError> {
    let permissions = get_builtin_permissions();
    let found = permissions
        .into_iter()
        .find(|p| p.permission == permission)
        .ok_or_else(|| PlatformError::not_found("Permission", &permission))?;

    Ok(Json(found))
}

/// Get role by name (code)
#[utoipa::path(
    get,
    path = "/{role_name}",
    tag = "bff-roles",
    operation_id = "getBffRolesByName",
    params(
        ("role_name" = String, Path, description = "Role name (code) or ID")
    ),
    responses(
        (status = 200, description = "Role found", body = BffRoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_role(
    State(state): State<BffRolesState>,
    _auth: Authenticated,
    Path(role_name): Path<String>,
) -> Result<Json<BffRoleResponse>, PlatformError> {
    let role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    };

    let role = role.ok_or_else(|| PlatformError::not_found("Role", &role_name))?;
    Ok(Json(role.into()))
}

/// Create a new role
#[utoipa::path(
    post,
    path = "",
    tag = "bff-roles",
    operation_id = "postBffRoles",
    request_body = BffCreateRoleRequest,
    responses(
        (status = 201, description = "Role created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate role code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_role(
    State(state): State<BffRolesState>,
    auth: Authenticated,
    Json(req): Json<BffCreateRoleRequest>,
) -> Result<(axum::http::StatusCode, Json<CreatedResponse>), PlatformError> {
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

    Ok((axum::http::StatusCode::CREATED, Json(CreatedResponse::new(id))))
}

/// Update role
#[utoipa::path(
    put,
    path = "/{role_name}",
    tag = "bff-roles",
    operation_id = "putBffRolesByName",
    params(
        ("role_name" = String, Path, description = "Role name (code) or ID")
    ),
    request_body = BffUpdateRoleRequest,
    responses(
        (status = 200, description = "Role updated", body = BffRoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_role(
    State(state): State<BffRolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
    Json(req): Json<BffUpdateRoleRequest>,
) -> Result<Json<BffRoleResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let mut role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    }
    .ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    if !role.can_modify() {
        return Err(PlatformError::validation(format!(
            "Role {} is from source {} and cannot be modified",
            role.name,
            role.source.as_str()
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
    if let Some(permissions) = req.permissions {
        role.permissions = permissions.into_iter().collect();
    }

    role.updated_at = chrono::Utc::now();
    state.role_repo.update(&role).await?;

    Ok(Json(role.into()))
}

/// Delete role
#[utoipa::path(
    delete,
    path = "/{role_name}",
    tag = "bff-roles",
    operation_id = "deleteBffRolesByName",
    params(
        ("role_name" = String, Path, description = "Role name (code) or ID")
    ),
    responses(
        (status = 204, description = "Role deleted"),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_role(
    State(state): State<BffRolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
) -> Result<axum::http::StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    }
    .ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    if !role.can_modify() {
        return Err(PlatformError::validation("This role cannot be deleted"));
    }

    state.role_repo.delete(&role.id).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── Permissions registry ──────────────────────────────────────────────────

fn get_builtin_permissions() -> Vec<BffPermissionResponse> {
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

fn perm(app: &str, ctx: &str, agg: &str, action: &str, desc: &str) -> BffPermissionResponse {
    BffPermissionResponse {
        permission: format!("{}:{}:{}:{}", app, ctx, agg, action),
        application: app.to_string(),
        context: ctx.to_string(),
        aggregate: agg.to_string(),
        action: action.to_string(),
        description: desc.to_string(),
    }
}

// ── Router ────────────────────────────────────────────────────────────────

/// Create BFF roles router (mounted at `/bff/roles`)
pub fn bff_roles_router(state: BffRolesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_role, list_roles))
        .routes(routes!(get_filter_applications))
        .routes(routes!(list_permissions))
        .routes(routes!(get_permission))
        .routes(routes!(get_role, update_role, delete_role))
        .with_state(state)
}
