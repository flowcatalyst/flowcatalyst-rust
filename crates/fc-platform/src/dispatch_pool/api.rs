//! Dispatch Pools Admin API
//!
//! REST endpoints for dispatch pool management.

use axum::{
    routing::{get, post},
    extract::{State, Path, Query},
    Json, Router,
};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{DispatchPool, DispatchPoolStatus};
use crate::DispatchPoolRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::{PaginationParams, CreatedResponse, SuccessResponse};
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCaseResult};
use crate::dispatch_pool::operations::{
    CreateDispatchPoolCommand, CreateDispatchPoolUseCase,
    UpdateDispatchPoolCommand, UpdateDispatchPoolUseCase,
    ArchiveDispatchPoolCommand, ArchiveDispatchPoolUseCase,
    DeleteDispatchPoolCommand, DeleteDispatchPoolUseCase,
};

/// Create dispatch pool request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDispatchPoolRequest {
    /// Unique code (URL-safe)
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description
    pub description: Option<String>,

    /// Client ID (null for anchor-level)
    pub client_id: Option<String>,

    /// Rate limit (messages per minute)
    pub rate_limit: Option<u32>,

    /// Max concurrent dispatches
    pub concurrency: Option<u32>,
}

/// Update dispatch pool request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDispatchPoolRequest {
    /// Human-readable name
    pub name: Option<String>,

    /// Description
    pub description: Option<String>,

    /// Rate limit (messages per minute)
    pub rate_limit: Option<u32>,

    /// Max concurrent dispatches
    pub concurrency: Option<u32>,
}

/// Dispatch pool response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub client_id: Option<String>,
    pub status: String,
    pub rate_limit: Option<u32>,
    pub concurrency: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<DispatchPool> for DispatchPoolResponse {
    fn from(p: DispatchPool) -> Self {
        Self {
            id: p.id,
            code: p.code,
            name: p.name,
            description: p.description,
            client_id: p.client_id,
            status: format!("{:?}", p.status).to_uppercase(),
            rate_limit: p.rate_limit,
            concurrency: p.concurrency,
            created_at: p.created_at.to_rfc3339(),
            updated_at: p.updated_at.to_rfc3339(),
        }
    }
}

/// Query parameters for dispatch pools list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DispatchPoolsQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by client ID
    pub client_id: Option<String>,

    /// Filter by status
    pub status: Option<String>,
}

/// Dispatch pools service state
#[derive(Clone)]
pub struct DispatchPoolsState<U: UnitOfWork + 'static> {
    pub dispatch_pool_repo: Arc<DispatchPoolRepository>,
    pub create_use_case: Arc<CreateDispatchPoolUseCase<U>>,
    pub update_use_case: Arc<UpdateDispatchPoolUseCase<U>>,
    pub archive_use_case: Arc<ArchiveDispatchPoolUseCase<U>>,
    pub delete_use_case: Arc<DeleteDispatchPoolUseCase<U>>,
}

fn parse_status(s: &str) -> Option<DispatchPoolStatus> {
    match s.to_uppercase().as_str() {
        "ACTIVE" => Some(DispatchPoolStatus::Active),
        "ARCHIVED" => Some(DispatchPoolStatus::Archived),
        _ => None,
    }
}

/// Create a new dispatch pool
#[utoipa::path(
    post,
    path = "",
    tag = "dispatch-pools",
    request_body = CreateDispatchPoolRequest,
    responses(
        (status = 201, description = "Dispatch pool created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Json(req): Json<CreateDispatchPoolRequest>,
) -> Result<Json<CreatedResponse>, PlatformError> {
    // Check access - anchor or client admin
    if !auth.0.is_anchor() {
        if let Some(ref client_id) = req.client_id {
            if !auth.0.can_access_client(client_id) {
                return Err(PlatformError::forbidden("No access to this client"));
            }
        } else {
            return Err(PlatformError::forbidden("Client ID required for non-anchor users"));
        }
    }

    let command = CreateDispatchPoolCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        client_id: req.client_id,
        rate_limit: req.rate_limit,
        concurrency: req.concurrency,
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.create_use_case.execute(command, ctx).await {
        UseCaseResult::Success(event) => {
            Ok(Json(CreatedResponse::new(event.dispatch_pool_id)))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Get dispatch pool by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "dispatch-pools",
    params(
        ("id" = String, Path, description = "Dispatch pool ID")
    ),
    responses(
        (status = 200, description = "Dispatch pool found", body = DispatchPoolResponse),
        (status = 404, description = "Dispatch pool not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DispatchPoolResponse>, PlatformError> {
    let pool = state.dispatch_pool_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("DispatchPool", &id))?;

    // Check access
    if !auth.0.is_anchor() {
        if let Some(ref client_id) = pool.client_id {
            if !auth.0.can_access_client(client_id) {
                return Err(PlatformError::forbidden("No access to this dispatch pool"));
            }
        }
    }

    Ok(Json(pool.into()))
}

/// List dispatch pools
#[utoipa::path(
    get,
    path = "",
    tag = "dispatch-pools",
    params(DispatchPoolsQuery),
    responses(
        (status = 200, description = "List of dispatch pools", body = Vec<DispatchPoolResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_dispatch_pools<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Query(query): Query<DispatchPoolsQuery>,
) -> Result<Json<Vec<DispatchPoolResponse>>, PlatformError> {
    let pools = if let Some(ref client_id) = query.client_id {
        // Check access
        if !auth.0.is_anchor() && !auth.0.can_access_client(client_id) {
            return Err(PlatformError::forbidden("No access to this client"));
        }
        state.dispatch_pool_repo.find_by_client(Some(client_id.as_str())).await?
    } else {
        // Get active pools by default, or all pools accessible to user
        state.dispatch_pool_repo.find_active().await?
    };

    // Filter by status if specified
    let status_filter = query.status.as_deref().and_then(parse_status);

    // Filter by access for non-anchor users and by status
    let filtered: Vec<DispatchPoolResponse> = pools.into_iter()
        .filter(|p| {
            // Status filter
            if let Some(ref status) = status_filter {
                if p.status != *status {
                    return false;
                }
            }
            // Access filter
            if auth.0.is_anchor() {
                true
            } else if let Some(ref cid) = p.client_id {
                auth.0.can_access_client(cid)
            } else {
                // Anchor-level pools visible to all authenticated users
                true
            }
        })
        .map(|p| p.into())
        .collect();

    Ok(Json(filtered))
}

/// Update dispatch pool
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "dispatch-pools",
    params(
        ("id" = String, Path, description = "Dispatch pool ID")
    ),
    request_body = UpdateDispatchPoolRequest,
    responses(
        (status = 200, description = "Dispatch pool updated", body = DispatchPoolResponse),
        (status = 404, description = "Dispatch pool not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateDispatchPoolRequest>,
) -> Result<Json<DispatchPoolResponse>, PlatformError> {
    // Check access first
    let pool = state.dispatch_pool_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("DispatchPool", &id))?;

    if !auth.0.is_anchor() {
        if let Some(ref client_id) = pool.client_id {
            if !auth.0.can_access_client(client_id) {
                return Err(PlatformError::forbidden("No access to this dispatch pool"));
            }
        } else {
            return Err(PlatformError::forbidden("Cannot update anchor-level dispatch pool"));
        }
    }

    let command = UpdateDispatchPoolCommand {
        id: id.clone(),
        name: req.name,
        description: req.description,
        rate_limit: req.rate_limit,
        concurrency: req.concurrency,
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.update_use_case.execute(command, ctx).await {
        UseCaseResult::Success(_event) => {
            // Fetch updated pool for response
            let pool = state.dispatch_pool_repo.find_by_id(&id).await?
                .ok_or_else(|| PlatformError::not_found("DispatchPool", &id))?;
            Ok(Json(pool.into()))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Archive dispatch pool
#[utoipa::path(
    post,
    path = "/{id}/archive",
    tag = "dispatch-pools",
    params(
        ("id" = String, Path, description = "Dispatch pool ID")
    ),
    responses(
        (status = 200, description = "Dispatch pool archived", body = DispatchPoolResponse),
        (status = 404, description = "Dispatch pool not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn archive_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DispatchPoolResponse>, PlatformError> {
    // Check access first
    let pool = state.dispatch_pool_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("DispatchPool", &id))?;

    if !auth.0.is_anchor() {
        if let Some(ref client_id) = pool.client_id {
            if !auth.0.can_access_client(client_id) {
                return Err(PlatformError::forbidden("No access to this dispatch pool"));
            }
        } else {
            return Err(PlatformError::forbidden("Cannot archive anchor-level dispatch pool"));
        }
    }

    let command = ArchiveDispatchPoolCommand { id: id.clone() };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.archive_use_case.execute(command, ctx).await {
        UseCaseResult::Success(_event) => {
            let pool = state.dispatch_pool_repo.find_by_id(&id).await?
                .ok_or_else(|| PlatformError::not_found("DispatchPool", &id))?;
            Ok(Json(pool.into()))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Delete dispatch pool
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "dispatch-pools",
    params(
        ("id" = String, Path, description = "Dispatch pool ID")
    ),
    responses(
        (status = 200, description = "Dispatch pool deleted", body = SuccessResponse),
        (status = 404, description = "Dispatch pool not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let command = DeleteDispatchPoolCommand { id };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.delete_use_case.execute(command, ctx).await {
        UseCaseResult::Success(_event) => Ok(Json(SuccessResponse::ok())),
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Create dispatch pools router
pub fn dispatch_pools_router<U: UnitOfWork + Clone>(state: DispatchPoolsState<U>) -> Router {
    Router::new()
        .route("/", post(create_dispatch_pool::<U>).get(list_dispatch_pools::<U>))
        .route("/:id", get(get_dispatch_pool::<U>).put(update_dispatch_pool::<U>).delete(delete_dispatch_pool::<U>))
        .route("/:id/archive", post(archive_dispatch_pool::<U>))
        .with_state(state)
}
