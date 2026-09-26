//! Event-type routes Go serves that Rust lacked:
//!
//! - `POST /api/event-types/{id}/schemas` (Go `eventtype/api/api.go:44`):
//!   `{version, schema}` → 200 with the event type. Rust's `/versions`
//!   numbers the version itself; Go's takes it from the body.
//! - `PUT /bff/event-types/{id}` (Go `shared/bff/event_types.go:43`): the
//!   same update as Rust's PATCH → 204.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::event_type::api::EventTypeResponse;
use crate::event_type::operations::{AddSchemaCommand, AddSchemaUseCase};
use crate::event_type::repository::EventTypeRepository;
use crate::shared::authorization_service::checks;
use crate::shared::bff_event_types_api::{BffEventTypesState, BffUpdateEventTypeRequest};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

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
pub struct AddEventTypeSchemaRequest {
    pub version: String,
    #[schema(value_type = Object)]
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
/// `/versions` once the permission is checked: validate, load (404), scope
/// (`CheckScopeAccess`), add the caller's version, answer the event type.
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
    let event_type = repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("EventType", &id))?;
    checks::check_scope_access(&auth.0, event_type.client_id.as_deref())?;
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

/// Update an event type's name and description (Go's PUT; Rust's PATCH).
#[utoipa::path(
    put,
    path = "/bff/event-types/{id}",
    tag = "bff-event-types",
    operation_id = "putBffEventTypesById",
    params(("id" = String, Path, description = "Event type id")),
    request_body = BffUpdateEventTypeRequest,
    responses((status = 204, description = "Updated"))
)]
pub async fn bff_put_event_type(
    State(state): State<EventTypeGoState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<BffUpdateEventTypeRequest>,
) -> Result<StatusCode, PlatformError> {
    checks::can_update_event_types(&auth.0)?;
    crate::shared::bff_event_types_api::update_event_type(
        State(state.bff),
        auth,
        Path(id),
        Json(req),
    )
    .await
}

/// Full-path router; merged at the root.
pub fn event_type_go_router(state: EventTypeGoState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(add_event_type_schema))
        .routes(routes!(bff_put_event_type))
        .with_state(state)
}
