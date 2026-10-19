//! Requeue by filter (Go `dispatchjob/operations/resend_by_filter.go`, handlers
//! in `dispatchjob/api/api.go`): reset every job a list filter matches, not
//! just the ids a caller has in hand, on the `/api` and `/bff` tiers.
//!
//! - `POST …/dispatch-jobs/requeue-by-filter/preview` → `{count, byStatus,
//!   eligibleCount}`
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
use chrono::{DateTime, Utc};
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::usecase::{ExecutionContext, UseCase};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::api::DispatchJobActionsState;
use super::operations::{reaches, ResendCommand};
use super::repository::MatchFilter;

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
        let ts = |name: &str, v: &Option<String>| -> Result<Option<DateTime<Utc>>, PlatformError> {
            match v.as_deref().filter(|v| !v.is_empty()) {
                None => Ok(None),
                Some(v) => DateTime::parse_from_rfc3339(v)
                    .map(|t| Some(t.with_timezone(&Utc)))
                    .map_err(|_| {
                        PlatformError::bad_request_code(
                            "INVALID_TIMESTAMP",
                            format!("{name} must be an RFC3339 timestamp"),
                        )
                    }),
            }
        };
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
            since: ts("since", &self.since)?,
            until: ts("until", &self.until)?,
            ..MatchFilter::default()
        })
    }
}

/// Body of `POST …/requeue-by-filter/preview`: the list's filter.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct RequeueByFilterPreviewRequest {
    #[serde(flatten)]
    pub filter: DispatchJobFilter,
}

/// The jobs a filter matches.
#[derive(Debug, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequeueByFilterPreviewResponse {
    /// Jobs the filter matches that the caller can see
    #[schema(value_type = i64)]
    pub count: i64,
    /// count broken down by status
    #[schema(value_type = HashMap<String, i64>)]
    pub by_status: BTreeMap<String, i64>,
    /// How many of them a requeue would reset for this caller; send it back as confirmCount
    #[schema(value_type = i64)]
    pub eligible_count: i64,
}

/// Body of `POST …/requeue-by-filter`: the list's filter, the eligible count
/// the caller confirmed, and a cap.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequeueByFilterRequest {
    #[serde(flatten)]
    pub filter: DispatchJobFilter,
    /// The eligibleCount the caller was shown. If more jobs are eligible now, nothing is reset and the response is 409 FILTER_MATCH_CHANGED.
    #[schema(value_type = i64, minimum = 0)]
    pub confirm_count: i64,
    /// Most jobs to reset in this request (default and maximum 10000)
    #[serde(default)]
    #[schema(value_type = Option<i64>, minimum = 0, maximum = 10000)]
    pub limit: Option<i64>,
}

/// A requeue-by-filter run.
#[derive(Debug, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequeueByFilterResponse {
    /// Jobs reset to PENDING
    #[schema(value_type = i64)]
    pub requeued: i64,
    /// The run stopped at its limit (or at confirmCount) with eligible jobs left
    pub truncated: bool,
    /// Eligible jobs still matching after the run; run again to continue
    #[schema(value_type = i64)]
    pub remaining: i64,
}

/// A caller's view of what a filter matches (Go `MatchSummary`).
#[derive(Debug, Default)]
struct MatchSummary {
    count: i64,
    by_status: BTreeMap<String, i64>,
    eligible: i64,
}

/// Go `VisibleTo`: anchors see every job; anyone else sees platform-scoped jobs
/// plus their own tenants'. The caller's own clientId filters can only narrow
/// within that.
fn visible_to(auth: &Authenticated, mut f: MatchFilter) -> MatchFilter {
    f.accessible = (!auth.0.is_anchor()).then(|| caller_reach::client_ids(&auth.0));
    f
}

/// Go `SummarizeMatches`: `eligible` counts the jobs the requeue use case would
/// reset for this caller (its tenant rule applied to each job's client).
async fn summarize(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    f: MatchFilter,
) -> Result<MatchSummary, PlatformError> {
    let buckets = state.repo.count_matches(&visible_to(auth, f)).await?;
    let mut s = MatchSummary::default();
    for b in buckets {
        s.count += b.count;
        *s.by_status.entry(b.status).or_insert(0) += b.count;
        if reaches(&auth.0, b.client_id.as_deref()) {
            s.eligible += b.count;
        }
    }
    Ok(s)
}

async fn preview(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    filter: &DispatchJobFilter,
) -> Result<Json<RequeueByFilterPreviewResponse>, PlatformError> {
    let f = filter.to_match_filter()?;
    let s = summarize(state, auth, f).await?;
    Ok(Json(RequeueByFilterPreviewResponse {
        count: s.count,
        by_status: s.by_status,
        eligible_count: s.eligible,
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

async fn requeue_by_filter(
    state: &DispatchJobActionsState,
    auth: &Authenticated,
    req: RequeueByFilterRequest,
) -> Result<Json<RequeueByFilterResponse>, PlatformError> {
    let f = req.filter.to_match_filter()?;
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

    // Re-count now: if more jobs are eligible than the caller confirmed,
    // nothing is reset and the caller is told the new counts.
    let now = summarize(state, auth, f.clone()).await?;
    if now.eligible > confirm {
        return Err(PlatformError::Coded {
            status: StatusCode::CONFLICT,
            code: "FILTER_MATCH_CHANGED".to_string(),
            message: format!(
                "{} jobs now match this filter and can be requeued, not the {} confirmed; review the new count and confirm again",
                now.eligible, confirm
            ),
            details: HashMap::from([
                ("count".to_string(), serde_json::json!(now.count)),
                ("eligibleCount".to_string(), serde_json::json!(now.eligible)),
                ("byStatus".to_string(), serde_json::json!(now.by_status)),
            ]),
        });
    }

    // Only walk jobs the use case would reset, so every id in a batch counts
    // towards the cap. Its tenant rule is still applied to each batch.
    let mut walk = visible_to(auth, f.clone());
    walk.exclude_platform_scoped = !reaches(&auth.0, None);

    let budget = limit.min(confirm);
    let mut requeued: i64 = 0;
    let mut handed: i64 = 0;
    let mut exhausted = false;
    while handed < budget {
        let page = (RESEND_BATCH_SIZE as i64).min(budget - handed);
        let ids = state
            .repo
            .matching_ids(&walk, page)
            .await
            .map_err(|e| interrupted(requeued, e))?;
        if ids.is_empty() {
            exhausted = true;
            break;
        }
        // Unknown and unreachable ids are dropped, as the plain requeue does.
        let jobs: Vec<(String, DateTime<Utc>)> = state
            .repo
            .heads(&ids)
            .await
            .map_err(|e| interrupted(requeued, e))?
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
            .into_result()
            .map_err(|e| interrupted(requeued, e.into()))?;
        requeued += event.ids.len() as i64;
        handed += ids.len() as i64;
        let full_page = ids.len() as i64 == page;
        walk.after_id = ids.last().cloned();
        if !full_page {
            exhausted = true;
            break;
        }
    }
    if exhausted || handed == 0 {
        return Ok(Json(RequeueByFilterResponse {
            requeued,
            truncated: false,
            remaining: 0,
        }));
    }
    // Stopped at the budget: count what is left after the last id handed out.
    let mut rest = f;
    rest.after_id = walk.after_id;
    let left = summarize(state, auth, rest).await?;
    Ok(Json(RequeueByFilterResponse {
        requeued,
        truncated: left.eligible > 0,
        remaining: left.eligible,
    }))
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
    checks::can_read_dispatch_jobs(&auth.0)?;
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
    checks::can_read_dispatch_jobs(&auth.0)?;
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
    checks::can_read_dispatch_jobs(&auth.0)?;
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
    checks::can_read_dispatch_jobs(&auth.0)?;
    requeue_by_filter(&state, &auth, req).await
}
