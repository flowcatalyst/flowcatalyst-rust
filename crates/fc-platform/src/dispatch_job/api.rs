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

use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::DispatchJobRepository;
use crate::{
    DispatchAttempt, DispatchJob, DispatchJobRead, DispatchKind, DispatchMetadata, DispatchMode,
    DispatchStatus, RetryStrategy,
};

/// Dispatch job response DTO (matches Java DispatchJobReadResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobResponse {
    pub id: String,
    pub external_id: Option<String>,
    pub source: Option<String>,
    pub kind: String,
    pub code: String,
    pub subject: Option<String>,
    pub event_id: Option<String>,
    pub correlation_id: Option<String>,
    pub target_url: String,
    pub protocol: String,
    pub client_id: Option<String>,
    pub subscription_id: Option<String>,
    pub service_account_id: Option<String>,
    pub dispatch_pool_id: Option<String>,
    pub message_group: Option<String>,
    pub mode: String,
    pub sequence: i32,
    pub status: String,
    pub attempt_count: u32,
    pub max_retries: u32,
    pub last_error: Option<String>,
    pub timeout_seconds: u32,
    pub retry_strategy: String,
    pub created_at: String,
    pub updated_at: String,
    pub scheduled_for: Option<String>,
    pub expires_at: Option<String>,
    pub completed_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub duration_millis: Option<i64>,
    pub idempotency_key: Option<String>,
    pub is_completed: bool,
    pub is_terminal: bool,
}

impl From<DispatchJob> for DispatchJobResponse {
    fn from(job: DispatchJob) -> Self {
        Self {
            id: job.id,
            external_id: job.external_id,
            source: job.source,
            kind: format!("{:?}", job.kind).to_uppercase(),
            code: job.code,
            subject: job.subject,
            event_id: job.event_id,
            correlation_id: job.correlation_id,
            target_url: job.target_url,
            protocol: format!("{:?}", job.protocol).to_uppercase(),
            client_id: job.client_id,
            subscription_id: job.subscription_id,
            service_account_id: job.service_account_id,
            dispatch_pool_id: job.dispatch_pool_id,
            message_group: job.message_group,
            mode: format!("{:?}", job.mode).to_uppercase(),
            sequence: job.sequence,
            status: format!("{:?}", job.status).to_uppercase(),
            attempt_count: job.attempt_count,
            max_retries: job.max_retries,
            last_error: job.last_error,
            timeout_seconds: job.timeout_seconds,
            retry_strategy: format!("{:?}", job.retry_strategy).to_uppercase(),
            created_at: job.created_at.to_rfc3339(),
            updated_at: job.updated_at.to_rfc3339(),
            scheduled_for: job.scheduled_for.map(|t| t.to_rfc3339()),
            expires_at: job.expires_at.map(|t| t.to_rfc3339()),
            completed_at: job.completed_at.map(|t| t.to_rfc3339()),
            last_attempt_at: job.last_attempt_at.map(|t| t.to_rfc3339()),
            duration_millis: job.duration_millis,
            idempotency_key: job.idempotency_key,
            is_completed: job.status == DispatchStatus::Completed,
            is_terminal: job.status.is_terminal(),
        }
    }
}

/// Dispatch job read projection response (matches Java DispatchJobReadResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobReadResponse {
    pub id: String,
    pub external_id: Option<String>,
    pub source: Option<String>,
    pub kind: String,
    pub code: String,
    pub subject: Option<String>,
    pub event_id: Option<String>,
    pub correlation_id: Option<String>,
    pub target_url: String,
    pub protocol: String,
    pub client_id: Option<String>,
    pub subscription_id: Option<String>,
    pub service_account_id: Option<String>,
    pub dispatch_pool_id: Option<String>,
    pub message_group: Option<String>,
    pub mode: String,
    pub sequence: i32,
    pub status: String,
    pub attempt_count: u32,
    pub max_retries: u32,
    pub last_error: Option<String>,
    pub timeout_seconds: u32,
    pub retry_strategy: String,
    pub created_at: String,
    pub updated_at: String,
    pub scheduled_for: Option<String>,
    pub expires_at: Option<String>,
    pub completed_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub duration_millis: Option<i64>,
    pub idempotency_key: Option<String>,
    pub is_completed: bool,
    pub is_terminal: bool,
    pub projected_at: Option<String>,
    pub application: Option<String>,
    pub subdomain: Option<String>,
    pub aggregate: Option<String>,
}

impl From<DispatchJobRead> for DispatchJobReadResponse {
    fn from(job: DispatchJobRead) -> Self {
        Self {
            id: job.id,
            external_id: job.external_id,
            source: job.source,
            kind: format!("{:?}", job.kind).to_uppercase(),
            code: job.code,
            subject: job.subject,
            event_id: job.event_id,
            correlation_id: job.correlation_id,
            target_url: job.target_url,
            protocol: format!("{:?}", job.protocol).to_uppercase(),
            client_id: job.client_id,
            subscription_id: job.subscription_id,
            service_account_id: job.service_account_id,
            dispatch_pool_id: job.dispatch_pool_id,
            message_group: job.message_group,
            mode: format!("{:?}", job.mode).to_uppercase(),
            sequence: job.sequence,
            status: format!("{:?}", job.status).to_uppercase(),
            attempt_count: job.attempt_count,
            max_retries: job.max_retries,
            last_error: job.last_error,
            timeout_seconds: job.timeout_seconds,
            retry_strategy: format!("{:?}", job.retry_strategy).to_uppercase(),
            created_at: job.created_at.to_rfc3339(),
            updated_at: job.updated_at.to_rfc3339(),
            scheduled_for: job.scheduled_for.map(|t| t.to_rfc3339()),
            expires_at: job.expires_at.map(|t| t.to_rfc3339()),
            completed_at: job.completed_at.map(|t| t.to_rfc3339()),
            last_attempt_at: job.last_attempt_at.map(|t| t.to_rfc3339()),
            duration_millis: job.duration_millis,
            idempotency_key: job.idempotency_key,
            is_completed: job.is_completed,
            is_terminal: job.is_terminal,
            projected_at: job.projected_at.map(|t| t.to_rfc3339()),
            application: job.application,
            subdomain: job.subdomain,
            aggregate: job.aggregate,
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
}

// ============================================================================
// Create Dispatch Job Request & Response
// ============================================================================

/// Request to create a new dispatch job
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDispatchJobRequest {
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

    /// Service account for authentication
    pub service_account_id: String,

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

/// Response for create dispatch job (matches Java DispatchJobResponse)
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

/// Dispatch attempt response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchAttemptResponse {
    pub attempt_number: u32,
    pub attempted_at: String,
    pub completed_at: Option<String>,
    pub duration_millis: Option<i64>,
    pub response_code: Option<u16>,
    pub response_body: Option<String>,
    pub success: bool,
    pub error_message: Option<String>,
    pub error_type: Option<String>,
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
            error_type: a.error_type.map(|t| format!("{:?}", t).to_uppercase()),
        }
    }
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

    // Check client access
    if let Some(ref cid) = job.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this dispatch job"));
        }
    }

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

    let mut client_ids = split_csv(query.client_ids.as_deref());
    let statuses = split_csv(query.statuses.as_deref());
    let applications = split_csv(query.applications.as_deref());
    let subdomains = split_csv(query.subdomains.as_deref());
    let aggregates = split_csv(query.aggregates.as_deref());
    let codes = split_csv(query.codes.as_deref());

    if !client_ids.is_empty() {
        for cid in &client_ids {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden(format!(
                    "No access to client: {}",
                    cid
                )));
            }
        }
    } else if !auth.0.is_anchor() {
        client_ids = auth
            .0
            .accessible_clients
            .iter()
            .filter(|c| c.as_str() != "*")
            .cloned()
            .collect();
        if client_ids.is_empty() {
            return Ok(Json(vec![]));
        }
    }

    for status_str in &statuses {
        match status_str.to_uppercase().as_str() {
            "PENDING" | "QUEUED" | "PROCESSING" | "COMPLETED" | "FAILED" | "CANCELLED"
            | "EXPIRED" => {}
            _ => {
                return Err(PlatformError::validation(format!(
                    "Invalid status: {}",
                    status_str
                )))
            }
        }
    }

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
    path = "/by-event/{event_id}",
    tag = "dispatch-jobs",
    operation_id = "getApiDispatchJobsByEventByEventId",
    params(
        ("event_id" = String, Path, description = "Event ID")
    ),
    responses(
        (status = 200, description = "Dispatch jobs for event", body = Vec<DispatchJobResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_jobs_for_event(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Path(event_id): Path<String>,
) -> Result<Json<Vec<DispatchJobResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs(&auth.0)?;

    let jobs = state.dispatch_job_repo.find_by_event_id(&event_id).await?;

    // Filter by client access
    let filtered: Vec<DispatchJobResponse> = jobs
        .into_iter()
        .filter(|j| match &j.client_id {
            Some(cid) => auth.0.can_access_client(cid),
            None => auth.0.is_anchor(),
        })
        .map(|j| j.into())
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
    crate::shared::authorization_service::checks::can_create_dispatch_jobs(&auth.0)?;

    // Validate client access if specified
    if let Some(ref cid) = req.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden(format!(
                "No access to client: {}",
                cid
            )));
        }
    }

    // Determine kind
    let kind = match req.kind.as_deref() {
        Some("TASK") => DispatchKind::Task,
        _ => DispatchKind::Event,
    };

    // Determine mode
    let mode = match req.mode.as_deref() {
        Some("NEXT_ON_ERROR") => DispatchMode::NextOnError,
        Some("BLOCK_ON_ERROR") => DispatchMode::BlockOnError,
        _ => DispatchMode::Immediate,
    };

    // Determine retry strategy
    let retry_strategy = match req.retry_strategy.as_deref() {
        Some("IMMEDIATE") => RetryStrategy::Immediate,
        Some("FIXED_DELAY") => RetryStrategy::FixedDelay,
        _ => RetryStrategy::ExponentialBackoff,
    };

    // Create the dispatch job
    let _now = chrono::Utc::now();
    let source = req.source.as_deref().unwrap_or("");
    let mut job = if kind == DispatchKind::Event {
        DispatchJob::for_event(
            req.event_id.as_deref().unwrap_or(""),
            &req.code,
            source,
            &req.target_url,
            &req.payload,
        )
    } else {
        DispatchJob::for_task(&req.code, source, &req.target_url, &req.payload)
    };

    // Apply optional fields
    if let Some(subject) = req.subject {
        job.subject = Some(subject);
    }
    if let Some(correlation_id) = req.correlation_id {
        job.correlation_id = Some(correlation_id);
    }
    if let Some(client_id) = req.client_id {
        job.client_id = Some(client_id);
    }
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

    job.service_account_id = Some(req.service_account_id);
    job.mode = mode;
    job.retry_strategy = retry_strategy;
    job.data_only = req.data_only;

    // Add metadata
    for (key, value) in req.metadata {
        job.metadata.push(DispatchMetadata { key, value });
    }

    // Mark as queued
    job.mark_queued();

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
    crate::shared::authorization_service::checks::can_create_dispatch_jobs(&auth.0)?;

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

    for job_req in req.jobs {
        // Validate client access if specified
        if let Some(ref cid) = job_req.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden(format!(
                    "No access to client: {}",
                    cid
                )));
            }
        }

        // Determine kind
        let kind = match job_req.kind.as_deref() {
            Some("TASK") => DispatchKind::Task,
            _ => DispatchKind::Event,
        };

        // Determine mode
        let mode = match job_req.mode.as_deref() {
            Some("NEXT_ON_ERROR") => DispatchMode::NextOnError,
            Some("BLOCK_ON_ERROR") => DispatchMode::BlockOnError,
            _ => DispatchMode::Immediate,
        };

        // Create the dispatch job
        let source = job_req.source.as_deref().unwrap_or("");
        let mut job = if kind == DispatchKind::Event {
            DispatchJob::for_event(
                job_req.event_id.as_deref().unwrap_or(""),
                &job_req.code,
                source,
                &job_req.target_url,
                &job_req.payload,
            )
        } else {
            DispatchJob::for_task(&job_req.code, source, &job_req.target_url, &job_req.payload)
        };

        // Apply optional fields
        if let Some(subject) = job_req.subject {
            job.subject = Some(subject);
        }
        if let Some(correlation_id) = job_req.correlation_id {
            job.correlation_id = Some(correlation_id);
        }
        if let Some(client_id) = job_req.client_id {
            job.client_id = Some(client_id);
        }
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

        job.service_account_id = Some(job_req.service_account_id);
        job.mode = mode;
        job.data_only = job_req.data_only;
        job.mark_queued();

        created_jobs.push(job);
    }

    // Bulk insert
    state.dispatch_job_repo.insert_many(&created_jobs).await?;

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

    // Check client access
    if let Some(ref cid) = job.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this dispatch job"));
        }
    }

    let attempts: Vec<DispatchAttemptResponse> = job.attempts.into_iter().map(Into::into).collect();
    Ok(Json(attempts))
}

// ============================================================================
// Filter Options Endpoint
// ============================================================================

/// A filter option with value and label (matches TS FilterOption)
#[derive(Debug, Serialize, ToSchema)]
pub struct FilterOption {
    pub value: String,
    pub label: String,
}

impl FilterOption {
    fn from_value(v: String) -> Self {
        Self {
            label: v.clone(),
            value: v,
        }
    }
}

/// Filter options for dispatch jobs dropdowns — cascading filter support.
/// Matches the TS version: queries distinct values from the read projection.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobFilterOptionsResponse {
    pub clients: Vec<FilterOption>,
    pub applications: Vec<FilterOption>,
    pub subdomains: Vec<FilterOption>,
    pub aggregates: Vec<FilterOption>,
    pub codes: Vec<FilterOption>,
    pub statuses: Vec<FilterOption>,
}

/// Get filter options for dispatch jobs
///
/// Returns distinct values from the read projection for cascading filter dropdowns.
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

    // Query distinct values from the read projection table
    let (clients, applications, subdomains, aggregates, codes, statuses) = tokio::try_join!(
        state.dispatch_job_repo.find_distinct_client_ids(),
        state.dispatch_job_repo.find_distinct_applications(),
        state.dispatch_job_repo.find_distinct_subdomains(),
        state.dispatch_job_repo.find_distinct_aggregates(),
        state.dispatch_job_repo.find_distinct_codes(),
        state.dispatch_job_repo.find_distinct_statuses(),
    )?;

    Ok(Json(DispatchJobFilterOptionsResponse {
        clients: clients.into_iter().map(FilterOption::from_value).collect(),
        applications: applications
            .into_iter()
            .map(FilterOption::from_value)
            .collect(),
        subdomains: subdomains
            .into_iter()
            .map(FilterOption::from_value)
            .collect(),
        aggregates: aggregates
            .into_iter()
            .map(FilterOption::from_value)
            .collect(),
        codes: codes.into_iter().map(FilterOption::from_value).collect(),
        statuses: statuses.into_iter().map(FilterOption::from_value).collect(),
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
        (status = 200, description = "Raw dispatch job data"),
        (status = 404, description = "Dispatch job not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dispatch_job_raw(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DispatchJob>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs_raw(&auth.0)?;

    let job = state
        .dispatch_job_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("DispatchJob", &id))?;

    // Check client access
    if let Some(ref cid) = job.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this dispatch job"));
        }
    }

    Ok(Json(job))
}

/// Paginated dispatch jobs response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedDispatchJobsResponse {
    pub items: Vec<DispatchJobResponse>,
    pub page: u32,
    pub size: u32,
}

/// Query for raw dispatch jobs list — `?size=` only.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct RawDispatchJobsQuery {
    /// Result size. Default 50, capped at 1000.
    pub size: Option<u32>,
}

/// List raw dispatch jobs (from msg_dispatch_jobs, not the read projection).
/// Returns the most recent rows; no pagination — msg_dispatch_jobs ingests
/// at high rates and page navigation is meaningless.
#[utoipa::path(
    get,
    path = "/raw",
    tag = "dispatch-jobs",
    operation_id = "getApiDispatchJobsRaw",
    params(RawDispatchJobsQuery),
    responses(
        (status = 200, description = "Raw dispatch jobs", body = Vec<DispatchJobResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_dispatch_jobs_raw(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Query(params): Query<RawDispatchJobsQuery>,
) -> Result<Json<Vec<DispatchJobResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs(&auth.0)?;

    let size = params.size.unwrap_or(50).clamp(1, 1000) as i64;
    let jobs = state
        .dispatch_job_repo
        .find_recent_with_cursor(None, size)
        .await?;
    let items = jobs.into_iter().map(DispatchJobResponse::from).collect();
    Ok(Json(items))
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
