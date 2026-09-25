//! Per-role permission grants by path and the permission catalogue writes,
//! as Go serves them (`role/api/api.go:46-56`, `shared/bff/roles.go:51`).
//!
//! - `GET    /api/roles/{roleName}/permissions`              → `{permissions}`
//! - `POST   /api/roles/{roleName}/permissions`              (body `{permission}`)
//! - `POST   /api/roles/{roleName}/permissions/{permission}`
//! - `DELETE /api/roles/{roleName}/permissions/{permission}`
//! - `DELETE /api/roles/permissions/{permission}`            → 204, idempotent
//! - `POST   /bff/roles/permissions`                          → 201
//!
//! Grant and revoke use Go's dedicated operations (idempotent, every role
//! source, `platform:admin:role:permission-*` events). On top of Go's
//! permission check Rust keeps anchor reach (owner decision #25) and the
//! role ceiling (owner ruling 14).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::role::api::{GrantPermissionRequest, RoleResponse};
use crate::role::operations::{
    DefinePermissionCommand, DefinePermissionUseCase, DeletePermissionCommand,
    DeletePermissionUseCase, GrantPermissionCommand, GrantPermissionUseCase,
    RevokePermissionCommand, RevokePermissionUseCase,
};
use crate::role::permission_repository::PermissionCatalogRepository;
use crate::role::repository::RoleRepository;
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

#[derive(Clone)]
pub struct RolePermissionsState {
    pub role_repo: Arc<RoleRepository>,
    pub permission_repo: Arc<PermissionCatalogRepository>,
    pub grant_use_case: Arc<GrantPermissionUseCase<PgUnitOfWork>>,
    pub revoke_use_case: Arc<RevokePermissionUseCase<PgUnitOfWork>>,
    pub define_use_case: Arc<DefinePermissionUseCase<PgUnitOfWork>>,
    pub delete_use_case: Arc<DeletePermissionUseCase<PgUnitOfWork>>,
}

impl RolePermissionsState {
    pub fn new(
        pool: &sqlx::PgPool,
        role_repo: Arc<RoleRepository>,
        uow: Arc<PgUnitOfWork>,
    ) -> Self {
        let permission_repo = Arc::new(PermissionCatalogRepository::new(pool));
        Self {
            grant_use_case: Arc::new(GrantPermissionUseCase::new(role_repo.clone(), uow.clone())),
            revoke_use_case: Arc::new(RevokePermissionUseCase::new(role_repo.clone(), uow.clone())),
            define_use_case: Arc::new(DefinePermissionUseCase::new(
                permission_repo.clone(),
                uow.clone(),
            )),
            delete_use_case: Arc::new(DeletePermissionUseCase::new(permission_repo.clone(), uow)),
            role_repo,
            permission_repo,
        }
    }
}

/// Go `RolePermissionListResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RolePermissionListResponse {
    pub permissions: Vec<String>,
}

/// Go `bffCreatePermissionRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffCreatePermissionRequest {
    #[serde(default)]
    pub application: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub aggregate: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Go `bffPermissionResponse`.
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

async fn role_by_name(
    state: &RolePermissionsState,
    name: &str,
) -> Result<RoleResponse, PlatformError> {
    // Go's resolveRole: the id first, then the name.
    let role = match state.role_repo.find_by_id(name).await? {
        Some(r) => Some(r),
        None => state.role_repo.find_by_name(name).await?,
    };
    role.map(Into::into)
        .ok_or_else(|| PlatformError::not_found("Role", name))
}

/// The permissions granted to a role (Go `listRolePermissions`).
#[utoipa::path(
    get,
    path = "/api/roles/{roleName}/permissions",
    tag = "roles",
    operation_id = "listRolePermissions",
    params(("roleName" = String, Path, description = "Role name")),
    responses(
        (status = 200, description = "The role's permissions", body = RolePermissionListResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_role_permissions(
    State(state): State<RolePermissionsState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
) -> Result<Json<RolePermissionListResponse>, PlatformError> {
    checks::can_read_roles(&auth.0)?;
    let role = state
        .role_repo
        .find_by_name(&role_name)
        .await?
        .ok_or_else(|| PlatformError::not_found("Role", &role_name))?;
    let mut permissions: Vec<String> = role.permissions.into_iter().collect();
    permissions.sort();
    Ok(Json(RolePermissionListResponse { permissions }))
}

async fn grant(
    state: &RolePermissionsState,
    auth: &Authenticated,
    role_name: String,
    permission: String,
) -> Result<Json<RoleResponse>, PlatformError> {
    let already = state
        .role_repo
        .find_by_name(&role_name)
        .await?
        .is_some_and(|r| r.permissions.contains(&permission));
    if !already {
        // Owner ruling 14: only a permission the caller holds.
        crate::role::ceiling::require_permissions(Some(&auth.0), [permission.as_str()])?;
    }
    let cmd = GrantPermissionCommand {
        role_name: role_name.clone(),
        permission,
        cross_application: auth.0.has_permission(crate::permissions::ADMIN_ALL),
    };
    state
        .grant_use_case
        .run(cmd, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(Json(role_by_name(state, &role_name).await?))
}

/// Grant a permission named in the path (Go `grantRolePermission`).
#[utoipa::path(
    post,
    path = "/api/roles/{roleName}/permissions/{permission}",
    tag = "roles",
    operation_id = "grantRolePermission",
    params(
        ("roleName" = String, Path, description = "Role name"),
        ("permission" = String, Path, description = "Permission to grant")
    ),
    responses(
        (status = 200, description = "The updated role", body = RoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn grant_role_permission(
    State(state): State<RolePermissionsState>,
    auth: Authenticated,
    Path((role_name, permission)): Path<(String, String)>,
) -> Result<Json<RoleResponse>, PlatformError> {
    checks::can_administer_roles(&auth.0, crate::permissions::iam::ROLE_UPDATE)?;
    grant(&state, &auth, role_name, permission).await
}

/// Grant a permission named in the body (Go `grantRolePermissionByBody`,
/// the SDK shape).
#[utoipa::path(
    post,
    path = "/api/roles/{roleName}/permissions",
    tag = "roles",
    operation_id = "grantRolePermissionByBody",
    params(("roleName" = String, Path, description = "Role name")),
    request_body = GrantPermissionRequest,
    responses(
        (status = 200, description = "The updated role", body = RoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn grant_role_permission_by_body(
    State(state): State<RolePermissionsState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
    Json(req): Json<GrantPermissionRequest>,
) -> Result<Json<RoleResponse>, PlatformError> {
    checks::can_administer_roles(&auth.0, crate::permissions::iam::ROLE_UPDATE)?;
    grant(&state, &auth, role_name, req.permission).await
}

/// Revoke a permission (Go `revokeRolePermission`). Revoking an absent
/// permission is a no-op that still answers 200 with the role.
#[utoipa::path(
    delete,
    path = "/api/roles/{roleName}/permissions/{permission}",
    tag = "roles",
    operation_id = "revokeRolePermission",
    params(
        ("roleName" = String, Path, description = "Role name"),
        ("permission" = String, Path, description = "Permission to revoke")
    ),
    responses(
        (status = 200, description = "The updated role", body = RoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn revoke_role_permission(
    State(state): State<RolePermissionsState>,
    auth: Authenticated,
    Path((role_name, permission)): Path<(String, String)>,
) -> Result<Json<RoleResponse>, PlatformError> {
    checks::can_administer_roles(&auth.0, crate::permissions::iam::ROLE_UPDATE)?;
    let held = state
        .role_repo
        .find_by_name(&role_name)
        .await?
        .is_some_and(|r| r.permissions.contains(&permission));
    if held {
        // Owner ruling 14: removal counts too.
        crate::role::ceiling::require_permissions(Some(&auth.0), [permission.as_str()])?;
    }
    let cmd = RevokePermissionCommand {
        role_name: role_name.clone(),
        permission,
    };
    state
        .revoke_use_case
        .run(cmd, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(Json(role_by_name(&state, &role_name).await?))
}

/// Delete a permission from the catalogue (Go `deletePermission`): 204,
/// also for a code the catalogue does not hold.
#[utoipa::path(
    delete,
    path = "/api/roles/permissions/{permission}",
    tag = "roles",
    operation_id = "deletePermission",
    params(("permission" = String, Path, description = "Permission code")),
    responses((status = 204, description = "Deleted (or absent)")),
    security(("bearer_auth" = []))
)]
pub async fn delete_catalog_permission(
    State(state): State<RolePermissionsState>,
    auth: Authenticated,
    Path(permission): Path<String>,
) -> Result<StatusCode, PlatformError> {
    checks::can_administer_roles(&auth.0, crate::permissions::iam::ROLE_DELETE)?;
    if state
        .permission_repo
        .find_by_code(&permission)
        .await?
        .is_none()
    {
        return Ok(StatusCode::NO_CONTENT);
    }
    state
        .delete_use_case
        .run(
            DeletePermissionCommand { permission },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Define a catalogue permission from its four segments (Go BFF
/// `createPermission`, anchor-gated). Idempotent by code: 201 either way.
#[utoipa::path(
    post,
    path = "/bff/roles/permissions",
    tag = "bff-roles",
    operation_id = "bffCreatePermission",
    request_body = BffCreatePermissionRequest,
    responses(
        (status = 201, description = "Defined", body = BffPermissionResponse),
        (status = 400, description = "A segment is not a lowercase token")
    )
)]
pub async fn bff_create_permission(
    State(state): State<RolePermissionsState>,
    auth: Authenticated,
    Json(body): Json<BffCreatePermissionRequest>,
) -> Result<(StatusCode, Json<BffPermissionResponse>), PlatformError> {
    checks::require_anchor(&auth.0)?;
    let cmd = DefinePermissionCommand {
        application: body.application.trim().to_string(),
        context: body.context.trim().to_string(),
        aggregate: body.aggregate.trim().to_string(),
        action: body.action.trim().to_string(),
        description: body.description,
    };
    let event = state
        .define_use_case
        .run(cmd.clone(), ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok((
        StatusCode::CREATED,
        Json(BffPermissionResponse {
            permission: event.permission,
            application: cmd.application,
            context: cmd.context,
            aggregate: cmd.aggregate,
            action: cmd.action,
            description: event.description.unwrap_or_default(),
        }),
    ))
}

/// Full-path router; merged at the root.
pub fn role_permissions_router(state: RolePermissionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            list_role_permissions,
            grant_role_permission_by_body
        ))
        .routes(routes!(grant_role_permission, revoke_role_permission))
        .routes(routes!(delete_catalog_permission))
        .routes(routes!(bff_create_permission))
        .with_state(state)
}
