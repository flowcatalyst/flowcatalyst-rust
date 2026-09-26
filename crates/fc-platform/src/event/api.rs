//! Events BFF API
//!
//! REST endpoints for event management.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::EventRepository;
use crate::{ContextData, Event, EventRead};

/// Context data for event filtering/searching
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextDataDto {
    pub key: String,
    pub value: String,
}

impl From<ContextDataDto> for ContextData {
    fn from(dto: ContextDataDto) -> Self {
        ContextData {
            key: dto.key,
            value: dto.value,
        }
    }
}

impl From<ContextData> for ContextDataDto {
    fn from(cd: ContextData) -> Self {
        ContextDataDto {
            key: cd.key,
            value: cd.value,
        }
    }
}

/// Create event request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventRequest {
    /// Event type code (e.g., "orders:fulfillment:shipment:shipped")
    pub event_type: String,

    /// Event source URI
    pub source: String,

    /// Event subject (optional context)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    /// Event payload data
    pub data: serde_json::Value,

    /// Message group for FIFO ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,

    /// Correlation ID for request tracing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    /// Causation ID - the event that caused this event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,

    /// Deduplication ID for exactly-once delivery
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deduplication_id: Option<String>,

    /// Client ID (optional, defaults to caller's client)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Context data for filtering/searching
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_data: Vec<ContextDataDto>,
}

/// Create event response - includes deduplication info and dispatch job count
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventResponse {
    pub event: EventResponse,
    /// Number of dispatch jobs created for matching subscriptions
    pub dispatch_job_count: usize,
    /// True if this was a deduplicated request (event already existed)
    pub is_duplicate: bool,
}

/// Event response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventResponse {
    pub id: String,
    pub spec_version: String,
    pub event_type: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub time: String,
    pub data: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deduplication_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_data: Vec<ContextDataDto>,
    pub created_at: String,
}

impl From<Event> for EventResponse {
    fn from(e: Event) -> Self {
        Self {
            id: e.id,
            spec_version: e.spec_version,
            event_type: e.event_type,
            source: e.source,
            subject: e.subject,
            time: e.time.to_rfc3339(),
            data: e.data,
            message_group: e.message_group,
            correlation_id: e.correlation_id,
            causation_id: e.causation_id,
            deduplication_id: e.deduplication_id,
            client_id: e.client_id,
            context_data: e.context_data.into_iter().map(Into::into).collect(),
            created_at: e.created_at.to_rfc3339(),
        }
    }
}

/// Event read projection response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventReadResponse {
    pub id: String,
    pub event_type: String,
    pub source: String,
    pub subject: Option<String>,
    pub time: String,
    pub application: Option<String>,
    pub subdomain: Option<String>,
    pub aggregate: Option<String>,
    pub event_name: Option<String>,
    pub message_group: Option<String>,
    pub correlation_id: Option<String>,
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    pub created_at: String,
}

impl From<EventRead> for EventReadResponse {
    fn from(e: EventRead) -> Self {
        let event_name = e.event_type.split(':').nth(3).map(String::from);
        Self {
            id: e.id,
            event_type: e.event_type,
            source: e.source,
            subject: e.subject,
            time: e.time.to_rfc3339(),
            application: e.application,
            subdomain: e.subdomain,
            aggregate: e.aggregate,
            event_name,
            message_group: e.message_group,
            correlation_id: e.correlation_id,
            client_id: e.client_id,
            client_name: e.client_name,
            created_at: e.projected_at.to_rfc3339(),
        }
    }
}

/// Query parameters for events list.
///
/// `msg_events_read` is an append-only firehose ingesting at high rates,
/// so this endpoint returns the most recent N rows only — no pagination.
/// Sort order is fixed to most-recent-first (`time DESC, id DESC`); if you
/// need to scan back further, narrow the filters or build a separate
/// report.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct EventsQuery {
    /// Result size (the SPA's; wins over `limit`). Default 100, max 1000.
    pub size: Option<i64>,

    /// Result size (the SDK's).
    pub limit: Option<i64>,

    /// Rows to skip.
    pub offset: Option<i64>,

    /// Exact event type
    #[serde(rename = "type")]
    pub event_type: Option<String>,

    /// Exact subject
    pub subject: Option<String>,

    /// Exact client id
    pub client_id: Option<String>,

    /// Accepted and ignored, as Go (no backing column on the projection).
    pub principal_id: Option<String>,

    /// RFC 3339 lower bound on createdAt (an unparsable value is ignored)
    pub since: Option<String>,

    /// RFC 3339 upper bound on createdAt (an unparsable value is ignored)
    pub until: Option<String>,

    /// Filter by client IDs (comma-separated)
    pub client_ids: Option<String>,

    /// Filter by event types (comma-separated)
    pub types: Option<String>,

    /// Filter by application codes (comma-separated)
    pub applications: Option<String>,

    /// Filter by subdomains (comma-separated)
    pub subdomains: Option<String>,

    /// Filter by aggregates (comma-separated)
    pub aggregates: Option<String>,

    /// Filter by correlation ID
    pub correlation_id: Option<String>,

    /// Exact source
    pub source: Option<String>,
}

fn split_csv(input: Option<&str>) -> Vec<String> {
    input
        .map(|s| {
            s.split(',')
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Events service state
#[derive(Clone)]
pub struct EventsState {
    pub event_repo: Arc<EventRepository>,
    /// Refuses an application's event types from a caller that may not
    /// sign as that application (S6, ruling 17a).
    pub signing: Arc<crate::dispatch_job::signing_guard::SigningGuard>,
}

/// Create a new event
///
/// Creates a new event in the event store. If a deduplicationId is provided and
/// an event with that ID already exists, the existing event is returned (idempotent operation).
/// Dispatch jobs are automatically created for matching subscriptions.
#[utoipa::path(
    post,
    path = "",
    tag = "events",
    operation_id = "postApiEvents",
    request_body = CreateEventRequest,
    responses(
        (status = 201, description = "Event created", body = CreateEventResponse),
        (status = 200, description = "Event already exists (idempotent)", body = CreateEventResponse),
        (status = 400, description = "Validation error"),
        (status = 403, description = "No access to client")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_event(
    State(state): State<EventsState>,
    auth: Authenticated,
    Json(req): Json<CreateEventRequest>,
) -> Result<(axum::http::StatusCode, Json<CreateEventResponse>), PlatformError> {
    // Go event/api/api.go:90: the ingest permission, exactly.
    crate::shared::authorization_service::checks::require_permission(
        &auth.0,
        crate::permissions::admin::BATCH_EVENTS_WRITE,
    )?;

    // The client the event is written under: an explicit one, else a
    // non-anchor's first client (Go event/api/api.go:103-108), and a
    // non-anchor never writes a platform-scoped event (owner decision #24).
    // Decided before the deduplication lookup, so a refused caller learns
    // nothing about stored events.
    let client_id = crate::shared::caller_reach::non_blank(req.client_id.clone()).or_else(|| {
        if auth.0.is_anchor() {
            None
        } else {
            crate::shared::caller_reach::client_ids(&auth.0)
                .into_iter()
                .next()
        }
    });
    let client_id = crate::shared::caller_reach::require_writable_client(&auth.0, client_id)?;

    // An application's event type only from a caller that may sign as it
    // (owner ruling 17a).
    state
        .signing
        .check_event_types(&auth.0, [req.event_type.as_str()])
        .await?;

    // Go (event/api/api.go `create`): an explicit JSON null is no data.
    if req.data.is_null() {
        return Err(PlatformError::bad_request_code(
            "VALIDATION",
            "data is required",
        ));
    }

    // Check for duplicate deduplication ID
    if let Some(ref dedup_id) = req.deduplication_id {
        if let Some(existing) = state.event_repo.find_by_deduplication_id(dedup_id).await? {
            // Return existing event for idempotency (no new dispatch jobs)
            return Ok((
                axum::http::StatusCode::OK,
                Json(CreateEventResponse {
                    event: existing.into(),
                    dispatch_job_count: 0,
                    is_duplicate: true,
                }),
            ));
        }
    }

    // Create event
    let mut event = Event::new(&req.event_type, &req.source, req.data);

    if let Some(subject) = req.subject {
        event = event.with_subject(subject);
    }
    if let Some(group) = req.message_group {
        event = event.with_message_group(group);
    }
    if let Some(corr_id) = req.correlation_id {
        event = event.with_correlation_id(corr_id);
    }
    if let Some(cause_id) = req.causation_id {
        event = event.with_causation_id(cause_id);
    }
    // Go's `event.New` gives every event a deduplication id:
    // `<type>-<fresh tsid>` unless the caller supplied one.
    let dedup_id = req
        .deduplication_id
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| {
            format!(
                "{}-{}",
                event.event_type,
                crate::shared::tsid::generate_untyped()
            )
        });
    event = event.with_deduplication_id(dedup_id);
    if let Some(cid) = client_id {
        event = event.with_client_id(cid);
    }
    if !req.context_data.is_empty() {
        event = event.with_context_data(req.context_data.into_iter().map(Into::into).collect());
    }

    state.event_repo.insert(&event).await?;

    // Dispatch jobs are created via the outbox processor calling the dispatch jobs endpoint
    let dispatch_job_count = 0;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(CreateEventResponse {
            event: event.into(),
            dispatch_job_count,
            is_duplicate: false,
        }),
    ))
}

/// Get event by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "events",
    operation_id = "getApiEventsById",
    params(
        ("id" = String, Path, description = "Event ID")
    ),
    responses(
        (status = 200, description = "Event found", body = EventDetailResponse),
        (status = 404, description = "Event not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_event(
    State(state): State<EventsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<EventDetailResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_events(&auth.0)?;

    // Go reads the read projection (`msg_events_read`), as the list does: an
    // event the projector has not reached yet is a 404, and the detail
    // agrees with the list it was opened from.
    let event = state
        .event_repo
        .find_read_detail_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Event", &id))?;

    // A client's event needs that client; a platform event is visible to any
    // holder of `event:view` (Go `getByID`).
    if let Some(cid) = event.client_id.as_deref() {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event"));
        }
    }

    Ok(Json(event.into()))
}

/// `GET /api/events/{id}`: Go's `EventResponse` (event/api/dto.go), from the
/// read projection. Absent members stay absent.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventDetailResponse {
    pub id: String,
    pub spec_version: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub source: String,
    pub subject: String,
    pub time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub data: Option<serde_json::Value>,
    pub deduplication_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projected_at: Option<String>,
    pub created_at: String,
}

impl From<crate::event::repository::EventReadDetail> for EventDetailResponse {
    fn from(e: crate::event::repository::EventReadDetail) -> Self {
        // `data` is stored as text; Go hands it back as raw JSON.
        let data = e
            .data
            .filter(|d| !d.is_empty())
            .map(|d| serde_json::from_str(&d).unwrap_or(serde_json::Value::String(d)));
        Self {
            id: e.id,
            spec_version: e.spec_version.unwrap_or_default(),
            event_type: e.event_type,
            source: e.source,
            subject: e.subject.unwrap_or_default(),
            time: e.time.to_rfc3339(),
            data,
            deduplication_id: e.deduplication_id.unwrap_or_default(),
            client_id: e.client_id,
            message_group: e.message_group,
            correlation_id: e.correlation_id,
            causation_id: e.causation_id,
            application: e.application,
            subdomain: e.subdomain,
            aggregate: e.aggregate,
            projected_at: e.projected_at.map(|t| t.to_rfc3339()),
            created_at: e.created_at.to_rfc3339(),
        }
    }
}

/// A list row: Go's slim `EventRead` (top-level `type`, absent members
/// absent, `projectedAt` always).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventListItem {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub projected_at: String,
}

impl From<EventRead> for EventListItem {
    fn from(e: EventRead) -> Self {
        Self {
            id: e.id,
            event_type: e.event_type,
            source: e.source,
            subject: e.subject.filter(|s| !s.is_empty()),
            time: e.time.to_rfc3339(),
            application: e.application,
            subdomain: e.subdomain,
            aggregate: e.aggregate,
            message_group: e.message_group,
            correlation_id: e.correlation_id,
            client_id: e.client_id,
            projected_at: e.projected_at.to_rfc3339(),
        }
    }
}

/// List events. Returns the most recent rows matching the filters; no
/// pagination — see `EventsQuery` for the rationale.
#[utoipa::path(
    get,
    path = "",
    tag = "events",
    operation_id = "getApiEvents",
    params(EventsQuery),
    responses(
        (status = 200, description = "List of events", body = Vec<EventListItem>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_events(
    State(state): State<EventsState>,
    auth: Authenticated,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<EventListItem>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_events(&auth.0)?;
    list_events_unchecked(&state, &auth, query).await
}

/// Go's `list` / `listRaw` body (event/api/api.go) once the permission is
/// checked: the read projection filtered, scoped in SQL for a non-anchor
/// caller (platform events plus its clients'), newest first.
pub(crate) async fn list_events_unchecked(
    state: &EventsState,
    auth: &Authenticated,
    query: EventsQuery,
) -> Result<Json<Vec<EventListItem>>, PlatformError> {
    let types = split_csv(query.types.as_deref());
    let applications = split_csv(query.applications.as_deref());
    let subdomains = split_csv(query.subdomains.as_deref());
    let aggregates = split_csv(query.aggregates.as_deref());
    let client_ids = split_csv(query.client_ids.as_deref());
    let accessible: Option<Vec<String>> = if auth.0.is_anchor() {
        None
    } else {
        Some(crate::shared::caller_reach::client_ids(&auth.0))
    };
    let ts = |v: Option<&str>| {
        v.filter(|v| !v.is_empty())
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
            .map(|t| t.with_timezone(&chrono::Utc))
    };
    // `size` (SPA) wins over `limit` (SDK); out of range is Go's 100.
    let limit = match query.size.filter(|s| *s > 0).or(query.limit) {
        Some(l) if (1..=1000).contains(&l) => l,
        _ => 100,
    };
    fn non_empty(v: &Option<String>) -> Option<&str> {
        v.as_deref().filter(|v| !v.is_empty())
    }
    let filter = crate::event::repository::EventReadFilter {
        event_type: non_empty(&query.event_type),
        types: &types,
        source: non_empty(&query.source),
        subject: non_empty(&query.subject),
        client_id: non_empty(&query.client_id),
        client_ids: &client_ids,
        accessible: accessible.as_deref(),
        applications: &applications,
        subdomains: &subdomains,
        aggregates: &aggregates,
        correlation_id: non_empty(&query.correlation_id),
        since: ts(query.since.as_deref()),
        until: ts(query.until.as_deref()),
        limit,
        offset: query.offset.unwrap_or(0).max(0),
    };
    let rows = state.event_repo.find_read_filtered(&filter).await?;
    Ok(Json(rows.into_iter().map(EventListItem::from).collect()))
}

/// Batch create events request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCreateEventsRequest {
    pub events: Vec<CreateEventRequest>,
}

/// Batch create response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCreateResponse {
    /// All created events (new and deduplicated)
    pub events: Vec<EventResponse>,
    /// Total number of events in response
    pub count: usize,
    /// Number of dispatch jobs created for matching subscriptions
    pub dispatch_job_count: usize,
    /// Number of events that were deduplicated (already existed)
    pub duplicate_count: usize,
}

/// Batch create events
///
/// Creates multiple events in a single operation. Maximum batch size is 100 events.
/// Dispatch jobs are automatically created for matching subscriptions.
/// Events with duplicate deduplicationIds are returned from the existing store.
#[utoipa::path(
    post,
    path = "/batch",
    tag = "events",
    operation_id = "postApiEventsBatch",
    request_body = BatchCreateEventsRequest,
    responses(
        (status = 201, description = "Events created", body = BatchCreateResponse),
        (status = 400, description = "Invalid request or batch size exceeds limit")
    ),
    security(("bearer_auth" = []))
)]
pub async fn batch_create_events(
    State(state): State<EventsState>,
    auth: Authenticated,
    Json(req): Json<BatchCreateEventsRequest>,
) -> Result<(axum::http::StatusCode, Json<BatchCreateResponse>), PlatformError> {
    // The same ingest permission as `/api/events/batch` (Go registers one
    // handler for both).
    crate::shared::authorization_service::checks::require_permission(
        &auth.0,
        crate::permissions::admin::BATCH_EVENTS_WRITE,
    )?;

    // Validate batch size
    if req.events.is_empty() {
        return Err(PlatformError::validation(
            "Request body must contain at least one event",
        ));
    }
    if req.events.len() > 100 {
        return Err(PlatformError::validation(
            "Batch size cannot exceed 100 events",
        ));
    }

    let mut all_events: Vec<Event> = Vec::new();
    let mut new_events: Vec<Event> = Vec::new();
    let mut duplicate_count = 0usize;

    // Every deduplication id of the batch looked up in one query; an event
    // repeating one already stored, or one earlier in this batch, is a
    // duplicate and answers with the event it repeats.
    let dedup_ids: Vec<String> = req
        .events
        .iter()
        .filter_map(|e| e.deduplication_id.clone())
        .collect();
    let mut known: std::collections::HashMap<String, Event> = state
        .event_repo
        .find_by_deduplication_ids(&dedup_ids)
        .await?
        .into_iter()
        .filter_map(|e| e.deduplication_id.clone().map(|d| (d, e)))
        .collect();

    // Every item's client first: one the caller may not write refuses the
    // whole batch before anything is looked up or written (owner decision
    // #24; the batch rule — no first-client default — as Go and Java's
    // /bff/events/batch, which is the batch ingest handler).
    let client_ids = req
        .events
        .iter()
        .map(|e| crate::shared::caller_reach::require_writable_client(&auth.0, e.client_id.clone()))
        .collect::<Result<Vec<_>, PlatformError>>()?;
    // And every item's type from a caller that may sign as its application
    // (owner ruling 17a).
    state
        .signing
        .check_event_types(&auth.0, req.events.iter().map(|e| e.event_type.as_str()))
        .await?;

    for (event_req, client_id) in req.events.into_iter().zip(client_ids) {
        if let Some(existing) = event_req
            .deduplication_id
            .as_deref()
            .and_then(|d| known.get(d))
        {
            all_events.push(existing.clone());
            duplicate_count += 1;
            continue;
        }

        // Create event
        let mut event = Event::new(&event_req.event_type, &event_req.source, event_req.data);

        if let Some(subject) = event_req.subject {
            event = event.with_subject(subject);
        }
        if let Some(group) = event_req.message_group {
            event = event.with_message_group(group);
        }
        if let Some(corr_id) = event_req.correlation_id {
            event = event.with_correlation_id(corr_id);
        }
        if let Some(cause_id) = event_req.causation_id {
            event = event.with_causation_id(cause_id);
        }
        if let Some(dedup_id) = event_req.deduplication_id {
            event = event.with_deduplication_id(dedup_id);
        }
        if let Some(cid) = client_id {
            event = event.with_client_id(cid);
        }
        if !event_req.context_data.is_empty() {
            event = event
                .with_context_data(event_req.context_data.into_iter().map(Into::into).collect());
        }

        if let Some(dedup_id) = event.deduplication_id.clone() {
            known.insert(dedup_id, event.clone());
        }
        new_events.push(event.clone());
        all_events.push(event);
    }

    // Bulk insert new events
    if !new_events.is_empty() {
        state.event_repo.insert_many(&new_events).await?;
    }

    // Dispatch jobs are created via the outbox processor calling the dispatch jobs endpoint
    let dispatch_job_count = 0;

    let count = all_events.len();
    let event_responses: Vec<EventResponse> = all_events.into_iter().map(Into::into).collect();

    Ok((
        axum::http::StatusCode::CREATED,
        Json(BatchCreateResponse {
            events: event_responses,
            count,
            dispatch_job_count,
            duplicate_count,
        }),
    ))
}

/// Event summary for list endpoints (no payload data)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventSummaryResponse {
    pub id: String,
    pub spec_version: String,
    pub event_type: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub created_at: String,
}

impl From<Event> for EventSummaryResponse {
    fn from(e: Event) -> Self {
        Self {
            id: e.id,
            spec_version: e.spec_version,
            event_type: e.event_type,
            source: e.source,
            subject: e.subject,
            time: e.time.to_rfc3339(),
            message_group: e.message_group,
            correlation_id: e.correlation_id,
            client_id: e.client_id,
            created_at: e.created_at.to_rfc3339(),
        }
    }
}

/// Paginated response (matches TS: { items, page, size })
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedEventsResponse {
    pub items: Vec<EventSummaryResponse>,
    pub page: u32,
    pub size: u32,
}

/// `GET /raw`: Go's SDK alias of `/list-raw` (the event list, gated on
/// `event:view-raw`).
#[utoipa::path(
    get,
    path = "/raw",
    tag = "events",
    operation_id = "getApiEventsRaw",
    params(EventsQuery),
    responses(
        (status = 200, description = "Events", body = Vec<EventListItem>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_events_raw(
    State(state): State<EventsState>,
    auth: Authenticated,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<EventListItem>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_events_raw(&auth.0)?;
    list_events_unchecked(&state, &auth, query).await
}

/// One `{value, label}` pair of the filter dropdowns (label = value).
#[derive(Debug, Serialize, ToSchema)]
pub struct EventFilterOption {
    pub value: String,
    pub label: String,
}

/// Go's `EventFilterOptionsResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventFilterOptionsResponse {
    pub applications: Vec<EventFilterOption>,
    pub subdomains: Vec<EventFilterOption>,
    pub event_types: Vec<EventFilterOption>,
}

/// Get filter options for the events read model.
#[utoipa::path(
    get,
    path = "/filter-options",
    tag = "events",
    operation_id = "getApiEventsFilterOptions",
    responses((status = 200, body = EventFilterOptionsResponse)),
    security(("bearer_auth" = []))
)]
pub async fn event_filter_options(
    State(state): State<EventsState>,
    auth: Authenticated,
) -> Result<Json<EventFilterOptionsResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_events(&auth.0)?;
    let repo = &state.event_repo;
    let (applications, subdomains, types) = tokio::try_join!(
        repo.distinct_read_values("application"),
        repo.distinct_read_values("subdomain"),
        repo.distinct_read_values("type"),
    )?;
    let options = |values: Vec<String>| {
        values
            .into_iter()
            .map(|v| EventFilterOption {
                label: v.clone(),
                value: v,
            })
            .collect()
    };
    Ok(Json(EventFilterOptionsResponse {
        applications: options(applications),
        subdomains: options(subdomains),
        event_types: options(types),
    }))
}

/// Create events router for the BFF tier (`/bff/events`). Cookie-auth, used
/// by the SPA. Includes `batch_create_events` — the SPA-facing batch that
/// fans out events to subscriptions.
pub fn events_router(state: EventsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_event, list_events))
        .routes(routes!(batch_create_events))
        .routes(routes!(list_events_raw))
        .routes(routes!(event_filter_options))
        .routes(routes!(get_event))
        .with_state(state)
}

/// Create events router for the API tier (`/api/events`). Bearer-auth, used
/// by SDK consumers. **No `batch_create_events`** — SDK callers use
/// `sdk_events_batch_router::POST /batch` (different handler, optimized for
/// high-volume insert without per-event fan-out). The two routers must not
/// both register `POST /batch` against the same prefix.
pub fn events_api_router(state: EventsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_event, list_events))
        .routes(routes!(list_events_raw))
        .routes(routes!(event_filter_options))
        .routes(routes!(get_event))
        .with_state(state)
}
