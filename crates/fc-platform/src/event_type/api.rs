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

use crate::event_type::entity::EventTypeStatus;
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

/// Update event type request: Go's `UpdateEventTypeRequest` (`name` is
/// required, a blank one is `NAME_REQUIRED`).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEventTypeRequest {
    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(default)]
    pub description: Option<String>,
}

/// Event type response DTO: Go's `EventTypeResponse`
/// (eventtype/api/dto.go).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub application: String,
    pub subdomain: String,
    pub aggregate: String,
    pub event_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub spec_versions: Vec<SpecVersionResponse>,
}

/// Schema version response (Go's `specVersionResponse`).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecVersionResponse {
    pub version: String,
    /// The schema document (`null` when none).
    #[schema(value_type = Option<Object>)]
    pub schema: Option<serde_json::Value>,
    pub status: String,
    pub created_at: String,
}

/// Event type list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeListResponse {
    pub items: Vec<EventTypeResponse>,
}

impl From<SpecVersion> for SpecVersionResponse {
    fn from(v: SpecVersion) -> Self {
        Self {
            version: v.version,
            schema: v.schema_content,
            status: v.status.as_str().to_string(),
            created_at: v.created_at.to_rfc3339(),
        }
    }
}

impl From<EventType> for EventTypeResponse {
    fn from(et: EventType) -> Self {
        Self {
            id: et.id,
            code: et.code,
            name: et.name,
            application: et.application,
            subdomain: et.subdomain,
            aggregate: et.aggregate,
            event_name: et.event_name,
            description: et.description,
            status: et.status.as_str().to_string(),
            source: et.source.as_str().to_string(),
            client_id: et.client_id,
            created_by: et.created_by,
            created_at: et.created_at.to_rfc3339(),
            updated_at: et.updated_at.to_rfc3339(),
            spec_versions: et.spec_versions.into_iter().map(|v| v.into()).collect(),
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

    let cmd = CreateEventTypeCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        client_id: req.client_id,
        schema: req.schema,
    };
    // Go's `CreateEventType`: the command is validated, then the scope is
    // checked (`CheckScopeAccess`: a client-scoped type needs that client,
    // a platform one anchor).
    state
        .create_use_case
        .validate(&cmd)
        .await
        .map_err(PlatformError::from)?;
    crate::shared::authorization_service::checks::check_scope_access(
        &auth.0,
        cmd.client_id.as_deref(),
    )?;
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
    let status: Option<EventTypeStatus> =
        crate::shared::enum_str::parse_opt(query.status.as_deref())?;
    let default_status = if query.application.is_none()
        && query.client_id.is_none()
        && status.is_none()
        && query.subdomain.is_none()
        && query.aggregate.is_none()
    {
        Some(EventTypeStatus::Current)
    } else {
        status
    };

    // Go accepts `clientId` but filters nothing by it (msg_event_types has no
    // client column there).
    let event_types = state
        .event_type_repo
        .find_with_filters(
            query.application.as_deref(),
            None,
            default_status,
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

    // Go `UpdateEventType.Validate`: the name is required.
    if req.name.trim().is_empty() {
        return Err(PlatformError::bad_request_code(
            "NAME_REQUIRED",
            "Event type name is required",
        ));
    }
    // Resource-level access check on the stored event type (Go
    // `CheckScopeAccess`).
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .or_not_found("EventType", &id)?;
    crate::shared::authorization_service::checks::check_scope_access(
        &auth.0,
        event_type.client_id.as_deref(),
    )?;

    let cmd = UpdateEventTypeCommand {
        event_type_id: id,
        name: Some(req.name),
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
    request_body = crate::event_type::go_api::AddEventTypeSchemaRequest,
    responses(
        (status = 200, description = "Schema version added", body = EventTypeResponse),
        (status = 400, description = "No version or schema"),
        (status = 404, description = "Event type not found"),
        (status = 409, description = "The version exists")
    ),
    security(("bearer_auth" = []))
)]
pub async fn add_schema_version(
    State(state): State<EventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<crate::event_type::go_api::AddEventTypeSchemaRequest>,
) -> Result<Json<EventTypeResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;
    // Go registers one handler for `/versions` and `/schemas`: the version
    // is the caller's, and a repeat is 409 `VERSION_EXISTS`.
    crate::event_type::go_api::add_schema(
        &state.event_type_repo,
        &state.add_schema_use_case,
        &auth,
        id,
        req,
    )
    .await
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
    crate::shared::authorization_service::checks::check_scope_access(
        &auth.0,
        event_type.client_id.as_deref(),
    )?;

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
