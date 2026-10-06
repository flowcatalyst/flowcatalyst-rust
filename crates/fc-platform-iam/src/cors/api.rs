//! CORS Admin API

use axum::{
    extract::{Path, State},
    Json,
};
use fc_platform_core::shared::id::CorsOriginId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use super::entity::CorsAllowedOrigin;
use super::repository::CorsOriginRepository;
use crate::cors::operations::AddCorsOriginUseCase;
use crate::cors::operations::DeleteCorsOriginUseCase;
use axum::http::StatusCode;
use fc_platform_core::shared::api_common::CreatedResponse;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::usecase::PgUnitOfWork;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = AddOriginRequest)]
pub struct CreateCorsOriginRequest {
    pub origin: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = AllowedOriginResponse)]
pub struct CorsOriginResponse {
    pub id: String,
    pub origin: String,
    // Go `AllowedOriginResponse`: absent when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl From<CorsAllowedOrigin> for CorsOriginResponse {
    fn from(c: CorsAllowedOrigin) -> Self {
        Self {
            id: c.id.into_string(),
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
#[schema(as = CorsOriginListResponse)]
pub struct CorsOriginsListResponse {
    pub cors_origins: Vec<CorsOriginResponse>,
    #[schema(value_type = i64)]
    pub total: usize,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = PublicAllowedResponse)]
pub struct AllowedOriginsResponse {
    pub origins: Vec<String>,
}

#[derive(Clone)]
pub struct CorsState {
    pub cors_repo: Arc<CorsOriginRepository>,
    pub add_use_case: Arc<AddCorsOriginUseCase<PgUnitOfWork>>,
    pub delete_use_case: Arc<DeleteCorsOriginUseCase<PgUnitOfWork>>,
}

/// Create a new CORS allowed origin
#[utoipa::path(
    post,
    path = "",
    tag = "cors-origins",
    operation_id = "addCorsOrigin",
    request_body = CreateCorsOriginRequest,
    responses(
        (status = 201, description = "CORS origin created", body = CreatedResponse),
        (status = 409, description = "Duplicate origin")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_cors_origin(
    State(state): State<CorsState>,
    auth: Authenticated,
    Json(req): Json<CreateCorsOriginRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), PlatformError> {
    use crate::cors::operations::AddCorsOriginCommand;
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    checks::can_create_cors_origins(&auth.0)?;

    let cmd = AddCorsOriginCommand {
        origin: req.origin,
        description: req.description,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    let event = state.add_use_case.run(cmd, ctx).await.into_result()?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse::new(event.origin_id)),
    ))
}

/// List all CORS allowed origins
#[utoipa::path(
    get,
    path = "",
    tag = "cors-origins",
    operation_id = "listCorsOrigins",
    responses(
        (status = 200, description = "List of CORS origins", body = CorsOriginsListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_cors_origins(
    State(state): State<CorsState>,
    auth: Authenticated,
) -> Result<Json<CorsOriginsListResponse>, PlatformError> {
    checks::can_read_cors_origins(&auth.0)?;

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
    tag = "cors-origins",
    operation_id = "publicAllowedOrigins",
    responses(
        (status = 200, description = "Allowed origins list", body = AllowedOriginsResponse)
    )
)]
/// Public, as in Go: the allowed origins are what a browser is told anyway,
/// and the handler consults no principal.
pub async fn get_allowed_origins(
    State(state): State<CorsState>,
) -> Result<Json<AllowedOriginsResponse>, PlatformError> {
    let origins = state.cors_repo.get_allowed_origins().await?;
    Ok(Json(AllowedOriginsResponse { origins }))
}

/// Get a CORS origin by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "cors-origins",
    operation_id = "getCorsOrigin",
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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<CorsOriginResponse>, PlatformError> {
    let id = CorsOriginId::from_wire(id);
    checks::can_read_cors_origins(&auth.0)?;

    let origin = state
        .cors_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("CorsAllowedOrigin", &id))?;
    Ok(Json(origin.into()))
}

/// Delete a CORS origin by ID
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "cors-origins",
    operation_id = "deleteCorsOrigin",
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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    let id = CorsOriginId::from_wire(id);
    use crate::cors::operations::DeleteCorsOriginCommand;
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    checks::can_delete_cors_origins(&auth.0)?;

    let cmd = DeleteCorsOriginCommand { origin_id: id };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}
