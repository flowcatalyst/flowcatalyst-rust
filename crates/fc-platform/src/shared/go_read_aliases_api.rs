//! Read routes Go serves under names Rust lacked, answered by Rust's own
//! handlers (Go `event/api/api.go:37,68`, `dispatchjob/api/api.go:69-105`):
//!
//! - `GET /api/events/list-raw`, `GET /bff/events/list-raw`: the event list,
//!   gated on `event:view-raw`
//! - `GET /api/dispatch-jobs/list-raw`, `GET /bff/dispatch-jobs/list-raw`:
//!   the dispatch-job list, gated on `dispatch-job:view-raw` (and `:view`)
//! - `GET /api/dispatch-jobs/event/{eventId}`, `GET /bff/dispatch-jobs/event/{eventId}`:
//!   Rust's `by-event` list (Go's alias spelling)

use axum::{
    extract::{Path, Query, State},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::dispatch_job::api::{DispatchJobReadResponse, DispatchJobsQuery, DispatchJobsState};
use crate::event::api::{EventListItem, EventsQuery, EventsState};
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Clone)]
pub struct ReadAliasesState {
    pub events: EventsState,
    pub dispatch_jobs: DispatchJobsState,
}

async fn events_list_raw(
    state: ReadAliasesState,
    auth: Authenticated,
    q: EventsQuery,
) -> Result<Json<Vec<EventListItem>>, PlatformError> {
    // Go's `listRaw` asks `event:view-raw` only (checked by the callers).
    crate::event::api::list_events_unchecked(&state.events, &auth, q).await
}

/// The event list, for a caller holding `event:view-raw` (Go `listEventsRaw`).
#[utoipa::path(get, path = "/api/events/list-raw", tag = "events",
    operation_id = "listEventsRaw", params(EventsQuery),
    responses((status = 200, description = "Events", body = Vec<EventListItem>)),
    security(("bearer_auth" = [])))]
pub async fn api_list_events_raw(
    State(state): State<ReadAliasesState>,
    auth: Authenticated,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Vec<EventListItem>>, PlatformError> {
    checks::can_read_events_raw(&auth.0)?;
    events_list_raw(state, auth, q).await
}

/// BFF twin of [`api_list_events_raw`].
#[utoipa::path(get, path = "/bff/events/list-raw", tag = "bff-events",
    operation_id = "listEventsRawBff", params(EventsQuery),
    responses((status = 200, description = "Events", body = Vec<EventListItem>)))]
pub async fn bff_list_events_raw(
    State(state): State<ReadAliasesState>,
    auth: Authenticated,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Vec<EventListItem>>, PlatformError> {
    checks::can_read_events_raw(&auth.0)?;
    events_list_raw(state, auth, q).await
}

/// The dispatch-job list, for a caller holding `dispatch-job:view-raw`
/// (Go `listDispatchJobsRaw`).
#[utoipa::path(get, path = "/api/dispatch-jobs/list-raw", tag = "dispatch-jobs",
    operation_id = "listDispatchJobsRaw", params(DispatchJobsQuery),
    responses((status = 200, description = "Dispatch jobs", body = Vec<DispatchJobReadResponse>)),
    security(("bearer_auth" = [])))]
pub async fn api_list_dispatch_jobs_raw(
    State(state): State<ReadAliasesState>,
    auth: Authenticated,
    Query(q): Query<DispatchJobsQuery>,
) -> Result<Json<Vec<DispatchJobReadResponse>>, PlatformError> {
    // Go's `listRaw` asks `dispatch-job:view-raw` only.
    checks::can_read_dispatch_jobs_raw(&auth.0)?;
    crate::dispatch_job::api::list_dispatch_jobs_unchecked(&state.dispatch_jobs, &auth, q).await
}

/// BFF twin of [`api_list_dispatch_jobs_raw`].
#[utoipa::path(get, path = "/bff/dispatch-jobs/list-raw", tag = "bff-dispatch-jobs",
    operation_id = "listDispatchJobsRawBff", params(DispatchJobsQuery),
    responses((status = 200, description = "Dispatch jobs", body = Vec<DispatchJobReadResponse>)))]
pub async fn bff_list_dispatch_jobs_raw(
    State(state): State<ReadAliasesState>,
    auth: Authenticated,
    Query(q): Query<DispatchJobsQuery>,
) -> Result<Json<Vec<DispatchJobReadResponse>>, PlatformError> {
    // Go's `listRaw` asks `dispatch-job:view-raw` only.
    checks::can_read_dispatch_jobs_raw(&auth.0)?;
    crate::dispatch_job::api::list_dispatch_jobs_unchecked(&state.dispatch_jobs, &auth, q).await
}

/// An event's dispatch jobs (Go `dispatchJobsByEvent`).
#[utoipa::path(get, path = "/api/dispatch-jobs/event/{eventId}", tag = "dispatch-jobs",
    operation_id = "dispatchJobsByEvent",
    params(("eventId" = String, Path, description = "Event id")),
    responses((status = 200, description = "Dispatch jobs", body = Vec<DispatchJobReadResponse>)),
    security(("bearer_auth" = [])))]
pub async fn api_dispatch_jobs_by_event(
    State(state): State<ReadAliasesState>,
    auth: Authenticated,
    Path(event_id): Path<String>,
) -> Result<Json<Vec<DispatchJobReadResponse>>, PlatformError> {
    checks::can_read_dispatch_jobs(&auth.0)?;
    crate::dispatch_job::api::get_jobs_for_event(State(state.dispatch_jobs), auth, Path(event_id))
        .await
}

/// BFF twin of [`api_dispatch_jobs_by_event`].
#[utoipa::path(get, path = "/bff/dispatch-jobs/event/{eventId}", tag = "bff-dispatch-jobs",
    operation_id = "listDispatchJobsByEventBff",
    params(("eventId" = String, Path, description = "Event id")),
    responses((status = 200, description = "Dispatch jobs", body = Vec<DispatchJobReadResponse>)))]
pub async fn bff_dispatch_jobs_by_event(
    State(state): State<ReadAliasesState>,
    auth: Authenticated,
    Path(event_id): Path<String>,
) -> Result<Json<Vec<DispatchJobReadResponse>>, PlatformError> {
    checks::can_read_dispatch_jobs(&auth.0)?;
    crate::dispatch_job::api::get_jobs_for_event(State(state.dispatch_jobs), auth, Path(event_id))
        .await
}

/// Full-path router; merged at the root.
pub fn read_aliases_router(state: ReadAliasesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(api_list_events_raw))
        .routes(routes!(bff_list_events_raw))
        .routes(routes!(api_list_dispatch_jobs_raw))
        .routes(routes!(bff_list_dispatch_jobs_raw))
        .routes(routes!(api_dispatch_jobs_by_event))
        .routes(routes!(bff_dispatch_jobs_by_event))
        .with_state(state)
}
