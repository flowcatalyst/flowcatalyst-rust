//! BFF (Browser-For-Frontend) read endpoints for the Scheduled Jobs UI.
//!
//! Cookie/session-authenticated, response shapes tuned for the admin UI.
//! Mutations go through `/api/scheduled-jobs/*` — this router is read-only.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::scheduled_job::entity::{InstanceStatus, ScheduledJobStatus, TriggerKind};
use crate::scheduled_job::{
    InstanceListFilters, ScheduledJob, ScheduledJobInstance, ScheduledJobInstanceLog,
    ScheduledJobInstanceRepository, ScheduledJobRepository,
};
use crate::shared::api_common::{PaginatedResponse, PaginationParams};
use crate::shared::error::{NotFoundExt, PlatformError};
use crate::shared::middleware::Authenticated;

#[derive(Clone)]
pub struct BffScheduledJobsState {
    pub repo: Arc<ScheduledJobRepository>,
    pub instance_repo: Arc<ScheduledJobInstanceRepository>,
    pub client_repo: Arc<crate::ClientRepository>,
}

// ── Response DTOs ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffScheduledJobResponse {
    pub id: String,
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub crons: Vec<String>,
    pub timezone: String,
    pub payload: Option<serde_json::Value>,
    pub concurrent: bool,
    pub tracks_completion: bool,
    pub timeout_seconds: Option<i32>,
    pub delivery_max_attempts: i32,
    pub target_url: Option<String>,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
    /// True if any instance is in a non-terminal state — used as the
    /// "currently running" badge.
    pub has_active_instance: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffScheduledJobInstanceResponse {
    pub id: String,
    pub scheduled_job_id: String,
    pub job_code: String,
    pub client_id: Option<String>,
    pub trigger_kind: String,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub fired_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub delivery_attempts: i32,
    pub delivery_error: Option<String>,
    pub completion_status: Option<String>,
    pub completion_result: Option<serde_json::Value>,
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
            completion_result: i.completion_result,
            correlation_id: i.correlation_id,
            created_at: i.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffInstanceLogResponse {
    pub id: String,
    pub instance_id: String,
    pub level: String,
    pub message: String,
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
            metadata: l.metadata,
            created_at: l.created_at,
        }
    }
}

/// Filter options for the list page dropdowns.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffScheduledJobsFilterOptions {
    pub clients: Vec<FilterOption>,
    pub statuses: Vec<FilterOption>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOption {
    pub value: String,
    pub label: String,
}

// ── Query params ────────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BffJobsQuery {
    pub client_id: Option<String>,
    pub status: Option<String>,
    pub search: Option<String>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BffInstancesQuery {
    pub status: Option<String>,
    pub trigger_kind: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn list_jobs(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Query(q): Query<BffJobsQuery>,
) -> Result<Json<PaginatedResponse<BffScheduledJobResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;

    let client_filter: Option<Option<&str>> = match q.client_id.as_deref() {
        Some("platform") => Some(None),
        Some(c) => Some(Some(c)),
        None => None,
    };
    let status_filter = q.status.as_deref().map(ScheduledJobStatus::from_str);

    let jobs = state
        .repo
        .find_with_filters(
            client_filter,
            status_filter,
            q.search.as_deref(),
            Some(q.pagination.limit()),
            Some(q.pagination.offset() as i64),
        )
        .await?;
    let total = state
        .repo
        .count_with_filters(client_filter, status_filter, q.search.as_deref())
        .await? as u64;

    let visible: Vec<ScheduledJob> = jobs
        .into_iter()
        .filter(|j| match &j.client_id {
            Some(cid) => auth.0.can_access_client(cid),
            None => auth.0.is_anchor(),
        })
        .collect();

    // Hydrate client names + active-instance flag.
    let mut data = Vec::with_capacity(visible.len());
    for j in visible {
        let client_name = match &j.client_id {
            Some(cid) => state
                .client_repo
                .find_by_id(cid)
                .await
                .ok()
                .flatten()
                .map(|c| c.name),
            None => Some("Platform".to_string()),
        };
        let active = state
            .instance_repo
            .has_active_instance(&j.id)
            .await
            .unwrap_or(false);
        data.push(BffScheduledJobResponse {
            id: j.id,
            client_id: j.client_id,
            client_name,
            code: j.code,
            name: j.name,
            description: j.description,
            status: j.status.as_str().into(),
            crons: j.crons,
            timezone: j.timezone,
            payload: j.payload,
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
        });
    }

    Ok(Json(PaginatedResponse::new(
        data,
        q.pagination.page(),
        q.pagination.size(),
        total,
    )))
}

async fn get_job(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<BffScheduledJobResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;
    let j = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    if let Some(cid) = &j.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this scheduled job"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can view platform-scoped scheduled jobs",
        ));
    }

    let client_name = match &j.client_id {
        Some(cid) => state
            .client_repo
            .find_by_id(cid)
            .await
            .ok()
            .flatten()
            .map(|c| c.name),
        None => Some("Platform".to_string()),
    };
    let active = state
        .instance_repo
        .has_active_instance(&j.id)
        .await
        .unwrap_or(false);

    Ok(Json(BffScheduledJobResponse {
        id: j.id,
        client_id: j.client_id,
        client_name,
        code: j.code,
        name: j.name,
        description: j.description,
        status: j.status.as_str().into(),
        crons: j.crons,
        timezone: j.timezone,
        payload: j.payload,
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
    }))
}

async fn list_instances(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Query(q): Query<BffInstancesQuery>,
) -> Result<Json<PaginatedResponse<BffScheduledJobInstanceResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_job_instances(&auth.0)?;
    let job = state
        .repo
        .find_by_id(&id)
        .await?
        .or_not_found("ScheduledJob", &id)?;
    if let Some(cid) = &job.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Anchor only"));
    }

    let status = q.status.as_deref().map(InstanceStatus::from_str);
    let trigger = q.trigger_kind.as_deref().map(TriggerKind::from_str);
    let filters = InstanceListFilters {
        scheduled_job_id: Some(&id),
        client_id: None,
        status,
        trigger_kind: trigger,
        from: q.from,
        to: q.to,
        limit: Some(q.pagination.limit()),
        offset: Some(q.pagination.offset() as i64),
    };
    let count_filters = InstanceListFilters {
        limit: None,
        offset: None,
        ..filters.clone()
    };
    let rows = state.instance_repo.list(&filters).await?;
    let total = state.instance_repo.count(&count_filters).await? as u64;
    Ok(Json(PaginatedResponse::new(
        rows.into_iter().map(Into::into).collect(),
        q.pagination.page(),
        q.pagination.size(),
        total,
    )))
}

async fn get_instance(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Path(instance_id): Path<String>,
) -> Result<Json<BffScheduledJobInstanceResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_job_instances(&auth.0)?;
    let inst = state
        .instance_repo
        .find_by_id(&instance_id)
        .await?
        .or_not_found("ScheduledJobInstance", &instance_id)?;
    if let Some(cid) = &inst.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Anchor only"));
    }
    Ok(Json(inst.into()))
}

async fn list_instance_logs(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
    Path(instance_id): Path<String>,
) -> Result<Json<Vec<BffInstanceLogResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_job_instances(&auth.0)?;
    let inst = state
        .instance_repo
        .find_by_id(&instance_id)
        .await?
        .or_not_found("ScheduledJobInstance", &instance_id)?;
    if let Some(cid) = &inst.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Anchor only"));
    }
    let logs = state
        .instance_repo
        .list_logs_for_instance(&instance_id, None)
        .await?;
    Ok(Json(logs.into_iter().map(Into::into).collect()))
}

async fn filter_options(
    State(state): State<BffScheduledJobsState>,
    auth: Authenticated,
) -> Result<Json<BffScheduledJobsFilterOptions>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_scheduled_jobs(&auth.0)?;

    // Clients the caller can see, plus the synthetic "platform" entry for
    // platform-scoped jobs (anchor only).
    let clients = state.client_repo.find_all().await.unwrap_or_default();
    let mut client_options: Vec<FilterOption> = clients
        .into_iter()
        .filter(|c| auth.0.can_access_client(&c.id))
        .map(|c| FilterOption {
            value: c.id.clone(),
            label: c.name,
        })
        .collect();
    if auth.0.is_anchor() {
        client_options.insert(
            0,
            FilterOption {
                value: "platform".into(),
                label: "Platform-scoped".into(),
            },
        );
    }

    let statuses = vec![
        FilterOption {
            value: "ACTIVE".into(),
            label: "Active".into(),
        },
        FilterOption {
            value: "PAUSED".into(),
            label: "Paused".into(),
        },
        FilterOption {
            value: "ARCHIVED".into(),
            label: "Archived".into(),
        },
    ];

    Ok(Json(BffScheduledJobsFilterOptions {
        clients: client_options,
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
