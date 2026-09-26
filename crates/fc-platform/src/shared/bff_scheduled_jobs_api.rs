//! BFF (Browser-For-Frontend) read endpoints for the Scheduled Jobs UI.
//!
//! Cookie/session-authenticated, response shapes tuned for the admin UI.
//! Mutations go through `/api/scheduled-jobs/*` — this router is read-only.
//!
//! Go's `shared/bff/scheduled_jobs.go` is the reference: every route asks
//! `platform:messaging:scheduled-job:view`; a job or instance the caller
//! cannot reach answers 404, as an unknown one does; optional members are
//! absent when unset; the list pages are `{data, page, size, total,
//! totalPages}`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::scheduled_job::entity::{InstanceStatus, TriggerKind};
use crate::scheduled_job::repository::JobListFilters;
use crate::scheduled_job::{
    InstanceListFilters, ScheduledJob, ScheduledJobInstance, ScheduledJobInstanceLog,
    ScheduledJobInstanceRepository, ScheduledJobRepository,
};
use crate::shared::authorization_service::AuthContext;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Clone)]
pub struct BffScheduledJobsState {
    pub repo: Arc<ScheduledJobRepository>,
    pub instance_repo: Arc<ScheduledJobInstanceRepository>,
    pub client_repo: Arc<crate::ClientRepository>,
    pub application_repo: Arc<crate::application::repository::ApplicationRepository>,
}

// ── Response DTOs ───────────────────────────────────────────────────────────

/// Go `bffScheduledJobResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffScheduledJobResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_name: Option<String>,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    pub crons: Vec<String>,
    pub timezone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    pub concurrent: bool,
    pub tracks_completion: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    pub delivery_max_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
    /// True if any instance is still in flight — the "currently running"
    /// badge.
    pub has_active_instance: bool,
}

/// Go `bffScheduledJobInstanceResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffScheduledJobInstanceResponse {
    pub id: String,
    pub scheduled_job_id: String,
    pub job_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub trigger_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<DateTime<Utc>>,
    pub fired_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub delivery_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<ScheduledJobInstance> for BffScheduledJobInstanceResponse {
    fn from(i: ScheduledJobInstance) -> Self {
        Self {
            id: i.id,
            scheduled_job_id: i.scheduled_job_id,
            job_code: i.job_code,
            client_id: i.client_id,
            trigger_kind: i.trigger_kind.as_str().into(),
            scheduled_for: i.scheduled_for,
            fired_at: i.fired_at,
            delivered_at: i.delivered_at,
            completed_at: i.completed_at,
            status: i.status.as_str().into(),
            delivery_attempts: i.delivery_attempts,
            delivery_error: i.delivery_error,
            completion_status: i.completion_status.map(|c| c.as_str().into()),
            completion_result: i.completion_result.filter(|v| !v.is_null()),
            correlation_id: i.correlation_id,
            created_at: i.created_at,
        }
    }
}

/// Go `bffInstanceLogResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffInstanceLogResponse {
    pub id: String,
    pub instance_id: String,
    pub level: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl From<ScheduledJobInstanceLog> for BffInstanceLogResponse {
    fn from(l: ScheduledJobInstanceLog) -> Self {
        Self {
            id: l.id,
            instance_id: l.instance_id,
            level: l.level.as_str().into(),
            message: l.message,
            metadata: l.metadata.filter(|v| !v.is_null()),
            created_at: l.created_at,
        }
    }
}

/// Go `bffPaginatedResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffPage<T> {
    pub data: Vec<T>,
    pub page: u32,
    pub size: u32,
    pub total: i64,
    pub total_pages: u32,
}

impl<T> BffPage<T> {
    fn new(data: Vec<T>, (page, size): (u32, u32), total: i64) -> Self {
        let total_pages = if size == 0 || total <= 0 {
            0
        } else {
            (total as f64 / size as f64).ceil() as u32
        };
        Self {
            data,
            page,
            size,
            total,
            total_pages,
        }
    }
}

/// Filter options for the list page dropdowns (Go
/// `bffScheduledJobsFilterOptions`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffScheduledJobsFilterOptions {
    pub clients: Vec<FilterOption>,
    pub applications: Vec<FilterOption>,
    pub statuses: Vec<FilterOption>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOption {
    pub value: String,
    pub label: String,
}

// ── Query parsing (Go's lenient parsers) ────────────────────────────────────

type RawQuery = HashMap<String, String>;

/// Go `parsePagination`: `page` (default 0), `size` or `pageSize`
/// (default 20, at most 200); an unparsable value keeps the default.
fn pagination(q: &RawQuery) -> (u32, u32) {
    let page = q.get("page").and_then(|p| p.parse().ok()).unwrap_or(0);
    let size = q
        .get("size")
        .filter(|s| !s.is_empty())
        .or_else(|| q.get("pageSize"))
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(20)
        .min(200);
    (page, size)
}

/// Go `splitCSV`: trimmed, empties dropped.
fn split_csv(q: &RawQuery, key: &str) -> Vec<String> {
    q.get(key)
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Go `parseTimeParam`: RFC 3339; anything else is no filter.
fn time_param(q: &RawQuery, key: &str) -> Option<DateTime<Utc>> {
    q.get(key)
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        .map(|t| t.with_timezone(&Utc))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Go `canViewJob` / `canViewInstance`: a platform-scoped row is the
/// anchor's; a client's row, whoever reaches that client.
fn can_view(auth: &AuthContext, client_id: Option<&str>) -> bool {
    match client_id {
        Some(cid) => auth.can_access_client(cid),
        None => auth.is_anchor(),
    }
}

async fn names(
    state: &BffScheduledJobsState,
) -> Result<(HashMap<String, String>, HashMap<String, String>), PlatformError> {
    let (clients, applications) = tokio::try_join!(
        state.client_repo.find_all(),
        state.application_repo.find_all()
    )?;
    Ok((
        clients.into_iter().map(|c| (c.id, c.name)).collect(),
        applications.into_iter().map(|a| (a.id, a.name)).collect(),
    ))
}

fn to_bff_job(
    j: ScheduledJob,
    clients: &HashMap<String, String>,
    applications: &HashMap<String, String>,
    active: bool,
) -> BffScheduledJobResponse {
    BffScheduledJobResponse {
        client_name: j.client_id.as_ref().and_then(|c| clients.get(c).cloned()),
        application_name: j
            .application_id
            .as_ref()
            .and_then(|a| applications.get(a).cloned()),
        id: j.id,
        client_id: j.client_id,
        application_id: j.application_id,
        code: j.code,
        name: j.name,
        description: j.description,
        status: j.status.as_str().into(),
        crons: j.crons,
        timezone: j.timezone,
        payload: j.payload.filter(|v| !v.is_null()),
        concurrent: j.concurrent,
        tracks_completion: j.tracks_completion,
        timeout_seconds: j.timeout_seconds,
        delivery_max_attempts: j.delivery_max_attempts,
        target_url: j.target_url,
        last_fired_at: j.last_fired_at,
        created_at: j.created_at,
        updated_at: j.updated_at,
        version: j.version,
        has_active_instance: active,
    }
}

/// A job the caller can see, or 404 (Go answers an unreachable job as an
/// unknown one).
async fn visible_job(
    state: &BffScheduledJobsState,
    auth: &AuthContext,
    id: &str,
) -> Result<ScheduledJob, PlatformError> {
    state
        .repo
        .find_by_id(id)
        .await?
        .filter(|j| can_view(auth, j.client_id.as_deref()))
        .ok_or_else(|| PlatformError::not_found("ScheduledJob", id))
}

async fn visible_instance(
    state: &BffScheduledJobsState,
    auth: &AuthContext,
    id: &str,
) -> Result<ScheduledJobInstance, PlatformError> {
    state
        .instance_repo
        .find_by_id(id)
        .await?
        .filter(|i| can_view(auth, i.client_id.as_deref()))
        .ok_or_else(|| PlatformError::not_found("ScheduledJobInstance", id))
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// `GET /bff/scheduled-jobs?clientIds=&applicationIds=&statuses=&search=&page=&size=`
async fn list_jobs(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Query(q): Query<RawQuery>,
) -> Result<Json<BffPage<BffScheduledJobResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;
    let page = pagination(&q);

    let mut filters = JobListFilters {
        client_ids: split_csv(&q, "clientIds"),
        application_ids: split_csv(&q, "applicationIds"),
        statuses: split_csv(&q, "statuses"),
        search: q
            .get("search")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    };
    // A non-anchor caller sees only its clients' jobs; that goes into the
    // query so the count and the page agree with the visible rows.
    if !auth.0.is_anchor() {
        let allowed: Vec<String> = if filters.client_ids.is_empty() {
            auth.0.accessible_clients.clone()
        } else {
            filters
                .client_ids
                .iter()
                .filter(|c| auth.0.can_access_client(c))
                .cloned()
                .collect()
        };
        if allowed.is_empty() {
            return Ok(Json(BffPage::new(vec![], page, 0)));
        }
        filters.client_ids = allowed;
    }

    let (page_no, size) = page;
    let (total, rows) = tokio::try_join!(
        state.repo.count_by_list_filters(&filters),
        state
            .repo
            .find_by_list_filters(&filters, size as i64, (page_no as i64) * (size as i64)),
    )?;
    let visible: Vec<ScheduledJob> = rows
        .into_iter()
        .filter(|j| can_view(&auth.0, j.client_id.as_deref()))
        .collect();
    let keys: Vec<(String, bool)> = visible
        .iter()
        .map(|j| (j.id.clone(), j.tracks_completion))
        .collect();
    let ((clients, applications), active) =
        tokio::try_join!(names(&state), state.instance_repo.active_job_ids(&keys))?;
    let data = visible
        .into_iter()
        .map(|j| {
            let is_active = active.contains(&j.id);
            to_bff_job(j, &clients, &applications, is_active)
        })
        .collect();

    Ok(Json(BffPage::new(data, page, total)))
}

async fn get_job(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<BffScheduledJobResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;
    let j = visible_job(&state, &auth.0, &id).await?;
    let keys = [(j.id.clone(), j.tracks_completion)];
    let ((clients, applications), active) =
        tokio::try_join!(names(&state), state.instance_repo.active_job_ids(&keys))?;
    let is_active = active.contains(&j.id);
    Ok(Json(to_bff_job(j, &clients, &applications, is_active)))
}

/// `GET /bff/scheduled-jobs/{id}/instances?status=&triggerKind=&from=&to=&page=&size=`
async fn list_instances(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Query(q): Query<RawQuery>,
) -> Result<Json<BffPage<BffScheduledJobInstanceResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;
    visible_job(&state, &auth.0, &id).await?;
    let page = pagination(&q);
    let (page_no, size) = page;

    let status = match q.get("status").filter(|s| !s.is_empty()) {
        Some(s) => Some(s.parse::<InstanceStatus>().map_err(|_| {
            PlatformError::bad_request_code(
                "INVALID_STATUS",
                "status must be a known instance status",
            )
        })?),
        None => None,
    };
    let trigger_kind = match q.get("triggerKind").filter(|s| !s.is_empty()) {
        Some(t) => Some(t.parse::<TriggerKind>().map_err(|_| {
            PlatformError::bad_request_code(
                "INVALID_TRIGGER_KIND",
                "triggerKind must be CRON, MANUAL, or BACKFILL",
            )
        })?),
        None => None,
    };
    let filters = InstanceListFilters {
        scheduled_job_id: Some(&id),
        client_id: None,
        status,
        trigger_kind,
        from: time_param(&q, "from"),
        to: time_param(&q, "to"),
        limit: Some(size as i64),
        offset: Some((page_no as i64) * (size as i64)),
    };
    let count_filters = InstanceListFilters {
        limit: None,
        offset: None,
        ..filters.clone()
    };
    let (rows, total) = tokio::try_join!(
        state.instance_repo.list(&filters),
        state.instance_repo.count(&count_filters)
    )?;
    Ok(Json(BffPage::new(
        rows.into_iter().map(Into::into).collect(),
        page,
        total,
    )))
}

async fn get_instance(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Path(instance_id): Path<String>,
) -> Result<Json<BffScheduledJobInstanceResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;
    let inst = visible_instance(&state, &auth.0, &instance_id).await?;
    Ok(Json(inst.into()))
}

/// A bare array: the SPA consumes the list directly.
async fn list_instance_logs(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Path(instance_id): Path<String>,
    Query(q): Query<RawQuery>,
) -> Result<Json<Vec<BffInstanceLogResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;
    visible_instance(&state, &auth.0, &instance_id).await?;
    let limit = q
        .get("limit")
        .and_then(|l| l.parse::<i64>().ok())
        .filter(|n| *n > 0);
    let logs = state
        .instance_repo
        .list_logs_for_instance(&instance_id, limit)
        .await?;
    Ok(Json(logs.into_iter().map(Into::into).collect()))
}

async fn filter_options(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
) -> Result<Json<BffScheduledJobsFilterOptions>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;

    let (clients, applications) = tokio::try_join!(
        state.client_repo.find_all(),
        state.application_repo.find_all()
    )?;

    // The synthetic "platform" entry for platform-scoped jobs (anchor
    // only), then the active clients the caller reaches, by name.
    let mut client_options = Vec::new();
    if auth.0.is_anchor() {
        client_options.push(FilterOption {
            value: "platform".into(),
            label: "Platform-scoped".into(),
        });
    }
    let mut visible: Vec<FilterOption> = clients
        .into_iter()
        .filter(|c| c.status == crate::client::entity::ClientStatus::Active)
        .filter(|c| auth.0.is_anchor() || auth.0.can_access_client(&c.id))
        .map(|c| FilterOption {
            value: c.id,
            label: c.name,
        })
        .collect();
    visible.sort_by(|a, b| a.label.cmp(&b.label));
    client_options.extend(visible);

    let mut app_options: Vec<FilterOption> = applications
        .into_iter()
        .filter(|a| a.active)
        .map(|a| FilterOption {
            value: a.id,
            label: a.name,
        })
        .collect();
    app_options.sort_by(|a, b| a.label.cmp(&b.label));

    let statuses = [
        ("ACTIVE", "Active"),
        ("PAUSED", "Paused"),
        ("ARCHIVED", "Archived"),
    ]
    .into_iter()
    .map(|(value, label)| FilterOption {
        value: value.into(),
        label: label.into(),
    })
    .collect();

    Ok(Json(BffScheduledJobsFilterOptions {
        clients: client_options,
        applications: app_options,
        statuses,
    }))
}

pub fn bff_scheduled_jobs_router(state: BffScheduledJobsState) -> Router {
    Router::new()
        .route("/", get(list_jobs))
        .route("/filter-options", get(filter_options))
        .route("/{id}", get(get_job))
        .route("/{id}/instances", get(list_instances))
        .route("/instances/{instanceId}", get(get_instance))
        .route("/instances/{instanceId}/logs", get(list_instance_logs))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> RawQuery {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn pagination_is_gos() {
        assert_eq!(pagination(&q(&[])), (0, 20));
        assert_eq!(pagination(&q(&[("page", "2"), ("size", "5")])), (2, 5));
        assert_eq!(pagination(&q(&[("pageSize", "7")])), (0, 7));
        assert_eq!(pagination(&q(&[("size", "900")])), (0, 200));
        assert_eq!(pagination(&q(&[("size", "x"), ("page", "y")])), (0, 20));
    }

    #[test]
    fn total_pages_round_up_and_zero_when_empty() {
        let p: BffPage<()> = BffPage::new(vec![], (0, 20), 41);
        assert_eq!(p.total_pages, 3);
        let p: BffPage<()> = BffPage::new(vec![], (0, 20), 0);
        assert_eq!(p.total_pages, 0);
    }
}
