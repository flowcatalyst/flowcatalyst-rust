//! Dispatch Job Processing Endpoints
//!
//! `POST /api/dispatch/process` — the callback the message router calls with
//! `{ "messageId": … }` for each queued dispatch job — and `POST
//! /api/dispatch/settled`, the router's report of BLOCK_ON_ERROR siblings it
//! ACKed untried. Ports of Go's `dispatchjob/processing` and
//! `dispatchjob/settled`, so Go's router and Rust's router both work
//! against this platform unchanged.
//!
//! `/process`:
//! 1. verifies `Authorization: Bearer <token>`, the scheduler's HMAC of the
//!    job id (the router forwards the queue message's `authToken`);
//! 2. loads the job; a missing or terminal job is ACKed without delivery;
//! 3. holds a BLOCK_ON_ERROR job whose group has an earlier held job: back
//!    to PENDING, ACKed, no budget spent;
//! 4. claims it conditionally (`PENDING/QUEUED → PROCESSING`). A copy that
//!    loses the claim to a live attempt is deferred (`ack:false` +
//!    `delaySeconds`) until that attempt's lease ends; one that finds the
//!    lease run out takes the claim over and delivers (the attempt died with
//!    its process); one that finds the job finished or back to PENDING is
//!    ACKed without delivering;
//! 5. delivers the webhook, records the attempt, and advances the job:
//!    COMPLETED; rescheduled without spending budget on a cooperative
//!    deferral (`ack:false`, 429); FAILED at once on 401/403 or when the
//!    budget is spent; otherwise PENDING with a `scheduled_for` backoff;
//! 6. answers `{"ack": true}`.
//!
//! Retries are the scheduler's: this endpoint always ACKs a message it
//! handled, and the poller re-dispatches the job when `scheduled_for` falls
//! due, so no queue redelivery races the poller into a double dispatch.
//!
//! One deliberate difference from Go: an internal error (the database
//! unreachable while loading, holding or claiming) answers **503**
//! `{"ack": false}` where Go answers 500, and so does `/settled`. Every
//! router (Go's, Rust's, and Java's conformance corpus) treats a 500 as the
//! target's permanent answer and ACKs the message away, which would leave
//! the job QUEUED until stale recovery; a 503 is retried (review R-57).
//! This endpoint never answers 500.
//!
//! A second one: a job left PROCESSING by an attempt that never finished is
//! recovered by the next copy of its message once the attempt's lease
//! ([`claim_lease`]) runs out, where Go acks that copy away and the job
//! stays PROCESSING for good. The delivery itself runs on its own task, so
//! a router hanging up mid-call never cuts an attempt short; only a dying
//! process leaves a claim behind. At-least-once: after such a death the
//! subscriber may see the message twice.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::post, Json, Router};
use chrono::Utc;
use dashmap::DashMap;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::dispatch_job::delivery_credentials::{DeliveryCredentials, Resolved};
use crate::dispatch_job::entity::{DispatchAttemptStatus, DispatchJob, ErrorType};
use crate::dispatch_job::repository::{
    DispatchJobRepository, NewDispatchAttempt, SETTLED_DEFAULT_REASON,
};
use crate::scheduler::DispatchAuthService;
use crate::shared::capped_body::{read_capped, DELIVERY_RESPONSE_CAP};
use crate::shared::webhook_signer;
use crate::ClientRepository;
use fc_common::DispatchMode;

/// `X-FlowCatalyst-Client: {clientId}:{clientCode}` (Go
/// `processing.clientHeader`).
pub const CLIENT_HEADER: &str = "X-FlowCatalyst-Client";

/// A job with no `timeout_seconds` gets this long (Go `defaultTimeout`).
const DEFAULT_DELIVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// The client's outer ceiling (Go's `http.Client.Timeout`).
const DELIVERY_CLIENT_CEILING: Duration = Duration::from_secs(120);

/// Slack on top of a delivery's own bound before its claim counts as dead
/// (see [`claim_lease`]): the credential lookup before it and the attempt
/// and status writes after it.
const CLAIM_LEASE_MARGIN: Duration = Duration::from_secs(30);

/// The backoff after a just-finished attempt `n` (1-based), clamped to the
/// last step (Go `retryBackoff`).
const RETRY_BACKOFF_SECS: [u64; 5] = [5, 15, 30, 60, 120];

/// Most bytes of a `/process` body read (Go: `io.LimitReader(…, 4<<10)`).
const PROCESS_BODY_LIMIT: usize = 4 << 10;

/// Most jobs in one `/settled` call, and most bytes read (Go).
const SETTLED_MAX_JOBS: usize = 10_000;
const SETTLED_BODY_LIMIT: usize = 1 << 20;

// ── Wire types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRequest {
    #[serde(default)]
    pub message_id: String,
}

/// The router's contract: `ack: false` asks it to retry via the queue.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessResponse {
    pub ack: bool,
    /// With `ack: false`: retry no sooner than this (the router's floor).
    #[serde(
        rename = "delaySeconds",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub delay_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub message: Option<String>,
}

fn reply(status: StatusCode, ack: bool, message: Option<&str>) -> Response {
    (
        status,
        Json(ProcessResponse {
            ack,
            delay_seconds: None,
            message: message.map(str::to_string),
        }),
    )
        .into_response()
}

/// `200 {"ack": false, "delaySeconds": n}`: keep the message, ask again in
/// `n` seconds (the router's cooperative deferral; no retry budget spent).
fn deferred(delay_seconds: u32, message: &str) -> Response {
    (
        StatusCode::OK,
        Json(ProcessResponse {
            ack: false,
            delay_seconds: Some(delay_seconds),
            message: Some(message.to_string()),
        }),
    )
        .into_response()
}

/// What a delivery sent (Go `RequestSummary`, stored as
/// `msg_dispatch_job_attempts.request_info`). Never a secret: the signing
/// account's code, header names, not values.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestSummary {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub signed_by: String,
    pub signature: bool,
    pub bearer: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub timestamp: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unsigned_reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target: String,
}

// ── State ────────────────────────────────────────────────────────────────

/// Caches a client's identifier (Go `client.NewCachedIdentifierResolver`):
/// a hit is kept for good (identifiers are immutable), a miss for a minute.
pub struct ClientCodeResolver {
    clients: Arc<ClientRepository>,
    cache: DashMap<String, (Option<String>, Instant)>,
    miss_ttl: Duration,
}

impl ClientCodeResolver {
    pub fn new(clients: Arc<ClientRepository>) -> Self {
        Self {
            clients,
            cache: DashMap::new(),
            miss_ttl: Duration::from_secs(60),
        }
    }

    pub async fn identifier(&self, client_id: &str) -> Option<String> {
        if let Some(entry) = self.cache.get(client_id) {
            let (code, at) = entry.value();
            if code.is_some() || at.elapsed() < self.miss_ttl {
                return code.clone();
            }
        }
        match self.clients.find_by_id(client_id).await {
            Ok(found) => {
                let code = found.map(|c| c.identifier).filter(|s| !s.is_empty());
                self.cache
                    .insert(client_id.to_string(), (code.clone(), Instant::now()));
                code
            }
            Err(e) => {
                warn!(client_id, error = %e, "client code lookup failed");
                None
            }
        }
    }
}

#[derive(Clone)]
pub struct DispatchProcessState {
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
    /// See [`delivery_http_client`].
    pub http_client: reqwest::Client,
    /// Whose credentials a delivery carries; `None` delivers bare.
    pub credentials: Option<Arc<DeliveryCredentials>>,
    /// Verifies the router's bearer. Required: without the application key
    /// the endpoints are not mounted at all (Go fails closed the same way).
    pub auth: DispatchAuthService,
    /// Resolves `clientCode` / `X-FlowCatalyst-Client`; `None` never does.
    pub client_codes: Option<Arc<ClientCodeResolver>>,
}

/// The subscriber client: no redirects (a 3xx is not a success) and Go's
/// two-minute outer ceiling; each delivery sets its own per-job timeout.
pub fn delivery_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(DELIVERY_CLIENT_CEILING)
        .build()
        .expect("the delivery HTTP client builds")
}

// ── /process ─────────────────────────────────────────────────────────────

/// The token as Go reads it: `Authorization` with a literal `"Bearer "`
/// prefix removed (a header without it is compared whole, and fails).
fn bearer(headers: &HeaderMap) -> &str {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    raw.strip_prefix("Bearer ").unwrap_or(raw)
}

/// The first JSON value in at most `limit` bytes (Go's `json.Decoder`
/// over a `LimitReader`: trailing data is ignored).
fn decode_first<'a, T: Deserialize<'a>>(body: &'a [u8], limit: usize) -> Option<T> {
    let body = &body[..body.len().min(limit)];
    serde_json::Deserializer::from_slice(body)
        .into_iter::<T>()
        .next()
        .and_then(|r| r.ok())
}

async fn process_dispatch(
    State(state): State<DispatchProcessState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let job_id = match decode_first::<ProcessRequest>(&body, PROCESS_BODY_LIMIT) {
        Some(req) if !req.message_id.trim().is_empty() => req.message_id,
        _ => return reply(StatusCode::BAD_REQUEST, true, Some("invalid messageId")),
    };

    // A forged callback must not trigger a delivery. The router always
    // carries a valid token, so it never lands here.
    let token = bearer(&headers);
    if token.is_empty() || !state.auth.verify(&job_id, token) {
        warn!(job_id = %job_id, "dispatch process: bad auth token");
        return reply(StatusCode::UNAUTHORIZED, false, Some("unauthorized"));
    }

    let repo = &state.dispatch_job_repo;
    let job = match repo.find_by_id(&job_id).await {
        Ok(Some(job)) => job,
        Ok(None) => return reply(StatusCode::OK, true, Some("job not found")),
        Err(e) => {
            error!(job_id = %job_id, error = %e, "dispatch process: load job failed");
            return reply(StatusCode::SERVICE_UNAVAILABLE, false, Some("load failed"));
        }
    };
    if job.status.is_terminal() {
        // A duplicate redelivery of a finished job.
        return reply(StatusCode::OK, true, None);
    }

    // The delivery-time half of the BLOCK_ON_ERROR hold: messages already
    // queued when a sibling ahead of them failed (or backed off) would
    // otherwise deliver past it.
    if job.mode == DispatchMode::BlockOnError {
        if let Some(group) = job.message_group.as_deref().filter(|g| !g.is_empty()) {
            match repo
                .group_held_before(group, job.sequence, job.created_at, &job.id)
                .await
            {
                Ok(false) => {}
                Ok(true) => {
                    if let Err(e) = repo.reschedule(&job.id, job.created_at, Utc::now()).await {
                        error!(job_id = %job_id, error = %e, "dispatch process: blocked-group revert failed");
                        return reply(
                            StatusCode::SERVICE_UNAVAILABLE,
                            false,
                            Some("revert failed"),
                        );
                    }
                    info!(job_id = %job_id, group, "dispatch held: group blocked, returned to PENDING");
                    return reply(StatusCode::OK, true, Some("group blocked"));
                }
                Err(e) => {
                    error!(job_id = %job_id, error = %e, "dispatch process: blocked-group check failed");
                    return reply(
                        StatusCode::SERVICE_UNAVAILABLE,
                        false,
                        Some("blocked check failed"),
                    );
                }
            }
        }
    }

    // Only one concurrent delivery: the conditional flip answers "did I
    // win?". A claim error means ownership is unknown, so never deliver.
    match repo.claim_for_delivery(&job.id, job.created_at).await {
        Ok(true) => {}
        Ok(false) => match lost_claim(repo, &job_id).await {
            Ok(taken_over) => return run_claimed(&state, taken_over).await,
            Err(answer) => return answer,
        },
        Err(e) => {
            error!(job_id = %job_id, error = %e, "dispatch process: claim failed");
            return reply(StatusCode::SERVICE_UNAVAILABLE, false, Some("claim failed"));
        }
    }
    run_claimed(&state, job).await
}

/// Deliver a job this call has claimed, record the attempt and advance it.
///
/// The work runs on a task of its own, so a caller that hangs up (a router
/// restarting mid-call) does not cancel it half way: once claimed, the
/// webhook's outcome is always recorded. Only a process that dies leaves a
/// claim behind, and [`lost_claim`] recovers that.
async fn run_claimed(state: &DispatchProcessState, job: DispatchJob) -> Response {
    let job_id = job.id.clone();
    let state = state.clone();
    match tokio::spawn(async move { deliver_and_record(&state, &job).await }).await {
        Ok(true) => reply(StatusCode::OK, true, None),
        // The job is still PROCESSING with its outcome unwritten. Go acks
        // here and the job stays PROCESSING; keep the message instead, so
        // its next copy takes the claim over once the lease runs out
        // (at-least-once) rather than waiting for stale recovery.
        Ok(false) => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            Some("status update failed"),
        ),
        Err(e) => {
            // A panic: the claim stays until its lease runs out, then a
            // redelivery takes it over.
            error!(job_id = %job_id, error = %e, "dispatch process: delivery task failed");
            reply(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                Some("delivery failed"),
            )
        }
    }
}

/// `false` when the outcome could not be written (the job is still
/// PROCESSING).
async fn deliver_and_record(state: &DispatchProcessState, job: &DispatchJob) -> bool {
    let repo = &state.dispatch_job_repo;
    let job_id = job.id.as_str();
    let attempt_number = job.attempt_count + 1;
    let attempted_at = Utc::now();
    let started = Instant::now();
    let result = deliver(state, job).await;
    let completed_at = Utc::now();
    let duration_ms = started.elapsed().as_millis() as i64;

    // Best effort: a recording failure never changes the outcome.
    let request_info = serde_json::to_value(&result.request).ok();
    let attempt = NewDispatchAttempt {
        dispatch_job_id: job_id,
        attempt_number,
        status: if result.success {
            DispatchAttemptStatus::Success
        } else {
            DispatchAttemptStatus::Failure
        },
        response_code: result.status,
        response_body: result.body.as_deref(),
        error_message: (!result.success).then_some(result.err_message.as_str()),
        error_type: if result.success {
            None
        } else {
            result.err_type
        },
        error_stack_trace: None,
        duration_millis: duration_ms,
        attempted_at,
        completed_at,
        request_info: request_info.as_ref(),
    };
    if let Err(e) = repo.insert_attempt(&attempt).await {
        warn!(job_id = %job_id, error = %e, "dispatch process: record attempt failed");
    }

    advance(repo, job, attempt_number, &result, duration_ms).await
}

/// How long a claimed attempt may hold its job before a redelivery may take
/// it over: the delivery's own bound (the job's timeout, capped by the
/// client's ceiling) plus [`CLAIM_LEASE_MARGIN`] for the recording writes.
pub fn claim_lease(job: &DispatchJob) -> Duration {
    let delivery = if job.timeout_seconds > 0 {
        Duration::from_secs(job.timeout_seconds as u64)
    } else {
        DEFAULT_DELIVERY_TIMEOUT
    };
    delivery.min(DELIVERY_CLIENT_CEILING) + CLAIM_LEASE_MARGIN
}

/// This call lost the claim. Go acks such a copy without delivering, which
/// is right while another attempt is live — and loses the job for good
/// when that attempt died with its process (a platform killed mid-delivery:
/// the webhook went out, the outcome was never written, the job stays
/// PROCESSING and the router's retry is acked away; delivery run 3,
/// `platform-down`). So:
/// - the job is PROCESSING and its claim is within [`claim_lease`]: an
///   attempt may be live. Answer `ack:false` with `delaySeconds` until the
///   lease ends, so the router keeps the message (and the group behind it)
///   and asks again;
/// - the lease has run out: that attempt is dead. Take the claim over and
///   deliver again — at-least-once; the subscriber may see it twice;
/// - anything else (finished, or already back to PENDING for a retry the
///   poller owns): ack without delivering, as Go.
///
/// `Ok` is the job, taken over and ready to deliver; `Err` is the answer.
async fn lost_claim(repo: &DispatchJobRepository, job_id: &str) -> Result<DispatchJob, Response> {
    let job = match repo.find_by_id(job_id).await {
        Ok(Some(job)) => job,
        Ok(None) => return Err(reply(StatusCode::OK, true, Some("job not found"))),
        Err(e) => {
            error!(job_id = %job_id, error = %e, "dispatch process: reload after a lost claim failed");
            return Err(reply(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                Some("load failed"),
            ));
        }
    };
    if job.status != crate::dispatch_job::entity::DispatchStatus::Processing {
        info!(job_id = %job_id, status = ?job.status, "dispatch process: already claimed, skipping duplicate delivery");
        return Err(reply(StatusCode::OK, true, Some("already claimed")));
    }
    let lease = claim_lease(&job);
    let claimed_at = job.last_attempt_at.unwrap_or(job.updated_at);
    let lease_ends = claimed_at + to_chrono(lease);
    let now = Utc::now();
    if now < lease_ends {
        let wait = (lease_ends - now).num_milliseconds().max(0) as u64;
        let delay = wait.div_ceil(1000).max(1) as u32;
        info!(job_id = %job_id, delay_seconds = delay, "dispatch process: delivery in progress elsewhere; asking the router to retry");
        return Err(deferred(delay, "delivery in progress"));
    }
    // `claimed_before` is the claim this call saw expire: a taker that got
    // there first has re-stamped it, and this one loses.
    match repo
        .reclaim_stale_delivery(&job.id, job.created_at, now - to_chrono(lease))
        .await
    {
        Ok(true) => {
            warn!(job_id = %job_id, claimed_at = %claimed_at, lease_secs = lease.as_secs(),
                "dispatch process: the previous attempt never finished; delivering again");
            Ok(job)
        }
        Ok(false) => Err(deferred(1, "delivery in progress")),
        Err(e) => {
            error!(job_id = %job_id, error = %e, "dispatch process: reclaim failed");
            Err(reply(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                Some("claim failed"),
            ))
        }
    }
}

/// Move the job on from one attempt's outcome (Go `advance`). `false` when
/// the status write failed and the job is still PROCESSING.
async fn advance(
    repo: &DispatchJobRepository,
    job: &DispatchJob,
    attempt_number: u32,
    res: &DeliveryResult,
    duration_ms: i64,
) -> bool {
    let id = job.id.as_str();
    let outcome = if res.success {
        debug!(job_id = %id, status = ?res.status, attempt = attempt_number, "dispatch delivered");
        repo.mark_completed(id, job.created_at, duration_ms).await
    } else if res.deferral {
        // Cooperative back-pressure: later, without spending the budget.
        info!(job_id = %id, retry_after_secs = res.retry_after.as_secs_f64(), reason = %res.err_message, "dispatch deferred");
        repo.reschedule(id, job.created_at, Utc::now() + to_chrono(res.retry_after))
            .await
    } else if matches!(res.status, Some(401) | Some(403)) {
        // The subscriber refused the credentials; a retry sends the same
        // ones. Terminal at once.
        warn!(job_id = %id, status = ?res.status, error = %res.err_message,
            "dispatch failed (subscriber refused credentials; not retried)");
        repo.mark_failed(id, job.created_at, &res.err_message, duration_ms)
            .await
    } else if attempt_number >= job.max_retries {
        warn!(job_id = %id, attempts = attempt_number, max = job.max_retries, error = %res.err_message,
            "dispatch failed (retries exhausted)");
        repo.mark_failed(id, job.created_at, &res.err_message, duration_ms)
            .await
    } else {
        let backoff = backoff_for(attempt_number);
        info!(job_id = %id, attempt = attempt_number, backoff_secs = backoff.as_secs(), error = %res.err_message,
            "dispatch retry scheduled");
        repo.schedule_retry(
            id,
            job.created_at,
            Utc::now() + to_chrono(backoff),
            &res.err_message,
        )
        .await
    };
    match outcome {
        Ok(()) => true,
        Err(e) => {
            warn!(job_id = %id, error = %e, "dispatch process: status update failed");
            false
        }
    }
}

fn to_chrono(d: Duration) -> chrono::Duration {
    chrono::Duration::from_std(d).unwrap_or_else(|_| chrono::Duration::seconds(30))
}

/// Go `backoffFor`.
pub fn backoff_for(attempt_number: u32) -> Duration {
    let i = (attempt_number.max(1) as usize - 1).min(RETRY_BACKOFF_SECS.len() - 1);
    Duration::from_secs(RETRY_BACKOFF_SECS[i])
}

// ── Delivery ─────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct DeliveryResult {
    success: bool,
    /// Cooperative back-pressure: retry later, no budget spent.
    deferral: bool,
    retry_after: Duration,
    status: Option<u16>,
    body: Option<String>,
    err_message: String,
    err_type: Option<ErrorType>,
    request: RequestSummary,
}

/// The request body (Go `buildPayload`): the raw payload when `data_only`,
/// otherwise the envelope, keys sorted as Go's `json.Marshal` of a map
/// sorts them, absent fields omitted.
pub fn build_payload(job: &DispatchJob, client_code: Option<&str>) -> Vec<u8> {
    if job.data_only {
        return job
            .payload
            .clone()
            .unwrap_or_else(|| "{}".to_string())
            .into_bytes();
    }
    let mut env: BTreeMap<&str, serde_json::Value> = BTreeMap::new();
    env.insert("id", job.id.clone().into());
    env.insert("type", job.code.clone().into());
    env.insert("attemptNumber", (job.attempt_count + 1).into());
    if let Some(v) = &job.source {
        env.insert("source", v.clone().into());
    }
    if let Some(v) = &job.subject {
        env.insert("subject", v.clone().into());
    }
    if let Some(v) = &job.correlation_id {
        env.insert("correlationId", v.clone().into());
    }
    if let Some(v) = &job.message_group {
        env.insert("messageGroup", v.clone().into());
    }
    if let Some(v) = &job.client_id {
        env.insert("clientId", v.clone().into());
        if let Some(code) = client_code.filter(|c| !c.is_empty()) {
            env.insert("clientCode", code.into());
        }
    }
    match &job.payload {
        // Embedded as JSON when it parses (member order kept, as Go's
        // RawMessage), otherwise as a string so nothing is dropped.
        Some(payload) => {
            let data = match serde_json::from_str::<&serde_json::value::RawValue>(payload) {
                Ok(raw) => compact_json(raw.get()),
                Err(_) => serde_json::to_string(payload).unwrap_or_default(),
            };
            splice_data(&env, &data)
        }
        None => serde_json::to_vec(&env).unwrap_or_else(|_| b"{}".to_vec()),
    }
}

/// Strip insignificant whitespace from valid JSON, keeping member order —
/// what Go's `json.Marshal` does to an embedded `RawMessage`.
fn compact_json(valid: &str) -> String {
    let mut out = String::with_capacity(valid.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in valid.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if !c.is_ascii_whitespace() {
            out.push(c);
        }
    }
    out
}

/// Serialise `env` with a `"data"` member holding already-rendered JSON,
/// in sorted key position.
fn splice_data(env: &BTreeMap<&str, serde_json::Value>, data: &str) -> Vec<u8> {
    let mut s = String::from("{");
    let mut first = true;
    let mut data_done = false;
    let push = |s: &mut String, key: &str, value: &str, first: &mut bool| {
        if !*first {
            s.push(',');
        }
        *first = false;
        s.push_str(&serde_json::to_string(key).unwrap_or_default());
        s.push(':');
        s.push_str(value);
    };
    for (k, v) in env {
        if !data_done && *k > "data" {
            push(&mut s, "data", data, &mut first);
            data_done = true;
        }
        push(&mut s, k, &v.to_string(), &mut first);
    }
    if !data_done {
        push(&mut s, "data", data, &mut first);
    }
    s.push('}');
    s.into_bytes()
}

/// `{clientId}:{clientCode}`, or nothing: never a half pair.
fn client_header_value(job: &DispatchJob, client_code: Option<&str>) -> Option<String> {
    let client_id = job.client_id.as_deref().filter(|s| !s.is_empty())?;
    let code = client_code.filter(|s| !s.is_empty())?;
    Some(format!("{client_id}:{code}"))
}

async fn deliver(state: &DispatchProcessState, job: &DispatchJob) -> DeliveryResult {
    let client_code = match (&state.client_codes, job.client_id.as_deref()) {
        (Some(resolver), Some(id)) if !id.is_empty() => resolver.identifier(id).await,
        _ => None,
    };
    let body = build_payload(job, client_code.as_deref());

    let mut summary = RequestSummary {
        target: job.target_url.clone(),
        ..RequestSummary::default()
    };
    let mut header_names: Vec<&'static str> =
        vec!["Content-Type", "X-Dispatch-Job-Id", "X-Event-Type"];
    let timeout = if job.timeout_seconds > 0 {
        Duration::from_secs(job.timeout_seconds as u64)
    } else {
        DEFAULT_DELIVERY_TIMEOUT
    };
    let mut request = state
        .http_client
        .post(&job.target_url)
        .timeout(timeout)
        .header(CONTENT_TYPE, "application/json")
        .header("X-Dispatch-Job-Id", &job.id)
        .header("X-Event-Type", &job.code);
    if let Some(v) = client_header_value(job, client_code.as_deref()) {
        request = request.header(CLIENT_HEADER, v);
        header_names.push(CLIENT_HEADER);
    }

    let credentials = match &state.credentials {
        Some(c) => Some(c.resolve_or_bare(job).await),
        None => None,
    };
    match &credentials {
        None => summary.unsigned_reason = "no credential resolver configured".to_string(),
        Some(c) if c.is_bare() => {
            summary.unsigned_reason = if c.reason.is_empty() {
                "no credentials resolved".to_string()
            } else {
                c.reason.clone()
            };
            warn!(job_id = %job.id, subscription_id = ?job.subscription_id,
                reason = %summary.unsigned_reason, "dispatch process: delivering unsigned");
        }
        Some(c) => {
            summary.signed_by = c.signed_by.clone().unwrap_or_default();
            let at = Utc::now();
            request = apply_credentials(request, c, at, &body);
            if c.bearer_token.is_some() {
                summary.bearer = true;
                header_names.push("Authorization");
            }
            if c.signing_secret.is_some() {
                summary.signature = true;
                summary.timestamp = webhook_signer::timestamp(at);
                header_names.push(webhook_signer::SIGNATURE_HEADER);
                header_names.push(webhook_signer::TIMESTAMP_HEADER);
            }
        }
    }
    header_names.sort_unstable();
    summary.headers = header_names.into_iter().map(str::to_string).collect();

    let mut result = exchange(request.body(body)).await;
    if !summary.unsigned_reason.is_empty() && !result.success && !result.deferral {
        result.err_message = format!(
            "{} (delivered unsigned: {})",
            result.err_message, summary.unsigned_reason
        );
    }
    result.request = summary;
    result
}

async fn exchange(request: reqwest::RequestBuilder) -> DeliveryResult {
    let response = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            let (msg, et) = if e.is_timeout() {
                ("Connection timeout".to_string(), ErrorType::Timeout)
            } else if e.is_builder() {
                (format!("build request: {e}"), ErrorType::Connection)
            } else {
                (format!("Connection error: {e}"), ErrorType::Connection)
            };
            return DeliveryResult {
                err_message: msg,
                err_type: Some(et),
                ..DeliveryResult::default()
            };
        }
    };
    let status = response.status();
    let code = status.as_u16();
    let retry_after = retry_after_or_default(response.headers());
    let capped = read_capped(response, DELIVERY_RESPONSE_CAP).await;
    let body_text = capped.text();

    if status.is_success() {
        if let Some(delay) = parse_deferral(&capped.bytes) {
            return DeliveryResult {
                deferral: true,
                retry_after: delay,
                status: Some(code),
                body: Some(body_text),
                err_message: "subscriber deferred (ack=false)".to_string(),
                ..DeliveryResult::default()
            };
        }
        return DeliveryResult {
            success: true,
            status: Some(code),
            body: Some(body_text),
            ..DeliveryResult::default()
        };
    }
    if code == 429 {
        return DeliveryResult {
            deferral: true,
            retry_after,
            status: Some(code),
            body: Some(body_text),
            err_message: "rate limited (429)".to_string(),
            ..DeliveryResult::default()
        };
    }
    DeliveryResult {
        status: Some(code),
        body: Some(body_text),
        err_message: format!("HTTP {status}"),
        err_type: Some(ErrorType::HttpError),
        ..DeliveryResult::default()
    }
}

/// A 2xx body `{"ack": false[, "delaySeconds": n]}` is a deferral (default
/// 30s). Anything else, including a malformed body, is not.
pub fn parse_deferral(body: &[u8]) -> Option<Duration> {
    #[derive(Deserialize)]
    struct Ack {
        ack: Option<bool>,
        #[serde(rename = "delaySeconds")]
        delay_seconds: Option<u32>,
    }
    if body.is_empty() {
        return None;
    }
    let parsed: Ack = serde_json::from_slice(body).ok()?;
    if parsed.ack != Some(false) {
        return None;
    }
    Some(Duration::from_secs(
        parsed.delay_seconds.map(u64::from).unwrap_or(30),
    ))
}

/// `Retry-After` in (possibly fractional) seconds, else 30s.
fn retry_after_or_default(headers: &reqwest::header::HeaderMap) -> Duration {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| *s > 0.0 && s.is_finite())
        .map(Duration::from_secs_f64)
        .unwrap_or(Duration::from_secs(30))
}

/// `Authorization: Bearer` when there is a token, and the
/// `X-FlowCatalyst-Timestamp` / `X-FlowCatalyst-Signature` pair when there is
/// a signing secret, signed over `body` at `at`.
pub fn apply_credentials(
    request: reqwest::RequestBuilder,
    credentials: &Resolved,
    at: chrono::DateTime<chrono::Utc>,
    body: &[u8],
) -> reqwest::RequestBuilder {
    let mut request = request;
    if let Some(token) = &credentials.bearer_token {
        request = request.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(secret) = &credentials.signing_secret {
        for (name, value) in webhook_signer::signature_headers(secret, at, body) {
            request = request.header(name, value);
        }
    }
    request
}

// ── /settled ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct SettledJob {
    #[serde(default)]
    id: String,
    #[serde(default)]
    token: String,
}

#[derive(Debug, Deserialize, Default)]
struct SettledRequest {
    #[serde(default)]
    reason: String,
    #[serde(default)]
    jobs: Vec<SettledJob>,
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SettledResponse {
    pub settled: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ids: Vec<String>,
}

fn settled_reply(status: StatusCode, body: SettledResponse) -> Response {
    (status, Json(body)).into_response()
}

/// Each id/token pair is verified on its own: one bad entry does not sink
/// the rest (Go `settled.Handler`).
async fn settled(State(state): State<DispatchProcessState>, body: Bytes) -> Response {
    let Some(req) = decode_first::<SettledRequest>(&body, SETTLED_BODY_LIMIT) else {
        return settled_reply(StatusCode::BAD_REQUEST, SettledResponse::default());
    };
    if req.jobs.is_empty() {
        return settled_reply(StatusCode::OK, SettledResponse::default());
    }
    if req.jobs.len() > SETTLED_MAX_JOBS {
        return settled_reply(StatusCode::BAD_REQUEST, SettledResponse::default());
    }
    let reason = match req.reason.trim() {
        "" => SETTLED_DEFAULT_REASON.to_string(),
        r => r.to_string(),
    };
    let mut ids = Vec::with_capacity(req.jobs.len());
    for j in &req.jobs {
        let id = j.id.trim();
        if id.is_empty() || j.token.trim().is_empty() {
            continue;
        }
        if !state.auth.verify(id, &j.token) {
            warn!(job_id = %id, "dispatch settled: bad auth token");
            continue;
        }
        ids.push(id.to_string());
    }
    if ids.is_empty() {
        return settled_reply(StatusCode::UNAUTHORIZED, SettledResponse::default());
    }
    match state.dispatch_job_repo.settle_acked(&ids, &reason).await {
        Ok(settled_ids) => {
            if !settled_ids.is_empty() {
                info!(count = settled_ids.len(), reason = %reason, "dispatch settled: siblings marked PENDING");
            }
            settled_reply(
                StatusCode::OK,
                SettledResponse {
                    settled: settled_ids.len(),
                    ids: settled_ids,
                },
            )
        }
        Err(e) => {
            // 503, not Go's 500: an internal error is transient, and every
            // router treats a 500 as a permanent answer.
            error!(error = %e, submitted = ids.len(), "dispatch settled: settle failed");
            settled_reply(StatusCode::SERVICE_UNAVAILABLE, SettledResponse::default())
        }
    }
}

// ── Router ───────────────────────────────────────────────────────────────

/// `/process` and `/settled`, nested under `/api/dispatch`. Outside the
/// platform's bearer middleware: both authenticate per job with the
/// scheduler's token.
pub fn dispatch_process_router(state: DispatchProcessState) -> Router {
    Router::new()
        .route("/process", post(process_dispatch))
        .route("/settled", post(settled))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> DispatchJob {
        let mut j = DispatchJob::for_event(
            Some("evt1"),
            "app:dom:agg:done",
            Some("src"),
            "http://t",
            "{\"b\":1, \"a\":2}",
        );
        j.id = "job1".into();
        j.data_only = false;
        j.subject = None;
        j.correlation_id = None;
        j.message_group = None;
        j.client_id = None;
        j
    }

    #[test]
    fn envelope_is_gos_sorted_map_with_absent_fields_omitted() {
        let j = job();
        let body = String::from_utf8(build_payload(&j, None)).unwrap();
        assert_eq!(
            body,
            r#"{"attemptNumber":1,"data":{"b":1,"a":2},"id":"job1","source":"src","type":"app:dom:agg:done"}"#
        );
    }

    #[test]
    fn compaction_keeps_strings_and_order() {
        assert_eq!(
            compact_json("{ \"z\" : \"a b\\\" c\" ,\n \"a\": [1, 2] }"),
            "{\"z\":\"a b\\\" c\",\"a\":[1,2]}"
        );
    }

    #[test]
    fn envelope_carries_client_code_only_with_a_client() {
        let mut j = job();
        j.client_id = Some("clt_1".into());
        j.message_group = Some("g".into());
        let body: serde_json::Value =
            serde_json::from_slice(&build_payload(&j, Some("acme"))).unwrap();
        assert_eq!(body["clientId"], "clt_1");
        assert_eq!(body["clientCode"], "acme");
        assert_eq!(body["messageGroup"], "g");
        let body: serde_json::Value = serde_json::from_slice(&build_payload(&j, None)).unwrap();
        assert!(body.get("clientCode").is_none());
        j.client_id = None;
        let body: serde_json::Value =
            serde_json::from_slice(&build_payload(&j, Some("acme"))).unwrap();
        assert!(body.get("clientCode").is_none(), "never a half pair");
    }

    #[test]
    fn envelope_passes_a_non_json_payload_as_a_string() {
        let mut j = job();
        j.payload = Some("not json".into());
        let body: serde_json::Value = serde_json::from_slice(&build_payload(&j, None)).unwrap();
        assert_eq!(body["data"], "not json");
        j.payload = None;
        let body: serde_json::Value = serde_json::from_slice(&build_payload(&j, None)).unwrap();
        assert!(body.get("data").is_none());
    }

    #[test]
    fn data_only_sends_the_raw_payload() {
        let mut j = job();
        j.data_only = true;
        assert_eq!(
            build_payload(&j, Some("acme")),
            b"{\"b\":1, \"a\":2}".to_vec()
        );
        j.payload = None;
        assert_eq!(build_payload(&j, None), b"{}".to_vec());
    }

    #[test]
    fn client_header_is_never_half_a_pair() {
        let mut j = job();
        assert_eq!(client_header_value(&j, Some("acme")), None);
        j.client_id = Some("clt_1".into());
        assert_eq!(client_header_value(&j, None), None);
        assert_eq!(
            client_header_value(&j, Some("acme")).as_deref(),
            Some("clt_1:acme")
        );
    }

    #[test]
    fn deferral_parsing_matches_go() {
        assert_eq!(parse_deferral(b""), None);
        assert_eq!(parse_deferral(b"not json"), None);
        assert_eq!(parse_deferral(br#"{"ack":true}"#), None);
        assert_eq!(parse_deferral(br#"{}"#), None);
        assert_eq!(
            parse_deferral(br#"{"ack":false}"#),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            parse_deferral(br#"{"ack":false,"delaySeconds":7}"#),
            Some(Duration::from_secs(7))
        );
        assert_eq!(parse_deferral(br#"{"ack":false,"delaySeconds":-1}"#), None);
    }

    #[test]
    fn backoff_matches_go() {
        let secs: Vec<u64> = (1..=7).map(|n| backoff_for(n).as_secs()).collect();
        assert_eq!(secs, vec![5, 15, 30, 60, 120, 120, 120]);
        assert_eq!(backoff_for(0).as_secs(), 5);
    }

    #[test]
    fn bearer_is_read_as_go_reads_it() {
        let mut h = HeaderMap::new();
        assert_eq!(bearer(&h), "");
        h.insert("authorization", "Bearer abc".parse().unwrap());
        assert_eq!(bearer(&h), "abc");
        h.insert("authorization", "bearer abc".parse().unwrap());
        assert_eq!(bearer(&h), "bearer abc");
    }

    #[test]
    fn process_body_decodes_like_gos_decoder() {
        let r: ProcessRequest = decode_first(br#"{"messageId":"j1"} trailing"#, 4096).unwrap();
        assert_eq!(r.message_id, "j1");
        assert!(decode_first::<ProcessRequest>(b"nope", 4096).is_none());
    }

    #[test]
    fn request_summary_serialises_like_go() {
        let s = RequestSummary {
            signed_by: "svc".into(),
            signature: true,
            bearer: false,
            timestamp: "2026-01-01T00:00:00.000Z".into(),
            headers: vec!["Content-Type".into()],
            unsigned_reason: String::new(),
            target: "http://t".into(),
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"signedBy":"svc","signature":true,"bearer":false,"timestamp":"2026-01-01T00:00:00.000Z","headers":["Content-Type"],"target":"http://t"}"#
        );
    }
}
