//! Processes API
//!
//! REST endpoints for process documentation. Process bodies are stored
//! verbatim (typically Mermaid source) and rendered client-side.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::process::entity::Process;
use crate::process::operations::{
    ArchiveProcessCommand, ArchiveProcessUseCase, CreateProcessCommand, CreateProcessUseCase,
    DeleteProcessCommand, DeleteProcessUseCase, UpdateProcessCommand, UpdateProcessUseCase,
};
use crate::process::repository::ProcessRepository;
use crate::shared::api_common::{CreatedResponse, PaginationParams};
use crate::shared::error::{NotFoundExt, PlatformError};
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UseCase};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateProcessRequest {
    /// Process code: {application}:{subdomain}:{process-name}
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Diagram body (typically Mermaid source).
    #[serde(default)]
    pub body: String,
    /// Defaults to `mermaid` if unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProcessRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub source: String,
    pub application: String,
    pub subdomain: String,
    pub process_name: String,
    pub body: String,
    pub diagram_type: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Process> for ProcessResponse {
    fn from(p: Process) -> Self {
        Self {
            id: p.id,
            code: p.code,
            name: p.name,
            description: p.description,
            status: p.status.as_str().to_string(),
            source: p.source.as_str().to_string(),
            application: p.application,
            subdomain: p.subdomain,
            process_name: p.process_name,
            body: p.body,
            diagram_type: p.diagram_type,
            tags: p.tags,
            created_at: p.created_at.to_rfc3339(),
            updated_at: p.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessListResponse {
    pub items: Vec<ProcessResponse>,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ProcessesQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    pub application: Option<String>,
    pub subdomain: Option<String>,
    pub status: Option<String>,
    pub search: Option<String>,
}

#[derive(Clone)]
pub struct ProcessesState {
    pub process_repo: Arc<ProcessRepository>,
    pub create_use_case: Arc<CreateProcessUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_use_case: Arc<UpdateProcessUseCase<crate::usecase::PgUnitOfWork>>,
    pub archive_use_case: Arc<ArchiveProcessUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_use_case: Arc<DeleteProcessUseCase<crate::usecase::PgUnitOfWork>>,
}

#[utoipa::path(
    post,
    path = "",
    tag = "processes",
    operation_id = "postApiProcesses",
    request_body = CreateProcessRequest,
    responses(
        (status = 201, description = "Process created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_process(
    State(state): State<ProcessesState>,
    auth: Authenticated,
    Json(req): Json<CreateProcessRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), PlatformError> {
    crate::shared::authorization_service::checks::can_create_processes(&auth.0)?;

    let cmd = CreateProcessCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        body: req.body,
        diagram_type: req.diagram_type,
        tags: req.tags,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse::new(event.process_id)),
    ))
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "processes",
    operation_id = "getApiProcessesById",
    params(("id" = String, Path, description = "Process ID")),
    responses(
        (status = 200, description = "Process found", body = ProcessResponse),
        (status = 404, description = "Process not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_process(
    State(state): State<ProcessesState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ProcessResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_processes(&auth.0)?;
    let process = state
        .process_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Process", &id)?;
    Ok(Json(process.into()))
}

#[utoipa::path(
    get,
    path = "/by-code/{code}",
    tag = "processes",
    operation_id = "getApiProcessesByCode",
    params(("code" = String, Path, description = "Process code")),
    responses(
        (status = 200, description = "Process found", body = ProcessResponse),
        (status = 404, description = "Process not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_process_by_code(
    State(state): State<ProcessesState>,
    auth: Authenticated,
    Path(code): Path<String>,
) -> Result<Json<ProcessResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_processes(&auth.0)?;
    let process = state
        .process_repo
        .find_by_code(&code)
        .await?
        .or_not_found("Process", &code)?;
    Ok(Json(process.into()))
}

#[utoipa::path(
    get,
    path = "",
    tag = "processes",
    operation_id = "getApiProcesses",
    params(ProcessesQuery),
    responses(
        (status = 200, description = "List of processes", body = ProcessListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_processes(
    State(state): State<ProcessesState>,
    auth: Authenticated,
    Query(query): Query<ProcessesQuery>,
) -> Result<Json<ProcessListResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_processes(&auth.0)?;

    // Default to CURRENT when no filters specified, matching event types.
    let default_status = if query.application.is_none()
        && query.subdomain.is_none()
        && query.status.is_none()
        && query.search.is_none()
    {
        Some("CURRENT".to_string())
    } else {
        query.status.clone()
    };

    let processes = state
        .process_repo
        .find_with_filters(
            query.application.as_deref(),
            query.subdomain.as_deref(),
            default_status.as_deref(),
            query.search.as_deref(),
        )
        .await?;

    let items: Vec<ProcessResponse> = processes.into_iter().map(|p| p.into()).collect();
    Ok(Json(ProcessListResponse { items }))
}

#[utoipa::path(
    put,
    path = "/{id}",
    tag = "processes",
    operation_id = "putApiProcessesById",
    params(("id" = String, Path, description = "Process ID")),
    request_body = UpdateProcessRequest,
    responses(
        (status = 204, description = "Process updated"),
        (status = 404, description = "Process not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_process(
    State(state): State<ProcessesState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateProcessRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_update_processes(&auth.0)?;

    // Ensure the process exists (gives a clean 404 before the use case runs).
    let _existing = state
        .process_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Process", &id)?;

    let cmd = UpdateProcessCommand {
        process_id: id,
        name: req.name,
        description: req.description,
        body: req.body,
        diagram_type: req.diagram_type,
        tags: req.tags,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/{id}/archive",
    tag = "processes",
    operation_id = "postApiProcessesByIdArchive",
    params(("id" = String, Path, description = "Process ID")),
    responses(
        (status = 204, description = "Process archived"),
        (status = 404, description = "Process not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn archive_process(
    State(state): State<ProcessesState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_write_processes(&auth.0)?;

    let _existing = state
        .process_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Process", &id)?;

    let cmd = ArchiveProcessCommand { process_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.archive_use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "processes",
    operation_id = "deleteApiProcessesById",
    params(("id" = String, Path, description = "Process ID")),
    responses(
        (status = 204, description = "Process deleted"),
        (status = 404, description = "Process not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_process(
    State(state): State<ProcessesState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_delete_processes(&auth.0)?;

    let _existing = state
        .process_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Process", &id)?;

    let cmd = DeleteProcessCommand { process_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn processes_router(state: ProcessesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_process, list_processes))
        .routes(routes!(get_process, update_process, delete_process))
        .routes(routes!(get_process_by_code))
        .routes(routes!(archive_process))
        .with_state(state)
}
