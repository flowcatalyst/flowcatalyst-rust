//! Event Types BFF API
//!
//! REST endpoints for event type management.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::event_type::bff::BffEventTypesState;
use crate::event_type::entity::EventTypeStatus;
use crate::event_type::operations::{AddSchemaCommand, AddSchemaUseCase};
use crate::event_type::repository::EventTypeRepository;
use crate::shared::api_common::PaginationParams;
use crate::shared::authorization_service::checks;
use crate::shared::error::{NotFoundExt, PlatformError};
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
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

    /// Events of this type are per-client (Go `clientScoped`): the
    /// subscription editor offers client-scoped types only to client-scoped
    /// subscriptions. Distinct from `clientId`, which scopes the type itself.
    #[serde(default)]
    pub client_scoped: bool,
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

    /// Events of this type are per-client; absent leaves it unchanged (Go
    /// `clientScoped`).
    #[serde(default)]
    pub client_scoped: Option<bool>,
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
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub updated_at: String,
    pub spec_versions: Vec<SpecVersionResponse>,
}

/// Schema version response (Go's `specVersionResponse`).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecVersionResponse {
    pub version: String,
    /// The schema document (`null` when none).
    #[schema(required = true, value_type = serde_json::Value)]
    pub schema: Option<serde_json::Value>,
    pub status: String,
    #[schema(format = DateTime)]
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
    #[param(ignore)]
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
    operation_id = "createEventType",
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
        code: CreateEventTypeCommand::parse_code(&req.code, &req.name)?,
        name: req.name,
        description: req.description,
        client_id: req.client_id,
        client_scoped: req.client_scoped,
        schema: req.schema,
    };
    // Go's `CreateEventType`: the use case validates the command (the code
    // parsed above), then checks the scope (`CheckScopeAccess`).
    let ctx = ExecutionContext::from_auth(&auth.0);
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
    operation_id = "getEventType",
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
    operation_id = "getEventTypeByCode",
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
    operation_id = "listEventTypes",
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
    operation_id = "updateEventType",
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
    // The use case loads the type (404) and checks the caller's scope on it
    // (Go `CheckScopeAccess`).
    let cmd = UpdateEventTypeCommand {
        event_type_id: id,
        name: Some(req.name),
        description: req.description,
        client_scoped: req.client_scoped,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.update_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Add schema version to event type
#[utoipa::path(
    post,
    path = "/{id}/versions",
    tag = "event-types",
    operation_id = "addEventTypeVersion",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    request_body = AddEventTypeSchemaRequest,
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
    Json(req): Json<AddEventTypeSchemaRequest>,
) -> Result<Json<EventTypeResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;
    // Go registers one handler for `/versions` and `/schemas`: the version
    // is the caller's, and a repeat is 409 `VERSION_EXISTS`.
    add_schema(
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
    operation_id = "deleteEventType",
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

    // The use case loads the type (404) and checks the caller's scope on it.
    let cmd = DeleteEventTypeCommand { event_type_id: id };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── Go-parity routes (formerly go_api.rs) ────────────────────────────────────
//
// Event-type routes Go serves that Rust lacked:
//
// - `POST /api/event-types/{id}/schemas` (Go `eventtype/api/api.go:44`):
//   `{version, schema}` → 200 with the event type. Rust's `/versions`
//   numbers the version itself; Go's takes it from the body.
// - `PUT /bff/event-types/{id}` (Go `shared/bff/event_types.go:43`): the
//   same update as Rust's PATCH → 204.

#[derive(Clone)]
pub struct EventTypeGoState {
    pub event_type_repo: Arc<EventTypeRepository>,
    pub add_schema_use_case: Arc<AddSchemaUseCase<PgUnitOfWork>>,
    pub bff: BffEventTypesState,
}

/// Go `AddSchemaRequest`: both members required (huma's 400 `VALIDATION`
/// when absent); a blank version or a `null` schema is refused by the
/// handler.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = AddSchemaRequest)]
pub struct AddEventTypeSchemaRequest {
    pub version: String,
    #[schema(value_type = serde_json::Value)]
    pub schema: serde_json::Value,
}

/// Add a schema version named in the body (Go `addEventTypeSchema`).
#[utoipa::path(
    post,
    path = "/api/event-types/{id}/schemas",
    tag = "event-types",
    operation_id = "addEventTypeSchema",
    params(("id" = String, Path, description = "Event type id")),
    request_body = AddEventTypeSchemaRequest,
    responses(
        (status = 200, description = "The event type", body = EventTypeResponse),
        (status = 400, description = "No version or schema"),
        (status = 404, description = "Unknown event type"),
        (status = 409, description = "The version exists")
    ),
    security(("bearer_auth" = []))
)]
pub async fn add_event_type_schema(
    State(state): State<EventTypeGoState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<AddEventTypeSchemaRequest>,
) -> Result<Json<EventTypeResponse>, PlatformError> {
    checks::can_write_event_types(&auth.0)?;
    add_schema(
        &state.event_type_repo,
        &state.add_schema_use_case,
        &auth,
        id,
        req,
    )
    .await
}

/// Go's `addSchema` (eventtype/api/api.go), shared by `/schemas` and
/// `/versions` once the permission is checked: validate, then the use case
/// loads (404), checks the scope (`CheckScopeAccess`) and adds the caller's
/// version; answer the event type.
pub(crate) async fn add_schema(
    repo: &EventTypeRepository,
    use_case: &AddSchemaUseCase<PgUnitOfWork>,
    auth: &Authenticated,
    id: String,
    req: AddEventTypeSchemaRequest,
) -> Result<Json<EventTypeResponse>, PlatformError> {
    if req.version.trim().is_empty() {
        return Err(PlatformError::bad_request_code(
            "VERSION_REQUIRED",
            "version is required",
        ));
    }
    let schema = Some(req.schema).filter(|s| !s.is_null()).ok_or_else(|| {
        PlatformError::bad_request_code("SCHEMA_REQUIRED", "schema payload is required")
    })?;
    // The use case loads the type (404) and checks the caller's scope on it.
    use_case
        .run(
            AddSchemaCommand {
                event_type_id: id.clone(),
                version: req.version.trim().to_string(),
                mime_type: "application/schema+json".to_string(),
                schema_content: Some(schema),
                schema_type: None,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    let refreshed = repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("EventType", &id))?;
    Ok(Json(refreshed.into()))
}
