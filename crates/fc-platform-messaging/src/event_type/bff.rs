//! BFF Event Types API
//!
//! Backend-For-Frontend endpoints for event type management.
//! Provides a UI-friendly view of event types at `/bff/event-types`.

use fc_platform_core::shared::id::EventTypeId;
use std::collections::HashSet;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::event_type::access;
use crate::event_type::api::EventTypeGoState;
use crate::event_type::bff;
use crate::event_type::entity::{EventType, EventTypeStatus, SpecVersion};
use crate::event_type::operations::SyncEventTypesCommand;
use crate::event_type::operations::{
    AddSchemaCommand, AddSchemaUseCase, ArchiveEventTypeCommand, ArchiveEventTypeUseCase,
    CreateEventTypeCommand, CreateEventTypeUseCase, DeleteEventTypeCommand, DeleteEventTypeUseCase,
    DeprecateSchemaCommand, DeprecateSchemaUseCase, FinaliseSchemaCommand, FinaliseSchemaUseCase,
    SyncEventTypesUseCase, UpdateEventTypeCommand, UpdateEventTypeUseCase,
};
use crate::event_type::repository::EventTypeRepository;
use crate::seed::platform_event_types;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::jsonb_text;
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

// ── Response DTOs ──────────────────────────────────────────────────────────

/// BFF spec version response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffSpecVersionResponse {
    pub id: String,
    pub version: String,
    pub status: String,
    pub schema_type: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<SpecVersion> for BffSpecVersionResponse {
    fn from(v: SpecVersion) -> Self {
        Self {
            id: v.id.into_string(),
            version: v.version,
            status: v.status.as_str().to_string(),
            schema_type: v.schema_type.as_str().to_string(),
            mime_type: v.mime_type,
            // The stored `jsonb` text, as Go serves it.
            schema: v.schema_content.as_ref().map(jsonb_text::jsonb_text),
            created_at: v.created_at.to_rfc3339(),
            updated_at: v.updated_at.to_rfc3339(),
        }
    }
}

/// BFF event type response — UI-friendly view
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffEventTypeResponse {
    pub id: String,
    /// Full code e.g. "myapp:orders:order:created"
    pub code: String,
    pub application: String,
    pub subdomain: String,
    pub aggregate: String,
    pub event: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// "CURRENT", "ARCHIVED"
    pub status: String,
    pub client_scoped: bool,
    pub spec_versions: Vec<BffSpecVersionResponse>,
    /// ISO8601
    pub created_at: String,
    /// ISO8601
    pub updated_at: String,
}

impl From<EventType> for BffEventTypeResponse {
    fn from(et: EventType) -> Self {
        Self {
            id: et.id.to_string(),
            code: et.code,
            application: et.application,
            subdomain: et.subdomain,
            aggregate: et.aggregate,
            event: et.event_name,
            name: et.name,
            description: et.description,
            status: et.status.as_str().to_string(),
            client_scoped: et.client_scoped,
            spec_versions: et.spec_versions.into_iter().map(|v| v.into()).collect(),
            created_at: et.created_at.to_rfc3339(),
            updated_at: et.updated_at.to_rfc3339(),
        }
    }
}

/// BFF event type list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffEventTypeListResponse {
    pub items: Vec<BffEventTypeResponse>,
    pub total: usize,
}

/// Filter options response (matches frontend FilterOptionsResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffFilterOptionsResponse {
    pub options: Vec<String>,
}

// ── Request DTOs ──────────────────────────────────────────────────────────

/// Create event type request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffCreateEventTypeRequest {
    /// Event type code (format: application:subdomain:aggregate:event)
    pub code: String,
    /// Human-readable name
    pub name: String,
    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Initial JSON schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
    /// Accepted and ignored: event types are platform-wide (see
    /// `CreateEventTypeCommand`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Events of this type are per-client: honoured on create exactly as
    /// `/api/event-types` does (Go's BFF, ruling 2026-09-06 #7).
    #[serde(default)]
    pub client_scoped: bool,
}

/// Update event type request (metadata only)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffUpdateEventTypeRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Add schema version request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffAddSchemaRequest {
    /// The version the user entered (Go requires it: 400
    /// `VERSION_REQUIRED` when blank).
    #[serde(default)]
    pub version: String,
    /// Schema content
    pub schema: serde_json::Value,
    /// MIME type (absent or blank: the schema type's default,
    /// "application/schema+json" for a JSON Schema)
    #[serde(default)]
    pub mime_type: String,
    /// Schema type (defaults to "JSON_SCHEMA")
    pub schema_type: Option<String>,
}

// ── Query parameters ──────────────────────────────────────────────────────

/// Query parameters for BFF event types list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct BffEventTypesQuery {
    /// Filter by status (CURRENT, ARCHIVED)
    pub status: Option<String>,
    /// Filter by application
    pub application: Option<String>,
    /// Filter by subdomain
    pub subdomain: Option<String>,
    /// Filter by aggregate
    pub aggregate: Option<String>,
}

/// Query parameters for subdomain filter (cascading)
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct BffSubdomainFilterQuery {
    /// Filter subdomains by application
    pub application: Option<String>,
}

/// Query parameters for aggregate filter (cascading)
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct BffAggregateFilterQuery {
    /// Filter aggregates by application
    pub application: Option<String>,
    /// Filter aggregates by subdomain
    pub subdomain: Option<String>,
}

// ── State ─────────────────────────────────────────────────────────────────

/// BFF event types service state
#[derive(Clone)]
pub struct BffEventTypesState {
    pub event_type_repo: Arc<EventTypeRepository>,
    pub sync_use_case: Arc<SyncEventTypesUseCase<PgUnitOfWork>>,
    pub unit_of_work: Arc<PgUnitOfWork>,
}

// ── Sync request DTO ──────────────────────────────────────────────────────

/// Request body for sync-platform endpoint
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffSyncPlatformRequest {
    /// Must be `platform` when given: the platform's own event types sync
    /// into `platform` only (owner ruling 16). Any other application is
    /// `400 PLATFORM_SYNC_ONLY`.
    #[serde(default)]
    pub application_code: Option<String>,
}

/// The application sync-platform syncs into.
const PLATFORM_APPLICATION_CODE: &str = "platform";

/// Owner ruling 16 (Java 635c2e3e): the platform's catalogue belongs to
/// `platform` only. A body naming no application (or a blank one) or
/// `platform` itself is fine; any other would put the platform's event
/// types into that application.
fn sync_platform_target(body: Option<BffSyncPlatformRequest>) -> Result<(), PlatformError> {
    match body
        .and_then(|b| b.application_code)
        .as_deref()
        .map(str::trim)
    {
        None | Some("") | Some(PLATFORM_APPLICATION_CODE) => Ok(()),
        Some(_) => Err(PlatformError::bad_request_code(
            "PLATFORM_SYNC_ONLY",
            "sync-platform syncs the platform's own event types into 'platform' only",
        )),
    }
}

/// Response for sync-platform endpoint
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffSyncSchemasResponse {
    pub created: u32,
    pub updated: u32,
    pub unchanged: u32,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffSyncPlatformResponse {
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub total: u32,
    pub schemas: BffSyncSchemasResponse,
}

// ── Handlers ──────────────────────────────────────────────────────────────

/// List event types with optional filters
#[utoipa::path(
    get,
    path = "",
    tag = "bff-event-types",
    operation_id = "getBffEventTypes",
    params(BffEventTypesQuery),
    responses(
        (status = 200, description = "List of event types", body = BffEventTypeListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_event_types(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Query(params): Query<QueryPairs>,
) -> Result<Json<BffEventTypeListResponse>, PlatformError> {
    // Go `list` (bugs 1, 2): GET /api/event-types's checks, then the facets,
    // each matching any of its (repeatable) values.
    let status = first_value(&params, "status")
        .map(str::parse::<EventTypeStatus>)
        .transpose()?;
    let items: Vec<BffEventTypeResponse> =
        visible_event_types(&state.event_type_repo, &auth, status)
            .await?
            .into_iter()
            .filter(|et| {
                match_any(
                    et,
                    &values(&params, "application"),
                    &values(&params, "subdomain"),
                    &values(&params, "aggregate"),
                )
            })
            .map(|et| et.into())
            .collect();

    let total = items.len();
    Ok(Json(BffEventTypeListResponse { items, total }))
}

/// A query string's parameters, in order; a repeated name keeps every value
/// (the SPA's multi-selects send `application=a&application=b`).
type QueryPairs = Vec<(String, String)>;

/// Every value of the parameter `name`, in order.
fn values<'a>(params: &'a QueryPairs, name: &str) -> Vec<&'a str> {
    params
        .iter()
        .filter(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
        .collect()
}

/// The first value of `name`, an empty one being absent (Go `q.Get` read
/// through `strPtr`).
fn first_value<'a>(params: &'a QueryPairs, name: &str) -> Option<&'a str> {
    values(params, name)
        .into_iter()
        .next()
        .filter(|v| !v.is_empty())
}

/// Go `eventtype.MatchAny`: the event type's application, subdomain and
/// aggregate are each one of the chosen values; an empty choice does not
/// filter.
fn match_any(
    et: &EventType,
    applications: &[&str],
    subdomains: &[&str],
    aggregates: &[&str],
) -> bool {
    let within = |set: &[&str], v: &str| set.is_empty() || set.contains(&v);
    within(applications, &et.application)
        && within(subdomains, &et.subdomain)
        && within(aggregates, &et.aggregate)
}

/// Go `visibleEventTypes`: the read permission, then every event type of
/// `status` (any when `None`) the caller may read. The BFF admits only the
/// messaging view permission ([`checks::can_read_event_types`]), whose
/// holders read every application's event types, so the application
/// confinement of `GET /api/event-types` has nothing to remove here; the
/// client scoping applies.
async fn visible_event_types(
    repo: &EventTypeRepository,
    auth: &Authenticated,
    status: Option<EventTypeStatus>,
) -> Result<Vec<EventType>, PlatformError> {
    checks::can_read_event_types(&auth.0)?;
    let rows = match status {
        Some(s) => repo.find_by_status(s).await?,
        None => repo.find_all().await?,
    };
    Ok(rows
        .into_iter()
        .filter(|et| access::ensure_visible(&auth.0, et).is_ok())
        .collect())
}

/// Get event type by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "bff-event-types",
    operation_id = "getBffEventTypesById",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    responses(
        (status = 200, description = "Event type found", body = BffEventTypeResponse),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_event_type(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<BffEventTypeResponse>, PlatformError> {
    let id = EventTypeId::from_wire(id);
    checks::can_read_event_types(&auth.0)?;

    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    // Check client access
    access::ensure_visible(&auth.0, &event_type)?;

    Ok(Json(event_type.into()))
}

/// Create a new event type
#[utoipa::path(
    post,
    path = "",
    tag = "bff-event-types",
    operation_id = "postBffEventTypes",
    request_body = BffCreateEventTypeRequest,
    responses(
        (status = 201, description = "Event type created", body = BffEventTypeResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_event_type(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Json(req): Json<BffCreateEventTypeRequest>,
) -> Result<(StatusCode, Json<BffEventTypeResponse>), PlatformError> {
    // The permission here; the use case requires anchor reach (every event
    // type is platform-wide; a `clientId` in the body is ignored).
    checks::can_create_event_types(&auth.0)?;

    let ctx = ExecutionContext::from_auth(&auth.0);

    let cmd = CreateEventTypeCommand {
        code: CreateEventTypeCommand::parse_code(&req.code, &req.name)?,
        name: req.name,
        description: req.description,
        client_scoped: req.client_scoped,
        schema: None,
    };

    let use_case =
        CreateEventTypeUseCase::new(state.event_type_repo.clone(), state.unit_of_work.clone());
    let event = use_case.run(cmd, ctx.clone()).await.into_result()?;
    let id = event.event_type_id.clone();

    // If an initial schema was provided, add it as a separate use case call
    if let Some(schema) = req.schema {
        let schema_cmd = AddSchemaCommand {
            event_type_id: id.clone(),
            version: "1.0".to_string(),
            mime_type: None,
            schema_content: Some(schema),
            schema_type: None,
        };
        let schema_use_case =
            AddSchemaUseCase::new(state.event_type_repo.clone(), state.unit_of_work.clone());
        schema_use_case.run(schema_cmd, ctx).await.into_result()?;
    }

    // Go answers the created event type, reloaded so its spec versions
    // are as stored.
    let created = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::internal("post-create reload failed"))?;
    Ok((StatusCode::CREATED, Json(created.into())))
}

/// Update event type metadata
#[utoipa::path(
    patch,
    path = "/{id}",
    tag = "bff-event-types",
    operation_id = "patchBffEventTypesById",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    request_body = BffUpdateEventTypeRequest,
    responses(
        (status = 204, description = "Event type updated"),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_event_type(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<BffUpdateEventTypeRequest>,
) -> Result<StatusCode, PlatformError> {
    let id = EventTypeId::from_wire(id);
    checks::can_update_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    access::ensure_modifiable(&auth.0, &event_type, "modify")?;

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = UpdateEventTypeCommand {
        event_type_id: id.clone(),
        name: req.name,
        description: req.description,
        // Go's BFF update takes name and description only.
        client_scoped: None,
    };

    let use_case =
        UpdateEventTypeUseCase::new(state.event_type_repo.clone(), state.unit_of_work.clone());
    use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Delete event type
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "bff-event-types",
    operation_id = "deleteBffEventTypesById",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    responses(
        (status = 204, description = "Event type deleted"),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_event_type(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    let id = EventTypeId::from_wire(id);
    checks::can_delete_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    access::ensure_modifiable(&auth.0, &event_type, "delete")?;

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = DeleteEventTypeCommand { event_type_id: id };

    let use_case =
        DeleteEventTypeUseCase::new(state.event_type_repo.clone(), state.unit_of_work.clone());
    use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Archive event type
#[utoipa::path(
    post,
    path = "/{id}/archive",
    tag = "bff-event-types",
    operation_id = "postBffEventTypesByIdArchive",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    responses(
        (status = 200, description = "Event type archived", body = BffEventTypeResponse),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn archive_event_type(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<BffEventTypeResponse>, PlatformError> {
    let id = EventTypeId::from_wire(id);
    checks::can_update_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    access::ensure_modifiable(&auth.0, &event_type, "archive")?;

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = ArchiveEventTypeCommand {
        event_type_id: id.clone(),
    };

    let use_case =
        ArchiveEventTypeUseCase::new(state.event_type_repo.clone(), state.unit_of_work.clone());
    use_case.run(cmd, ctx).await.into_result()?;

    // Re-fetch for the response
    let archived = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    Ok(Json(archived.into()))
}

/// Add a schema version to an event type
#[utoipa::path(
    post,
    path = "/{id}/schemas",
    tag = "bff-event-types",
    operation_id = "postBffEventTypesByIdSchemas",
    params(
        ("id" = String, Path, description = "Event type ID")
    ),
    request_body = BffAddSchemaRequest,
    responses(
        (status = 200, description = "Schema version added", body = BffEventTypeResponse),
        (status = 404, description = "Event type not found"),
        (status = 400, description = "Cannot add schema to archived event type")
    ),
    security(("bearer_auth" = []))
)]
pub async fn add_schema(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<BffAddSchemaRequest>,
) -> Result<Json<BffEventTypeResponse>, PlatformError> {
    let id = EventTypeId::from_wire(id);
    checks::can_update_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    access::ensure_visible(&auth.0, &event_type)?;

    // The version is the one the user entered (Go `addSchema`), not the
    // next number: a repeat is 409 `VERSION_EXISTS`.
    let version = req.version.trim().to_string();
    if version.is_empty() {
        return Err(PlatformError::bad_request_code(
            "VERSION_REQUIRED",
            "version is required",
        ));
    }

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = AddSchemaCommand {
        event_type_id: id.clone(),
        version,
        mime_type: Some(req.mime_type),
        schema_content: Some(req.schema),
        schema_type: req.schema_type,
    };

    let use_case = AddSchemaUseCase::new(state.event_type_repo.clone(), state.unit_of_work.clone());
    use_case.run(cmd, ctx).await.into_result()?;

    // Re-fetch for the response
    let updated = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    Ok(Json(updated.into()))
}

/// Finalise a schema version (FINALISING -> CURRENT)
#[utoipa::path(
    post,
    path = "/{id}/schemas/{version}/finalise",
    tag = "bff-event-types",
    operation_id = "postBffEventTypesByIdSchemasByVersionFinalise",
    params(
        ("id" = String, Path, description = "Event type ID"),
        ("version" = String, Path, description = "Schema version (e.g. 1.0)")
    ),
    responses(
        (status = 200, description = "Schema finalised", body = BffEventTypeResponse),
        (status = 404, description = "Event type or version not found"),
        (status = 400, description = "Schema not in FINALISING status")
    ),
    security(("bearer_auth" = []))
)]
pub async fn finalise_schema(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Path((id, version)): Path<(String, String)>,
) -> Result<Json<BffEventTypeResponse>, PlatformError> {
    let id = EventTypeId::from_wire(id);
    checks::can_update_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    access::ensure_visible(&auth.0, &event_type)?;

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = FinaliseSchemaCommand {
        event_type_id: id.clone(),
        version,
    };

    let use_case =
        FinaliseSchemaUseCase::new(state.event_type_repo.clone(), state.unit_of_work.clone());
    use_case.run(cmd, ctx).await.into_result()?;

    // Re-fetch for the response
    let updated = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    Ok(Json(updated.into()))
}

/// Deprecate a schema version (CURRENT -> DEPRECATED)
#[utoipa::path(
    post,
    path = "/{id}/schemas/{version}/deprecate",
    tag = "bff-event-types",
    operation_id = "postBffEventTypesByIdSchemasByVersionDeprecate",
    params(
        ("id" = String, Path, description = "Event type ID"),
        ("version" = String, Path, description = "Schema version (e.g. 1.0)")
    ),
    responses(
        (status = 200, description = "Schema deprecated", body = BffEventTypeResponse),
        (status = 404, description = "Event type or version not found"),
        (status = 400, description = "Schema cannot be deprecated")
    ),
    security(("bearer_auth" = []))
)]
pub async fn deprecate_schema(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Path((id, version)): Path<(String, String)>,
) -> Result<Json<BffEventTypeResponse>, PlatformError> {
    let id = EventTypeId::from_wire(id);
    checks::can_update_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    access::ensure_visible(&auth.0, &event_type)?;

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = DeprecateSchemaCommand {
        event_type_id: id.clone(),
        version,
    };

    let use_case =
        DeprecateSchemaUseCase::new(state.event_type_repo.clone(), state.unit_of_work.clone());
    use_case.run(cmd, ctx).await.into_result()?;

    // Re-fetch for the response
    let updated = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    Ok(Json(updated.into()))
}

// ── Sync endpoint ─────────────────────────────────────────────────────────

/// Trigger a platform-side sync of event types for an application
#[utoipa::path(
    post,
    path = "/sync-platform",
    tag = "bff-event-types",
    operation_id = "postBffEventTypesSyncPlatform",
    request_body = BffSyncPlatformRequest,
    responses(
        (status = 200, description = "Event types synced", body = BffSyncPlatformResponse),
        (status = 400, description = "PLATFORM_SYNC_ONLY: the body named another application")
    ),
    security(("bearer_auth" = []))
)]
pub async fn sync_platform(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    body: Option<Json<BffSyncPlatformRequest>>,
) -> Result<Json<BffSyncPlatformResponse>, PlatformError> {
    checks::can_sync_platform_event_types(&auth.0)?;

    sync_platform_target(body.map(|b| b.0))?;
    let cmd = platform_sync_command();
    let ctx = ExecutionContext::from_auth(&auth.0);
    let event = state.sync_use_case.run(cmd, ctx).await.into_result()?;

    Ok(Json(BffSyncPlatformResponse {
        created: event.created,
        updated: event.updated,
        deleted: event.deleted,
        total: event.synced_codes.len() as u32,
        schemas: BffSyncSchemasResponse {
            created: event.schemas_created,
            updated: event.schemas_updated,
            unchanged: event.schemas_unchanged,
        },
    }))
}

/// The sync of the platform's own event types (`platform:*`, from
/// `seed::platform_event_types`), shared by `POST
/// /bff/event-types/sync-platform` and the server-rendered `fc-web` UI.
/// Additive: types no longer defined are kept.
pub fn platform_sync_command() -> SyncEventTypesCommand {
    use crate::event_type::operations::{SyncEventTypeInput, SyncEventTypesCommand};
    let event_types = platform_event_types::definitions()
        .iter()
        .map(|def| SyncEventTypeInput {
            code: def.code.clone(),
            name: def.name.clone(),
            description: def.description.clone(),
            schema: def.schema.clone(),
        })
        .collect();
    SyncEventTypesCommand {
        application_code: PLATFORM_APPLICATION_CODE.to_string(),
        event_types,
        remove_unlisted: false,
    }
}

// ── Filter endpoints ──────────────────────────────────────────────────────

/// Get distinct applications from event types (for filter dropdown)
#[utoipa::path(
    get,
    path = "/filters/applications",
    tag = "bff-event-types",
    operation_id = "getBffEventTypesFiltersApplications",
    responses(
        (status = 200, description = "Application filter options", body = BffFilterOptionsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_applications(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
) -> Result<Json<BffFilterOptionsResponse>, PlatformError> {
    // Every event type the caller may read, of any status (Go
    // `eventTypeApplications`, bug 1).
    let event_types = visible_event_types(&state.event_type_repo, &auth, None).await?;
    Ok(Json(distinct_options(&event_types, |et| &et.application)))
}

/// The sorted distinct values of one facet, as the filter-options envelope.
fn distinct_options(
    rows: &[EventType],
    facet: impl Fn(&EventType) -> &String,
) -> BffFilterOptionsResponse {
    let mut options: Vec<String> = rows
        .iter()
        .map(|et| facet(et).clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    options.sort();
    BffFilterOptionsResponse { options }
}

/// Get distinct subdomains (with optional application filter)
#[utoipa::path(
    get,
    path = "/filters/subdomains",
    tag = "bff-event-types",
    operation_id = "getBffEventTypesFiltersSubdomains",
    params(BffSubdomainFilterQuery),
    responses(
        (status = 200, description = "Subdomain filter options", body = BffFilterOptionsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_subdomains(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Query(params): Query<QueryPairs>,
) -> Result<Json<BffFilterOptionsResponse>, PlatformError> {
    // The subdomains of the visible event types of any of the chosen
    // (repeatable) applications (Go `filterSubdomains`, bugs 1, 2).
    let event_types: Vec<EventType> = visible_event_types(&state.event_type_repo, &auth, None)
        .await?
        .into_iter()
        .filter(|et| match_any(et, &values(&params, "application"), &[], &[]))
        .collect();
    Ok(Json(distinct_options(&event_types, |et| &et.subdomain)))
}

/// Get distinct aggregates (with optional application and subdomain filters)
#[utoipa::path(
    get,
    path = "/filters/aggregates",
    tag = "bff-event-types",
    operation_id = "getBffEventTypesFiltersAggregates",
    params(BffAggregateFilterQuery),
    responses(
        (status = 200, description = "Aggregate filter options", body = BffFilterOptionsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_aggregates(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Query(params): Query<QueryPairs>,
) -> Result<Json<BffFilterOptionsResponse>, PlatformError> {
    // The aggregates of the visible event types of any of the chosen
    // applications and subdomains (Go `filterAggregates`, bugs 1, 2).
    let event_types: Vec<EventType> = visible_event_types(&state.event_type_repo, &auth, None)
        .await?
        .into_iter()
        .filter(|et| {
            match_any(
                et,
                &values(&params, "application"),
                &values(&params, "subdomain"),
                &[],
            )
        })
        .collect();
    Ok(Json(distinct_options(&event_types, |et| &et.aggregate)))
}

// ── Router ────────────────────────────────────────────────────────────────

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
    let id = EventTypeId::from_wire(id);
    checks::can_update_event_types(&auth.0)?;
    bff::update_event_type(State(state.bff), auth, Path(id.into_string()), Json(req)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(code: Option<&str>) -> Option<BffSyncPlatformRequest> {
        Some(BffSyncPlatformRequest {
            application_code: code.map(str::to_string),
        })
    }

    /// Owner ruling 16: sync-platform targets `platform` only.
    #[test]
    fn sync_platform_syncs_into_platform_only() {
        assert!(sync_platform_target(None).is_ok());
        assert!(sync_platform_target(body(None)).is_ok());
        assert!(sync_platform_target(body(Some("  "))).is_ok());
        assert!(sync_platform_target(body(Some("platform"))).is_ok());
        assert!(sync_platform_target(body(Some(" platform "))).is_ok());
        match sync_platform_target(body(Some("orders"))) {
            Err(PlatformError::Coded { status, code, .. }) => {
                assert_eq!(
                    (status.as_u16(), code.as_str()),
                    (400, "PLATFORM_SYNC_ONLY")
                )
            }
            other => panic!("expected PLATFORM_SYNC_ONLY, got {other:?}"),
        }
    }
}
