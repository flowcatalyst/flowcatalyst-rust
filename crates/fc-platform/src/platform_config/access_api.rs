//! Config Access Admin API

use axum::{
    extract::{State, Path},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::access_entity::PlatformConfigAccess;
use super::access_repository::PlatformConfigAccessRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccessRequest {
    pub role_code: String,
    pub can_read: Option<bool>,
    pub can_write: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccessRequest {
    pub can_read: Option<bool>,
    pub can_write: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccessResponse {
    pub id: String,
    pub application_code: String,
    pub role_code: String,
    pub can_read: bool,
    pub can_write: bool,
    pub created_at: String,
}

impl From<PlatformConfigAccess> for AccessResponse {
    fn from(a: PlatformConfigAccess) -> Self {
        Self {
            id: a.id,
            application_code: a.application_code,
            role_code: a.role_code,
            can_read: a.can_read,
            can_write: a.can_write,
            created_at: a.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccessListResponse {
    pub items: Vec<AccessResponse>,
}

#[derive(Clone)]
pub struct ConfigAccessState {
    pub access_repo: Arc<PlatformConfigAccessRepository>,
}

/// List config access grants for an application
#[utoipa::path(
    get,
    path = "/{app_code}",
    tag = "config-access",
    operation_id = "getApiAdminConfigAccessByAppCode",
    params(
        ("app_code" = String, Path, description = "Application code")
    ),
    responses(
        (status = 200, description = "Access list", body = AccessListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_access(
    State(state): State<ConfigAccessState>,
    _auth: Authenticated,
    Path(app_code): Path<String>,
) -> Result<Json<AccessListResponse>, PlatformError> {
    let items = state.access_repo.find_by_application(&app_code).await?;
    Ok(Json(AccessListResponse {
        items: items.into_iter().map(|a| a.into()).collect(),
    }))
}

/// Create a config access grant
#[utoipa::path(
    post,
    path = "/{app_code}",
    tag = "config-access",
    operation_id = "postApiAdminConfigAccessByAppCode",
    params(
        ("app_code" = String, Path, description = "Application code")
    ),
    request_body = CreateAccessRequest,
    responses(
        (status = 201, description = "Access created", body = AccessResponse),
        (status = 409, description = "Access already exists")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_access(
    State(state): State<ConfigAccessState>,
    _auth: Authenticated,
    Path(app_code): Path<String>,
    Json(req): Json<CreateAccessRequest>,
) -> Result<(axum::http::StatusCode, Json<AccessResponse>), PlatformError> {
    if state.access_repo.find_by_application_and_role(&app_code, &req.role_code).await?.is_some() {
        return Err(PlatformError::conflict(format!("Access grant already exists for {}/{}", app_code, req.role_code)));
    }

    let mut access = PlatformConfigAccess::new(&app_code, &req.role_code);
    if let Some(cr) = req.can_read { access.can_read = cr; }
    if let Some(cw) = req.can_write { access.can_write = cw; }

    state.access_repo.insert(&access).await?;
    Ok((axum::http::StatusCode::CREATED, Json(access.into())))
}

/// Update a config access grant
#[utoipa::path(
    put,
    path = "/{app_code}/{role_code}",
    tag = "config-access",
    operation_id = "putApiAdminConfigAccessByAppCodeByRoleCode",
    params(
        ("app_code" = String, Path, description = "Application code"),
        ("role_code" = String, Path, description = "Role code")
    ),
    request_body = UpdateAccessRequest,
    responses(
        (status = 200, description = "Access updated", body = AccessResponse),
        (status = 404, description = "Access not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_access(
    State(state): State<ConfigAccessState>,
    _auth: Authenticated,
    Path((app_code, role_code)): Path<(String, String)>,
    Json(req): Json<UpdateAccessRequest>,
) -> Result<Json<AccessResponse>, PlatformError> {
    let mut access = state.access_repo.find_by_application_and_role(&app_code, &role_code).await?
        .ok_or_else(|| PlatformError::not_found("PlatformConfigAccess", &format!("{}/{}", app_code, role_code)))?;

    if let Some(cr) = req.can_read { access.can_read = cr; }
    if let Some(cw) = req.can_write { access.can_write = cw; }

    state.access_repo.update(&access).await?;
    Ok(Json(access.into()))
}

/// Delete a config access grant
#[utoipa::path(
    delete,
    path = "/{app_code}/{role_code}",
    tag = "config-access",
    operation_id = "deleteApiAdminConfigAccessByAppCodeByRoleCode",
    params(
        ("app_code" = String, Path, description = "Application code"),
        ("role_code" = String, Path, description = "Role code")
    ),
    responses(
        (status = 204, description = "Access deleted"),
        (status = 404, description = "Access not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_access(
    State(state): State<ConfigAccessState>,
    _auth: Authenticated,
    Path((app_code, role_code)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, PlatformError> {
    let deleted = state.access_repo.delete_by_application_and_role(&app_code, &role_code).await?;
    if deleted {
        Ok(axum::http::StatusCode::NO_CONTENT)
    } else {
        Err(PlatformError::not_found("PlatformConfigAccess", &format!("{}/{}", app_code, role_code)))
    }
}

pub fn config_access_router(state: ConfigAccessState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_access, create_access))
        .routes(routes!(update_access, delete_access))
        .with_state(state)
}
