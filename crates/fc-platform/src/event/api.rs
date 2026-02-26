//! Events BFF API
//!
//! REST endpoints for event management.

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{Event, EventRead, ContextData};
use crate::EventRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::PaginationParams;
use crate::shared::middleware::Authenticated;

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
        Self {
            id: e.id,
            event_type: e.event_type,
            source: e.source,
            subject: e.subject,
            time: e.time.to_rfc3339(),
            application: e.application,
            subdomain: e.subdomain,
            aggregate: e.aggregate,
            event_name: e.event_name,
            message_group: e.message_group,
            correlation_id: e.correlation_id,
            client_id: e.client_id,
            client_name: e.client_name,
            created_at: e.created_at.to_rfc3339(),
        }
    }
}

/// Query parameters for events list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct EventsQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by event type
    pub event_type: Option<String>,

    /// Filter by correlation ID
    pub correlation_id: Option<String>,

    /// Filter by client ID
    pub client_id: Option<String>,
}

/// Events service state
#[derive(Clone)]
pub struct EventsState {
    pub event_repo: Arc<EventRepository>,
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
    operation_id = "postApiBffEvents",
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
    // Verify permission
    crate::shared::authorization_service::checks::can_write_events(&auth.0)?;

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

    // Determine client ID
    let client_id = req.client_id.or_else(|| {
        if auth.0.is_anchor() {
            None
        } else {
            auth.0.accessible_clients.first().cloned()
        }
    });

    // Validate client access if specified
    if let Some(ref cid) = client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden(format!("No access to client: {}", cid)));
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
    if let Some(dedup_id) = req.deduplication_id {
        event = event.with_deduplication_id(dedup_id);
    }
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
    operation_id = "getApiBffEventsById",
    params(
        ("id" = String, Path, description = "Event ID")
    ),
    responses(
        (status = 200, description = "Event found", body = EventResponse),
        (status = 404, description = "Event not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_event(
    State(state): State<EventsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<EventResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_events(&auth.0)?;

    let event = state.event_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Event", &id))?;

    // Check client access
    if let Some(ref cid) = event.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event"));
        }
    }

    Ok(Json(event.into()))
}

/// List events
#[utoipa::path(
    get,
    path = "",
    tag = "events",
    operation_id = "getApiBffEvents",
    params(EventsQuery),
    responses(
        (status = 200, description = "List of events", body = Vec<EventResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_events(
    State(state): State<EventsState>,
    auth: Authenticated,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<EventResponse>>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_events(&auth.0)?;

    let events = if let Some(ref corr_id) = query.correlation_id {
        state.event_repo.find_by_correlation_id(corr_id).await?
    } else if let Some(ref event_type) = query.event_type {
        state.event_repo.find_by_type(event_type, query.pagination.size() as i64).await?
    } else if let Some(ref client_id) = query.client_id {
        if !auth.0.can_access_client(client_id) {
            return Err(PlatformError::forbidden(format!("No access to client: {}", client_id)));
        }
        state.event_repo.find_by_client(client_id, query.pagination.size() as i64).await?
    } else {
        // Return empty for now - need proper listing with pagination
        vec![]
    };

    // Filter by client access
    let filtered: Vec<EventResponse> = events.into_iter()
        .filter(|e| {
            match &e.client_id {
                Some(cid) => auth.0.can_access_client(cid),
                None => auth.0.is_anchor(), // Anchor-level events only visible to anchors
            }
        })
        .map(|e| e.into())
        .collect();

    Ok(Json(filtered))
}

/// Batch create events request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCreateEventsRequest {
    pub events: Vec<CreateEventRequest>,
}

/// Batch create response (matches Java BatchEventResponse)
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
    operation_id = "postApiBffEventsBatch",
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
) -> Result<Json<BatchCreateResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_events(&auth.0)?;

    // Validate batch size
    if req.events.is_empty() {
        return Err(PlatformError::validation("Request body must contain at least one event"));
    }
    if req.events.len() > 100 {
        return Err(PlatformError::validation("Batch size cannot exceed 100 events"));
    }

    let mut all_events: Vec<Event> = Vec::new();
    let mut new_events: Vec<Event> = Vec::new();
    let mut duplicate_count = 0usize;

    for event_req in req.events.into_iter() {
        // Check for duplicate deduplication ID
        if let Some(ref dedup_id) = event_req.deduplication_id {
            if let Some(existing) = state.event_repo.find_by_deduplication_id(dedup_id).await? {
                all_events.push(existing);
                duplicate_count += 1;
                continue;
            }
        }

        // Determine client ID
        let client_id = event_req.client_id.or_else(|| {
            if auth.0.is_anchor() {
                None
            } else {
                auth.0.accessible_clients.first().cloned()
            }
        });

        // Validate client access if specified
        if let Some(ref cid) = client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden(format!("No access to client: {}", cid)));
            }
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
            event = event.with_context_data(event_req.context_data.into_iter().map(Into::into).collect());
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

    Ok(Json(BatchCreateResponse {
        events: event_responses,
        count,
        dispatch_job_count,
        duplicate_count,
    }))
}

/// Create events router
pub fn events_router(state: EventsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_event, list_events))
        .routes(routes!(batch_create_events))
        .routes(routes!(get_event))
        .with_state(state)
}
