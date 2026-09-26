//! Dispatch Jobs BFF API
//!
//! REST endpoints for managing dispatch jobs.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::dispatch_job::entity::{parse_dispatch_mode, parse_dispatch_status};
use crate::shared::enum_str::{non_empty, parse_opt};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::DispatchJobRepository;
use crate::{
    DispatchAttempt, DispatchJob, DispatchJobRead, DispatchKind, DispatchMetadata, RetryStrategy,
};

/// Dispatch job response DTO: Go's `DispatchJobResponse`
/// (dispatchjob/api/dto.go). Absent members stay absent (`omitempty`); the
/// job row carries no attempts, so `attempts` is only ever absent here (the
/// attempts route lists them).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    pub kind: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub target_url: String,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    pub payload_content_type: String,
    pub data_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    pub mode: String,
    pub sequence: i32,
    pub timeout_seconds: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_id: Option<String>,
    pub max_retries: u32,
    pub retry_strategy: String,
    pub status: String,
    pub attempt_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<DispatchAttemptResponse>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[schema(value_type = Vec<Object>)]
    pub metadata: Vec<DispatchMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_millis: Option<i64>,
}

impl From<DispatchJob> for DispatchJobResponse {
    fn from(job: DispatchJob) -> Self {
        Self {
            id: job.id,
            external_id: job.external_id,
            kind: job.kind.as_str().to_string(),
            code: job.code,
            source: job.source,
            subject: job.subject,
            target_url: job.target_url,
            protocol: job.protocol.as_str().to_string(),
            payload: job.payload,
            payload_content_type: job.payload_content_type,
            data_only: job.data_only,
            event_id: job.event_id,
            correlation_id: job.correlation_id,
            client_id: job.client_id,
            subscription_id: job.subscription_id,
            service_account_id: job.service_account_id,
            dispatch_pool_id: job.dispatch_pool_id,
            message_group: job.message_group,
            mode: job.mode.as_str().to_string(),
            sequence: job.sequence,
            timeout_seconds: job.timeout_seconds,
            schema_id: job.schema_id,
            max_retries: job.max_retries,
            retry_strategy: job.retry_strategy.as_str().to_string(),
            status: job.status.as_str().to_string(),
            attempt_count: job.attempt_count,
            last_error: job.last_error,
            attempts: job.attempts.into_iter().map(Into::into).collect(),
            metadata: job.metadata,
            idempotency_key: job.idempotency_key,
            created_at: job.created_at.to_rfc3339(),
            updated_at: job.updated_at.to_rfc3339(),
            scheduled_for: job.scheduled_for.map(|t| t.to_rfc3339()),
            expires_at: job.expires_at.map(|t| t.to_rfc3339()),
            last_attempt_at: job.last_attempt_at.map(|t| t.to_rfc3339()),
            completed_at: job.completed_at.map(|t| t.to_rfc3339()),
            duration_millis: job.duration_millis,
        }
    }
}

/// Dispatch job read-projection row: Go's slim `DispatchJobRead`
/// (dispatchjob/api/dto.go), the list, list-raw and by-event shape the SPA
/// binds.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobReadResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<String>,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub status: String,
    pub kind: String,
    pub target_url: String,
    pub mode: String,
    pub dispatch_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<String>,
    pub attempt_count: u32,
}

/// Go's `splitCode`: application, subdomain and aggregate from the
/// colon-delimited code, each absent when blank.
fn split_code(code: &str) -> [Option<String>; 3] {
    let mut parts = code
        .split(':')
        .map(|p| (!p.is_empty()).then(|| p.to_string()));
    [
        parts.next().flatten(),
        parts.next().flatten(),
        parts.next().flatten(),
    ]
}

impl From<DispatchJobRead> for DispatchJobReadResponse {
    fn from(job: DispatchJobRead) -> Self {
        let [application, subdomain, aggregate] = split_code(&job.code);
        let mode = job.mode.as_str().to_string();
        Self {
            id: job.id,
            event_id: job.event_id,
            subscription_id: job.subscription_id,
            client_id: job.client_id,
            application,
            subdomain,
            aggregate,
            code: job.code,
            source: job.source,
            subject: job.subject,
            status: job.status.as_str().to_string(),
            kind: job.kind.as_str().to_string(),
            target_url: job.target_url,
            dispatch_mode: mode.clone(),
            mode,
            correlation_id: job.correlation_id,
            message_group: job.message_group,
            scheduled_for: job.scheduled_for.map(|t| t.to_rfc3339()),
            created_at: job.created_at.to_rfc3339(),
            updated_at: job.updated_at.to_rfc3339(),
            completed_at: job.completed_at.map(|t| t.to_rfc3339()),
            last_attempt_at: job.last_attempt_at.map(|t| t.to_rfc3339()),
            attempt_count: job.attempt_count,
        }
    }
}

/// Query parameters for dispatch jobs list.
///
/// `msg_dispatch_jobs_read` is an append-only firehose, so this endpoint
/// returns the most recent N rows only — no pagination. Sort order is
/// fixed to most-recent-first (`created_at DESC, id DESC`); narrow filters
/// or look up by id if you need older rows.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DispatchJobsQuery {
    /// Result size. Default 50, capped at 1000.
    pub size: Option<u32>,

    /// Filter by event ID
    pub event_id: Option<String>,

    /// Filter by correlation ID
    pub correlation_id: Option<String>,

    /// Filter by subscription ID
    pub subscription_id: Option<String>,

    /// Filter by client IDs (comma-separated)
    pub client_ids: Option<String>,

    /// Filter by statuses (comma-separated)
    pub statuses: Option<String>,

    /// Filter by application codes (comma-separated)
    pub applications: Option<String>,

    /// Filter by subdomains (comma-separated)
    pub subdomains: Option<String>,

    /// Filter by aggregates (comma-separated)
    pub aggregates: Option<String>,

    /// Filter by codes (comma-separated)
    pub codes: Option<String>,

    /// Free-text search across code, subject, source
    pub source: Option<String>,
}

fn split_csv(input: Option<&str>) -> Vec<String> {
    input
        .map(|s| {
            s.split(',')
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Dispatch jobs service state
#[derive(Clone)]
pub struct DispatchJobsState {
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
    /// Refuses a job signed by an identity the caller may not use (S5).
    pub signing: Arc<crate::dispatch_job::signing_guard::SigningGuard>,
}

// ============================================================================
// Create Dispatch Job Request & Response
// ============================================================================

/// Request to create a new dispatch job
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDispatchJobRequest {
    /// Caller-supplied job id, honoured on the batch routes only (Go's
    /// `BatchItem.ID`; its single create has no id). 1 to 13 letters,
    /// digits, `_` or `-`; an id already taken, or repeated in the batch,
    /// refuses the batch 409 `DUPLICATE_ID`. Minted when absent.
    #[serde(default)]
    pub id: Option<String>,

    /// Source system/application
    pub source: Option<String>,

    /// The kind of dispatch job (EVENT or TASK)
    #[serde(default)]
    pub kind: Option<String>,

    /// The event type or task code
    pub code: String,

    /// CloudEvents-style subject/aggregate reference
    pub subject: Option<String>,

    /// Source event ID (required for EVENT kind)
    pub event_id: Option<String>,

    /// Correlation ID for distributed tracing
    pub correlation_id: Option<String>,

    /// Target URL for webhook delivery
    pub target_url: String,

    /// Payload to deliver (JSON string)
    pub payload: String,

    /// Content type of payload
    pub payload_content_type: Option<String>,

    /// If true, send raw payload only
    #[serde(default)]
    pub data_only: bool,

    /// Service account for authentication. Required by the single create
    /// (400 `VALIDATION` without it); optional on the batch routes, as in Go
    /// (`BatchItem.ServiceAccountID`), whose outbox items — the Laravel SDK's
    /// among them — carry none.
    #[serde(default)]
    pub service_account_id: Option<String>,

    /// Client ID
    pub client_id: Option<String>,

    /// Subscription ID that created this job
    pub subscription_id: Option<String>,

    /// Dispatch mode for ordering
    pub mode: Option<String>,

    /// Rate limiting pool ID
    pub dispatch_pool_id: Option<String>,

    /// Message group for FIFO ordering
    pub message_group: Option<String>,

    /// Sequence number within message group
    pub sequence: Option<i32>,

    /// Timeout in seconds for HTTP call
    pub timeout_seconds: Option<u32>,

    /// Maximum retry attempts
    pub max_retries: Option<u32>,

    /// Retry strategy
    pub retry_strategy: Option<String>,

    /// Idempotency key for deduplication
    pub idempotency_key: Option<String>,

    /// External reference ID
    pub external_id: Option<String>,

    /// Custom metadata
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

/// Response for create dispatch job
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDispatchJobResponse {
    pub job: DispatchJobResponse,
}

/// Batch create dispatch jobs request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCreateDispatchJobsRequest {
    pub jobs: Vec<CreateDispatchJobRequest>,
}

/// Batch create dispatch jobs response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCreateDispatchJobsResponse {
    pub jobs: Vec<DispatchJobResponse>,
    pub count: usize,
}

/// Dispatch attempt response DTO: Go's `AttemptDTO`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchAttemptResponse {
    pub attempt_number: u32,
    pub attempted_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_millis: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    /// What the platform sent on this attempt (Go's `RequestSummary`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub request: Option<serde_json::Value>,
}

impl From<DispatchAttempt> for DispatchAttemptResponse {
    fn from(a: DispatchAttempt) -> Self {
        Self {
            attempt_number: a.attempt_number,
            attempted_at: a.attempted_at.to_rfc3339(),
            completed_at: a.completed_at.map(|t| t.to_rfc3339()),
            duration_millis: a.duration_millis,
            response_code: a.response_code,
            response_body: a.response_body,
            success: a.success,
            error_message: a.error_message,
            error_type: a.error_type.map(|t| t.as_str().to_string()),
            request: None,
        }
    }
}

impl From<crate::dispatch_job::repository::RecordedAttempt> for DispatchAttemptResponse {
    fn from(r: crate::dispatch_job::repository::RecordedAttempt) -> Self {
        let mut out = Self::from(r.attempt);
        out.request = r.request_info.filter(|v| !v.is_null());
        out
    }
}

/// Go's `CheckScopeAccess` on a job read by id: 403 `SCOPE_FORBIDDEN`.
fn check_job_scope(auth: &Authenticated, client_id: Option<&str>) -> Result<(), PlatformError> {
    crate::shared::authorization_service::checks::check_scope_access(&auth.0, client_id)
}

/// Get dispatch job by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "dispatch-jobs",
    operation_id = "getApiDispatchJobsById",
    params(
        ("id" = String, Path, description = "Dispatch job ID")
    ),
    responses(
        (status = 200, description = "Dispatch job found", body = DispatchJobResponse),
        (status = 404, description = "Dispatch job not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dispatch_job(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs(&auth.0)?;

    let job = state
        .dispatch_job_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("DispatchJob", &id))?;
    check_job_scope(&auth, job.client_id.as_deref())?;

    Ok(Json(job.into()))
}

/// List dispatch jobs. Returns the most recent rows matching the filters;
/// no pagination — see `DispatchJobsQuery` for the rationale.
#[utoipa::path(
    get,
    path = "",
    tag = "dispatch-jobs",
    operation_id = "getApiDispatchJobs",
    params(DispatchJobsQuery),
    responses(
        (status = 200, description = "List of dispatch jobs", body = Vec<DispatchJobReadResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_dispatch_jobs(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Query(query): Query<DispatchJobsQuery>,
) -> Result<Json<Vec<DispatchJobReadResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs(&auth.0)?;

    let statuses = split_csv(query.statuses.as_deref());
    let applications = split_csv(query.applications.as_deref());
    let subdomains = split_csv(query.subdomains.as_deref());
    let aggregates = split_csv(query.aggregates.as_deref());
    let codes = split_csv(query.codes.as_deref());

    let Some(client_ids) = crate::shared::caller_reach::read_client_filter(
        &auth.0,
        split_csv(query.client_ids.as_deref()),
    )?
    else {
        return Ok(Json(vec![]));
    };

    // Unknown (or miscased) statuses are a 400, not an empty result.
    let statuses = statuses
        .iter()
        .map(|s| Ok(parse_dispatch_status(s)?.as_str().to_string()))
        .collect::<Result<Vec<_>, PlatformError>>()?;

    let size = query.size.unwrap_or(50).clamp(1, 1000) as i64;

    let jobs = state
        .dispatch_job_repo
        .find_read_with_cursor(
            &client_ids,
            &statuses,
            &applications,
            &subdomains,
            &aggregates,
            &codes,
            query.source.as_deref(),
            None,
            size,
        )
        .await?;

    let items = jobs
        .into_iter()
        .map(DispatchJobReadResponse::from)
        .collect();
    Ok(Json(items))
}

/// Get dispatch jobs for an event
#[utoipa::path(
    get,
    path = "/by-event/{eventId}",
    tag = "dispatch-jobs",
    operation_id = "getApiDispatchJobsByEventByEventId",
    params(
        ("eventId" = String, Path, description = "Event ID")
    ),
    responses(
        (status = 200, description = "Dispatch jobs for event", body = Vec<DispatchJobReadResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_jobs_for_event(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Path(event_id): Path<String>,
) -> Result<Json<Vec<DispatchJobReadResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs(&auth.0)?;

    // Go `byEvent`: the read projection, newest first, keeping only the jobs
    // the caller may reach (`CanAccessScope`: a platform job is an anchor's
    // or super-admin's).
    let jobs = state
        .dispatch_job_repo
        .find_read_by_event_id(&event_id)
        .await?;
    let filtered: Vec<DispatchJobReadResponse> = jobs
        .into_iter()
        .filter(|j| {
            crate::shared::authorization_service::checks::check_scope_access(
                &auth.0,
                j.client_id.as_deref(),
            )
            .is_ok()
        })
        .map(Into::into)
        .collect();

    Ok(Json(filtered))
}

// ============================================================================
// Create Dispatch Job Endpoints
// ============================================================================

/// Create a new dispatch job
///
/// Creates and queues a new dispatch job for webhook delivery.
#[utoipa::path(
    post,
    path = "",
    tag = "dispatch-jobs",
    operation_id = "postApiDispatchJobs",
    request_body = CreateDispatchJobRequest,
    responses(
        (status = 201, description = "Dispatch job created", body = crate::shared::api_common::CreatedResponse),
        (status = 400, description = "Invalid request"),
        (status = 403, description = "No access to client")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_dispatch_job(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Json(req): Json<CreateDispatchJobRequest>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<crate::shared::api_common::CreatedResponse>,
    ),
    PlatformError,
> {
    // Go shared/sdk/dispatch_job_create.go:72: the ingest permission, with
    // Go's body.
    crate::shared::authorization_service::checks::require_permission(
        &auth.0,
        crate::permissions::admin::BATCH_DISPATCH_JOBS_WRITE,
    )?;

    // Go's single create requires the account (dispatch_job_create.go:92-94).
    let Some(service_account_id) = crate::shared::caller_reach::non_blank(req.service_account_id)
    else {
        return Err(PlatformError::bad_request_code(
            "VALIDATION",
            "serviceAccountId is required",
        ));
    };

    // The client the job is written under (owner decision #24).
    let client_id = crate::shared::caller_reach::require_writable_client(&auth.0, req.client_id)?;

    // Determine kind
    // Absent/empty means EVENT; anything else must be an exact kind (400).
    let kind: DispatchKind = parse_opt(non_empty(req.kind.as_deref()))?.unwrap_or_default();

    // Determine mode
    let mode = parse_dispatch_mode(req.mode.as_deref());

    // Determine retry strategy
    // Absent/empty means exponential; anything else must be a known strategy (400).
    let retry_strategy: RetryStrategy =
        parse_opt(non_empty(req.retry_strategy.as_deref()))?.unwrap_or_default();

    // Create the dispatch job
    let _now = chrono::Utc::now();
    let source = req.source.as_deref();
    let mut job = if kind == DispatchKind::Event {
        DispatchJob::for_event(
            req.event_id.as_deref(),
            &req.code,
            source,
            &req.target_url,
            &req.payload,
        )
    } else {
        // A task keeps a supplied eventId, as in Go (sdk/dispatch_jobs_batch.go:144).
        DispatchJob {
            event_id: req.event_id.clone(),
            ..DispatchJob::for_task(&req.code, source, &req.target_url, &req.payload)
        }
    };

    // Apply optional fields
    if let Some(subject) = req.subject {
        job.subject = Some(subject);
    }
    if let Some(correlation_id) = req.correlation_id {
        job.correlation_id = Some(correlation_id);
    }
    job.client_id = client_id;
    if let Some(subscription_id) = req.subscription_id {
        job.subscription_id = Some(subscription_id);
    }
    if let Some(dispatch_pool_id) = req.dispatch_pool_id {
        job.dispatch_pool_id = Some(dispatch_pool_id);
    }
    if let Some(message_group) = req.message_group {
        job.message_group = Some(message_group);
    }
    if let Some(sequence) = req.sequence {
        job.sequence = sequence;
    }
    if let Some(timeout) = req.timeout_seconds {
        job.timeout_seconds = timeout;
    }
    if let Some(max_retries) = req.max_retries {
        job.max_retries = max_retries;
    }
    if let Some(idempotency_key) = req.idempotency_key {
        job.idempotency_key = Some(idempotency_key);
    }
    if let Some(external_id) = req.external_id {
        job.external_id = Some(external_id);
    }
    if let Some(content_type) = req.payload_content_type {
        job.payload_content_type = content_type;
    }

    job.service_account_id = Some(service_account_id);
    job.mode = mode;
    job.retry_strategy = retry_strategy;
    job.data_only = req.data_only;

    // Add metadata
    for (key, value) in req.metadata {
        job.metadata.push(DispatchMetadata { key, value });
    }

    // Created PENDING (the entity's default), as Go inserts it: the
    // scheduler claims and queues it.

    // The identity that would sign it must be the caller's to use.
    state
        .signing
        .check_jobs(&auth.0, std::slice::from_ref(&job))
        .await?;

    // Insert into database
    let id = job.id.clone();
    state.dispatch_job_repo.insert(&job).await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(crate::shared::api_common::CreatedResponse::new(id)),
    ))
}

/// Create multiple dispatch jobs in batch
///
/// Creates multiple dispatch jobs in a single operation. Maximum batch size is 100 jobs.
#[utoipa::path(
    post,
    path = "/batch",
    tag = "dispatch-jobs",
    operation_id = "postApiDispatchJobsBatch",
    request_body = BatchCreateDispatchJobsRequest,
    responses(
        (status = 201, description = "Dispatch jobs created", body = BatchCreateDispatchJobsResponse),
        (status = 400, description = "Invalid request or batch size exceeds limit")
    ),
    security(("bearer_auth" = []))
)]
pub async fn batch_create_dispatch_jobs(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Json(req): Json<BatchCreateDispatchJobsRequest>,
) -> Result<Json<BatchCreateDispatchJobsResponse>, PlatformError> {
    // Go shared/sdk/dispatch_job_create.go:72: the ingest permission, with
    // Go's body.
    crate::shared::authorization_service::checks::require_permission(
        &auth.0,
        crate::permissions::admin::BATCH_DISPATCH_JOBS_WRITE,
    )?;

    // Validate batch size
    if req.jobs.is_empty() {
        return Err(PlatformError::validation(
            "Request body must contain at least one dispatch job",
        ));
    }
    if req.jobs.len() > 100 {
        return Err(PlatformError::validation(
            "Batch size cannot exceed 100 dispatch jobs",
        ));
    }

    let mut created_jobs: Vec<DispatchJob> = Vec::new();
    let mut supplied = crate::shared::batch_api::SuppliedJobIds::default();

    for job_req in req.jobs {
        // The client the job is written under (owner decision #24).
        let client_id =
            crate::shared::caller_reach::require_writable_client(&auth.0, job_req.client_id)?;

        // Determine kind
        // Absent/empty means EVENT; anything else must be an exact kind (400).
        let kind: DispatchKind = parse_opt(non_empty(job_req.kind.as_deref()))?.unwrap_or_default();

        // Determine mode
        let mode = parse_dispatch_mode(job_req.mode.as_deref());

        // Create the dispatch job
        let source = job_req.source.as_deref();
        let mut job = if kind == DispatchKind::Event {
            DispatchJob::for_event(
                job_req.event_id.as_deref(),
                &job_req.code,
                source,
                &job_req.target_url,
                &job_req.payload,
            )
        } else {
            // A task keeps a supplied eventId, as in Go (sdk/dispatch_jobs_batch.go:144).
            DispatchJob {
                event_id: job_req.event_id.clone(),
                ..DispatchJob::for_task(
                    &job_req.code,
                    source,
                    &job_req.target_url,
                    &job_req.payload,
                )
            }
        };

        // Apply optional fields
        if let Some(subject) = job_req.subject {
            job.subject = Some(subject);
        }
        if let Some(correlation_id) = job_req.correlation_id {
            job.correlation_id = Some(correlation_id);
        }
        job.client_id = client_id;
        if let Some(subscription_id) = job_req.subscription_id {
            job.subscription_id = Some(subscription_id);
        }
        if let Some(dispatch_pool_id) = job_req.dispatch_pool_id {
            job.dispatch_pool_id = Some(dispatch_pool_id);
        }
        if let Some(message_group) = job_req.message_group {
            job.message_group = Some(message_group);
        }
        if let Some(timeout) = job_req.timeout_seconds {
            job.timeout_seconds = timeout;
        }
        if let Some(max_retries) = job_req.max_retries {
            job.max_retries = max_retries;
        }

        job.service_account_id = crate::shared::caller_reach::non_blank(job_req.service_account_id);
        job.mode = mode;
        job.data_only = job_req.data_only;
        if let Some(id) = supplied.claim(job_req.id.as_deref())? {
            job.id = id;
        }
        // PENDING, as Go inserts it; the scheduler queues it.
        created_jobs.push(job);
    }

    // Every job's signer must be the caller's to use, before anything is
    // written.
    state.signing.check_jobs(&auth.0, &created_jobs).await?;

    // Bulk insert; a supplied id that already names a job refuses it all.
    let taken = state
        .dispatch_job_repo
        .insert_new(&created_jobs, supplied.ids())
        .await?;
    if !taken.is_empty() {
        return Err(crate::shared::batch_api::job_ids_taken(&taken));
    }

    let count = created_jobs.len();
    let job_responses: Vec<DispatchJobResponse> =
        created_jobs.into_iter().map(Into::into).collect();

    Ok(Json(BatchCreateDispatchJobsResponse {
        jobs: job_responses,
        count,
    }))
}

/// Get all attempts for a dispatch job
///
/// Retrieves the full history of webhook delivery attempts for a job.
#[utoipa::path(
    get,
    path = "/{id}/attempts",
    tag = "dispatch-jobs",
    operation_id = "getApiDispatchJobsByIdAttempts",
    params(
        ("id" = String, Path, description = "Dispatch job ID")
    ),
    responses(
        (status = 200, description = "Attempts list returned", body = Vec<DispatchAttemptResponse>),
        (status = 404, description = "Dispatch job not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dispatch_job_attempts(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<Vec<DispatchAttemptResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs(&auth.0)?;

    let job = state
        .dispatch_job_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("DispatchJob", &id))?;
    check_job_scope(&auth, job.client_id.as_deref())?;

    // The job row carries none: they live in `msg_dispatch_job_attempts`.
    let attempts: Vec<DispatchAttemptResponse> = state
        .dispatch_job_repo
        .find_attempts(&id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(attempts))
}

// ============================================================================
// Filter Options Endpoint
// ============================================================================

/// Go's `DispatchJobFilterOptionsResponse`: distinct facet values of the
/// read projection as plain string arrays.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobFilterOptionsResponse {
    pub statuses: Vec<String>,
    pub codes: Vec<String>,
    pub client_ids: Vec<String>,
    pub dispatch_pool_ids: Vec<String>,
    pub subscription_ids: Vec<String>,
    pub kinds: Vec<String>,
}

/// Get filter options for dispatch jobs
///
/// Returns distinct values from the read projection for the filter dropdowns.
#[utoipa::path(
    get,
    path = "/filter-options",
    tag = "dispatch-jobs",
    operation_id = "getApiDispatchJobsFilterOptions",
    responses(
        (status = 200, description = "Filter options", body = DispatchJobFilterOptionsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_options(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
) -> Result<Json<DispatchJobFilterOptionsResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs(&auth.0)?;

    let repo = &state.dispatch_job_repo;
    let (statuses, codes, client_ids, dispatch_pool_ids, subscription_ids, kinds) = tokio::try_join!(
        repo.distinct_read_values("status"),
        repo.distinct_read_values("code"),
        repo.distinct_read_values("client_id"),
        repo.distinct_read_values("dispatch_pool_id"),
        repo.distinct_read_values("subscription_id"),
        repo.distinct_read_values("kind"),
    )?;

    Ok(Json(DispatchJobFilterOptionsResponse {
        statuses,
        codes,
        client_ids,
        dispatch_pool_ids,
        subscription_ids,
        kinds,
    }))
}

// ============================================================================
// Raw Endpoint
// ============================================================================

/// Get raw dispatch job data by ID
///
/// Returns the full DispatchJob entity serialized directly as JSON (not the DTO).
#[utoipa::path(
    get,
    path = "/{id}/raw",
    tag = "dispatch-jobs",
    operation_id = "getApiDispatchJobsByIdRaw",
    params(
        ("id" = String, Path, description = "Dispatch job ID")
    ),
    responses(
        (status = 200, description = "Raw dispatch job data", body = DispatchJobResponse),
        (status = 404, description = "Dispatch job not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dispatch_job_raw(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs_raw(&auth.0)?;

    let job = state
        .dispatch_job_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("DispatchJob", &id))?;
    check_job_scope(&auth, job.client_id.as_deref())?;

    // Go's `getRaw` answers the same shape as `getByID`.
    Ok(Json(job.into()))
}

/// Paginated dispatch jobs response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedDispatchJobsResponse {
    pub items: Vec<DispatchJobResponse>,
    pub page: u32,
    pub size: u32,
}

/// `GET /raw`: Go's SDK alias of `/list-raw` (the filtered list of the read
/// projection, gated on `dispatch-job:view-raw`).
#[utoipa::path(
    get,
    path = "/raw",
    tag = "dispatch-jobs",
    operation_id = "getApiDispatchJobsRaw",
    params(DispatchJobsQuery),
    responses(
        (status = 200, description = "Dispatch jobs", body = Vec<DispatchJobReadResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_dispatch_jobs_raw(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Query(query): Query<DispatchJobsQuery>,
) -> Result<Json<Vec<DispatchJobReadResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs_raw(&auth.0)?;
    list_dispatch_jobs(State(state), auth, Query(query)).await
}

/// Create dispatch jobs router for the BFF tier (`/bff/dispatch-jobs`).
/// Cookie-auth, used by the SPA. Includes `batch_create_dispatch_jobs` —
/// the SPA-facing batch.
pub fn dispatch_jobs_router(state: DispatchJobsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_dispatch_jobs, create_dispatch_job))
        .routes(routes!(batch_create_dispatch_jobs))
        .routes(routes!(get_filter_options))
        .routes(routes!(list_dispatch_jobs_raw))
        .routes(routes!(get_dispatch_job))
        .routes(routes!(get_dispatch_job_raw))
        .routes(routes!(get_dispatch_job_attempts))
        .routes(routes!(get_jobs_for_event))
        .with_state(state)
}

/// Create dispatch jobs router for the API tier (`/api/dispatch-jobs`).
/// Bearer-auth, used by SDK consumers. **No `batch_create_dispatch_jobs`**
/// — SDK callers use `sdk_dispatch_jobs_batch_router::POST /batch` (the
/// high-volume bulk-insert path). The two routers must not both register
/// `POST /batch` at the same prefix (axum panics on overlap).
pub fn dispatch_jobs_api_router(state: DispatchJobsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_dispatch_jobs, create_dispatch_job))
        .routes(routes!(get_filter_options))
        .routes(routes!(list_dispatch_jobs_raw))
        .routes(routes!(get_dispatch_job))
        .routes(routes!(get_dispatch_job_raw))
        .routes(routes!(get_dispatch_job_attempts))
        .routes(routes!(get_jobs_for_event))
        .with_state(state)
}
