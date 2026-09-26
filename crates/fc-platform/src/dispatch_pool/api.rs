//! Dispatch Pools Admin API
//!
//! REST endpoints for dispatch pool management.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::dispatch_pool::operations::{
    ArchiveDispatchPoolCommand, ArchiveDispatchPoolUseCase, CreateDispatchPoolCommand,
    CreateDispatchPoolUseCase, DeleteDispatchPoolCommand, DeleteDispatchPoolUseCase,
    UpdateDispatchPoolCommand, UpdateDispatchPoolUseCase,
};
use crate::shared::api_common::PaginationParams;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase};
use crate::DispatchPoolRepository;
use crate::{DispatchPool, DispatchPoolStatus};

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

    /// Rate limit (messages per minute; absent: none)
    pub rate_limit: Option<i32>,

    /// Max concurrent dispatches (default 10)
    pub concurrency: Option<i32>,
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
    pub rate_limit: Option<i32>,

    /// Max concurrent dispatches
    pub concurrency: Option<i32>,
}

/// Dispatch pool response DTO (Go `DispatchPoolResponse`: optional members
/// are omitted when unset).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<i32>,
    pub concurrency: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_identifier: Option<String>,
    pub status: String,
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
            rate_limit: p.rate_limit,
            concurrency: p.concurrency,
            client_id: p.client_id,
            client_identifier: p.client_identifier,
            status: p.status.as_str().to_string(),
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

/// Dispatch pools list response (matches TS `{ pools, total }` shape)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolListResponse {
    pub pools: Vec<DispatchPoolResponse>,
    pub total: u32,
}

/// Dispatch pools service state
#[derive(Clone)]
pub struct DispatchPoolsState<U: UnitOfWork + 'static> {
    pub dispatch_pool_repo: Arc<DispatchPoolRepository>,
    pub create_use_case: Arc<CreateDispatchPoolUseCase<U>>,
    pub update_use_case: Arc<UpdateDispatchPoolUseCase<U>>,
    pub archive_use_case: Arc<ArchiveDispatchPoolUseCase<U>>,
    pub delete_use_case: Arc<DeleteDispatchPoolUseCase<U>>,
    pub suspend_use_case: Arc<crate::dispatch_pool::operations::SuspendDispatchPoolUseCase<U>>,
    pub activate_use_case: Arc<crate::dispatch_pool::operations::ActivateDispatchPoolUseCase<U>>,
}

/// Create a new dispatch pool
#[utoipa::path(
    post,
    path = "",
    tag = "dispatch-pools",
    operation_id = "postApiDispatchPools",
    request_body = CreateDispatchPoolRequest,
    responses(
        (status = 201, description = "Dispatch pool created", body = crate::shared::api_common::CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Json(req): Json<CreateDispatchPoolRequest>,
) -> Result<(StatusCode, Json<crate::shared::api_common::CreatedResponse>), PlatformError> {
    // Go `CanWriteDispatchPools` (dispatchpool/api/api.go): a pool
    // permission first; the use case then validates (400) and checks the
    // caller's reach into the requested client (403 SCOPE_FORBIDDEN).
    crate::checks::can_write_dispatch_pools(&auth.0)?;

    let command = CreateDispatchPoolCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        client_id: req.client_id,
        rate_limit: req.rate_limit,
        concurrency: req.concurrency,
        caller: Some(auth.0.clone()),
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.create_use_case.run(command, ctx).await.into_result() {
        Ok(event) => Ok((
            StatusCode::CREATED,
            Json(crate::shared::api_common::CreatedResponse::new(
                event.pool_id,
            )),
        )),
        Err(err) => Err(err.into()),
    }
}

/// Get dispatch pool by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "dispatch-pools",
    operation_id = "getApiDispatchPoolsById",
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
    crate::checks::can_read_dispatch_pools(&auth.0)?;

    let pool = state
        .dispatch_pool_repo
        .find_by_id(&id)
        .await?
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
    operation_id = "getApiDispatchPools",
    params(DispatchPoolsQuery),
    responses(
        (status = 200, description = "List of dispatch pools", body = DispatchPoolListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_dispatch_pools<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Query(query): Query<DispatchPoolsQuery>,
) -> Result<Json<DispatchPoolListResponse>, PlatformError> {
    crate::checks::can_read_dispatch_pools(&auth.0)?;

    // Go: the filters as given (no status filter means every status), then
    // `FilterClientScoped`.
    let status_filter: Option<DispatchPoolStatus> =
        crate::shared::enum_str::parse_opt(query.status.as_deref())?;
    let pools = state
        .dispatch_pool_repo
        .find_with_filters(
            status_filter,
            query.client_id.as_deref().filter(|c| !c.is_empty()),
        )
        .await?;
    let filtered: Vec<DispatchPoolResponse> = pools
        .into_iter()
        .filter(|p| {
            p.client_id
                .as_deref()
                .is_none_or(|cid| crate::shared::caller_reach::reaches_client(&auth.0, cid))
        })
        .map(|p| p.into())
        .collect();

    let total = filtered.len() as u32;
    Ok(Json(DispatchPoolListResponse {
        pools: filtered,
        total,
    }))
}

/// Update dispatch pool
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "dispatch-pools",
    operation_id = "putApiDispatchPoolsById",
    params(
        ("id" = String, Path, description = "Dispatch pool ID")
    ),
    request_body = UpdateDispatchPoolRequest,
    responses(
        (status = 204, description = "Dispatch pool updated"),
        (status = 404, description = "Dispatch pool not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateDispatchPoolRequest>,
) -> Result<StatusCode, PlatformError> {
    // Go `CanWriteDispatchPools` (dispatchpool/api/api.go): a pool
    // permission first; client reach is checked below.
    crate::checks::can_write_dispatch_pools(&auth.0)?;

    // The use case validates, loads (404) and checks the caller's scope on
    // the pool (403 SCOPE_FORBIDDEN), in Go's order.
    let command = UpdateDispatchPoolCommand {
        id: id.clone(),
        name: req.name,
        description: req.description,
        rate_limit: req.rate_limit,
        concurrency: req.concurrency,
        caller: Some(auth.0.clone()),
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.update_use_case.run(command, ctx).await.into_result() {
        Ok(_event) => Ok(StatusCode::NO_CONTENT),
        Err(err) => Err(err.into()),
    }
}

/// Archive dispatch pool
#[utoipa::path(
    post,
    path = "/{id}/archive",
    tag = "dispatch-pools",
    operation_id = "postApiDispatchPoolsByIdArchive",
    params(
        ("id" = String, Path, description = "Dispatch pool ID")
    ),
    responses(
        (status = 204, description = "Dispatch pool archived"),
        (status = 404, description = "Dispatch pool not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn archive_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    // Go `CanWriteDispatchPools` (dispatchpool/api/api.go): a pool
    // permission first; client reach is checked below.
    crate::checks::can_write_dispatch_pools(&auth.0)?;
    // Check access first
    let pool = state
        .dispatch_pool_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("DispatchPool", &id))?;

    // Go `CheckScopeAccess`: a client's pool needs that client, a platform
    // pool anchor scope.
    crate::shared::caller_reach::require_scope_access(&auth.0, pool.client_id.as_deref())?;

    let command = ArchiveDispatchPoolCommand { id: id.clone() };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.archive_use_case.run(command, ctx).await.into_result() {
        // Unconditional, as Go: 204 however often it is sent.
        Ok(_event) => Ok(StatusCode::NO_CONTENT),
        Err(err) => Err(err.into()),
    }
}

/// Suspend dispatch pool
#[utoipa::path(
    post,
    path = "/{id}/suspend",
    tag = "dispatch-pools",
    operation_id = "postApiDispatchPoolsByIdSuspend",
    params(
        ("id" = String, Path, description = "Dispatch pool ID")
    ),
    responses(
        (status = 204, description = "Dispatch pool suspended"),
        (status = 404, description = "Dispatch pool not found"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn suspend_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    // Go `CanWriteDispatchPools` (dispatchpool/api/api.go): a pool
    // permission first; client reach is checked below.
    crate::checks::can_write_dispatch_pools(&auth.0)?;
    // Check access first
    let pool = state
        .dispatch_pool_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("DispatchPool", &id))?;

    // Go `CheckScopeAccess`: a client's pool needs that client, a platform
    // pool anchor scope.
    crate::shared::caller_reach::require_scope_access(&auth.0, pool.client_id.as_deref())?;

    // Go's SuspendDispatchPool: status SUSPENDED, event
    // platform:admin:dispatch-pool:suspended (it used to archive the pool).
    let command = crate::dispatch_pool::operations::SuspendDispatchPoolCommand { id: id.clone() };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.suspend_use_case.run(command, ctx).await.into_result() {
        // Unconditional, as Go: 204 however often it is sent.
        Ok(_event) => Ok(StatusCode::NO_CONTENT),
        Err(err) => Err(err.into()),
    }
}

/// Activate dispatch pool
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "dispatch-pools",
    operation_id = "postApiDispatchPoolsByIdActivate",
    params(
        ("id" = String, Path, description = "Dispatch pool ID")
    ),
    responses(
        (status = 204, description = "Dispatch pool activated"),
        (status = 404, description = "Dispatch pool not found"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn activate_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    // Go `CanWriteDispatchPools` (dispatchpool/api/api.go): a pool
    // permission first; client reach is checked below.
    crate::checks::can_write_dispatch_pools(&auth.0)?;
    // Check access first
    let pool = state
        .dispatch_pool_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("DispatchPool", &id))?;

    // Go `CheckScopeAccess`: a client's pool needs that client, a platform
    // pool anchor scope.
    crate::shared::caller_reach::require_scope_access(&auth.0, pool.client_id.as_deref())?;

    // Go's ActivateDispatchPool: status ACTIVE, event
    // platform:admin:dispatch-pool:activated (it used to change nothing).
    let command = crate::dispatch_pool::operations::ActivateDispatchPoolCommand { id: id.clone() };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state
        .activate_use_case
        .run(command, ctx)
        .await
        .into_result()
    {
        // Unconditional, as Go: 204 however often it is sent.
        Ok(_event) => Ok(StatusCode::NO_CONTENT),
        Err(err) => Err(err.into()),
    }
}

/// Delete dispatch pool
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "dispatch-pools",
    operation_id = "deleteApiDispatchPoolsById",
    params(
        ("id" = String, Path, description = "Dispatch pool ID")
    ),
    responses(
        (status = 204, description = "Dispatch pool deleted"),
        (status = 404, description = "Dispatch pool not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_dispatch_pool<U: UnitOfWork>(
    State(state): State<DispatchPoolsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_delete_dispatch_pools(&auth.0)?;

    // Go: 404 for a missing pool, then the caller's scope on it.
    let pool = state
        .dispatch_pool_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("DispatchPool", &id))?;
    crate::shared::caller_reach::require_scope_access(&auth.0, pool.client_id.as_deref())?;

    let command = DeleteDispatchPoolCommand { id };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.delete_use_case.run(command, ctx).await.into_result() {
        Ok(_event) => Ok(StatusCode::NO_CONTENT),
        Err(err) => Err(err.into()),
    }
}

/// Create dispatch pools router
pub fn dispatch_pools_router<U: UnitOfWork + Clone>(state: DispatchPoolsState<U>) -> Router {
    Router::new()
        .route(
            "/",
            post(create_dispatch_pool::<U>).get(list_dispatch_pools::<U>),
        )
        .route(
            "/{id}",
            get(get_dispatch_pool::<U>)
                .put(update_dispatch_pool::<U>)
                .delete(delete_dispatch_pool::<U>),
        )
        .route("/{id}/archive", post(archive_dispatch_pool::<U>))
        .route("/{id}/suspend", post(suspend_dispatch_pool::<U>))
        .route("/{id}/activate", post(activate_dispatch_pool::<U>))
        .with_state(state)
}
