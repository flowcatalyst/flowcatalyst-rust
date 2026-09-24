//! Dispatch Jobs Batch API
//!
//! Exposes dispatch job batch creation at `/api/dispatch-jobs/batch`.

use axum::{
    extract::{DefaultBodyLimit, State},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::dispatch_job::api::CreateDispatchJobRequest;
use crate::dispatch_job::entity::parse_dispatch_mode;
use crate::shared::batch_api::{BatchResponse, BatchResultItem};
use crate::shared::enum_str::{non_empty, parse_opt};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::{DispatchJob, DispatchJobRepository, DispatchKind, DispatchMetadata, RetryStrategy};

#[derive(Clone)]
pub struct SdkDispatchJobsState {
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
}

/// SDK batch dispatch-jobs request. The wrapper key is `items` (1:1 with the
/// outbox dispatcher `BatchRequest{items}` and the events/audit batch
/// endpoints); each item is a `CreateDispatchJobRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SdkBatchDispatchJobsRequest {
    pub items: Vec<CreateDispatchJobRequest>,
}

async fn sdk_batch_create_dispatch_jobs(
    State(state): State<SdkDispatchJobsState>,
    auth: Authenticated,
    Json(req): Json<SdkBatchDispatchJobsRequest>,
) -> Result<Json<BatchResponse>, PlatformError> {
    // Validate batch size
    if req.items.is_empty() {
        return Err(PlatformError::validation(
            "Request body must contain at least one dispatch job",
        ));
    }
    if req.items.len() > 1000 {
        return Err(PlatformError::validation(
            "Batch size cannot exceed 1000 dispatch jobs",
        ));
    }

    let mut created_jobs: Vec<DispatchJob> = Vec::new();

    for job_req in req.items {
        // Validate client access if specified
        if let Some(ref cid) = job_req.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden(format!(
                    "No access to client: {}",
                    cid
                )));
            }
        }

        // Absent/empty means EVENT; anything else must be an exact kind (400).
        let kind: DispatchKind = parse_opt(non_empty(job_req.kind.as_deref()))?.unwrap_or_default();

        let mode = parse_dispatch_mode(job_req.mode.as_deref());

        // Absent/empty means exponential; anything else must be a known strategy (400).
        let retry_strategy: RetryStrategy =
            parse_opt(non_empty(job_req.retry_strategy.as_deref()))?.unwrap_or_default();

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

    // Per-item result list — 1:1 with the outbox/SDK contract
    // {results:[{id,status,error?}]}. Insert is all-or-nothing, so every
    // persisted job reports SUCCESS.
    let results: Vec<BatchResultItem> = created_jobs
        .iter()
        .map(|job| BatchResultItem {
            id: job.id.clone(),
            status: "SUCCESS".to_string(),
        })
        .collect();

    Ok(Json(BatchResponse { results }))
}

pub fn sdk_dispatch_jobs_batch_router(state: SdkDispatchJobsState) -> Router {
    Router::new()
        .route("/batch", post(sdk_batch_create_dispatch_jobs))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}
