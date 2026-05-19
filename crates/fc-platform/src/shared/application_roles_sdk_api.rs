//! Application Roles SDK API
//!
//! REST endpoints for applications to manage their own roles.
//! Used by application SDKs to sync role definitions.

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::role::operations::{
    CreateRoleCommand, CreateRoleUseCase, DeleteRoleCommand, DeleteRoleUseCase,
};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use crate::{ApplicationRepository, RoleRepository};
use crate::{AuthRole, RoleSource};

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
        let short_name = role
            .name
            .split(':')
            .nth(1)
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

/// Query parameters for listing roles
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRolesQuery {
    /// Filter by source (CODE, DATABASE, SDK)
    pub source: Option<String>,
}

/// Application Roles SDK state
#[derive(Clone)]
pub struct ApplicationRolesSdkState {
    pub application_repo: Arc<ApplicationRepository>,
    pub role_repo: Arc<RoleRepository>,
    pub create_use_case: Arc<CreateRoleUseCase<PgUnitOfWork>>,
    pub delete_use_case: Arc<DeleteRoleUseCase<PgUnitOfWork>>,
}

/// List all roles for an application
#[utoipa::path(
    get,
    path = "/{appCode}/roles",
    tag = "application-roles-sdk",
    params(
        ("appCode" = String, Path, description = "Application code"),
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
    state
        .application_repo
        .find_by_code(&app_code)
        .await?
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
    let role_dtos: Vec<RoleDto> = roles.into_iter().map(RoleDto::from_role).collect();

    Ok(Json(ListRolesResponse {
        roles: role_dtos,
        total,
    }))
}

/// Create a single role
#[utoipa::path(
    post,
    path = "/{appCode}/roles",
    tag = "application-roles-sdk",
    params(
        ("appCode" = String, Path, description = "Application code")
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
    auth: Authenticated,
    Path(app_code): Path<String>,
    Json(req): Json<CreateRoleRequest>,
) -> Result<Json<RoleDto>, PlatformError> {
    // Verify application exists (the use case doesn't load the app row,
    // so we keep this pre-check to surface a 404 cleanly).
    state
        .application_repo
        .find_by_code(&app_code)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &app_code))?;

    let display_name = req.display_name.clone().unwrap_or_else(|| req.name.clone());
    let cmd = CreateRoleCommand {
        application_code: app_code.clone(),
        role_name: req.name.clone(),
        display_name,
        description: req.description,
        permissions: req.permissions,
        client_managed: req.client_managed,
        source: RoleSource::Sdk,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.create_use_case.run(cmd, ctx).await.into_result()?;

    // Re-load the persisted role so the response carries the canonical state
    // (id + role_id + permissions normalised).
    let role_code = format!("{}:{}", app_code, req.name);
    let role = state
        .role_repo
        .find_by_name(&role_code)
        .await?
        .ok_or_else(|| PlatformError::internal("Role disappeared after create"))?;
    Ok(Json(RoleDto::from_role(role)))
}

/// Delete a role (SDK-sourced only)
#[utoipa::path(
    delete,
    path = "/{appCode}/roles/{roleName}",
    tag = "application-roles-sdk",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("roleName" = String, Path, description = "Role name (without app prefix)")
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
    auth: Authenticated,
    Path((app_code, role_name)): Path<(String, String)>,
) -> Result<(), PlatformError> {
    let role_code = format!("{}:{}", app_code, role_name);

    // Look up the role first so we can enforce SDK-only deletion as a 400
    // before invoking the use case (use case is source-agnostic).
    let role = state
        .role_repo
        .find_by_name(&role_code)
        .await?
        .ok_or_else(|| PlatformError::not_found("Role", &role_code))?;

    if role.source != RoleSource::Sdk {
        return Err(PlatformError::validation(
            "Cannot delete non-SDK role. Only SDK-sourced roles can be deleted via API.",
        ));
    }

    let cmd = DeleteRoleCommand {
        role_id: role.id.clone(),
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;

    Ok(())
}

/// Create application roles SDK router
pub fn application_roles_sdk_router(state: ApplicationRolesSdkState) -> Router {
    Router::new()
        .route("/{appCode}/roles", get(list_roles).post(create_role))
        .route("/{appCode}/roles/{roleName}", delete(delete_role))
        .with_state(state)
}
