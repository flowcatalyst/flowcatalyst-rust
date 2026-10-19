//! Operator routes on dispatch jobs, on the `/api` and `/bff` tiers alike
//! (Go `dispatchjob/api/api.go`):
//!
//! - `POST …/dispatch-jobs/requeue` `{ids, redriveCompleted?, confirmRedriveCompleted?}`
//!   → `{requeued, redriven, skippedInFlight, skippedCompleted, skipped[]}`
//!   (`dispatch-job:requeue`; `dispatch-job:redrive-completed` as well when
//!   `redriveCompleted`)
//! - `POST …/dispatch-jobs/{id}/cancel`   → the job (`dispatch-job:cancel`; FAILED only)
//! - `POST …/dispatch-jobs/{id}/complete` → the job (`dispatch-job:complete`; FAILED only)
//! - `PUT …/dispatch-jobs/{id}/payload`   → the job (`dispatch-job:edit-payload`)
//! - `POST …/dispatch-jobs/{id}/sign`     → the delivery a send would make now (view-raw)
//!
//! `dispatch-job:view` is read-only since Go's split of the write actions off
//! it (`ad37de21`).

use axum::{
    extract::{Path, State},
    Json,
};
use fc_platform_core::shared::id::ClientId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use utoipa::ToSchema;

use super::operations::{
    EditDispatchJobPayloadUseCase, EditPayloadCommand, RequeueDispatchJobsUseCase, ResendCommand,
    SettleDispatchJobUseCase, SkippedJob, StatusFlipCommand,
};
use super::repository::{DispatchJobActionsRepository, JobHead};
use crate::dispatch_job::api::DispatchJobResponse;
use crate::dispatch_job::delivery_credentials::DeliveryCredentials;
use crate::dispatch_job::entity::{DispatchJob, DispatchStatus};
use crate::dispatch_job::repository::DispatchJobRepository;
use crate::dispatch_job::requeue_rules::SKIP_IN_FLIGHT;
use fc_platform_core::directory::ClientDirectory;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::shared::webhook_signer;
use fc_platform_core::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use std::slice;

#[derive(Clone)]
pub struct DispatchJobActionsState {
    pub repo: Arc<DispatchJobActionsRepository>,
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
    pub client_repo: Arc<dyn ClientDirectory>,
    pub credentials: Arc<DeliveryCredentials>,
    pub requeue_use_case: Arc<RequeueDispatchJobsUseCase<PgUnitOfWork>>,
    pub settle_use_case: Arc<SettleDispatchJobUseCase<PgUnitOfWork>>,
    pub edit_payload_use_case: Arc<EditDispatchJobPayloadUseCase<PgUnitOfWork>>,
    /// The widest `created_at` window a requeue-by-filter may ask for
    /// (`FC_DISPATCH_JOB_MAX_WINDOW_DAYS`); below 1 means the default 31.
    pub max_window_days: i64,
}

/// Go `RequeueRequest`: the body of `POST /dispatch-jobs/requeue`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequeueRequest {
    /// Dispatch job ids to reset to PENDING for re-dispatch. QUEUED and PROCESSING jobs are never reset; COMPLETED jobs only with redriveCompleted and confirmRedriveCompleted. Skipped jobs are reported, not an error.
    #[serde(default)]
    #[schema(required = true)]
    pub ids: Vec<String>,
    /// Also re-deliver COMPLETED jobs. Needs confirmRedriveCompleted=REDRIVE and the dispatch-job:redrive-completed permission (403 without it).
    #[serde(default)]
    pub redrive_completed: bool,
    /// The literal REDRIVE, confirming redriveCompleted. Without both fields, COMPLETED jobs are skipped.
    #[serde(default)]
    pub confirm_redrive_completed: String,
}

/// Go `SkippedJobResponse`: a job a requeue found and left alone.
#[derive(Debug, Serialize, ToSchema)]
pub struct SkippedJobResponse {
    pub id: String,
    pub status: String,
    /// IN_FLIGHT: QUEUED or PROCESSING, never reset. COMPLETED_REDRIVE_NOT_CONFIRMED: completed jobs are redriven only with redriveCompleted and confirmRedriveCompleted.
    #[schema(inline, example = "IN_FLIGHT")]
    pub reason: SkipReason,
}

/// Why a requeue left a job alone.
#[derive(Debug, Serialize, ToSchema)]
pub enum SkipReason {
    #[serde(rename = "IN_FLIGHT")]
    InFlight,
    #[serde(rename = "COMPLETED_REDRIVE_NOT_CONFIRMED")]
    CompletedRedriveNotConfirmed,
}

/// Go `RequeueResponse`: how many jobs were reset.
#[derive(Debug, Default, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequeueResponse {
    /// Jobs reset to PENDING (redriven COMPLETED jobs included)
    #[schema(value_type = i64)]
    pub requeued: usize,
    /// How many of them were COMPLETED jobs re-delivered (0 unless the request redrove)
    #[schema(value_type = i64)]
    pub redriven: usize,
    /// QUEUED or PROCESSING jobs left alone
    #[schema(value_type = i64)]
    pub skipped_in_flight: usize,
    /// COMPLETED jobs left alone because the redrive was not requested and confirmed
    #[schema(value_type = i64)]
    pub skipped_completed: usize,
    /// The skipped jobs, with the reason
    pub skipped: Vec<SkippedJobResponse>,
}

impl From<SkippedJob> for SkippedJobResponse {
    fn from(j: SkippedJob) -> Self {
        Self {
            id: j.id,
            status: j.status,
            reason: if j.reason == SKIP_IN_FLIGHT {
                SkipReason::InFlight
            } else {
                SkipReason::CompletedRedriveNotConfirmed
            },
        }
    }
}

/// Go `EditPayloadRequest`: the body of `PUT /dispatch-jobs/{id}/payload`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditPayloadRequest {
    /// The new payload text (UTF-8, at most 256 KiB; valid JSON when the content type is JSON)
    #[schema(min_length = 1)]
    pub payload: String,
    /// New media type; omit to keep the job's
    #[serde(default)]
    pub payload_content_type: Option<String>,
}

/// Go `RequestSummary`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_by: Option<String>,
    pub signature: bool,
    pub bearer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsigned_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Go `DeliveryPlan`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DeliveryPlan {
    pub request: RequestSummary,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

/// Go `requireRequeue`: gates requeue by ids and by filter. A request that
/// asks to redrive COMPLETED jobs additionally needs
/// `dispatch-job:redrive-completed`; asking without it is a 403, not a quiet skip.
pub(crate) fn can_requeue_dispatch_jobs_with(
    auth: &Authenticated,
    redrive_completed: bool,
) -> Result<(), PlatformError> {
    checks::can_requeue_dispatch_jobs(&auth.0)?;
    if redrive_completed {
        checks::can_redrive_completed_dispatch_jobs(&auth.0)?;
    }
    Ok(())
}

/// The reachable ones of `ids`, as a requeue command: unknown and unreachable
/// ids are dropped silently, as in Go (a bulk contract, and the ids the audit
/// row records).
pub(crate) async fn resend_command(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    ids: &[String],
    redrive_completed: bool,
    confirm_redrive_completed: &str,
) -> Result<ResendCommand, PlatformError> {
    let jobs: Vec<JobHead> = state
        .repo
        .heads(ids)
        .await?
        .into_iter()
        .filter(|j| super::operations::reaches(&auth.0, j.client_id.as_deref()))
        .collect();
    Ok(ResendCommand {
        ids: jobs.iter().map(|j| j.id.clone()).collect(),
        redrive_completed,
        confirm_redrive_completed: confirm_redrive_completed.to_string(),
        jobs,
    })
}

/// Go `requeue`: resets the given jobs to PENDING so the scheduler
/// re-dispatches them (clears scheduled_for, attempt_count and the terminal
/// stamps). A job in flight (QUEUED, PROCESSING) is never reset and a COMPLETED
/// job only by a confirmed redrive; the rest are skipped and reported.
async fn requeue(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    req: RequeueRequest,
) -> Result<Json<RequeueResponse>, PlatformError> {
    if req.ids.is_empty() {
        return Ok(Json(RequeueResponse::default()));
    }
    let command = resend_command(
        state,
        auth,
        &req.ids,
        req.redrive_completed,
        &req.confirm_redrive_completed,
    )
    .await?;
    let event = state
        .requeue_use_case
        .run(command, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(Json(RequeueResponse {
        requeued: event.ids.len(),
        redriven: event.redriven_count,
        skipped_in_flight: event.skipped_in_flight,
        skipped_completed: event.skipped_completed,
        skipped: event.skipped.iter().cloned().map(Into::into).collect(),
    }))
}

async fn settle(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    id: String,
    target: DispatchStatus,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    let head = state
        .repo
        .heads(slice::from_ref(&id))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| PlatformError::not_found_code("DispatchJob", &id))?;
    // The use case checks the caller's scope on the job (Go
    // `CheckScopeAccess`, post-load).
    state
        .settle_use_case
        .run(
            StatusFlipCommand {
                id: id.clone(),
                target,
                current_status: head.status,
                created_at: head.created_at,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    let job = state
        .dispatch_job_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("DispatchJob", &id))?;
    Ok(Json(job.into()))
}

/// Go `editPayload`: validate the body's shape (before the job is read, as Go's
/// `Validate` runs first), load the job (404), then run the use case (scope,
/// status rule, JSON check), and answer the reloaded job.
async fn edit_payload(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    id: String,
    req: EditPayloadRequest,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    let command = EditPayloadCommand {
        id: id.clone(),
        payload: req.payload,
        payload_content_type: req.payload_content_type,
    };
    command.validate_shape().map_err(PlatformError::from)?;
    if state.repo.edit_view(&id).await?.is_none() {
        return Err(PlatformError::not_found_code("DispatchJob", &id));
    }
    state
        .edit_payload_use_case
        .run(command, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    let job = state
        .dispatch_job_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("DispatchJob", &id))?;
    Ok(Json(job.into()))
}

/// Go `buildPayload`: the raw payload for a data-only job, else the
/// envelope with its keys in order (Go marshals a map).
fn delivery_body(job: &DispatchJob, client_code: Option<&str>) -> String {
    if job.data_only {
        return job.payload.clone().unwrap_or_else(|| "{}".to_string());
    }
    let mut m: BTreeMap<&str, serde_json::Value> = BTreeMap::new();
    m.insert("id", job.id.clone().into());
    m.insert("type", job.code.clone().into());
    m.insert("attemptNumber", (job.attempt_count + 1).into());
    let mut opt = |k: &'static str, v: &Option<String>| {
        if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
            m.insert(k, v.clone().into());
        }
    };
    opt("source", &job.source);
    opt("subject", &job.subject);
    opt("correlationId", &job.correlation_id);
    opt("messageGroup", &job.message_group);
    opt("clientId", &job.client_id);
    if job.client_id.is_some() {
        if let Some(code) = client_code {
            m.insert("clientCode", code.into());
        }
    }
    let data = match job.payload.as_deref() {
        Some(p) => serde_json::from_str(p).unwrap_or_else(|_| serde_json::Value::String(p.into())),
        None => serde_json::Value::Null,
    };
    m.insert("data", data);
    serde_json::to_string(&m).unwrap_or_default()
}

async fn sign(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    id: String,
) -> Result<Json<DeliveryPlan>, PlatformError> {
    let job = state
        .dispatch_job_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("DispatchJob", &id))?;
    if !super::operations::reaches(&auth.0, job.client_id.as_deref()) {
        return Err(super::operations::scope_forbidden(job.client_id.as_deref()).into());
    }
    let client_code = match job.client_id.as_deref() {
        Some(c) => state
            .client_repo
            .find_by_id(&ClientId::from_wire(c))
            .await
            .ok()
            .flatten()
            .map(|c| c.identifier),
        None => None,
    };
    let body = delivery_body(&job, client_code.as_deref());

    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    headers.insert("Content-Type".into(), "application/json".into());
    headers.insert("X-Dispatch-Job-Id".into(), job.id.clone());
    headers.insert("X-Event-Type".into(), job.code.clone());
    if let (Some(cid), Some(code)) = (job.client_id.as_deref(), client_code.as_deref()) {
        headers.insert("X-FlowCatalyst-Client".into(), format!("{cid}:{code}"));
    }
    let creds = state.credentials.resolve_or_bare(&job).await;
    let mut timestamp = None;
    if creds.bearer_token.is_some() {
        headers.insert("Authorization".into(), "Bearer ••••••".into());
    }
    if let Some(secret) = &creds.signing_secret {
        for (name, value) in
            webhook_signer::signature_headers(secret, chrono::Utc::now(), body.as_bytes())
        {
            if name == webhook_signer::TIMESTAMP_HEADER {
                timestamp = Some(value.clone());
            }
            headers.insert(name.to_string(), value);
        }
    }
    let unsigned_reason = creds.is_bare().then(|| {
        if creds.reason.is_empty() {
            "no credentials resolved".to_string()
        } else {
            creds.reason.clone()
        }
    });
    Ok(Json(DeliveryPlan {
        request: RequestSummary {
            signed_by: creds.signed_by.clone(),
            signature: creds.signing_secret.is_some(),
            bearer: creds.bearer_token.is_some(),
            timestamp,
            headers: headers.keys().cloned().collect(),
            unsigned_reason,
            target: Some(job.target_url.clone()).filter(|t| !t.is_empty()),
        },
        headers,
        body,
    }))
}

// ── /api ─────────────────────────────────────────────────────────────────

/// Requeue dispatch jobs (Go `requeueDispatchJobs`): a job that is not in
/// flight goes back to PENDING with a fresh attempt budget; COMPLETED jobs only
/// with `redriveCompleted` and `confirmRedriveCompleted: "REDRIVE"`.
#[utoipa::path(post, path = "/api/dispatch-jobs/requeue", tag = "dispatch-jobs",
    operation_id = "requeueDispatchJobs", request_body = RequeueRequest,
    responses((status = 200, description = "How many were requeued", body = RequeueResponse)),
    security(("bearer_auth" = [])))]
pub async fn api_requeue_dispatch_jobs(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Json(req): Json<RequeueRequest>,
) -> Result<Json<RequeueResponse>, PlatformError> {
    can_requeue_dispatch_jobs_with(&auth, req.redrive_completed)?;
    requeue(&state, &auth, req).await
}

/// Cancel a FAILED dispatch job (Go `cancelDispatchJob`).
#[utoipa::path(post, path = "/api/dispatch-jobs/{id}/cancel", tag = "dispatch-jobs",
    operation_id = "cancelDispatchJob",
    params(("id" = String, Path, description = "Dispatch job id")),
    responses((status = 200, description = "The job", body = DispatchJobResponse),
               (status = 409, description = "Not FAILED")),
    security(("bearer_auth" = [])))]
pub async fn api_cancel_dispatch_job(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    checks::can_cancel_dispatch_jobs(&auth.0)?;
    settle(&state, &auth, id, DispatchStatus::Cancelled).await
}

/// Mark a FAILED dispatch job completed (Go `completeDispatchJob`).
#[utoipa::path(post, path = "/api/dispatch-jobs/{id}/complete", tag = "dispatch-jobs",
    operation_id = "completeDispatchJob",
    params(("id" = String, Path, description = "Dispatch job id")),
    responses((status = 200, description = "The job", body = DispatchJobResponse),
               (status = 409, description = "Not FAILED")),
    security(("bearer_auth" = [])))]
pub async fn api_complete_dispatch_job(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    checks::can_complete_dispatch_jobs(&auth.0)?;
    settle(&state, &auth, id, DispatchStatus::Completed).await
}

/// Replace the payload of a PENDING or FAILED dispatch job (Go
/// `editDispatchJobPayload`). Status and attempts are unchanged; the operator
/// requeues afterwards.
#[utoipa::path(put, path = "/api/dispatch-jobs/{id}/payload", tag = "dispatch-jobs",
    operation_id = "editDispatchJobPayload", request_body = EditPayloadRequest,
    params(("id" = String, Path, description = "Dispatch job id")),
    responses((status = 200, description = "The job", body = DispatchJobResponse),
               (status = 409, description = "NOT_EDITABLE: the job is not PENDING or FAILED")),
    security(("bearer_auth" = [])))]
pub async fn api_edit_dispatch_job_payload(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<EditPayloadRequest>,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    checks::can_edit_dispatch_job_payloads(&auth.0)?;
    edit_payload(&state, &auth, id, req).await
}

/// The delivery a send would make now, signed as it would be; nothing is
/// sent or written (Go `signDispatchJob`).
#[utoipa::path(post, path = "/api/dispatch-jobs/{id}/sign", tag = "dispatch-jobs",
    operation_id = "signDispatchJob",
    params(("id" = String, Path, description = "Dispatch job id")),
    responses((status = 200, description = "The delivery", body = DeliveryPlan)),
    security(("bearer_auth" = [])))]
pub async fn api_sign_dispatch_job(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DeliveryPlan>, PlatformError> {
    checks::can_read_dispatch_jobs_raw(&auth.0)?;
    sign(&state, &auth, id).await
}

// ── /bff ─────────────────────────────────────────────────────────────────

/// BFF twin of [`api_requeue_dispatch_jobs`].
#[utoipa::path(post, path = "/bff/dispatch-jobs/requeue", tag = "bff-dispatch-jobs",
    operation_id = "requeueDispatchJobsBff", request_body = RequeueRequest,
    responses((status = 200, description = "How many were requeued", body = RequeueResponse)))]
pub async fn bff_requeue_dispatch_jobs(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Json(req): Json<RequeueRequest>,
) -> Result<Json<RequeueResponse>, PlatformError> {
    can_requeue_dispatch_jobs_with(&auth, req.redrive_completed)?;
    requeue(&state, &auth, req).await
}

/// BFF twin of [`api_cancel_dispatch_job`].
#[utoipa::path(post, path = "/bff/dispatch-jobs/{id}/cancel", tag = "bff-dispatch-jobs",
    operation_id = "cancelDispatchJobBff",
    params(("id" = String, Path, description = "Dispatch job id")),
    responses((status = 200, description = "The job", body = DispatchJobResponse)))]
pub async fn bff_cancel_dispatch_job(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    checks::can_cancel_dispatch_jobs(&auth.0)?;
    settle(&state, &auth, id, DispatchStatus::Cancelled).await
}

/// BFF twin of [`api_complete_dispatch_job`].
#[utoipa::path(post, path = "/bff/dispatch-jobs/{id}/complete", tag = "bff-dispatch-jobs",
    operation_id = "completeDispatchJobBff",
    params(("id" = String, Path, description = "Dispatch job id")),
    responses((status = 200, description = "The job", body = DispatchJobResponse)))]
pub async fn bff_complete_dispatch_job(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    checks::can_complete_dispatch_jobs(&auth.0)?;
    settle(&state, &auth, id, DispatchStatus::Completed).await
}

/// BFF twin of [`api_edit_dispatch_job_payload`].
#[utoipa::path(put, path = "/bff/dispatch-jobs/{id}/payload", tag = "bff-dispatch-jobs",
    operation_id = "editDispatchJobPayloadBff", request_body = EditPayloadRequest,
    params(("id" = String, Path, description = "Dispatch job id")),
    responses((status = 200, description = "The job", body = DispatchJobResponse)))]
pub async fn bff_edit_dispatch_job_payload(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<EditPayloadRequest>,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    checks::can_edit_dispatch_job_payloads(&auth.0)?;
    edit_payload(&state, &auth, id, req).await
}

/// BFF twin of [`api_sign_dispatch_job`].
#[utoipa::path(post, path = "/bff/dispatch-jobs/{id}/sign", tag = "bff-dispatch-jobs",
    operation_id = "signDispatchJobBff",
    params(("id" = String, Path, description = "Dispatch job id")),
    responses((status = 200, description = "The delivery", body = DeliveryPlan)))]
pub async fn bff_sign_dispatch_job(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<DeliveryPlan>, PlatformError> {
    checks::can_read_dispatch_jobs_raw(&auth.0)?;
    sign(&state, &auth, id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch_job::entity::DispatchJob;

    #[test]
    fn the_envelope_is_go_shaped() {
        let mut job = DispatchJob::for_event(
            Some("evn_1"),
            "shop:orders:order:shipped",
            Some("shop"),
            "https://example.test/hook",
            r#"{"a":1}"#,
        );
        job.client_id = Some("clt_1".into());
        let body = delivery_body(&job, Some("acme"));
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["data"], serde_json::json!({ "a": 1 }));
        assert_eq!(v["clientCode"], "acme");
        assert_eq!(v["attemptNumber"], 1);
        // Keys in order, as Go marshals a map.
        let keys: Vec<&str> = body
            .trim_matches(|c| c == '{' || c == '}')
            .split(',')
            .filter_map(|kv| kv.split(':').next())
            .filter(|k| k.starts_with('"'))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }
}
