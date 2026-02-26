//! Dispatch Jobs BFF API
//!
//! REST endpoints for managing dispatch jobs.

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    DispatchJob, DispatchJobRead, DispatchStatus, DispatchKind, DispatchMode,
    DispatchAttempt, RetryStrategy, DispatchMetadata,
};
use crate::DispatchJobRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::PaginationParams;
use crate::shared::middleware::Authenticated;

/// Dispatch job response DTO (matches Java DispatchJobReadResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobResponse {
    pub id: String,
    pub external_id: Option<String>,
    pub source: String,
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
            scheduled_for: job.next_retry_at.map(|t| t.to_rfc3339()),
            expires_at: None, // Not tracked in Rust domain yet
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
    pub source: String,
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
        }
    }
}

/// Query parameters for dispatch jobs list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DispatchJobsQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by event ID
    pub event_id: Option<String>,

    /// Filter by correlation ID
    pub correlation_id: Option<String>,

    /// Filter by subscription ID
    pub subscription_id: Option<String>,

    /// Filter by client ID
    pub client_id: Option<String>,

    /// Filter by status
    pub status: Option<String>,
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
    pub source: String,

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
    operation_id = "getApiBffDispatchJobsById",
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

    let job = state.dispatch_job_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("DispatchJob", &id))?;

    // Check client access
    if let Some(ref cid) = job.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this dispatch job"));
        }
    }

    Ok(Json(job.into()))
}

/// List dispatch jobs
#[utoipa::path(
    get,
    path = "",
    tag = "dispatch-jobs",
    operation_id = "getApiBffDispatchJobs",
    params(DispatchJobsQuery),
    responses(
        (status = 200, description = "List of dispatch jobs", body = Vec<DispatchJobResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_dispatch_jobs(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Query(query): Query<DispatchJobsQuery>,
) -> Result<Json<Vec<DispatchJobResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_dispatch_jobs(&auth.0)?;

    let jobs = if let Some(ref event_id) = query.event_id {
        state.dispatch_job_repo.find_by_event_id(event_id).await?
    } else if let Some(ref corr_id) = query.correlation_id {
        state.dispatch_job_repo.find_by_correlation_id(corr_id).await?
    } else if let Some(ref sub_id) = query.subscription_id {
        state.dispatch_job_repo.find_by_subscription_id(sub_id, query.pagination.size() as i64).await?
    } else if let Some(ref client_id) = query.client_id {
        if !auth.0.can_access_client(client_id) {
            return Err(PlatformError::forbidden(format!("No access to client: {}", client_id)));
        }
        state.dispatch_job_repo.find_by_client(client_id, query.pagination.size() as i64).await?
    } else if let Some(ref status_str) = query.status {
        let status = match status_str.to_uppercase().as_str() {
            "PENDING" => DispatchStatus::Pending,
            "QUEUED" => DispatchStatus::Queued,
            "IN_PROGRESS" => DispatchStatus::InProgress,
            "COMPLETED" => DispatchStatus::Completed,
            "FAILED" => DispatchStatus::Failed,
            "EXPIRED" => DispatchStatus::Expired,
            _ => return Err(PlatformError::validation(format!("Invalid status: {}", status_str))),
        };
        state.dispatch_job_repo.find_by_status(status, query.pagination.size() as i64).await?
    } else {
        // Return empty for now - need proper listing
        vec![]
    };

    // Filter by client access
    let filtered: Vec<DispatchJobResponse> = jobs.into_iter()
        .filter(|j| {
            match &j.client_id {
                Some(cid) => auth.0.can_access_client(cid),
                None => auth.0.is_anchor(),
            }
        })
        .map(|j| j.into())
        .collect();

    Ok(Json(filtered))
}

/// Get dispatch jobs for an event
#[utoipa::path(
    get,
    path = "/by-event/{event_id}",
    tag = "dispatch-jobs",
    operation_id = "getApiBffDispatchJobsByEventByEventId",
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
    let filtered: Vec<DispatchJobResponse> = jobs.into_iter()
        .filter(|j| {
            match &j.client_id {
                Some(cid) => auth.0.can_access_client(cid),
                None => auth.0.is_anchor(),
            }
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
    operation_id = "postApiBffDispatchJobs",
    request_body = CreateDispatchJobRequest,
    responses(
        (status = 201, description = "Dispatch job created", body = DispatchJobResponse),
        (status = 400, description = "Invalid request"),
        (status = 403, description = "No access to client")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_dispatch_job(
    State(state): State<DispatchJobsState>,
    auth: Authenticated,
    Json(req): Json<CreateDispatchJobRequest>,
) -> Result<(axum::http::StatusCode, Json<DispatchJobResponse>), PlatformError> {
    crate::shared::authorization_service::checks::can_create_dispatch_jobs(&auth.0)?;

    // Validate client access if specified
    if let Some(ref cid) = req.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden(format!("No access to client: {}", cid)));
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
    let mut job = if kind == DispatchKind::Event {
        DispatchJob::for_event(
            req.event_id.as_deref().unwrap_or(""),
            &req.code,
            &req.source,
            &req.target_url,
            &req.payload,
        )
    } else {
        DispatchJob::for_task(&req.code, &req.source, &req.target_url, &req.payload)
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
    state.dispatch_job_repo.insert(&job).await?;

    Ok((axum::http::StatusCode::CREATED, Json(job.into())))
}

/// Create multiple dispatch jobs in batch
///
/// Creates multiple dispatch jobs in a single operation. Maximum batch size is 100 jobs.
#[utoipa::path(
    post,
    path = "/batch",
    tag = "dispatch-jobs",
    operation_id = "postApiBffDispatchJobsBatch",
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
        return Err(PlatformError::validation("Request body must contain at least one dispatch job"));
    }
    if req.jobs.len() > 100 {
        return Err(PlatformError::validation("Batch size cannot exceed 100 dispatch jobs"));
    }

    let mut created_jobs: Vec<DispatchJob> = Vec::new();

    for job_req in req.jobs {
        // Validate client access if specified
        if let Some(ref cid) = job_req.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden(format!("No access to client: {}", cid)));
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
        let mut job = if kind == DispatchKind::Event {
            DispatchJob::for_event(
                job_req.event_id.as_deref().unwrap_or(""),
                &job_req.code,
                &job_req.source,
                &job_req.target_url,
                &job_req.payload,
            )
        } else {
            DispatchJob::for_task(&job_req.code, &job_req.source, &job_req.target_url, &job_req.payload)
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
    let job_responses: Vec<DispatchJobResponse> = created_jobs.into_iter().map(Into::into).collect();

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
    operation_id = "getApiBffDispatchJobsByIdAttempts",
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

    let job = state.dispatch_job_repo.find_by_id(&id).await?
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

/// Create dispatch jobs router
pub fn dispatch_jobs_router(state: DispatchJobsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_dispatch_jobs, create_dispatch_job))
        .routes(routes!(batch_create_dispatch_jobs))
        .routes(routes!(get_dispatch_job))
        .routes(routes!(get_dispatch_job_attempts))
        .routes(routes!(get_jobs_for_event))
        .with_state(state)
}
