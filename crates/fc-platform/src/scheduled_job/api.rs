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
    /// The application the job belongs to (Go `applicationId`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
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

/// `POST /api/scheduled-jobs/{id}/fire` response: Go's `FireNowResponse`.
/// `id` is the instance id, kept beside `instanceId` for callers of the
/// earlier `{id}` shape.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FireNowResponse {
    pub id: String,
    pub scheduled_job_id: String,
    pub instance_id: String,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FireRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLogRequest {
    /// Required, as Go's `WriteInstanceLogRequest` (huma: no `omitempty`).
    pub level: LogLevelDto,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevelDto {
    Debug,
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

/// Go's `CompleteInstanceRequest`: two dialects, every member optional.
/// The SDK sends `{status: SUCCESS|FAILURE, result}`; the SPA sends
/// `{status: <instance status>, completionStatus, completionResult}`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceCompleteRequest {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub completion_status: Option<String>,
    #[serde(default)]
    pub completion_result: Option<serde_json::Value>,
    /// SDK alias for `completionResult`.
    #[serde(default)]
    pub result: Option<serde_json::Value>,
}

/// Go's `resolveInstanceCompletion`: `SUCCESS`/`FAILURE` (or none) complete
/// the instance with that outcome; any other value is the instance status
/// itself. An explicit `completionStatus` wins. `None` for a status that is
/// neither.
fn resolve_instance_completion(
    status: Option<&str>,
    completion_status: Option<&str>,
) -> Option<(InstanceStatus, Option<CompletionStatus>)> {
    let explicit = match completion_status.filter(|c| !c.is_empty()) {
        Some(c) => Some(c.to_ascii_uppercase().parse::<CompletionStatus>().ok()?),
        None => None,
    };
    let status = status.unwrap_or("").to_ascii_uppercase();
    match status.as_str() {
        "" => Some((InstanceStatus::Completed, explicit)),
        "SUCCESS" | "FAILURE" => Some((
            InstanceStatus::Completed,
            explicit.or_else(|| status.parse::<CompletionStatus>().ok()),
        )),
        other => Some((other.parse::<InstanceStatus>().ok()?, explicit)),
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
/// Go's `ScheduledJobResponse`: absent members stay absent (`omitempty`).
pub struct ScheduledJobResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    pub crons: Vec<String>,
    pub timezone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    pub concurrent: bool,
    pub tracks_completion: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    pub delivery_max_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
            application_id: job.application_id,
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
/// Go's `ScheduledJobInstanceResponse`: absent members stay absent.
pub struct ScheduledJobInstanceResponse {
    pub id: String,
    pub scheduled_job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub job_code: String,
    pub trigger_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<DateTime<Utc>>,
    pub fired_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub delivery_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
/// Go's `ScheduledJobInstanceLogResponse`.
pub struct InstanceLogResponse {
    pub id: String,
    pub instance_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub level: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl From<ScheduledJobInstanceLog> for InstanceLogResponse {
    fn from(l: ScheduledJobInstanceLog) -> Self {
        Self {
            id: l.id,
            instance_id: l.instance_id,
            scheduled_job_id: l.scheduled_job_id,
            client_id: l.client_id,
            level: l.level.as_str().into(),
            message: l.message,
            metadata: l.metadata,
            created_at: l.created_at,
        }
    }
}

// ── Authorization helpers ───────────────────────────────────────────────────

/// Go's by-id write rule (`auth.CheckScopeAccess` in each use case): 403
/// `SCOPE_FORBIDDEN`.
fn check_scope_access(auth: &Authenticated, client_id: Option<&str>) -> Result<(), PlatformError> {
    crate::shared::authorization_service::checks::check_scope_access(&auth.0, client_id)
}

/// Go's read rule (`getByID`, `getByCode`, `getInstance`): a client's job
/// needs that client (403 `FORBIDDEN` with `message`); a platform job is
/// readable by any holder of the read permission.
fn check_read_access(
    auth: &Authenticated,
    client_id: Option<&str>,
    message: &str,
) -> Result<(), PlatformError> {
    match client_id {
        Some(cid) if !auth.0.can_access_client(cid) => Err(PlatformError::forbidden(message)),
        _ => Ok(()),
    }
}

/// Go's `CreateScheduledJob` authorize phase: a client's job needs that
/// client, a platform job an anchor.
fn check_create_access(auth: &Authenticated, client_id: Option<&str>) -> Result<(), PlatformError> {
    match client_id {
        Some(cid) if !auth.0.can_access_client(cid) => Err(PlatformError::forbidden(format!(
            "No access to client: {cid}"
        ))),
        Some(_) => Ok(()),
        None if auth.0.is_anchor() => Ok(()),
        None => Err(PlatformError::forbidden(
            "Only anchor users can create platform-scoped jobs",
        )),
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

    let cmd = CreateScheduledJobCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        client_id: req.client_id,
        application_id: req.application_id,
        crons: req.crons,
        timezone: req.timezone,
        payload: req.payload,
        concurrent: req.concurrent,
        tracks_completion: req.tracks_completion,
        timeout_seconds: req.timeout_seconds,
        delivery_max_attempts: req.delivery_max_attempts,
        target_url: req.target_url,
    };
    // Go validates the command before its authorize phase checks the scope.
    state
        .create_use_case
        .validate(&cmd)
        .await
        .map_err(PlatformError::from)?;
    check_create_access(&auth, cmd.client_id.as_deref())?;
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
    let status_filter =
        crate::shared::enum_str::parse_opt::<ScheduledJobStatus>(q.status.as_deref())?;

    // Scoped in SQL, as Go: a non-anchor sees platform jobs and its own
    // clients' jobs, and COUNT agrees with the page.
    let accessible: Option<Vec<String>> = if auth.0.is_anchor() {
        None
    } else {
        Some(crate::shared::caller_reach::client_ids(&auth.0))
    };
    let visible: Vec<ScheduledJob> = state
        .repo
        .find_with_filters_scoped(
            client_filter,
            status_filter,
            q.search.as_deref(),
            accessible.as_deref(),
            Some(q.pagination.limit()),
            Some(q.pagination.offset() as i64),
        )
        .await?;
    let total = state
        .repo
        .count_with_filters_scoped(
            client_filter,
            status_filter,
            q.search.as_deref(),
            accessible.as_deref(),
        )
        .await? as u64;

    // hasActiveInstance for the whole page in one query.
    let ids: Vec<String> = visible.iter().map(|j| j.id.clone()).collect();
    let tracking: Vec<String> = visible
        .iter()
        .filter(|j| j.tracks_completion)
        .map(|j| j.id.clone())
        .collect();
    let active = state
        .instance_repo
        .jobs_with_active_instances(&ids, &tracking)
        .await
        .unwrap_or_default();
    let data: Vec<ScheduledJobResponse> = visible
        .into_iter()
        .map(|j| {
            let is_active = active.contains(&j.id);
            ScheduledJobResponse::from(j, is_active)
        })
        .collect();

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
    check_read_access(
        &auth,
        job.client_id.as_deref(),
        "No access to this scheduled job",
    )?;
    let active = state
        .instance_repo
        .has_active_instance_for(&job.id, job.tracks_completion)
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
    check_read_access(
        &auth,
        job.client_id.as_deref(),
        "No access to this scheduled job",
    )?;
    let active = state
        .instance_repo
        .has_active_instance_for(&job.id, job.tracks_completion)
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
    crate::shared::authorization_service::checks::can_write_scheduled_jobs(&auth.0)?;

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
    crate::shared::authorization_service::checks::can_write_scheduled_jobs(&auth.0)?;
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
    crate::shared::authorization_service::checks::can_write_scheduled_jobs(&auth.0)?;
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
    crate::shared::authorization_service::checks::can_write_scheduled_jobs(&auth.0)?;
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
    responses((status = 202, body = FireNowResponse), (status = 404), (status = 409)),
    security(("bearer_auth" = []))
)]
pub async fn fire_scheduled_job(
    State(state): State<ScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    // The body is optional, as in Go: a fire with no body (and no
    // Content-Type) is a fire with no correlation id.
    req: Option<Json<FireRequest>>,
) -> Result<(StatusCode, Json<FireNowResponse>), PlatformError> {
    let req = req.map(|Json(r)| r).unwrap_or_default();
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
        Json(FireNowResponse {
            id: event.instance_id.clone(),
            scheduled_job_id: event.scheduled_job_id,
            instance_id: event.instance_id,
        }),
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
    // Go lists by job id without loading the job (an unknown id is an empty
    // page); a job the caller cannot read stays refused.
    if let Some(job) = state.repo.find_by_id(&id).await? {
        check_read_access(
            &auth,
            job.client_id.as_deref(),
            "No access to this scheduled job",
        )?;
    }

    let status = crate::shared::enum_str::parse_opt::<InstanceStatus>(q.status.as_deref())?;
    let trigger = crate::shared::enum_str::parse_opt::<TriggerKind>(q.trigger_kind.as_deref())?;
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
    check_read_access(
        &auth,
        inst.client_id.as_deref(),
        "No access to this instance",
    )?;
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
    // Go answers an unknown instance's logs with an empty array, not 404.
    let Some(inst) = state.instance_repo.find_by_id(&instance_id).await? else {
        return Ok(Json(Vec::new()));
    };
    check_read_access(
        &auth,
        inst.client_id.as_deref(),
        "No access to this instance",
    )?;
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
    responses((status = 204), (status = 400), (status = 403), (status = 404)),
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
        id: crate::shared::tsid::generate(crate::EntityType::ScheduledJobInstanceLog),
        instance_id: inst.id.clone(),
        scheduled_job_id: Some(inst.scheduled_job_id.clone()),
        client_id: inst.client_id.clone(),
        level: req.level.into(),
        message: req.message,
        metadata: req.metadata,
        created_at: Utc::now(),
    };
    state.instance_repo.insert_log(&log).await?;
    Ok(StatusCode::NO_CONTENT)
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

    let (status, completion) =
        resolve_instance_completion(req.status.as_deref(), req.completion_status.as_deref())
            .ok_or_else(|| {
                PlatformError::bad_request_code(
                    "INVALID_STATUS",
                    "status must be SUCCESS, FAILURE, or a known instance status",
                )
            })?;
    // The SPA sends `completionResult`, the SDK `result`.
    let result = req.completion_result.or(req.result);
    state
        .instance_repo
        .record_completion(
            &inst.id,
            inst.created_at,
            status,
            completion,
            result.as_ref(),
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
