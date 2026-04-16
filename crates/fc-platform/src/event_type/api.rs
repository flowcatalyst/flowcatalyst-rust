//! Event Types BFF API
//!
//! REST endpoints for event type management.

use axum::{
    extract::{State, Path, Query},
    http::StatusCode,
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{EventType, SpecVersion};
use crate::EventTypeRepository;
use crate::event_type::operations::{
    SyncEventTypesCommand, SyncEventTypesUseCase, SyncEventTypeInput,
};
use crate::usecase::{ExecutionContext, UseCase, UseCaseResult};
use crate::shared::error::{PlatformError, NotFoundExt};
use crate::shared::api_common::PaginationParams;
use crate::shared::middleware::Authenticated;

/// Create event type request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventTypeRequest {
    /// Event type code (e.g., "orders:fulfillment:shipment:shipped")
    /// Format: {application}:{subdomain}:{aggregate}:{event}
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Initial JSON schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,

    /// Client ID (optional, null = anchor-level)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// Update event type request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEventTypeRequest {
    /// Human-readable name
    pub name: Option<String>,

    /// Description
    pub description: Option<String>,
}

/// Add schema version request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddSchemaVersionRequest {
    /// JSON schema for this version
    pub schema: serde_json::Value,
}

/// Event type response DTO (matches Java BffEventTypeResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub application: String,
    pub subdomain: String,
    pub aggregate: String,
    #[serde(rename = "event")]
    pub event_name: String,
    pub spec_versions: Vec<SpecVersionResponse>,
    pub created_at: String,
    pub updated_at: String,
}

/// Schema version response (matches Java BffSpecVersionResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecVersionResponse {
    /// Version string (converted from u32 to "X.0" format for frontend compatibility)
    pub version: String,
    pub status: String,
    /// Schema content (included for detail views)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// Event type list response (matches Java BffEventTypeListResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeListResponse {
    pub items: Vec<EventTypeResponse>,
}

impl From<SpecVersion> for SpecVersionResponse {
    fn from(v: SpecVersion) -> Self {
        Self {
            version: v.version,
            status: format!("{:?}", v.status).to_uppercase(),
            schema: v.schema_content,
        }
    }
}

impl From<EventType> for EventTypeResponse {
    fn from(et: EventType) -> Self {
        Self {
            id: et.id,
            code: et.code,
            name: et.name,
            description: et.description,
            status: format!("{:?}", et.status).to_uppercase(),
            application: et.application,
            subdomain: et.subdomain,
            aggregate: et.aggregate,
            event_name: et.event_name,
            spec_versions: et.spec_versions.into_iter().map(|v| v.into()).collect(),
            created_at: et.created_at.to_rfc3339(),
            updated_at: et.updated_at.to_rfc3339(),
        }
    }
}

/// Query parameters for event types list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct EventTypesQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by application
    pub application: Option<String>,

    /// Filter by client ID
    pub client_id: Option<String>,

    /// Filter by status
    pub status: Option<String>,
}

/// Sync event types request (admin)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncEventTypesRequest {
    /// Application code
    pub application_code: String,
    /// Event types to sync
    pub event_types: Vec<SyncEventTypeInputRequest>,
}

/// A single event type input for sync
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncEventTypeInputRequest {
    /// Full code (application:subdomain:aggregate:event)
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Sync query parameters
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct SyncQuery {
    /// Remove items not in the sync list
    #[serde(default)]
    pub remove_unlisted: bool,
}

/// Sync result response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncResultResponse {
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
}

/// Event types service state
#[derive(Clone)]
pub struct EventTypesState {
    pub event_type_repo: Arc<EventTypeRepository>,
    pub sync_use_case: Arc<SyncEventTypesUseCase<crate::usecase::PgUnitOfWork>>,
}

/// Create a new event type
#[utoipa::path(
    post,
    path = "",
    tag = "event-types",
    operation_id = "postApiAdminEventTypes",
    request_body = CreateEventTypeRequest,
    responses(
        (status = 201, description = "Event type created", body = crate::shared::api_common::CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_event_type(
    State(state): State<EventTypesState>,
    auth: Authenticated,
    Json(req): Json<CreateEventTypeRequest>,
) -> Result<(StatusCode, Json<crate::shared::api_common::CreatedResponse>), PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Validate client access if specified
    if let Some(ref cid) = req.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden(format!("No access to client: {}", cid)));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Only anchor users can create anchor-level event types"));
    }

    // Check for duplicate code
    if let Some(_) = state.event_type_repo.find_by_code(&req.code).await? {
        return Err(PlatformError::duplicate("EventType", "code", &req.code));
    }

    // Create event type (code is parsed to extract application:subdomain:aggregate:event)
    let mut event_type = EventType::new(&req.code, &req.name)
        .map_err(|e| PlatformError::validation(e))?;

    if let Some(desc) = req.description {
        event_type = event_type.with_description(desc);
    }
    if let Some(cid) = req.client_id {
        event_type = event_type.with_client_id(cid);
    }
    if let Some(schema) = req.schema {
        let spec = SpecVersion::new(&event_type.id, "1.0", Some(schema));
        event_type.add_schema_version(spec);
    }

    let id = event_type.id.clone();
    state.event_type_repo.insert(&event_type).await?;

    Ok((StatusCode::CREATED, Json(crate::shared::api_common::CreatedResponse::new(id))))
}

/// Get event type by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "event-types",
    operation_id = "getApiAdminEventTypesById",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    responses(
        (status = 200, description = "Event type found", body = EventTypeResponse),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_event_type(
    State(state): State<EventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<EventTypeResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_event_types(&auth.0)?;

    let event_type = state.event_type_repo.find_by_id(&id).await?
        .or_not_found("EventType", &id)?;

    // Check client access
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    }

    Ok(Json(event_type.into()))
}

/// Get event type by code
#[utoipa::path(
    get,
    path = "/by-code/{code}",
    tag = "event-types",
    operation_id = "getApiAdminEventTypesByCodeByCode",
    params(
        ("code" = String, Path, description = "Event type code")
    ),
    responses(
        (status = 200, description = "Event type found", body = EventTypeResponse),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_event_type_by_code(
    State(state): State<EventTypesState>,
    auth: Authenticated,
    Path(code): Path<String>,
) -> Result<Json<EventTypeResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_event_types(&auth.0)?;

    let event_type = state.event_type_repo.find_by_code(&code).await?
        .ok_or_else(|| PlatformError::EventTypeNotFound { code: code.clone() })?;

    // Check client access
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    }

    Ok(Json(event_type.into()))
}

/// List event types
#[utoipa::path(
    get,
    path = "",
    tag = "event-types",
    operation_id = "getApiAdminEventTypes",
    params(EventTypesQuery),
    responses(
        (status = 200, description = "List of event types", body = EventTypeListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_event_types(
    State(state): State<EventTypesState>,
    auth: Authenticated,
    Query(query): Query<EventTypesQuery>,
) -> Result<Json<EventTypeListResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_event_types(&auth.0)?;

    // Default to CURRENT status when no filters are provided (matches find_active behavior)
    let default_status = if query.application.is_none() && query.client_id.is_none() && query.status.is_none() {
        Some("CURRENT".to_string())
    } else {
        query.status.clone()
    };

    let event_types = state.event_type_repo.find_with_filters(
        query.application.as_deref(),
        query.client_id.as_deref(),
        default_status.as_deref(),
    ).await?;

    // Filter by client access
    let items: Vec<EventTypeResponse> = event_types.into_iter()
        .filter(|et| {
            match &et.client_id {
                Some(cid) => auth.0.can_access_client(cid),
                None => true, // Anchor-level event types visible to all
            }
        })
        .map(|et| et.into())
        .collect();

    Ok(Json(EventTypeListResponse { items }))
}

/// Update event type
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "event-types",
    operation_id = "putApiAdminEventTypesById",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    request_body = UpdateEventTypeRequest,
    responses(
        (status = 204, description = "Event type updated"),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_event_type(
    State(state): State<EventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateEventTypeRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    let mut event_type = state.event_type_repo.find_by_id(&id).await?
        .or_not_found("EventType", &id)?;

    // Check client access
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Only anchor users can modify anchor-level event types"));
    }

    // Update fields
    if let Some(name) = req.name {
        event_type.name = name;
    }
    if let Some(desc) = req.description {
        event_type.description = Some(desc);
    }
    event_type.updated_at = chrono::Utc::now();

    state.event_type_repo.update(&event_type).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Add schema version to event type
#[utoipa::path(
    post,
    path = "/{id}/versions",
    tag = "event-types",
    operation_id = "postApiAdminEventTypesByIdSchemas",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    request_body = AddSchemaVersionRequest,
    responses(
        (status = 200, description = "Schema version added", body = EventTypeResponse),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn add_schema_version(
    State(state): State<EventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<AddSchemaVersionRequest>,
) -> Result<Json<EventTypeResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    let mut event_type = state.event_type_repo.find_by_id(&id).await?
        .or_not_found("EventType", &id)?;

    // Check client access
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    }

    let next_version = format!("{}.0", event_type.spec_versions.len() + 1);
    let spec = SpecVersion::new(&event_type.id, &next_version, Some(req.schema));
    event_type.add_schema_version(spec);
    state.event_type_repo.update(&event_type).await?;

    Ok(Json(event_type.into()))
}

/// Delete event type (archive)
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "event-types",
    operation_id = "deleteApiAdminEventTypesById",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    responses(
        (status = 204, description = "Event type archived"),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_event_type(
    State(state): State<EventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    let mut event_type = state.event_type_repo.find_by_id(&id).await?
        .or_not_found("EventType", &id)?;

    // Check client access
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Only anchor users can delete anchor-level event types"));
    }

    event_type.archive();
    state.event_type_repo.update(&event_type).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Sync event types
#[utoipa::path(
    post,
    path = "/sync",
    tag = "event-types",
    operation_id = "postApiAdminEventTypesSync",
    params(SyncQuery),
    request_body = SyncEventTypesRequest,
    responses(
        (status = 200, description = "Event types synced", body = SyncResultResponse),
        (status = 400, description = "Validation error"),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn sync_event_types(
    State(state): State<EventTypesState>,
    auth: Authenticated,
    Query(query): Query<SyncQuery>,
    Json(req): Json<SyncEventTypesRequest>,
) -> Result<Json<SyncResultResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    let command = SyncEventTypesCommand {
        application_code: req.application_code,
        event_types: req.event_types.into_iter().map(|et| SyncEventTypeInput {
            code: et.code,
            name: et.name,
            description: et.description,
            schema: None,
        }).collect(),
        remove_unlisted: query.remove_unlisted,
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.sync_use_case.run(command, ctx).await {
        UseCaseResult::Success(event) => {
            Ok(Json(SyncResultResponse {
                created: event.created,
                updated: event.updated,
                deleted: event.deleted,
            }))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Create event types router
pub fn event_types_router(state: EventTypesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_event_type, list_event_types))
        .routes(routes!(get_event_type, update_event_type, delete_event_type))
        .routes(routes!(get_event_type_by_code))
        .routes(routes!(add_schema_version))
        .routes(routes!(sync_event_types))
        .with_state(state)
}
