//! BFF Event Types API
//!
//! Backend-For-Frontend endpoints for event type management.
//! Provides a UI-friendly view of event types at `/bff/event-types`.

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashSet;

use crate::event_type::entity::{EventType, EventTypeStatus, SpecVersion};
use crate::event_type::repository::EventTypeRepository;
use crate::event_type::operations::{
    SyncEventTypesUseCase,
    CreateEventTypeCommand, CreateEventTypeUseCase,
    UpdateEventTypeCommand, UpdateEventTypeUseCase,
    DeleteEventTypeCommand, DeleteEventTypeUseCase,
    ArchiveEventTypeCommand, ArchiveEventTypeUseCase,
    AddSchemaCommand, AddSchemaUseCase,
    FinaliseSchemaCommand, FinaliseSchemaUseCase,
    DeprecateSchemaCommand, DeprecateSchemaUseCase,
};
use crate::application::repository::ApplicationRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::CreatedResponse;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

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
            id: v.id,
            version: v.version,
            status: v.status.as_str().to_string(),
            schema_type: v.schema_type.as_str().to_string(),
            mime_type: v.mime_type,
            schema: v.schema_content.map(|v| serde_json::to_string(&v).unwrap_or_default()),
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
            id: et.id,
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
    /// Client ID (optional, null = anchor-level)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
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
    /// Schema content
    pub schema: serde_json::Value,
    /// MIME type (defaults to "application/schema+json")
    #[serde(default = "default_mime_type")]
    pub mime_type: String,
    /// Schema type (defaults to "JSON_SCHEMA")
    pub schema_type: Option<String>,
}

fn default_mime_type() -> String {
    "application/schema+json".to_string()
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
    pub application_repo: Option<Arc<ApplicationRepository>>,
    pub sync_use_case: Arc<SyncEventTypesUseCase<crate::usecase::PgUnitOfWork>>,
    pub unit_of_work: Arc<PgUnitOfWork>,
}

// ── Sync request DTO ──────────────────────────────────────────────────────

/// Request body for sync-platform endpoint
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffSyncPlatformRequest {
    /// Application code to sync event types for
    pub application_code: String,
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
    Query(query): Query<BffEventTypesQuery>,
) -> Result<Json<BffEventTypeListResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_event_types(&auth.0)?;

    // Start with status-based or full list
    let event_types = if let Some(ref status) = query.status {
        let s = EventTypeStatus::from_str(status);
        state.event_type_repo.find_by_status(s).await?
    } else if let Some(ref app) = query.application {
        state.event_type_repo.find_by_application(app).await?
    } else {
        state.event_type_repo.find_all().await?
    };

    // Apply in-memory filters for subdomain/aggregate and client access
    let items: Vec<BffEventTypeResponse> = event_types
        .into_iter()
        .filter(|et| {
            // Filter by application if we fetched by status
            if query.status.is_some() {
                if let Some(ref app) = query.application {
                    if &et.application != app {
                        return false;
                    }
                }
            }
            // Filter by subdomain
            if let Some(ref sub) = query.subdomain {
                if &et.subdomain != sub {
                    return false;
                }
            }
            // Filter by aggregate
            if let Some(ref agg) = query.aggregate {
                if &et.aggregate != agg {
                    return false;
                }
            }
            // Client access filtering
            match &et.client_id {
                Some(cid) => auth.0.can_access_client(cid),
                None => true,
            }
        })
        .map(|et| et.into())
        .collect();

    let total = items.len();
    Ok(Json(BffEventTypeListResponse { items, total }))
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
    crate::shared::authorization_service::checks::can_read_event_types(&auth.0)?;

    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    // Check client access
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    }

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
        (status = 201, description = "Event type created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_event_type(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Json(req): Json<BffCreateEventTypeRequest>,
) -> Result<(axum::http::StatusCode, Json<CreatedResponse>), PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Validate client access if specified
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

    let ctx = ExecutionContext::from_auth(&auth.0);

    let cmd = CreateEventTypeCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        client_id: req.client_id,
        schema: None,
    };

    let use_case = CreateEventTypeUseCase::new(
        state.event_type_repo.clone(),
        state.unit_of_work.clone(),
    );
    let event = use_case.run(cmd, ctx.clone()).await.into_result()?;
    let id = event.event_type_id.clone();

    // If an initial schema was provided, add it as a separate use case call
    if let Some(schema) = req.schema {
        let schema_cmd = AddSchemaCommand {
            event_type_id: id.clone(),
            version: "1.0".to_string(),
            mime_type: "application/schema+json".to_string(),
            schema_content: Some(schema),
            schema_type: None,
        };
        let schema_use_case = AddSchemaUseCase::new(
            state.event_type_repo.clone(),
            state.unit_of_work.clone(),
        );
        schema_use_case.run(schema_cmd, ctx).await.into_result()?;
    }

    Ok((
        axum::http::StatusCode::CREATED,
        Json(CreatedResponse::new(id)),
    ))
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
) -> Result<axum::http::StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can modify anchor-level event types",
        ));
    }

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = UpdateEventTypeCommand {
        event_type_id: id.clone(),
        name: req.name,
        description: req.description,
    };

    let use_case = UpdateEventTypeUseCase::new(
        state.event_type_repo.clone(),
        state.unit_of_work.clone(),
    );
    use_case.run(cmd, ctx).await.into_result()?;

    Ok(axum::http::StatusCode::NO_CONTENT)
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
) -> Result<axum::http::StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can delete anchor-level event types",
        ));
    }

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = DeleteEventTypeCommand {
        event_type_id: id,
    };

    let use_case = DeleteEventTypeUseCase::new(
        state.event_type_repo.clone(),
        state.unit_of_work.clone(),
    );
    use_case.run(cmd, ctx).await.into_result()?;

    Ok(axum::http::StatusCode::NO_CONTENT)
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
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can archive anchor-level event types",
        ));
    }

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = ArchiveEventTypeCommand {
        event_type_id: id.clone(),
    };

    let use_case = ArchiveEventTypeUseCase::new(
        state.event_type_repo.clone(),
        state.unit_of_work.clone(),
    );
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
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    }

    // Calculate next version
    let next_version = format!("{}.0", event_type.spec_versions.len() + 1);

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = AddSchemaCommand {
        event_type_id: id.clone(),
        version: next_version,
        mime_type: req.mime_type,
        schema_content: Some(req.schema),
        schema_type: req.schema_type,
    };

    let use_case = AddSchemaUseCase::new(
        state.event_type_repo.clone(),
        state.unit_of_work.clone(),
    );
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
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    }

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = FinaliseSchemaCommand {
        event_type_id: id.clone(),
        version,
    };

    let use_case = FinaliseSchemaUseCase::new(
        state.event_type_repo.clone(),
        state.unit_of_work.clone(),
    );
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
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    // Fetch to check client access before calling the use case
    let event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    }

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = DeprecateSchemaCommand {
        event_type_id: id.clone(),
        version,
    };

    let use_case = DeprecateSchemaUseCase::new(
        state.event_type_repo.clone(),
        state.unit_of_work.clone(),
    );
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
        (status = 400, description = "Validation error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn sync_platform(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    body: Option<Json<BffSyncPlatformRequest>>,
) -> Result<Json<BffSyncPlatformResponse>, PlatformError> {
    use crate::event_type::operations::{SyncEventTypesCommand, SyncEventTypeInput};

    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    let application_code = body.map(|b| b.0.application_code).unwrap_or_else(|| "platform".to_string());

    let definitions = crate::seed::platform_event_types::definitions();
    let inputs: Vec<SyncEventTypeInput> = definitions.iter().map(|def| SyncEventTypeInput {
        code: def.code.clone(),
        name: def.name.clone(),
        description: def.description.clone(),
        schema: def.schema.clone(),
    }).collect();

    let cmd = SyncEventTypesCommand {
        application_code,
        event_types: inputs,
        remove_unlisted: false,
    };
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
    _auth: Authenticated,
) -> Result<Json<BffFilterOptionsResponse>, PlatformError> {
    let event_types = state.event_type_repo.find_active().await?;

    let mut options: Vec<String> = event_types
        .iter()
        .map(|et| et.application.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    options.sort();

    Ok(Json(BffFilterOptionsResponse { options }))
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
    _auth: Authenticated,
    Query(query): Query<BffSubdomainFilterQuery>,
) -> Result<Json<BffFilterOptionsResponse>, PlatformError> {
    let event_types = state.event_type_repo.find_active().await?;

    let filtered: Vec<_> = if let Some(ref app) = query.application {
        event_types
            .into_iter()
            .filter(|et| &et.application == app)
            .collect()
    } else {
        event_types
    };

    let mut options: Vec<String> = filtered
        .iter()
        .map(|et| et.subdomain.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    options.sort();

    Ok(Json(BffFilterOptionsResponse { options }))
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
    _auth: Authenticated,
    Query(query): Query<BffAggregateFilterQuery>,
) -> Result<Json<BffFilterOptionsResponse>, PlatformError> {
    let event_types = state.event_type_repo.find_active().await?;

    let filtered: Vec<_> = event_types
        .into_iter()
        .filter(|et| {
            let app_match = query
                .application
                .as_ref()
                .map_or(true, |app| &et.application == app);
            let sub_match = query
                .subdomain
                .as_ref()
                .map_or(true, |sub| &et.subdomain == sub);
            app_match && sub_match
        })
        .collect();

    let mut options: Vec<String> = filtered
        .iter()
        .map(|et| et.aggregate.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    options.sort();

    Ok(Json(BffFilterOptionsResponse { options }))
}

// ── Router ────────────────────────────────────────────────────────────────

/// Create BFF event types router (mounted at `/bff/event-types`)
pub fn bff_event_types_router(state: BffEventTypesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_event_type, list_event_types))
        .routes(routes!(sync_platform))
        .routes(routes!(get_filter_applications))
        .routes(routes!(get_filter_subdomains))
        .routes(routes!(get_filter_aggregates))
        .routes(routes!(get_event_type, update_event_type, delete_event_type))
        .routes(routes!(archive_event_type))
        .routes(routes!(add_schema))
        .routes(routes!(finalise_schema))
        .routes(routes!(deprecate_schema))
        .with_state(state)
}
