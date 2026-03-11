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

use crate::event_type::entity::{EventType, EventTypeStatus, SpecVersion, SpecVersionStatus};
use crate::event_type::repository::EventTypeRepository;
use crate::application::repository::ApplicationRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::CreatedResponse;
use crate::shared::middleware::Authenticated;

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
    pub schema: Option<serde_json::Value>,
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
            schema: v.schema_content,
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

/// Filter option for dropdowns
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffFilterOption {
    pub value: String,
    pub label: String,
}

/// Application filter options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffApplicationsResponse {
    pub applications: Vec<BffFilterOption>,
}

/// Subdomain filter options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffSubdomainsResponse {
    pub subdomains: Vec<BffFilterOption>,
}

/// Aggregate filter options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffAggregatesResponse {
    pub aggregates: Vec<BffFilterOption>,
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

    // Check for duplicate code
    if let Some(_) = state.event_type_repo.find_by_code(&req.code).await? {
        return Err(PlatformError::duplicate("EventType", "code", &req.code));
    }

    let mut event_type =
        EventType::new(&req.code, &req.name).map_err(|e| PlatformError::validation(e))?;

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
        (status = 200, description = "Event type updated", body = BffEventTypeResponse),
        (status = 404, description = "Event type not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_event_type(
    State(state): State<BffEventTypesState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<BffUpdateEventTypeRequest>,
) -> Result<Json<BffEventTypeResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_event_types(&auth.0)?;

    let mut event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    // Check client access
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can modify anchor-level event types",
        ));
    }

    if let Some(name) = req.name {
        event_type.name = name;
    }
    if let Some(desc) = req.description {
        event_type.description = Some(desc);
    }
    event_type.updated_at = chrono::Utc::now();

    state.event_type_repo.update(&event_type).await?;

    Ok(Json(event_type.into()))
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
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can delete anchor-level event types",
        ));
    }

    state.event_type_repo.delete(&id).await?;

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

    let mut event_type = state
        .event_type_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("EventType", &id))?;

    // Check client access
    if let Some(ref cid) = event_type.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this event type"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can archive anchor-level event types",
        ));
    }

    event_type.archive();
    state.event_type_repo.update(&event_type).await?;

    Ok(Json(event_type.into()))
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

    let mut event_type = state
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

    // Business rule: cannot add schema to archived event type
    if event_type.status == EventTypeStatus::Archived {
        return Err(PlatformError::validation(
            "Cannot add schema to an archived event type",
        ));
    }

    // Calculate next version
    let next_version = format!("{}.0", event_type.spec_versions.len() + 1);
    let mut spec = SpecVersion::new(&event_type.id, &next_version, Some(req.schema));
    spec.mime_type = req.mime_type;
    if let Some(ref st) = req.schema_type {
        spec.schema_type = crate::event_type::entity::SchemaType::from_str(st);
    }

    // Persist the new spec version
    state.event_type_repo.insert_spec_version(&spec).await?;

    // Add to in-memory event type and update
    event_type.add_schema_version(spec);
    state.event_type_repo.update(&event_type).await?;

    Ok(Json(event_type.into()))
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

    let mut event_type = state
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

    // Find the target version
    let target_idx = event_type
        .spec_versions
        .iter()
        .position(|sv| sv.version == version)
        .ok_or_else(|| {
            PlatformError::not_found("SchemaVersion", &format!("{}:{}", id, version))
        })?;

    // Must be FINALISING
    if event_type.spec_versions[target_idx].status != SpecVersionStatus::Finalising {
        return Err(PlatformError::validation(format!(
            "Schema version '{}' is not in FINALISING status",
            version
        )));
    }

    // Extract major version for auto-deprecation
    let target_major: Option<u32> = version.split('.').next().and_then(|s| s.parse().ok());

    // Auto-deprecate existing CURRENT versions with same major
    if let Some(major) = target_major {
        for sv in &mut event_type.spec_versions {
            if sv.status == SpecVersionStatus::Current {
                let sv_major: Option<u32> =
                    sv.version.split('.').next().and_then(|s| s.parse().ok());
                if sv_major == Some(major) {
                    sv.status = SpecVersionStatus::Deprecated;
                    sv.updated_at = chrono::Utc::now();
                    state.event_type_repo.update_spec_version(sv).await?;
                }
            }
        }
    }

    // Finalise target version
    event_type.spec_versions[target_idx].status = SpecVersionStatus::Current;
    event_type.spec_versions[target_idx].updated_at = chrono::Utc::now();
    event_type.updated_at = chrono::Utc::now();

    state
        .event_type_repo
        .update_spec_version(&event_type.spec_versions[target_idx])
        .await?;
    state.event_type_repo.update(&event_type).await?;

    Ok(Json(event_type.into()))
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

    let mut event_type = state
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

    // Find target version
    let target_idx = event_type
        .spec_versions
        .iter()
        .position(|sv| sv.version == version)
        .ok_or_else(|| {
            PlatformError::not_found("SchemaVersion", &format!("{}:{}", id, version))
        })?;

    // Cannot deprecate FINALISING schemas
    if event_type.spec_versions[target_idx].status == SpecVersionStatus::Finalising {
        return Err(PlatformError::validation(
            "Cannot deprecate a schema that is still in FINALISING status",
        ));
    }

    // Cannot deprecate already deprecated
    if event_type.spec_versions[target_idx].status == SpecVersionStatus::Deprecated {
        return Err(PlatformError::validation(
            "Schema version is already deprecated",
        ));
    }

    // Deprecate
    event_type.spec_versions[target_idx].status = SpecVersionStatus::Deprecated;
    event_type.spec_versions[target_idx].updated_at = chrono::Utc::now();
    event_type.updated_at = chrono::Utc::now();

    state
        .event_type_repo
        .update_spec_version(&event_type.spec_versions[target_idx])
        .await?;
    state.event_type_repo.update(&event_type).await?;

    Ok(Json(event_type.into()))
}

// ── Filter endpoints ──────────────────────────────────────────────────────

/// Get distinct applications from event types (for filter dropdown)
#[utoipa::path(
    get,
    path = "/filters/applications",
    tag = "bff-event-types",
    operation_id = "getBffEventTypesFiltersApplications",
    responses(
        (status = 200, description = "Application filter options", body = BffApplicationsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_applications(
    State(state): State<BffEventTypesState>,
    _auth: Authenticated,
) -> Result<Json<BffApplicationsResponse>, PlatformError> {
    let event_types = state.event_type_repo.find_active().await?;

    let mut applications: Vec<BffFilterOption> = event_types
        .iter()
        .map(|et| et.application.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|app| BffFilterOption {
            value: app.clone(),
            label: app,
        })
        .collect();
    applications.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(Json(BffApplicationsResponse { applications }))
}

/// Get distinct subdomains (with optional application filter)
#[utoipa::path(
    get,
    path = "/filters/subdomains",
    tag = "bff-event-types",
    operation_id = "getBffEventTypesFiltersSubdomains",
    params(BffSubdomainFilterQuery),
    responses(
        (status = 200, description = "Subdomain filter options", body = BffSubdomainsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_subdomains(
    State(state): State<BffEventTypesState>,
    _auth: Authenticated,
    Query(query): Query<BffSubdomainFilterQuery>,
) -> Result<Json<BffSubdomainsResponse>, PlatformError> {
    let event_types = state.event_type_repo.find_active().await?;

    let filtered: Vec<_> = if let Some(ref app) = query.application {
        event_types
            .into_iter()
            .filter(|et| &et.application == app)
            .collect()
    } else {
        event_types
    };

    let mut subdomains: Vec<BffFilterOption> = filtered
        .iter()
        .map(|et| et.subdomain.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|sub| BffFilterOption {
            value: sub.clone(),
            label: sub,
        })
        .collect();
    subdomains.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(Json(BffSubdomainsResponse { subdomains }))
}

/// Get distinct aggregates (with optional application and subdomain filters)
#[utoipa::path(
    get,
    path = "/filters/aggregates",
    tag = "bff-event-types",
    operation_id = "getBffEventTypesFiltersAggregates",
    params(BffAggregateFilterQuery),
    responses(
        (status = 200, description = "Aggregate filter options", body = BffAggregatesResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_aggregates(
    State(state): State<BffEventTypesState>,
    _auth: Authenticated,
    Query(query): Query<BffAggregateFilterQuery>,
) -> Result<Json<BffAggregatesResponse>, PlatformError> {
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

    let mut aggregates: Vec<BffFilterOption> = filtered
        .iter()
        .map(|et| et.aggregate.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|agg| BffFilterOption {
            value: agg.clone(),
            label: agg,
        })
        .collect();
    aggregates.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(Json(BffAggregatesResponse { aggregates }))
}

// ── Router ────────────────────────────────────────────────────────────────

/// Create BFF event types router (mounted at `/bff/event-types`)
pub fn bff_event_types_router(state: BffEventTypesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_event_type, list_event_types))
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
