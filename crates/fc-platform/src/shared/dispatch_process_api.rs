//! Dispatch Job Processing Endpoint
//!
//! `POST /api/dispatch/process` — the callback URL that the message router
//! calls with `{ messageId }`. This endpoint:
//!
//! 1. Loads the dispatch job by ID
//! 2. Resolves auth credentials (service account token if configured)
//! 3. Delivers the webhook to the target URL
//! 4. Records the attempt in `msg_dispatch_job_attempts`
//! 5. Updates the dispatch job status
//! 6. Returns ACK/NACK to the router

use axum::{extract::State, routing::post, Json, Router};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

use crate::dispatch_job::entity::DispatchStatus;
use crate::dispatch_job::repository::{DispatchJobRepository, NewDispatchAttempt};
use crate::shared::error::PlatformError;

// ── Request / Response ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRequest {
    pub message_id: String,
}

#[derive(Debug, Serialize)]
pub struct ProcessResponse {
    /// true = ACK (remove from queue), false = NACK (retry later)
    pub ack: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ── State ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct DispatchProcessState {
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
    pub http_client: reqwest::Client,
}

// ── Handler ──────────────────────────────────────────────────────────────

async fn process_dispatch(
    State(state): State<DispatchProcessState>,
    Json(req): Json<ProcessRequest>,
) -> Result<Json<ProcessResponse>, PlatformError> {
    let job_id = &req.message_id;

    // 1. Load the dispatch job
    let job = match state.dispatch_job_repo.find_by_id(job_id).await? {
        Some(j) => j,
        None => {
            warn!(job_id = %job_id, "Dispatch job not found, ACKing to remove from queue");
            return Ok(Json(ProcessResponse {
                ack: true,
                message: Some("Job not found".to_string()),
            }));
        }
    };

    // 2. Check if already terminal
    if job.status.is_terminal() {
        debug!(job_id = %job_id, status = ?job.status, "Job already terminal, ACKing");
        return Ok(Json(ProcessResponse {
            ack: true,
            message: None,
        }));
    }

    // 3. Update status to PROCESSING
    if let Err(e) = state
        .dispatch_job_repo
        .update_status(job_id, job.created_at, DispatchStatus::Processing)
        .await
    {
        error!(job_id = %job_id, error = %e, "Failed to update job to PROCESSING");
    }

    // 4. Deliver the webhook
    let attempt_number = job.attempt_count + 1;
    let start = Instant::now();

    let mut request = state
        .http_client
        .post(&job.target_url)
        .header(CONTENT_TYPE, "application/json")
        .header("X-Dispatch-Job-Id", job_id)
        .header("X-Event-Type", &job.code);

    // Add auth token if service account is configured
    // TODO: resolve service account → token when auth is wired
    if let Some(ref sa_id) = job.service_account_id {
        debug!(job_id = %job_id, service_account = %sa_id, "Service account configured (token resolution not yet implemented)");
    }

    // Build payload
    let body = if job.data_only {
        // Send just the data payload
        job.payload.clone().unwrap_or_default()
    } else {
        // Send CloudEvents-style envelope
        serde_json::to_string(&serde_json::json!({
            "id": job.id,
            "type": job.code,
            "source": job.source,
            "subject": job.subject,
            "data": job.payload.as_deref().and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok()),
            "correlationId": job.correlation_id,
            "messageGroup": job.message_group,
            "clientId": job.client_id,
            "attemptNumber": attempt_number,
        })).unwrap_or_default()
    };

    request = request.body(body);

    let result = request.send().await;
    let duration_ms = start.elapsed().as_millis() as i64;

    // 5. Process result and record attempt
    struct AttemptResult {
        ack: bool,
        new_status: DispatchStatus,
        response_code: Option<u16>,
        error_message: Option<String>,
        error_type: Option<&'static str>,
        response_body: Option<String>,
    }

    let outcome = match result {
        Ok(response) => {
            let status_code = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();

            if (200..300).contains(&(status_code as i32)) {
                // Check for explicit ack=false (deferred)
                if let Ok(resp) = serde_json::from_str::<serde_json::Value>(&body) {
                    if resp.get("ack") == Some(&serde_json::Value::Bool(false)) {
                        info!(job_id = %job_id, "Webhook deferred (ack=false)");
                        AttemptResult {
                            ack: false,
                            new_status: DispatchStatus::Pending,
                            response_code: Some(status_code),
                            error_message: None,
                            error_type: None,
                            response_body: Some(body),
                        }
                    } else {
                        debug!(job_id = %job_id, status_code, "Webhook delivered successfully");
                        AttemptResult {
                            ack: true,
                            new_status: DispatchStatus::Completed,
                            response_code: Some(status_code),
                            error_message: None,
                            error_type: None,
                            response_body: Some(body),
                        }
                    }
                } else {
                    debug!(job_id = %job_id, status_code, "Webhook delivered successfully");
                    AttemptResult {
                        ack: true,
                        new_status: DispatchStatus::Completed,
                        response_code: Some(status_code),
                        error_message: None,
                        error_type: None,
                        response_body: Some(body),
                    }
                }
            } else if status_code == 429 {
                warn!(job_id = %job_id, "Webhook rate limited (429)");
                AttemptResult {
                    ack: false,
                    new_status: DispatchStatus::Pending,
                    response_code: Some(status_code),
                    error_message: Some("Rate limited".to_string()),
                    error_type: Some("HTTP_ERROR"),
                    response_body: Some(body),
                }
            } else if (400..500).contains(&(status_code as i32)) {
                warn!(job_id = %job_id, status_code, "Webhook rejected (4xx)");
                let should_fail = attempt_number >= job.max_retries;
                let status = if should_fail {
                    DispatchStatus::Failed
                } else {
                    DispatchStatus::Pending
                };
                AttemptResult {
                    ack: should_fail,
                    new_status: status,
                    response_code: Some(status_code),
                    error_message: Some(format!("HTTP {}", status_code)),
                    error_type: Some("HTTP_ERROR"),
                    response_body: Some(body),
                }
            } else {
                warn!(job_id = %job_id, status_code, "Webhook server error (5xx)");
                let should_fail = attempt_number >= job.max_retries;
                let status = if should_fail {
                    DispatchStatus::Failed
                } else {
                    DispatchStatus::Pending
                };
                AttemptResult {
                    ack: should_fail,
                    new_status: status,
                    response_code: Some(status_code),
                    error_message: Some(format!("HTTP {}", status_code)),
                    error_type: Some("HTTP_ERROR"),
                    response_body: Some(body),
                }
            }
        }
        Err(e) => {
            let (error_msg, err_type) = if e.is_timeout() {
                ("Connection timeout".to_string(), "TIMEOUT")
            } else if e.is_connect() {
                (format!("Connection error: {}", e), "CONNECTION")
            } else {
                (format!("Request failed: {}", e), "UNKNOWN")
            };
            warn!(job_id = %job_id, error = %error_msg, "Webhook delivery failed");
            let should_fail = attempt_number >= job.max_retries;
            let status = if should_fail {
                DispatchStatus::Failed
            } else {
                DispatchStatus::Pending
            };
            AttemptResult {
                ack: should_fail,
                new_status: status,
                response_code: None,
                error_message: Some(error_msg),
                error_type: Some(err_type),
                response_body: None,
            }
        }
    };

    // 6. Record attempt
    let attempt_status = if outcome.new_status == DispatchStatus::Completed {
        "SUCCESS"
    } else {
        "FAILED"
    };
    if let Err(e) = state
        .dispatch_job_repo
        .insert_attempt(&NewDispatchAttempt {
            dispatch_job_id: job_id,
            attempt_number,
            status: attempt_status,
            response_code: outcome.response_code,
            response_body: outcome.response_body.as_deref(),
            error_message: outcome.error_message.as_deref(),
            error_type: outcome.error_type,
            error_stack_trace: None, // not applicable for HTTP delivery
            duration_millis: duration_ms,
        })
        .await
    {
        error!(job_id = %job_id, error = %e, "Failed to record dispatch attempt");
    }

    // 7. Update dispatch job status
    if let Err(e) = state
        .dispatch_job_repo
        .update_after_attempt(
            job_id,
            job.created_at,
            outcome.new_status,
            attempt_number,
            duration_ms,
            outcome.error_message.as_deref(),
        )
        .await
    {
        error!(job_id = %job_id, error = %e, "Failed to update dispatch job after attempt");
    }

    Ok(Json(ProcessResponse {
        ack: outcome.ack,
        message: outcome.error_message,
    }))
}

// ── Router ───────────────────────────────────────────────────────────────

pub fn dispatch_process_router(state: DispatchProcessState) -> Router {
    Router::new()
        .route("/process", post(process_dispatch))
        .with_state(state)
}
