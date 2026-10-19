//! Requeue by filter (Go `dispatchjob/operations/resend_by_filter.go`, handlers
//! in `dispatchjob/api/api.go`): reset every job a list filter matches, not
//! just the ids a caller has in hand, on the `/api` and `/bff` tiers.
//!
//! - `POST …/dispatch-jobs/requeue-by-filter/preview` → `{count, byStatus,
//!   eligibleCount, completedCount, inFlightCount, redriveEligibleCount, window}`
//! - `POST …/dispatch-jobs/requeue-by-filter` → `{requeued, truncated,
//!   remaining}`, or 409 `FILTER_MATCH_CHANGED` with the new counts in `details`
//!
//! It is not a use case of its own. It walks the matching ids (keyset, id order,
//! [`RESEND_BATCH_SIZE`] a page) and hands each page to the existing
//! [`RequeueDispatchJobsUseCase`], so each page is one transaction with that
//! use case's own event and audit row, exactly as if the caller had sent the
//! ids to `POST …/requeue`. It never resets more than `min(limit, confirmCount)`
//! jobs. Batches already committed stay committed if a later one fails.
//!
//! Both routes are gated on `dispatch-job:view`, as requeue is (checked in each
//! handler, before the filter is read).

use std::collections::{BTreeMap, HashMap};

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::usecase::{ExecutionContext, UseCase};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::api::{can_requeue_dispatch_jobs_with, resend_command, DispatchJobActionsState};
use super::operations::reaches;
use super::repository::MatchFilter;
use crate::dispatch_job::requeue_rules::{
    classify_for_requeue, redrive_confirmed, resettable_statuses, Verdict, REDRIVE_CONFIRMATION,
};
use crate::dispatch_job::window::{enforce_window, parse_timestamp, Window, WindowResponse};

/// How many ids go to one requeue run.
pub const RESEND_BATCH_SIZE: usize = 500;
/// The most one request resets; also the default.
pub const RESEND_BY_FILTER_MAX_LIMIT: i64 = 10_000;

/// Go `DispatchJobFilter`: the list's filter, under the list's own names.
/// The plural members are comma-separated multi-filters, as on the list.
#[derive(Debug, Default, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobFilter {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub dispatch_pool_id: Option<String>,
    #[serde(default)]
    pub subscription_id: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    /// RFC3339 timestamp
    #[serde(default)]
    pub since: Option<String>,
    /// RFC3339 timestamp
    #[serde(default)]
    pub until: Option<String>,
    /// Exact message group
    #[serde(default)]
    pub message_group: Option<String>,
    /// CSV of client ids
    #[serde(default)]
    pub client_ids: Option<String>,
    /// CSV of statuses
    #[serde(default)]
    pub statuses: Option<String>,
    /// CSV of application codes
    #[serde(default)]
    pub applications: Option<String>,
    /// CSV of subdomains
    #[serde(default)]
    pub subdomains: Option<String>,
    /// CSV of aggregates
    #[serde(default)]
    pub aggregates: Option<String>,
    /// CSV of codes
    #[serde(default)]
    pub codes: Option<String>,
    /// Free-text source filter
    #[serde(default)]
    pub source: Option<String>,
}

fn split_csv(input: Option<&str>) -> Vec<String> {
    input
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn non_empty(v: &Option<String>) -> Option<String> {
    v.as_deref().filter(|v| !v.is_empty()).map(str::to_string)
}

impl DispatchJobFilter {
    /// Go `toParams(strict = true)`: an unparseable `since`/`until` is a 400
    /// here. The list ignores one (and shows more rows), but a bulk requeue
    /// that silently dropped a bound would reset more jobs than was asked for.
    pub fn to_match_filter(&self) -> Result<MatchFilter, PlatformError> {
        Ok(MatchFilter {
            status: non_empty(&self.status),
            statuses: split_csv(self.statuses.as_deref()),
            client_id: non_empty(&self.client_id),
            client_ids: split_csv(self.client_ids.as_deref()),
            dispatch_pool_id: non_empty(&self.dispatch_pool_id),
            subscription_id: non_empty(&self.subscription_id),
            code: non_empty(&self.code),
            codes: split_csv(self.codes.as_deref()),
            source: non_empty(&self.source),
            message_group: non_empty(&self.message_group),
            applications: split_csv(self.applications.as_deref()),
            subdomains: split_csv(self.subdomains.as_deref()),
            aggregates: split_csv(self.aggregates.as_deref()),
            since: parse_timestamp("since", self.since.as_deref(), true)?,
            until: parse_timestamp("until", self.until.as_deref(), true)?,
            ..MatchFilter::default()
        })
    }
}

/// Body of `POST …/requeue-by-filter/preview`: the list's filter. With no
/// since/until the last 24 hours are counted; a window beyond
/// `FC_DISPATCH_JOB_MAX_WINDOW_DAYS` (default 31) is 400 `WINDOW_TOO_LARGE`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct RequeueByFilterPreviewRequest {
    #[serde(flatten)]
    pub filter: DispatchJobFilter,
}

/// The jobs a filter matches.
#[derive(Debug, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequeueByFilterPreviewResponse {
    /// Jobs the filter matches in the window that the caller can see
    #[schema(value_type = i64)]
    pub count: i64,
    /// count broken down by status
    #[schema(value_type = HashMap<String, i64>)]
    pub by_status: BTreeMap<String, i64>,
    /// How many of them a requeue would reset for this caller (not COMPLETED, not in flight); send it back as confirmCount
    #[schema(value_type = i64)]
    pub eligible_count: i64,
    /// COMPLETED jobs the filter matches that the caller can see. A requeue skips them; they are redriven only with redriveCompleted and confirmRedriveCompleted
    #[schema(value_type = i64)]
    pub completed_count: i64,
    /// QUEUED or PROCESSING jobs the filter matches; a requeue never resets them
    #[schema(value_type = i64)]
    pub in_flight_count: i64,
    /// eligibleCount + completedCount: the confirmCount of a request that also redrives COMPLETED jobs
    #[schema(value_type = i64)]
    pub redrive_eligible_count: i64,
    /// The created_at window the counts cover
    pub window: WindowResponse,
}

/// Body of `POST …/requeue-by-filter`: the list's filter, the eligible count
/// the caller confirmed, and a cap. The same window rules as the preview apply.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequeueByFilterRequest {
    #[serde(flatten)]
    pub filter: DispatchJobFilter,
    /// The eligibleCount the caller was shown (redriveEligibleCount when redriveCompleted). If more jobs would be reset now, nothing is reset and the response is 409 FILTER_MATCH_CHANGED.
    #[schema(value_type = i64, minimum = 0)]
    pub confirm_count: i64,
    /// Most jobs to reset in this request (default and maximum 10000)
    #[serde(default)]
    #[schema(value_type = Option<i64>, minimum = 0, maximum = 10000)]
    pub limit: Option<i64>,
    /// Also re-deliver the COMPLETED jobs the filter matches. Needs confirmRedriveCompleted=REDRIVE and the dispatch-job:redrive-completed permission (403 without it). Without both fields COMPLETED jobs are skipped and counted in skippedCompleted.
    #[serde(default)]
    pub redrive_completed: bool,
    /// The literal REDRIVE, confirming redriveCompleted.
    #[serde(default)]
    pub confirm_redrive_completed: String,
}

/// A requeue-by-filter run.
#[derive(Debug, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequeueByFilterResponse {
    /// Jobs reset to PENDING (redriven COMPLETED jobs included)
    #[schema(value_type = i64)]
    pub requeued: i64,
    /// How many of them were COMPLETED jobs re-delivered
    #[schema(value_type = i64)]
    pub redriven: i64,
    /// QUEUED or PROCESSING matches left alone (never reset)
    #[schema(value_type = i64)]
    pub skipped_in_flight: i64,
    /// COMPLETED matches left alone because the redrive was not requested and confirmed
    #[schema(value_type = i64)]
    pub skipped_completed: i64,
    /// The run stopped at its limit (or at confirmCount) with eligible jobs left
    pub truncated: bool,
    /// Eligible jobs still matching after the run; run again to continue
    #[schema(value_type = i64)]
    pub remaining: i64,
    /// The created_at window the run was held to
    pub window: WindowResponse,
}

/// A caller's view of what a filter matches (Go `MatchSummary`).
#[derive(Debug, Default)]
struct MatchSummary {
    /// Every job the filter matches that the caller can see.
    count: i64,
    by_status: BTreeMap<String, i64>,
    /// How many a requeue would reset for this caller: its tenant rule
    /// ([`reaches`]) applied to each job's client and the status guard (jobs in
    /// flight and COMPLETED jobs are not eligible). Sent back as confirmCount.
    eligible: i64,
    /// COMPLETED jobs the caller can see in the match. They are redriven only
    /// with the two redrive fields.
    completed: i64,
    /// QUEUED or PROCESSING jobs the caller can see in the match. A requeue
    /// never resets them.
    in_flight: i64,
}

impl MatchSummary {
    /// The count a confirmed redrive must confirm: eligible plus COMPLETED.
    fn redrive_eligible(&self) -> i64 {
        self.eligible + self.completed
    }

    /// The count a run must have been confirmed for.
    fn to_reset(&self, redrive: bool) -> i64 {
        if redrive {
            self.redrive_eligible()
        } else {
            self.eligible
        }
    }
}

/// Go `VisibleTo`: anchors see every job; anyone else sees platform-scoped jobs
/// plus their own tenants'. The caller's own clientId filters can only narrow
/// within that.
fn visible_to(auth: &Authenticated, mut f: MatchFilter) -> MatchFilter {
    f.accessible = (!auth.0.is_anchor()).then(|| caller_reach::client_ids(&auth.0));
    f
}

/// Go `SummarizeMatches`: counts the jobs `f` matches for the caller. `f` must
/// already carry its time window: the count scans the window's partitions and
/// nothing more.
async fn summarize(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    f: MatchFilter,
) -> Result<MatchSummary, PlatformError> {
    let buckets = state.repo.count_matches(&visible_to(auth, f)).await?;
    let mut s = MatchSummary::default();
    for b in buckets {
        s.count += b.count;
        *s.by_status.entry(b.status.clone()).or_insert(0) += b.count;
        if !reaches(&auth.0, b.client_id.as_deref()) {
            continue;
        }
        match classify_for_requeue(&b.status) {
            Verdict::Requeue => s.eligible += b.count,
            Verdict::Completed => s.completed += b.count,
            Verdict::InFlight => s.in_flight += b.count,
        }
    }
    Ok(s)
}

/// The filter held to its time window (Go `scoped`): none given is the last 24
/// hours, wider than the configured maximum is a 400.
fn windowed(
    state: &DispatchJobActionsState,
    filter: &DispatchJobFilter,
) -> Result<(MatchFilter, Window), PlatformError> {
    let mut f = filter.to_match_filter()?;
    let enforced = enforce_window(f.since, f.until, Utc::now(), state.max_window_days)?;
    f.since = Some(enforced.since);
    f.until = enforced.until;
    Ok((f, enforced.window))
}

async fn preview(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    filter: &DispatchJobFilter,
) -> Result<Json<RequeueByFilterPreviewResponse>, PlatformError> {
    let (f, window) = windowed(state, filter)?;
    let s = summarize(state, auth, f).await?;
    let redrive_eligible_count = s.redrive_eligible();
    Ok(Json(RequeueByFilterPreviewResponse {
        count: s.count,
        by_status: s.by_status,
        eligible_count: s.eligible,
        completed_count: s.completed,
        in_flight_count: s.in_flight,
        redrive_eligible_count,
        window: window.into(),
    }))
}

/// A failure part-way through: the batches before it are committed, so the
/// caller is told how many jobs were already reset (Go `interrupted`).
fn interrupted(requeued: i64, err: PlatformError) -> PlatformError {
    if requeued == 0 {
        return err;
    }
    tracing::error!(requeued, error = %err, "requeue by filter stopped part-way");
    PlatformError::Coded {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "REQUEUE_INTERRUPTED".to_string(),
        message: format!(
            "requeue stopped after {requeued} jobs were reset; run it again to continue"
        ),
        details: HashMap::from([("requeued".to_string(), serde_json::json!(requeued))]),
    }
}

/// Go `ResendByFilter`: resets the jobs the filter matches, at most
/// `min(limit, confirmCount)` of them.
///
/// Status guard: jobs QUEUED or PROCESSING are never reset; COMPLETED jobs are
/// reset only when the request confirms a redrive (both fields), in which case
/// confirmCount covers eligible + completed. Without a confirmed redrive the
/// COMPLETED matches are skipped and counted in the result.
async fn requeue_by_filter(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    req: RequeueByFilterRequest,
) -> Result<Json<RequeueByFilterResponse>, PlatformError> {
    let (f, window) = windowed(state, &req.filter)?;
    let confirm = req.confirm_count;
    if confirm < 0 {
        return Err(PlatformError::bad_request_code(
            "INVALID_CONFIRM_COUNT",
            "confirmCount must not be negative",
        ));
    }
    let limit = match req.limit.unwrap_or(0) {
        l if !(0..=RESEND_BY_FILTER_MAX_LIMIT).contains(&l) => {
            return Err(PlatformError::bad_request_code(
                "INVALID_LIMIT",
                format!("limit must be between 1 and {RESEND_BY_FILTER_MAX_LIMIT}"),
            ))
        }
        0 => RESEND_BY_FILTER_MAX_LIMIT,
        l => l,
    };
    let redrive = redrive_confirmed(req.redrive_completed, &req.confirm_redrive_completed);

    // Re-count now: if more jobs would be reset than the caller confirmed,
    // nothing is reset and the caller is told the new counts.
    let now = summarize(state, auth, f.clone()).await?;
    let to_reset = now.to_reset(redrive);
    if to_reset > confirm {
        return Err(PlatformError::Coded {
            status: StatusCode::CONFLICT,
            code: "FILTER_MATCH_CHANGED".to_string(),
            message: format!(
                "{to_reset} jobs now match this filter and can be requeued, not the {confirm} confirmed; review the new count and confirm again"
            ),
            details: HashMap::from([
                ("count".to_string(), serde_json::json!(now.count)),
                ("eligibleCount".to_string(), serde_json::json!(now.eligible)),
                (
                    "completedCount".to_string(),
                    serde_json::json!(now.completed),
                ),
                ("inFlightCount".to_string(), serde_json::json!(now.in_flight)),
                (
                    "redriveEligibleCount".to_string(),
                    serde_json::json!(now.redrive_eligible()),
                ),
                ("byStatus".to_string(), serde_json::json!(now.by_status)),
            ]),
        });
    }

    // Only walk jobs the use case would reset, so every id in a batch counts
    // towards the cap. Its tenant rule is still applied to each batch.
    let mut walk = visible_to(auth, f.clone());
    walk.exclude_platform_scoped = !reaches(&auth.0, None);
    walk.status_in = resettable_statuses(redrive);

    let budget = limit.min(confirm);
    let mut res = RunTally {
        skipped_in_flight: now.in_flight,
        skipped_completed: if redrive { 0 } else { now.completed },
        ..RunTally::default()
    };
    let mut handed: i64 = 0;
    let mut exhausted = false;
    while handed < budget {
        let page = (RESEND_BATCH_SIZE as i64).min(budget - handed);
        let ids = state
            .repo
            .matching_ids(&walk, page)
            .await
            .map_err(|e| interrupted(res.requeued, e))?;
        if ids.is_empty() {
            exhausted = true;
            break;
        }
        // Unknown and unreachable ids are dropped, as the plain requeue does.
        let command = resend_command(
            state,
            auth,
            &ids,
            redrive,
            if redrive { REDRIVE_CONFIRMATION } else { "" },
        )
        .await
        .map_err(|e| interrupted(res.requeued, e))?;
        let event = state
            .requeue_use_case
            .run(command, ExecutionContext::from_auth(&auth.0))
            .await
            .into_result()
            .map_err(|e| interrupted(res.requeued, e.into()))?;
        res.requeued += event.ids.len() as i64;
        res.redriven += event.redriven_count as i64;
        res.skipped_in_flight += event.skipped_in_flight as i64;
        res.skipped_completed += event.skipped_completed as i64;
        handed += ids.len() as i64;
        let full_page = ids.len() as i64 == page;
        walk.after_id = ids.last().cloned();
        if !full_page {
            exhausted = true;
            break;
        }
    }
    let mut remaining = 0;
    if !(exhausted || handed == 0) {
        // Stopped at the budget: count what is left after the last id handed out.
        let mut rest = f;
        rest.after_id = walk.after_id;
        remaining = summarize(state, auth, rest).await?.to_reset(redrive);
    }
    Ok(Json(RequeueByFilterResponse {
        requeued: res.requeued,
        redriven: res.redriven,
        skipped_in_flight: res.skipped_in_flight,
        skipped_completed: res.skipped_completed,
        truncated: remaining > 0,
        remaining,
        window: window.into(),
    }))
}

/// What a run has done so far.
#[derive(Debug, Default)]
struct RunTally {
    requeued: i64,
    redriven: i64,
    skipped_in_flight: i64,
    skipped_completed: i64,
}

// ── /api ─────────────────────────────────────────────────────────────────

/// Count the dispatch jobs a filter matches and how many a requeue would reset
#[utoipa::path(post, path = "/api/dispatch-jobs/requeue-by-filter/preview", tag = "dispatch-jobs",
    operation_id = "previewRequeueDispatchJobsByFilter", request_body = RequeueByFilterPreviewRequest,
    responses((status = 200, description = "OK", body = RequeueByFilterPreviewResponse)),
    security(("bearer_auth" = [])))]
pub async fn api_preview_requeue_by_filter(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Json(req): Json<RequeueByFilterPreviewRequest>,
) -> Result<Json<RequeueByFilterPreviewResponse>, PlatformError> {
    checks::can_requeue_or_read_dispatch_jobs(&auth.0)?;
    preview(&state, &auth, &req.filter).await
}

/// Reset every dispatch job a filter matches to PENDING (confirmed count, capped)
#[utoipa::path(post, path = "/api/dispatch-jobs/requeue-by-filter", tag = "dispatch-jobs",
    operation_id = "requeueDispatchJobsByFilter", request_body = RequeueByFilterRequest,
    responses((status = 200, description = "OK", body = RequeueByFilterResponse),
               (status = 409, description = "FILTER_MATCH_CHANGED")),
    security(("bearer_auth" = [])))]
pub async fn api_requeue_by_filter(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Json(req): Json<RequeueByFilterRequest>,
) -> Result<Json<RequeueByFilterResponse>, PlatformError> {
    can_requeue_dispatch_jobs_with(&auth, req.redrive_completed)?;
    requeue_by_filter(&state, &auth, req).await
}

// ── /bff ─────────────────────────────────────────────────────────────────

/// BFF twin of [`api_preview_requeue_by_filter`].
#[utoipa::path(post, path = "/bff/dispatch-jobs/requeue-by-filter/preview", tag = "bff-dispatch-jobs",
    operation_id = "previewRequeueDispatchJobsByFilterBff", request_body = RequeueByFilterPreviewRequest,
    responses((status = 200, description = "OK", body = RequeueByFilterPreviewResponse)))]
pub async fn bff_preview_requeue_by_filter(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Json(req): Json<RequeueByFilterPreviewRequest>,
) -> Result<Json<RequeueByFilterPreviewResponse>, PlatformError> {
    checks::can_requeue_or_read_dispatch_jobs(&auth.0)?;
    preview(&state, &auth, &req.filter).await
}

/// BFF twin of [`api_requeue_by_filter`].
#[utoipa::path(post, path = "/bff/dispatch-jobs/requeue-by-filter", tag = "bff-dispatch-jobs",
    operation_id = "requeueDispatchJobsByFilterBff", request_body = RequeueByFilterRequest,
    responses((status = 200, description = "OK", body = RequeueByFilterResponse),
               (status = 409, description = "FILTER_MATCH_CHANGED")))]
pub async fn bff_requeue_by_filter(
    State(state): State<DispatchJobActionsState>,
    auth: Authenticated,
    Json(req): Json<RequeueByFilterRequest>,
) -> Result<Json<RequeueByFilterResponse>, PlatformError> {
    can_requeue_dispatch_jobs_with(&auth, req.redrive_completed)?;
    requeue_by_filter(&state, &auth, req).await
}
