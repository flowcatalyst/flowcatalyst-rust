//! Scheduled Job HTTP API.
//!
//! Routes mounted at `/api/scheduled-jobs`. Two distinct caller groups share
//! the namespace:
//!
//!   * Admin / control-plane: CRUD, status transitions, manual fire, history
//!     reads. Permissioned via `can_*_scheduled_jobs` and resource-level
//!     client-access checks.
//!   * SDK callback: `/instances/:id/log` and `/instances/:id/complete`.
//!     Permissioned via `application_service::SCHEDULED_JOB_INSTANCE_WRITE`
//!     and bound to the instance's `client_id`. These bypass the use-case
//!     layer (see CLAUDE.md infrastructure-processing exemption).

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::scheduled_job::entity::{
    CompletionStatus, InstanceStatus, LogLevel, ScheduledJobStatus, TriggerKind,
};
use crate::scheduled_job::operations::{
    ArchiveScheduledJobCommand, ArchiveScheduledJobUseCase, CreateScheduledJobCommand,
    CreateScheduledJobUseCase, DeleteScheduledJobCommand, DeleteScheduledJobUseCase,
    FireScheduledJobCommand, FireScheduledJobUseCase, PauseScheduledJobCommand,
    PauseScheduledJobUseCase, ResumeScheduledJobCommand, ResumeScheduledJobUseCase,
    UpdateScheduledJobCommand, UpdateScheduledJobUseCase,
};
use crate::scheduled_job::{
    InstanceListFilters, ScheduledJob, ScheduledJobInstance, ScheduledJobInstanceLog,
    ScheduledJobInstanceRepository, ScheduledJobRepository,
};
use crate::shared::api_common::{CreatedResponse, PaginatedResponse, PaginationParams};
use crate::shared::error::{NotFoundExt, PlatformError};
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ScheduledJobsState {
    pub repo: Arc<ScheduledJobRepository>,
    pub instance_repo: Arc<ScheduledJobInstanceRepository>,
    pub create_use_case: Arc<CreateScheduledJobUseCase<PgUnitOfWork>>,
    pub update_use_case: Arc<UpdateScheduledJobUseCase<PgUnitOfWork>>,
    pub pause_use_case: Arc<PauseScheduledJobUseCase<PgUnitOfWork>>,
    pub resume_use_case: Arc<ResumeScheduledJobUseCase<PgUnitOfWork>>,
    pub archive_use_case: Arc<ArchiveScheduledJobUseCase<PgUnitOfWork>>,
    pub delete_use_case: Arc<DeleteScheduledJobUseCase<PgUnitOfWork>>,
    pub fire_use_case: Arc<FireScheduledJobUseCase<PgUnitOfWork>>,
}

// ── Request DTOs ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduledJobRequest {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// None = platform-scoped (anchor only); Some = client-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub crons: Vec<String>,
    #[serde(default = "default_tz")]
    pub timezone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub concurrent: bool,
    #[serde(default)]
    pub tracks_completion: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    #[serde(default = "default_attempts")]
    pub delivery_max_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}
fn default_tz() -> String {
    "UTC".into()
}
fn default_attempts() -> i32 {
    3
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduledJobRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crons: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracks_completion: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_max_attempts: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FireRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLogRequest {
    pub message: String,
    #[serde(default)]
    pub level: LogLevelDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevelDto {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl From<LogLevelDto> for LogLevel {
    fn from(v: LogLevelDto) -> Self {
        match v {
            LogLevelDto::Debug => LogLevel::Debug,
            LogLevelDto::Info => LogLevel::Info,
            LogLevelDto::Warn => LogLevel::Warn,
            LogLevelDto::Error => LogLevel::Error,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceCompleteRequest {
    pub status: CompletionStatusDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum CompletionStatusDto {
    Success,
    Failure,
}

impl From<CompletionStatusDto> for CompletionStatus {
    fn from(v: CompletionStatusDto) -> Self {
        match v {
            CompletionStatusDto::Success => CompletionStatus::Success,
            CompletionStatusDto::Failure => CompletionStatus::Failure,
        }
    }
}

// ── Query parameters ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListJobsQuery {
    /// Filter by client. Pass the literal `platform` to filter platform-scoped.
    pub client_id: Option<String>,
    pub status: Option<String>,
    pub search: Option<String>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListInstancesQuery {
    pub status: Option<String>,
    pub trigger_kind: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ByCodeQuery {
    pub client_id: Option<String>,
}

// ── Response DTOs ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobResponse {
    pub id: String,
    pub client_id: Option<String>,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub crons: Vec<String>,
    pub timezone: String,
    pub payload: Option<serde_json::Value>,
    pub concurrent: bool,
    pub tracks_completion: bool,
    pub timeout_seconds: Option<i32>,
    pub delivery_max_attempts: i32,
    pub target_url: Option<String>,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub version: i32,
    /// Computed: true if any non-terminal instance currently exists.
    pub has_active_instance: bool,
}

impl ScheduledJobResponse {
    fn from(job: ScheduledJob, has_active_instance: bool) -> Self {
        Self {
            id: job.id,
            client_id: job.client_id,
            code: job.code,
            name: job.name,
            description: job.description,
            status: job.status.as_str().into(),
            crons: job.crons,
            timezone: job.timezone,
            payload: job.payload,
            concurrent: job.concurrent,
            tracks_completion: job.tracks_completion,
            timeout_seconds: job.timeout_seconds,
            delivery_max_attempts: job.delivery_max_attempts,
            target_url: job.target_url,
            last_fired_at: job.last_fired_at,
            created_at: job.created_at,
            updated_at: job.updated_at,
            created_by: job.created_by,
            updated_by: job.updated_by,
            version: job.version,
            has_active_instance,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobInstanceResponse {
    pub id: String,
    pub scheduled_job_id: String,
    pub client_id: Option<String>,
    pub job_code: String,
    pub trigger_kind: String,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub fired_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub delivery_attempts: i32,
    pub delivery_error: Option<String>,
    pub completion_status: Option<String>,
    pub completion_result: Option<serde_json::Value>,
    pub correlation_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<ScheduledJobInstance> for ScheduledJobInstanceResponse {
    fn from(i: ScheduledJobInstance) -> Self {
        Self {
            id: i.id,
            scheduled_job_id: i.scheduled_job_id,
            client_id: i.client_id,
            job_code: i.job_code,
            trigger_kind: i.trigger_kind.as_str().into(),
            scheduled_for: i.scheduled_for,
            fired_at: i.fired_at,
            delivered_at: i.delivered_at,
            completed_at: i.completed_at,
            status: i.status.as_str().into(),
            delivery_attempts: i.delivery_attempts,
            delivery_error: i.delivery_error,
            completion_status: i.completion_status.map(|c| c.as_str().into()),
            completion_result: i.completion_result,
            correlation_id: i.correlation_id,
            created_at: i.created_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLogResponse {
    pub id: String,
    pub instance_id: String,
    pub level: String,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl From<ScheduledJobInstanceLog> for InstanceLogResponse {
    fn from(l: ScheduledJobInstanceLog) -> Self {
        Self {
            id: l.id,
            instance_id: l.instance_id,
            level: l.level.as_str().into(),
            message: l.message,
            metadata: l.metadata,
            created_at: l.created_at,
        }
    }
}

// ── Authorization helpers ───────────────────────────────────────────────────

/// Returns Ok if the caller can act on a scheduled-job whose `client_id` is
/// `Some(c)` (member of c) or `None` (caller is anchor or has ADMIN_ALL).
fn check_scope_access(auth: &Authenticated, client_id: Option<&str>) -> Result<(), PlatformError> {
    match client_id {
        Some(cid) => {
            if auth.0.can_access_client(cid) {
                Ok(())
            } else {
                Err(PlatformError::forbidden(format!(
                    "No access to client: {}",
                    cid
                )))
            }
        }
        None => {
            if auth.0.is_anchor() || auth.0.has_permission(crate::permissions::ADMIN_ALL) {
                Ok(())
            } else {
                Err(PlatformError::forbidden(
                    "Only anchor users can manage platform-scoped scheduled jobs",
                ))
            }
        }
    }
}

// ── CRUD handlers ───────────────────────────────────────────────────────────

#[utoipa::path(
    post, path = "", tag = "scheduled-jobs",
    operation_id = "postApiScheduledJobs",
    request_body = CreateScheduledJobRequest,
    responses((status = 201, body = CreatedResponse), (status = 400), (status = 403), (status = 409)),
    security(("bearer_auth" = []))
)]
pub async fn create_scheduled_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Json(req): Json<CreateScheduledJobRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), PlatformError> {
    crate::shared::authorization_service::checks::can_create_scheduled_jobs(&auth.0)?;
    check_scope_access(&auth, req.client_id.as_deref())?;

    let cmd = CreateScheduledJobCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        client_id: req.client_id,
        crons: req.crons,
        timezone: req.timezone,
        payload: req.payload,
        concurrent: req.concurrent,
        tracks_completion: req.tracks_completion,
        timeout_seconds: req.timeout_seconds,
        delivery_max_attempts: req.delivery_max_attempts,
        target_url: req.target_url,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse::new(event.scheduled_job_id)),
    ))
}

#[utoipa::path(
    get, path = "", tag = "scheduled-jobs",
    operation_id = "getApiScheduledJobs",
    params(ListJobsQuery),
    responses((status = 200, body = PaginatedResponse<ScheduledJobResponse>)),
    security(("bearer_auth" = []))
)]
pub async fn list_scheduled_jobs(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Query(q): Query<ListJobsQuery>,
) -> Result<Json<PaginatedResponse<ScheduledJobResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;

    let client_filter: Option<Option<&str>> = match q.client_id.as_deref() {
        Some("platform") => Some(None),
        Some(c) => Some(Some(c)),
        None => None,
    };
    let status_filter = q.status.as_deref().map(ScheduledJobStatus::from_str);

    let jobs = state
        .repo
        .find_with_filters(
            client_filter,
            status_filter,
            q.search.as_deref(),
            Some(q.pagination.limit()),
            Some(q.pagination.offset() as i64),
        )
        .await?;
    let total = state
        .repo
        .count_with_filters(client_filter, status_filter, q.search.as_deref())
        .await? as u64;

    // Filter by client access. Platform-scoped jobs visible only to anchor.
    let visible: Vec<ScheduledJob> = jobs
        .into_iter()
        .filter(|j| match &j.client_id {
            Some(cid) => auth.0.can_access_client(cid),
            None => auth.0.is_anchor(),
        })
        .collect();

    // Hydrate has_active_instance per row. Small N (page size) — fine
    // sequentially; replace with a single GROUP BY query if pages get wide.
    let mut data = Vec::with_capacity(visible.len());
    for j in visible {
        let active = state
            .instance_repo
            .has_active_instance(&j.id)
            .await
            .unwrap_or(false);
        data.push(ScheduledJobResponse::from(j, active));
    }

    Ok(Json(PaginatedResponse::new(
        data,
        q.pagination.page(),
        q.pagination.size(),
        total,
    )))
}

#[utoipa::path(
    get, path = "/{id}", tag = "scheduled-jobs",
    operation_id = "getApiScheduledJobsById",
    params(("id" = String, Path, description = "Scheduled job ID")),
    responses((status = 200, body = ScheduledJobResponse), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn get_scheduled_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ScheduledJobResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;

    let job = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    check_scope_access(&auth, job.client_id.as_deref())?;
    let active = state
        .instance_repo
        .has_active_instance(&job.id)
        .await
        .unwrap_or(false);
    Ok(Json(ScheduledJobResponse::from(job, active)))
}

#[utoipa::path(
    get, path = "/by-code/{code}", tag = "scheduled-jobs",
    operation_id = "getApiScheduledJobsByCode",
    params(
        ("code" = String, Path, description = "Scheduled job code"),
        ByCodeQuery,
    ),
    responses((status = 200, body = ScheduledJobResponse), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn get_scheduled_job_by_code(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(code): Path<String>,
    Query(q): Query<ByCodeQuery>,
) -> Result<Json<ScheduledJobResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;

    let cid = q.client_id.as_deref();
    let job = state
        .repo
        .find_by_code(cid, &code)
        .await?
        .or_not_found("ScheduledJob", &code)?;
    check_scope_access(&auth, job.client_id.as_deref())?;
    let active = state
        .instance_repo
        .has_active_instance(&job.id)
        .await
        .unwrap_or(false);
    Ok(Json(ScheduledJobResponse::from(job, active)))
}

#[utoipa::path(
    put, path = "/{id}", tag = "scheduled-jobs",
    operation_id = "putApiScheduledJobsById",
    params(("id" = String, Path, description = "Scheduled job ID")),
    request_body = UpdateScheduledJobRequest,
    responses((status = 204), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn update_scheduled_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateScheduledJobRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_update_scheduled_jobs(&auth.0)?;

    let existing = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    check_scope_access(&auth, existing.client_id.as_deref())?;

    let cmd = UpdateScheduledJobCommand {
        scheduled_job_id: id,
        name: req.name,
        description: req.description,
        crons: req.crons,
        timezone: req.timezone,
        payload: req.payload,
        concurrent: req.concurrent,
        tracks_completion: req.tracks_completion,
        timeout_seconds: req.timeout_seconds,
        delivery_max_attempts: req.delivery_max_attempts,
        target_url: req.target_url,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/{id}/pause", tag = "scheduled-jobs",
    operation_id = "postApiScheduledJobsByIdPause",
    params(("id" = String, Path, description = "Scheduled job ID")),
    responses((status = 204), (status = 404), (status = 409)),
    security(("bearer_auth" = []))
)]
pub async fn pause_scheduled_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_pause_scheduled_jobs(&auth.0)?;
    let existing = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    check_scope_access(&auth, existing.client_id.as_deref())?;

    let cmd = PauseScheduledJobCommand {
        scheduled_job_id: id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.pause_use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/{id}/resume", tag = "scheduled-jobs",
    operation_id = "postApiScheduledJobsByIdResume",
    params(("id" = String, Path, description = "Scheduled job ID")),
    responses((status = 204), (status = 404), (status = 409)),
    security(("bearer_auth" = []))
)]
pub async fn resume_scheduled_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_pause_scheduled_jobs(&auth.0)?;
    let existing = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    check_scope_access(&auth, existing.client_id.as_deref())?;

    let cmd = ResumeScheduledJobCommand {
        scheduled_job_id: id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.resume_use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/{id}/archive", tag = "scheduled-jobs",
    operation_id = "postApiScheduledJobsByIdArchive",
    params(("id" = String, Path, description = "Scheduled job ID")),
    responses((status = 204), (status = 404), (status = 409)),
    security(("bearer_auth" = []))
)]
pub async fn archive_scheduled_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_update_scheduled_jobs(&auth.0)?;
    let existing = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    check_scope_access(&auth, existing.client_id.as_deref())?;

    let cmd = ArchiveScheduledJobCommand {
        scheduled_job_id: id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.archive_use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete, path = "/{id}", tag = "scheduled-jobs",
    operation_id = "deleteApiScheduledJobsById",
    params(("id" = String, Path, description = "Scheduled job ID")),
    responses((status = 204), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn delete_scheduled_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_delete_scheduled_jobs(&auth.0)?;
    let existing = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    check_scope_access(&auth, existing.client_id.as_deref())?;

    let cmd = DeleteScheduledJobCommand {
        scheduled_job_id: id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/{id}/fire", tag = "scheduled-jobs",
    operation_id = "postApiScheduledJobsByIdFire",
    params(("id" = String, Path, description = "Scheduled job ID")),
    request_body = FireRequest,
    responses((status = 202, body = CreatedResponse), (status = 404), (status = 409)),
    security(("bearer_auth" = []))
)]
pub async fn fire_scheduled_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<FireRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), PlatformError> {
    crate::shared::authorization_service::checks::can_fire_scheduled_jobs(&auth.0)?;
    let existing = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    check_scope_access(&auth, existing.client_id.as_deref())?;

    let cmd = FireScheduledJobCommand {
        scheduled_job_id: id,
        correlation_id: req.correlation_id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.fire_use_case.run(cmd, ctx).await.into_result()?;
    Ok((
        StatusCode::ACCEPTED,
        Json(CreatedResponse::new(event.instance_id)),
    ))
}

// ── Instance reads (admin) ──────────────────────────────────────────────────

#[utoipa::path(
    get, path = "/{id}/instances", tag = "scheduled-jobs",
    operation_id = "getApiScheduledJobsByIdInstances",
    params(("id" = String, Path, description = "Scheduled job ID"), ListInstancesQuery),
    responses((status = 200, body = PaginatedResponse<ScheduledJobInstanceResponse>)),
    security(("bearer_auth" = []))
)]
pub async fn list_instances_for_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Query(q): Query<ListInstancesQuery>,
) -> Result<Json<PaginatedResponse<ScheduledJobInstanceResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_job_instances(&auth.0)?;
    let job = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    check_scope_access(&auth, job.client_id.as_deref())?;

    let status = q.status.as_deref().map(InstanceStatus::from_str);
    let trigger = q.trigger_kind.as_deref().map(TriggerKind::from_str);
    let filters = InstanceListFilters {
        scheduled_job_id: Some(&id),
        client_id: None,
        status,
        trigger_kind: trigger,
        from: q.from,
        to: q.to,
        limit: Some(q.pagination.limit()),
        offset: Some(q.pagination.offset() as i64),
    };
    let count_filters = InstanceListFilters {
        limit: None,
        offset: None,
        ..filters.clone()
    };
    let rows = state.instance_repo.list(&filters).await?;
    let total = state.instance_repo.count(&count_filters).await? as u64;
    let data: Vec<_> = rows.into_iter().map(Into::into).collect();
    Ok(Json(PaginatedResponse::new(
        data,
        q.pagination.page(),
        q.pagination.size(),
        total,
    )))
}

#[utoipa::path(
    get, path = "/instances/{instanceId}", tag = "scheduled-jobs",
    operation_id = "getApiScheduledJobsInstancesById",
    params(("instanceId" = String, Path, description = "Instance ID")),
    responses((status = 200, body = ScheduledJobInstanceResponse), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn get_instance(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(instance_id): Path<String>,
) -> Result<Json<ScheduledJobInstanceResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_job_instances(&auth.0)?;
    let inst = state
        .instance_repo
        .find_by_id(&instance_id)
        .await?
        .or_not_found("ScheduledJobInstance", &instance_id)?;
    check_scope_access(&auth, inst.client_id.as_deref())?;
    Ok(Json(inst.into()))
}

#[utoipa::path(
    get, path = "/instances/{instanceId}/logs", tag = "scheduled-jobs",
    operation_id = "getApiScheduledJobsInstancesByIdLogs",
    params(("instanceId" = String, Path, description = "Instance ID")),
    responses((status = 200, body = Vec<InstanceLogResponse>), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn list_instance_logs(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(instance_id): Path<String>,
) -> Result<Json<Vec<InstanceLogResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_job_instances(&auth.0)?;
    let inst = state
        .instance_repo
        .find_by_id(&instance_id)
        .await?
        .or_not_found("ScheduledJobInstance", &instance_id)?;
    check_scope_access(&auth, inst.client_id.as_deref())?;
    let logs = state
        .instance_repo
        .list_logs_for_instance(&instance_id, None)
        .await?;
    Ok(Json(logs.into_iter().map(Into::into).collect()))
}

// ── SDK callback path (infrastructure write — bypasses UoW) ────────────────

#[utoipa::path(
    post, path = "/instances/{instanceId}/log", tag = "scheduled-jobs",
    operation_id = "postApiScheduledJobsInstancesByIdLog",
    params(("instanceId" = String, Path, description = "Instance ID")),
    request_body = InstanceLogRequest,
    responses((status = 202), (status = 403), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn post_instance_log(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(instance_id): Path<String>,
    Json(req): Json<InstanceLogRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_write_scheduled_job_instance(&auth.0)?;
    let inst = state
        .instance_repo
        .find_by_id(&instance_id)
        .await?
        .or_not_found("ScheduledJobInstance", &instance_id)?;
    check_scope_access(&auth, inst.client_id.as_deref())?;

    let log = ScheduledJobInstanceLog {
        id: crate::TsidGenerator::generate(crate::EntityType::ScheduledJobInstanceLog),
        instance_id: inst.id.clone(),
        scheduled_job_id: Some(inst.scheduled_job_id.clone()),
        client_id: inst.client_id.clone(),
        level: req.level.into(),
        message: req.message,
        metadata: req.metadata,
        created_at: Utc::now(),
    };
    state.instance_repo.insert_log(&log).await?;
    Ok(StatusCode::ACCEPTED)
}

#[utoipa::path(
    post, path = "/instances/{instanceId}/complete", tag = "scheduled-jobs",
    operation_id = "postApiScheduledJobsInstancesByIdComplete",
    params(("instanceId" = String, Path, description = "Instance ID")),
    request_body = InstanceCompleteRequest,
    responses((status = 204), (status = 403), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn post_instance_complete(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(instance_id): Path<String>,
    Json(req): Json<InstanceCompleteRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_write_scheduled_job_instance(&auth.0)?;
    let inst = state
        .instance_repo
        .find_by_id(&instance_id)
        .await?
        .or_not_found("ScheduledJobInstance", &instance_id)?;
    check_scope_access(&auth, inst.client_id.as_deref())?;

    state
        .instance_repo
        .record_completion(
            &inst.id,
            inst.created_at,
            req.status.into(),
            req.result.as_ref(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Router ──────────────────────────────────────────────────────────────────

pub fn scheduled_jobs_router(state: ScheduledJobsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_scheduled_job, list_scheduled_jobs))
        .routes(routes!(
            get_scheduled_job,
            update_scheduled_job,
            delete_scheduled_job
        ))
        .routes(routes!(get_scheduled_job_by_code))
        .routes(routes!(pause_scheduled_job))
        .routes(routes!(resume_scheduled_job))
        .routes(routes!(archive_scheduled_job))
        .routes(routes!(fire_scheduled_job))
        .routes(routes!(list_instances_for_job))
        .routes(routes!(get_instance))
        .routes(routes!(list_instance_logs))
        .routes(routes!(post_instance_log))
        .routes(routes!(post_instance_complete))
        .with_state(state)
}
