//! Roles Admin API
//!
//! REST endpoints for role management.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::application::repository::ApplicationRepository;
use crate::role::entity::{AuthRole, RoleSource};
use crate::role::repository::RoleRepository;
use crate::shared::api_common::PaginationParams;
use crate::shared::error::PlatformError;
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

    /// Replace the role's permission set. Omit to leave permissions unchanged.
    pub permissions: Option<Vec<String>>,

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

/// A role as Go's `/api/roles` serves it (`RoleResponse`,
/// role/api/dto.go): `applicationId` and `description` absent when unset,
/// no `shortName` (that is the BFF shape), permissions sorted.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    pub name: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
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
        let mut permissions: Vec<String> = r.permissions.into_iter().collect();
        permissions.sort();
        Self {
            id: r.id,
            application_id: r.application_id,
            name: r.name,
            display_name: r.display_name,
            description: r.description,
            application_code: r.application_code,
            permissions,
            source: r.source.as_str().to_string(),
            client_managed: r.client_managed,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

/// Role list response
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
    pub application_repo: Arc<ApplicationRepository>,
    pub create_use_case:
        Arc<crate::role::operations::CreateRoleUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_use_case:
        Arc<crate::role::operations::UpdateRoleUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_use_case:
        Arc<crate::role::operations::DeleteRoleUseCase<crate::usecase::PgUnitOfWork>>,
    /// The permission catalogue (`iam_permissions`), which Go's
    /// `/api/roles/permissions` lists.
    pub permission_repo: Arc<crate::role::permission_repository::PermissionCatalogRepository>,
}

/// A permission catalogue row, Go's `PermissionResponse`
/// (role/api/dto.go, `permissionFromRow`): `name` is the code (the
/// catalogue has no display name), `category` the
/// `application:context:aggregate` triple.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    pub permission: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

impl From<crate::role::permission_catalog::CatalogPermission> for PermissionResponse {
    fn from(p: crate::role::permission_catalog::CatalogPermission) -> Self {
        Self {
            category: Some(format!("{}:{}:{}", p.subdomain, p.context, p.aggregate)),
            name: p.code.clone(),
            permission: p.code,
            description: p.description,
        }
    }
}

/// Permission list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionListResponse {
    pub permissions: Vec<PermissionResponse>,
    pub total: usize,
}

/// Go `ApplicationFilterListResponse`: the distinct application codes
/// roles use.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationFilterListResponse {
    pub application_codes: Vec<String>,
}

/// A role by id, falling back to its name (Go `resolveRole`,
/// role/api/api.go): the SPA addresses roles by id, the SDKs by name, on
/// the same routes.
async fn resolve_role(repo: &RoleRepository, id_or_name: &str) -> Result<AuthRole, PlatformError> {
    if let Some(role) = repo.find_by_id(id_or_name).await? {
        return Ok(role);
    }
    repo.find_by_name(id_or_name)
        .await?
        .ok_or_else(|| PlatformError::not_found("Role", id_or_name))
}

/// Create a new role
#[utoipa::path(
    post,
    path = "",
    tag = "roles",
    operation_id = "postApiRoles",
    request_body = CreateRoleRequest,
    responses(
        (status = 201, description = "Role created", body = crate::shared::api_common::CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate role code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_role(
    State(state): State<RolesState>,
    auth: Authenticated,
    Json(req): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<crate::shared::api_common::CreatedResponse>), PlatformError> {
    use crate::role::operations::CreateRoleCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::can_write_roles(&auth.0)?;
    // Owner ruling 14: only permissions the caller holds.
    crate::role::ceiling::require_permissions(
        Some(&auth.0),
        req.permissions.iter().map(String::as_str),
    )?;

    let cmd = CreateRoleCommand {
        application_code: req.application_code,
        role_name: req.role_name,
        display_name: req.display_name,
        description: req.description,
        permissions: req.permissions,
        client_managed: req.client_managed,
        source: crate::role::entity::RoleSource::Database,
        // Owner ruling 15: a super-admin may use another application's
        // permissions through the admin API.
        cross_application: auth.0.has_permission(crate::permissions::ADMIN_ALL),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;

    Ok((
        StatusCode::CREATED,
        Json(crate::shared::api_common::CreatedResponse::new(
            event.role_id,
        )),
    ))
}

/// Get a role by id, or by name (Go `getByID` with `resolveRole`).
#[utoipa::path(
    get,
    path = "/{roleName}",
    tag = "roles",
    operation_id = "getApiRolesByName",
    params(
        ("roleName" = String, Path, description = "Role id or name")
    ),
    responses(
        (status = 200, description = "Role found", body = RoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_role(
    State(state): State<RolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
) -> Result<Json<RoleResponse>, PlatformError> {
    crate::checks::can_read_roles(&auth.0)?;

    let role = resolve_role(&state.role_repo, &role_name).await?;
    Ok(Json(role.into()))
}

/// Get role by code (name)
#[utoipa::path(
    get,
    path = "/by-code/{code}",
    tag = "roles",
    operation_id = "getApiRolesByCodeByCode",
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
    auth: Authenticated,
    Path(code): Path<String>,
) -> Result<Json<RoleResponse>, PlatformError> {
    crate::checks::can_read_roles(&auth.0)?;

    let role = state
        .role_repo
        .find_by_name(&code)
        .await?
        .ok_or_else(|| PlatformError::not_found("Role", &code))?;

    Ok(Json(role.into()))
}

/// List roles
#[utoipa::path(
    get,
    path = "",
    tag = "roles",
    operation_id = "getApiRoles",
    params(RolesQuery),
    responses(
        (status = 200, description = "List of roles", body = RoleListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_roles(
    State(state): State<RolesState>,
    auth: Authenticated,
    Query(query): Query<RolesQuery>,
) -> Result<Json<RoleListResponse>, PlatformError> {
    crate::checks::can_read_roles(&auth.0)?;

    let source: Option<RoleSource> = crate::shared::enum_str::parse_opt(query.source.as_deref())?;

    let roles = state
        .role_repo
        .find_with_filters(
            query.application_code.as_deref(),
            source,
            query.client_managed,
        )
        .await?;

    let roles: Vec<RoleResponse> = roles.into_iter().map(|r| r.into()).collect();

    let total = roles.len();
    Ok(Json(RoleListResponse { roles, total }))
}

/// Update role
#[utoipa::path(
    put,
    path = "/{roleName}",
    tag = "roles",
    operation_id = "putApiRolesByName",
    params(
        ("roleName" = String, Path, description = "Role id or name")
    ),
    request_body = UpdateRoleRequest,
    responses(
        (status = 204, description = "Role updated"),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_role(
    State(state): State<RolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<StatusCode, PlatformError> {
    use crate::role::operations::UpdateRoleCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::can_write_roles(&auth.0)?;

    let role = resolve_role(&state.role_repo, &role_name).await?;
    // Owner ruling 14: only permissions the caller holds may be added or
    // removed.
    if let Some(ref permissions) = req.permissions {
        let before: Vec<String> = role.permissions.iter().cloned().collect();
        crate::role::ceiling::require_permissions(
            Some(&auth.0),
            crate::role::ceiling::changed(&before, permissions)
                .iter()
                .map(String::as_str),
        )?;
    }

    let cmd = UpdateRoleCommand {
        role_id: role.id,
        display_name: req.display_name,
        description: req.description,
        permissions: req.permissions,
        client_managed: req.client_managed,
        cross_application: auth.0.has_permission(crate::permissions::ADMIN_ALL),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Delete role
#[utoipa::path(
    delete,
    path = "/{roleName}",
    tag = "roles",
    operation_id = "deleteApiRolesByName",
    params(
        ("roleName" = String, Path, description = "Role id or name")
    ),
    responses(
        (status = 204, description = "Role deleted"),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_role(
    State(state): State<RolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
) -> Result<StatusCode, PlatformError> {
    use crate::role::operations::DeleteRoleCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::can_administer_roles(
        &auth.0,
        crate::permissions::iam::ROLE_DELETE,
    )?;

    let role = resolve_role(&state.role_repo, &role_name).await?;
    // Owner ruling 14: deleting a role withdraws every permission it holds.
    crate::role::ceiling::require_permissions(
        Some(&auth.0),
        role.permissions.iter().map(String::as_str),
    )?;

    let cmd = DeleteRoleCommand { role_id: role.id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// The distinct application codes roles use (Go `applicationFilters`).
#[utoipa::path(
    get,
    path = "/filters/applications",
    tag = "roles",
    operation_id = "getApiRolesFiltersApplications",
    responses(
        (status = 200, description = "Application codes", body = ApplicationFilterListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_applications(
    State(state): State<RolesState>,
    auth: Authenticated,
) -> Result<Json<ApplicationFilterListResponse>, PlatformError> {
    crate::checks::can_read_roles(&auth.0)?;

    let application_codes = state.role_repo.find_application_codes().await?;
    Ok(Json(ApplicationFilterListResponse { application_codes }))
}

/// The permission catalogue (`iam_permissions`), as Go's
/// `listPermissions`.
#[utoipa::path(
    get,
    path = "/permissions",
    tag = "roles",
    operation_id = "getApiRolesPermissions",
    responses(
        (status = 200, description = "List of permissions", body = PermissionListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_permissions(
    State(state): State<RolesState>,
    auth: Authenticated,
) -> Result<Json<PermissionListResponse>, PlatformError> {
    crate::checks::can_read_roles(&auth.0)?;

    let permissions: Vec<PermissionResponse> = state
        .permission_repo
        .find_all()
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let total = permissions.len();
    Ok(Json(PermissionListResponse { permissions, total }))
}

/// One permission catalogue row (Go `getPermission`).
#[utoipa::path(
    get,
    path = "/permissions/{permission}",
    tag = "roles",
    operation_id = "getApiRolesPermissionsByPermission",
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
    State(state): State<RolesState>,
    auth: Authenticated,
    Path(permission): Path<String>,
) -> Result<Json<PermissionResponse>, PlatformError> {
    crate::checks::can_read_roles(&auth.0)?;

    let found = state
        .permission_repo
        .find_by_code(&permission)
        .await?
        .ok_or_else(|| PlatformError::not_found("Permission", &permission))?;

    Ok(Json(found.into()))
}

/// Get roles by source (CODE, DATABASE, SDK)
#[utoipa::path(
    get,
    path = "/by-source/{source}",
    tag = "roles",
    operation_id = "getApiRolesBySourceBySource",
    params(
        ("source" = String, Path, description = "Role source (CODE, DATABASE, SDK)")
    ),
    responses(
        (status = 200, description = "Roles filtered by source", body = Vec<RoleResponse>),
        (status = 400, description = "Invalid source")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_roles_by_source(
    State(state): State<RolesState>,
    auth: Authenticated,
    Path(source): Path<String>,
) -> Result<Json<Vec<RoleResponse>>, PlatformError> {
    crate::checks::can_read_roles(&auth.0)?;

    // Go `role.ParseSource`: the exact upper-case names only.
    let source = match source.as_str() {
        "CODE" => RoleSource::Code,
        "DATABASE" => RoleSource::Database,
        "SDK" => RoleSource::Sdk,
        _ => {
            return Err(PlatformError::bad_request_code(
                "INVALID_SOURCE",
                "source must be CODE, DATABASE, or SDK",
            ))
        }
    };
    let roles = state.role_repo.find_by_source(source).await?;
    let response: Vec<RoleResponse> = roles.into_iter().map(|r| r.into()).collect();
    Ok(Json(response))
}

/// Get roles by application ID
#[utoipa::path(
    get,
    path = "/by-application/{applicationId}",
    tag = "roles",
    operation_id = "getApiRolesByApplicationByApplicationId",
    params(
        ("applicationId" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Roles filtered by application ID", body = Vec<RoleResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_roles_by_application_id(
    State(state): State<RolesState>,
    auth: Authenticated,
    Path(application_id): Path<String>,
) -> Result<Json<Vec<RoleResponse>>, PlatformError> {
    crate::checks::can_read_roles(&auth.0)?;

    let roles = state
        .role_repo
        .find_by_application_id(&application_id)
        .await?;
    let response: Vec<RoleResponse> = roles.into_iter().map(|r| r.into()).collect();
    Ok(Json(response))
}

/// Create roles router
pub fn roles_router(state: RolesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_role, list_roles))
        .routes(routes!(get_filter_applications))
        .routes(routes!(list_permissions))
        .routes(routes!(get_permission))
        .routes(routes!(get_role_by_code))
        .routes(routes!(get_roles_by_source))
        .routes(routes!(get_roles_by_application_id))
        .routes(routes!(get_role, update_role, delete_role))
        // Grant/revoke by role name: `role::permission_api` (Go's operations).
        .with_state(state)
}
