//! SDK Roles API — role management for external SDK access

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    Json, Router,
    routing::get,
};
use serde::Deserialize;

use crate::application::repository::ApplicationRepository;
use crate::role::api::{CreateRoleRequest, RoleListResponse, RoleResponse, UpdateRoleRequest};
use crate::role::entity::{AuthRole, RoleSource};
use crate::role::repository::RoleRepository;
use crate::shared::api_common::CreatedResponse;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Clone)]
pub struct SdkRolesState {
    pub role_repo: Arc<RoleRepository>,
    pub application_repo: Arc<ApplicationRepository>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SdkRolesQuery {
    pub application: Option<String>,
    pub source: Option<String>,
}

/// List SDK roles
#[utoipa::path(
    get,
    path = "",
    tag = "sdk-roles",
    operation_id = "getApiSdkRoles",
    params(
        ("application" = Option<String>, Query, description = "Filter by application code"),
        ("source" = Option<String>, Query, description = "Filter by role source"),
    ),
    responses(
        (status = 200, description = "List of roles", body = RoleListResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list_sdk_roles(
    _auth: Authenticated,
    State(state): State<SdkRolesState>,
    Query(query): Query<SdkRolesQuery>,
) -> Result<Json<RoleListResponse>, PlatformError> {
    let roles = if let Some(source) = &query.source {
        let source = RoleSource::from_str(source);
        state.role_repo.find_by_source(source).await?
    } else if let Some(application) = &query.application {
        state.role_repo.find_by_application(application).await?
    } else {
        state.role_repo.find_all().await?
    };

    let items: Vec<RoleResponse> = roles.into_iter().map(RoleResponse::from).collect();
    let total = items.len();
    Ok(Json(RoleListResponse { roles: items, total }))
}

/// Get SDK role by name
#[utoipa::path(
    get,
    path = "/{role_name}",
    tag = "sdk-roles",
    operation_id = "getApiSdkRolesByName",
    params(
        ("role_name" = String, Path, description = "Role name"),
    ),
    responses(
        (status = 200, description = "Role details", body = RoleResponse),
        (status = 404, description = "Role not found"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get_sdk_role(
    _auth: Authenticated,
    State(state): State<SdkRolesState>,
    Path(role_name): Path<String>,
) -> Result<Json<RoleResponse>, PlatformError> {
    let role = state
        .role_repo
        .find_by_name(&role_name)
        .await?
        .ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    Ok(Json(RoleResponse::from(role)))
}

/// Create SDK role
#[utoipa::path(
    post,
    path = "",
    tag = "sdk-roles",
    operation_id = "postApiSdkRoles",
    request_body = CreateRoleRequest,
    responses(
        (status = 201, description = "Role created", body = CreatedResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn create_sdk_role(
    _auth: Authenticated,
    State(state): State<SdkRolesState>,
    Json(body): Json<CreateRoleRequest>,
) -> Result<Json<CreatedResponse>, PlatformError> {
    let mut role = AuthRole::new(
        &body.application_code,
        &body.role_name,
        &body.display_name,
    );
    role.source = RoleSource::Sdk;
    if let Some(ref desc) = body.description {
        role.description = Some(desc.clone());
    }
    for perm in &body.permissions {
        role.permissions.insert(perm.clone());
    }
    role.client_managed = body.client_managed;

    let id = role.id.clone();
    state.role_repo.insert(&role).await?;

    Ok(Json(CreatedResponse::new(id)))
}

/// Update SDK role by name
#[utoipa::path(
    put,
    path = "/{role_name}",
    tag = "sdk-roles",
    operation_id = "putApiSdkRolesByName",
    params(
        ("role_name" = String, Path, description = "Role name"),
    ),
    request_body = UpdateRoleRequest,
    responses(
        (status = 200, description = "Role updated", body = RoleResponse),
        (status = 404, description = "Role not found"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn update_sdk_role(
    _auth: Authenticated,
    State(state): State<SdkRolesState>,
    Path(role_name): Path<String>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, PlatformError> {
    let mut role = state
        .role_repo
        .find_by_name(&role_name)
        .await?
        .ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    if let Some(display_name) = body.display_name {
        role.display_name = display_name;
    }
    if let Some(description) = body.description {
        role.description = Some(description);
    }
    if let Some(client_managed) = body.client_managed {
        role.client_managed = client_managed;
    }
    role.updated_at = chrono::Utc::now();

    state.role_repo.update(&role).await?;

    Ok(Json(RoleResponse::from(role)))
}

/// Delete SDK role by name
#[utoipa::path(
    delete,
    path = "/{role_name}",
    tag = "sdk-roles",
    operation_id = "deleteApiSdkRolesByName",
    params(
        ("role_name" = String, Path, description = "Role name"),
    ),
    responses(
        (status = 200, description = "Role deleted"),
        (status = 404, description = "Role not found"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn delete_sdk_role(
    _auth: Authenticated,
    State(state): State<SdkRolesState>,
    Path(role_name): Path<String>,
) -> Result<Json<serde_json::Value>, PlatformError> {
    let role = state
        .role_repo
        .find_by_name(&role_name)
        .await?
        .ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    state.role_repo.delete(&role.id).await?;

    Ok(Json(serde_json::json!({ "message": "Role deleted" })))
}

pub fn sdk_roles_router(state: SdkRolesState) -> Router {
    Router::new()
        .route("/", get(list_sdk_roles).post(create_sdk_role))
        .route(
            "/:role_name",
            get(get_sdk_role)
                .put(update_sdk_role)
                .delete(delete_sdk_role),
        )
        .with_state(state)
}
