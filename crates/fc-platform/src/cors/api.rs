//! CORS Admin API

use axum::{
    routing::{get, post, delete as axum_delete},
    extract::{State, Path},
    Json, Router,
};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::entity::CorsAllowedOrigin;
use super::repository::CorsOriginRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCorsOriginRequest {
    pub origin: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CorsOriginResponse {
    pub id: String,
    pub origin: String,
    pub description: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<CorsAllowedOrigin> for CorsOriginResponse {
    fn from(c: CorsAllowedOrigin) -> Self {
        Self {
            id: c.id,
            origin: c.origin,
            description: c.description,
            created_by: c.created_by,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CorsOriginsListResponse {
    pub cors_origins: Vec<CorsOriginResponse>,
    pub total: usize,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AllowedOriginsResponse {
    pub origins: Vec<String>,
}

#[derive(Clone)]
pub struct CorsState {
    pub cors_repo: Arc<CorsOriginRepository>,
}

/// Create a new CORS allowed origin
#[utoipa::path(
    post,
    path = "",
    tag = "cors",
    operation_id = "postApiAdminPlatformCors",
    request_body = CreateCorsOriginRequest,
    responses(
        (status = 201, description = "CORS origin created", body = CorsOriginResponse),
        (status = 409, description = "Duplicate origin")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_cors_origin(
    State(state): State<CorsState>,
    auth: Authenticated,
    Json(req): Json<CreateCorsOriginRequest>,
) -> Result<(axum::http::StatusCode, Json<CorsOriginResponse>), PlatformError> {
    if state.cors_repo.find_by_origin(&req.origin).await?.is_some() {
        return Err(PlatformError::duplicate("CorsAllowedOrigin", "origin", &req.origin));
    }

    let origin = CorsAllowedOrigin::new(
        req.origin,
        req.description,
        Some(auth.0.principal_id.clone()),
    );
    state.cors_repo.insert(&origin).await?;
    Ok((axum::http::StatusCode::CREATED, Json(origin.into())))
}

/// List all CORS allowed origins
#[utoipa::path(
    get,
    path = "",
    tag = "cors",
    operation_id = "getApiAdminPlatformCors",
    responses(
        (status = 200, description = "List of CORS origins", body = CorsOriginsListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_cors_origins(
    State(state): State<CorsState>,
    _auth: Authenticated,
) -> Result<Json<CorsOriginsListResponse>, PlatformError> {
    let origins = state.cors_repo.find_all().await?;
    let total = origins.len();
    Ok(Json(CorsOriginsListResponse {
        cors_origins: origins.into_iter().map(|o| o.into()).collect(),
        total,
    }))
}

/// Get list of allowed origin strings
#[utoipa::path(
    get,
    path = "/allowed",
    tag = "cors",
    operation_id = "getApiAdminPlatformCorsAllowed",
    responses(
        (status = 200, description = "Allowed origins list", body = AllowedOriginsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_allowed_origins(
    State(state): State<CorsState>,
    _auth: Authenticated,
) -> Result<Json<AllowedOriginsResponse>, PlatformError> {
    let origins = state.cors_repo.get_allowed_origins().await?;
    Ok(Json(AllowedOriginsResponse { origins }))
}

/// Get a CORS origin by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "cors",
    operation_id = "getApiAdminPlatformCorsById",
    params(
        ("id" = String, Path, description = "CORS origin ID")
    ),
    responses(
        (status = 200, description = "CORS origin found", body = CorsOriginResponse),
        (status = 404, description = "CORS origin not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_cors_origin(
    State(state): State<CorsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<CorsOriginResponse>, PlatformError> {
    let origin = state.cors_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("CorsAllowedOrigin", &id))?;
    Ok(Json(origin.into()))
}

/// Delete a CORS origin by ID
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "cors",
    operation_id = "deleteApiAdminPlatformCorsById",
    params(
        ("id" = String, Path, description = "CORS origin ID")
    ),
    responses(
        (status = 204, description = "CORS origin deleted"),
        (status = 404, description = "CORS origin not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_cors_origin(
    State(state): State<CorsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, PlatformError> {
    let _ = state.cors_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("CorsAllowedOrigin", &id))?;
    state.cors_repo.delete(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub fn cors_router(state: CorsState) -> Router {
    Router::new()
        .route("/", post(create_cors_origin).get(list_cors_origins))
        .route("/allowed", get(get_allowed_origins))
        .route("/:id", get(get_cors_origin).delete(delete_cors_origin))
        .with_state(state)
}
