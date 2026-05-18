//! Event Types BFF API
//!
//! REST endpoints for event type management.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::shared::api_common::PaginationParams;
use crate::shared::error::{NotFoundExt, PlatformError};
use crate::shared::middleware::Authenticated;
use crate::EventTypeRepository;
use crate::{EventType, SpecVersion};

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

    /// Filter by subdomain
    pub subdomain: Option<String>,

    /// Filter by aggregate
    pub aggregate: Option<String>,
}

/// Event types service state
#[derive(Clone)]
pub struct EventTypesState {
    pub event_type_repo: Arc<EventTypeRepository>,
    pub create_use_case:
        Arc<crate::event_type::operations::CreateEventTypeUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_use_case:
        Arc<crate::event_type::operations::UpdateEventTypeUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_use_case:
        Arc<crate::event_type::operations::DeleteEventTypeUseCase<crate::usecase::PgUnitOfWork>>,
    pub add_schema_use_case:
        Arc<crate::event_type::operations::AddSchemaUseCase<crate::usecase::PgUnitOfWork>>,
}

/// Create a new event type
#[utoipa::path(
    post,
    path = "",
    tag = "event-types",
    operation_id = "postApiEventTypes",
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
    use crate::event_type::operations::CreateEventTypeCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Resource-level client access (unchanged — this is the rule that
    // anchor-level event types require anchor scope, partner/client-scoped
    // event types require access to the client).
    if let Some(ref cid) = req.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden(format!(
                "No access to client: {}",
                cid
            )));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can create anchor-level event types",
        ));
    }

    let cmd = CreateEventTypeCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        client_id: req.client_id,
        schema: req.schema,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;

    Ok((
        StatusCode::CREATED,
        Json(crate::shared::api_common::CreatedResponse::new(
            event.event_type_id,
        )),
    ))
}

/// Get event type by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "event-types",
    operation_id = "getApiEventTypesById",
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

    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
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
    operation_id = "getApiEventTypesByCodeByCode",
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

    let event_type = state
        .event_type_repo
        .find_by_code(&code)
        .await?
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
    operation_id = "getApiEventTypes",
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
    let default_status = if query.application.is_none()
        && query.client_id.is_none()
        && query.status.is_none()
        && query.subdomain.is_none()
        && query.aggregate.is_none()
    {
        Some("CURRENT".to_string())
    } else {
        query.status.clone()
    };

    let event_types = state
        .event_type_repo
        .find_with_filters(
            query.application.as_deref(),
            query.client_id.as_deref(),
            default_status.as_deref(),
            query.subdomain.as_deref(),
            query.aggregate.as_deref(),
        )
        .await?;

    // Filter by client access
    let items: Vec<EventTypeResponse> = event_types
        .into_iter()
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
    operation_id = "putApiEventTypesById",
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
    use crate::event_type::operations::UpdateEventTypeCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Resource-level access check on the stored event type.
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .or_not_found("EventType", &id)?;
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can modify anchor-level event types",
        ));
    }

    let cmd = UpdateEventTypeCommand {
        event_type_id: id,
        name: req.name,
        description: req.description,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Add schema version to event type
#[utoipa::path(
    post,
    path = "/{id}/versions",
    tag = "event-types",
    operation_id = "postApiEventTypesByIdSchemas",
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
    use crate::event_type::operations::AddSchemaCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .or_not_found("EventType", &id)?;
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    }

    let next_version = format!("{}.0", event_type.spec_versions.len() + 1);
    let cmd = AddSchemaCommand {
        event_type_id: id.clone(),
        version: next_version,
        mime_type: "application/schema+json".to_string(),
        schema_content: Some(req.schema),
        schema_type: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .add_schema_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let refreshed = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .or_not_found("EventType", &id)?;
    Ok(Json(refreshed.into()))
}

/// Delete event type (archive)
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "event-types",
    operation_id = "deleteApiEventTypesById",
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
    use crate::event_type::operations::DeleteEventTypeCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .or_not_found("EventType", &id)?;
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can delete anchor-level event types",
        ));
    }

    let cmd = DeleteEventTypeCommand { event_type_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Create event types router
pub fn event_types_router(state: EventTypesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_event_type, list_event_types))
        .routes(routes!(
            get_event_type,
            update_event_type,
            delete_event_type
        ))
        .routes(routes!(get_event_type_by_code))
        .routes(routes!(add_schema_version))
        .with_state(state)
}
