//! Dispatch Jobs Batch API
//!
//! Exposes dispatch job batch creation at `/api/dispatch-jobs/batch`.

use axum::{
    routing::post,
    extract::State,
    Json, Router,
};
use std::sync::Arc;

use crate::dispatch_job::api::{
    BatchCreateDispatchJobsRequest, BatchCreateDispatchJobsResponse,
    DispatchJobResponse,
};
use crate::{
    DispatchJob, DispatchJobRepository, DispatchKind, DispatchMode,
    DispatchMetadata, RetryStrategy,
};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Clone)]
pub struct SdkDispatchJobsState {
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
}

async fn sdk_batch_create_dispatch_jobs(
    State(state): State<SdkDispatchJobsState>,
    auth: Authenticated,
    Json(req): Json<BatchCreateDispatchJobsRequest>,
) -> Result<Json<BatchCreateDispatchJobsResponse>, PlatformError> {
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

        let kind = match job_req.kind.as_deref() {
            Some("TASK") => DispatchKind::Task,
            _ => DispatchKind::Event,
        };

        let mode = match job_req.mode.as_deref() {
            Some("NEXT_ON_ERROR") => DispatchMode::NextOnError,
            Some("BLOCK_ON_ERROR") => DispatchMode::BlockOnError,
            _ => DispatchMode::Immediate,
        };

        let retry_strategy = match job_req.retry_strategy.as_deref() {
            Some("IMMEDIATE") => RetryStrategy::Immediate,
            Some("FIXED_DELAY") => RetryStrategy::FixedDelay,
            _ => RetryStrategy::ExponentialBackoff,
        };

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
        if let Some(idempotency_key) = job_req.idempotency_key {
            job.idempotency_key = Some(idempotency_key);
        }
        if let Some(external_id) = job_req.external_id {
            job.external_id = Some(external_id);
        }
        if let Some(content_type) = job_req.payload_content_type {
            job.payload_content_type = content_type;
        }

        job.service_account_id = Some(job_req.service_account_id);
        job.mode = mode;
        job.retry_strategy = retry_strategy;
        job.data_only = job_req.data_only;

        for (key, value) in job_req.metadata {
            job.metadata.push(DispatchMetadata { key, value });
        }

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

pub fn sdk_dispatch_jobs_batch_router(state: SdkDispatchJobsState) -> Router {
    Router::new()
        .route("/batch", post(sdk_batch_create_dispatch_jobs))
        .with_state(state)
}
