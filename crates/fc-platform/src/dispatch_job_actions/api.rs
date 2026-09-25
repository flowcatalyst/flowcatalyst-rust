//! Operator routes on dispatch jobs, on the `/api` and `/bff` tiers alike
//! (Go `dispatchjob/api/api.go:75-78` and `:109-112`):
//!
//! - `POST …/dispatch-jobs/requeue` `{ids}`   → `{requeued}` (view)
//! - `POST …/dispatch-jobs/{id}/cancel`       → the job (view; FAILED only)
//! - `POST …/dispatch-jobs/{id}/complete`     → the job (view; FAILED only)
//! - `POST …/dispatch-jobs/{id}/sign`         → the delivery a send would make now (view-raw)
//!
//! Go gates requeue, cancel and complete on `dispatch-job:view` itself;
//! Rust keeps Go's permission.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::operations::{
    RequeueDispatchJobsUseCase, ResendCommand, SettleDispatchJobUseCase, StatusFlipCommand,
};
use super::repository::DispatchJobActionsRepository;
use crate::dispatch_job::api::DispatchJobResponse;
use crate::dispatch_job::delivery_credentials::DeliveryCredentials;
use crate::shared::authorization_service::{checks, AuthContext};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::shared::webhook_signer;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use crate::{ClientRepository, DispatchJobRepository};

#[derive(Clone)]
pub struct DispatchJobActionsState {
    pub repo: Arc<DispatchJobActionsRepository>,
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
    pub client_repo: Arc<ClientRepository>,
    pub credentials: Arc<DeliveryCredentials>,
    pub requeue_use_case: Arc<RequeueDispatchJobsUseCase<PgUnitOfWork>>,
    pub settle_use_case: Arc<SettleDispatchJobUseCase<PgUnitOfWork>>,
}

/// Go `RequeueRequest`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RequeueRequest {
    #[serde(default)]
    pub ids: Vec<String>,
}

/// Go `RequeueResponse`.
#[derive(Debug, Serialize, ToSchema)]
pub struct RequeueResponse {
    pub requeued: usize,
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

/// Go `CanAccessScope`: a client's job needs that client; a client-less
/// one anchor or super-admin.
fn reaches(ctx: &AuthContext, client_id: Option<&str>) -> bool {
    match client_id {
        Some(c) => ctx.can_access_client(c),
        None => ctx.is_anchor() || ctx.has_permission(crate::permissions::ADMIN_ALL),
    }
}

fn scope_forbidden(client_id: Option<&str>) -> PlatformError {
    PlatformError::forbidden_code(
        "SCOPE_FORBIDDEN",
        if client_id.is_some() {
            "no access to this resource's client"
        } else {
            "anchor scope required for this resource"
        },
    )
}

async fn requeue(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    ids: Vec<String>,
) -> Result<Json<RequeueResponse>, PlatformError> {
    if ids.is_empty() {
        return Ok(Json(RequeueResponse { requeued: 0 }));
    }
    // Unknown and unreachable ids are dropped silently, as in Go.
    let jobs: Vec<(String, chrono::DateTime<chrono::Utc>)> = state
        .repo
        .heads(&ids)
        .await?
        .into_iter()
        .filter(|j| reaches(&auth.0, j.client_id.as_deref()))
        .map(|j| (j.id, j.created_at))
        .collect();
    let command = ResendCommand {
        ids: jobs.iter().map(|(id, _)| id.clone()).collect(),
        jobs,
    };
    let event = state
        .requeue_use_case
        .run(command, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(Json(RequeueResponse {
        requeued: event.ids.len(),
    }))
}

async fn settle(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    id: String,
    target: &'static str,
) -> Result<Json<DispatchJobResponse>, PlatformError> {
    let head = state
        .repo
        .heads(std::slice::from_ref(&id))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| PlatformError::not_found_code("DispatchJob", &id))?;
    if !reaches(&auth.0, head.client_id.as_deref()) {
        return Err(scope_forbidden(head.client_id.as_deref()));
    }
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

/// Go `buildPayload`: the raw payload for a data-only job, else the
/// envelope with its keys in order (Go marshals a map).
fn delivery_body(job: &crate::DispatchJob, client_code: Option<&str>) -> String {
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
    if !reaches(&auth.0, job.client_id.as_deref()) {
        return Err(scope_forbidden(job.client_id.as_deref()));
    }
    let client_code = match job.client_id.as_deref() {
        Some(c) => state
            .client_repo
            .find_by_id(c)
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

/// Requeue dispatch jobs (Go `requeueDispatchJobs`): any status back to
/// PENDING with a fresh attempt budget.
#[utoipa::path(post, path = "/api/dispatch-jobs/requeue", tag = "dispatch-jobs",
    operation_id = "requeueDispatchJobs", request_body = RequeueRequest,
    responses((status = 200, description = "How many were requeued", body = RequeueResponse)),
    security(("bearer_auth" = [])))]
pub async fn api_requeue_dispatch_jobs(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Json(req): Json<RequeueRequest>,
) -> Result<Json<RequeueResponse>, PlatformError> {
    checks::can_read_dispatch_jobs(&auth.0)?;
    requeue(&state, &auth, req.ids).await
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
    checks::can_read_dispatch_jobs(&auth.0)?;
    settle(&state, &auth, id, "CANCELLED").await
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
    checks::can_read_dispatch_jobs(&auth.0)?;
    settle(&state, &auth, id, "COMPLETED").await
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
    checks::can_read_dispatch_jobs(&auth.0)?;
    requeue(&state, &auth, req.ids).await
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
    checks::can_read_dispatch_jobs(&auth.0)?;
    settle(&state, &auth, id, "CANCELLED").await
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
    checks::can_read_dispatch_jobs(&auth.0)?;
    settle(&state, &auth, id, "COMPLETED").await
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

/// Full-path router; merged at the root.
pub fn dispatch_job_actions_router(state: DispatchJobActionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(api_requeue_dispatch_jobs))
        .routes(routes!(api_cancel_dispatch_job))
        .routes(routes!(api_complete_dispatch_job))
        .routes(routes!(api_sign_dispatch_job))
        .routes(routes!(bff_requeue_dispatch_jobs))
        .routes(routes!(bff_cancel_dispatch_job))
        .routes(routes!(bff_complete_dispatch_job))
        .routes(routes!(bff_sign_dispatch_job))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_is_go_shaped() {
        let mut job = crate::DispatchJob::for_event(
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
