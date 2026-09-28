//! Debug BFF API
//!
//! Raw/debug endpoints for admin access to transactional data.
//! These endpoints query the raw collections (events, dispatch_jobs)
//! rather than the optimized read projections.

use crate::shared::authorization_service::checks;
use crate::shared::error::{PlatformError, Result};
use crate::shared::middleware::Authenticated;
use crate::{DispatchJob, Event};
use crate::{DispatchJobRepository, EventRepository};
use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::event::api::ContextDataDto;

/// Debug list query — `?size=` only. Debug grids look at the most recent
/// rows; no pagination.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugListQuery {
    /// Result size. Default 50, capped at 1000.
    pub size: Option<u32>,
}

impl DebugListQuery {
    fn limit(&self) -> i64 {
        self.size.unwrap_or(50).clamp(1, 1000) as i64
    }
}

// ============================================================================
// State
// ============================================================================

#[derive(Clone)]
pub struct DebugState {
    pub event_repo: Arc<EventRepository>,
    pub dispatch_job_repo: Arc<DispatchJobRepository>,
}

// ============================================================================
// DTOs - Raw Events
// ============================================================================

/// Go's `RawEventResponse` (event/api/dto.go), the row of
/// `GET /bff/debug/events` and the body of `GET /bff/debug/events/{id}`: the
/// write-side `msg_events` row with its context data. The type is
/// `eventType` here, not `type` (the SPA's raw-event page binds it). Absent
/// members are left out, as Go's `omitempty` leaves them.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RawEventResponse {
    pub id: String,
    pub spec_version: String,
    pub event_type: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[schema(format = DateTime)]
    pub time: String,
    /// The event payload; absent when the row has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<serde_json::Value>)]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deduplication_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[schema(required = false)]
    pub context_data: Vec<ContextDataDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl From<&Event> for RawEventResponse {
    fn from(event: &Event) -> Self {
        let blank = |v: &Option<String>| v.clone().filter(|s| !s.is_empty());
        Self {
            id: event.id.clone(),
            spec_version: event.spec_version.clone(),
            event_type: event.event_type.clone(),
            source: event.source.clone(),
            subject: blank(&event.subject),
            time: event.time.to_rfc3339(),
            data: Some(event.data.clone()).filter(|d| !d.is_null()),
            message_group: event.message_group.clone(),
            correlation_id: event.correlation_id.clone(),
            causation_id: event.causation_id.clone(),
            deduplication_id: blank(&event.deduplication_id),
            context_data: event
                .context_data
                .iter()
                .cloned()
                .map(ContextDataDto::from)
                .collect(),
            client_id: event.client_id.clone(),
        }
    }
}

// ============================================================================
// DTOs - Raw Dispatch Jobs
// ============================================================================

/// Go's `RawDispatchJobResponse` (dispatchjob/api/dto.go), the row of
/// `GET /bff/debug/dispatch-jobs`: the write-side `msg_dispatch_jobs` row,
/// with the payload's length (not the payload) and the attempt count. Absent
/// members are left out, as Go's `omitempty` leaves them.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RawDispatchJobResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub kind: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub target_url: String,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    pub mode: String,
    pub sequence: i32,
    pub status: String,
    #[schema(value_type = i32)]
    pub attempt_count: u32,
    pub max_retries: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub timeout_seconds: u32,
    pub retry_strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(format = DateTime)]
    pub scheduled_for: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(format = DateTime)]
    pub completed_at: Option<String>,
    pub payload_content_type: String,
    /// The payload's length in bytes (the payload itself stays out).
    #[schema(value_type = i64)]
    pub payload_length: usize,
    #[schema(value_type = i64)]
    pub attempt_history_count: usize,
}

impl From<&DispatchJob> for RawDispatchJobResponse {
    fn from(job: &DispatchJob) -> Self {
        Self {
            id: job.id.clone(),
            external_id: job.external_id.clone(),
            source: job.source.clone(),
            kind: job.kind.as_str().to_string(),
            code: job.code.clone(),
            subject: job.subject.clone(),
            event_id: job.event_id.clone(),
            correlation_id: job.correlation_id.clone(),
            target_url: job.target_url.clone(),
            protocol: job.protocol.as_str().to_string(),
            client_id: job.client_id.clone(),
            subscription_id: job.subscription_id.clone(),
            service_account_id: job.service_account_id.clone(),
            dispatch_pool_id: job.dispatch_pool_id.clone(),
            message_group: job.message_group.clone(),
            mode: job.mode.as_str().to_string(),
            sequence: job.sequence,
            status: job.status.as_str().to_string(),
            attempt_count: job.attempt_count,
            max_retries: job.max_retries,
            last_error: job.last_error.clone(),
            timeout_seconds: job.timeout_seconds,
            retry_strategy: job.retry_strategy.as_str().to_string(),
            idempotency_key: job.idempotency_key.clone(),
            created_at: job.created_at.to_rfc3339(),
            updated_at: job.updated_at.to_rfc3339(),
            scheduled_for: job.scheduled_for.map(|t| t.to_rfc3339()),
            completed_at: job.completed_at.map(|t| t.to_rfc3339()),
            payload_content_type: job.payload_content_type.clone(),
            payload_length: job.payload.as_ref().map(|p| p.len()).unwrap_or(0),
            attempt_history_count: job.attempts.len(),
        }
    }
}

// ============================================================================
// Handlers - Raw Events
// ============================================================================

/// List raw events (debug/admin). Returns the most recent rows; no
/// pagination — `msg_events` ingests at high rates and page navigation is
/// meaningless.
pub(super) async fn list_raw_events(
    State(state): State<DebugState>,
    auth: Authenticated,
    Query(params): Query<DebugListQuery>,
) -> Result<Json<Vec<RawEventResponse>>> {
    checks::can_read_events_raw(&auth.0)?;
    let events = state
        .event_repo
        .find_recent_with_cursor(None, params.limit())
        .await?;
    let items = events.iter().map(RawEventResponse::from).collect();
    Ok(Json(items))
}

/// Get a single raw event by ID (debug/admin only)
pub(super) async fn get_raw_event(
    State(state): State<DebugState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<RawEventResponse>> {
    checks::can_read_events_raw(&auth.0)?;
    let event = state
        .event_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Event", &id))?;

    Ok(Json(RawEventResponse::from(&event)))
}

// ============================================================================
// Handlers - Raw Dispatch Jobs
// ============================================================================

/// List raw dispatch jobs (debug/admin). Returns the most recent rows; no
/// pagination — `msg_dispatch_jobs` ingests at high rates and page
/// navigation is meaningless.
pub(super) async fn list_raw_dispatch_jobs(
    State(state): State<DebugState>,
    auth: Authenticated,
    Query(params): Query<DebugListQuery>,
) -> Result<Json<Vec<RawDispatchJobResponse>>> {
    checks::can_read_dispatch_jobs_raw(&auth.0)?;
    let jobs = state
        .dispatch_job_repo
        .find_recent_with_cursor(None, params.limit())
        .await?;
    let items = jobs.iter().map(RawDispatchJobResponse::from).collect();
    Ok(Json(items))
}

/// Get a single raw dispatch job by ID (debug/admin only)
pub(super) async fn get_raw_dispatch_job(
    State(state): State<DebugState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<RawDispatchJobResponse>> {
    checks::can_read_dispatch_jobs_raw(&auth.0)?;
    let job = state
        .dispatch_job_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("DispatchJob", &id))?;

    Ok(Json(RawDispatchJobResponse::from(&job)))
}

// ============================================================================
// Router
// ============================================================================
