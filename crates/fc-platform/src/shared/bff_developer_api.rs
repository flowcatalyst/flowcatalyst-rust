//! /bff/developer — frontend Developer portal endpoints
//!
//! Cookie-authenticated reads of an application's OpenAPI document and its
//! event types, plus a write endpoint that re-syncs the platform's own
//! OpenAPI document (the dynamic utoipa-generated spec captured at boot).
//!
//! Visibility is scoped per principal: non-anchor users only see applications
//! their `iam_principal_application_access` grants include. The 'platform'
//! application is always visible to every developer-role holder — that's the
//! whole point of seeding it.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::application::repository::ApplicationRepository;
use crate::application_openapi_spec::operations::{SyncOpenApiSpecCommand, SyncOpenApiSpecUseCase};
use crate::application_openapi_spec::repository::OpenApiSpecRepository;
use crate::event_type::repository::EventTypeRepository;
use crate::shared::authorization_service::AuthContext;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UseCase};
use crate::PrincipalRepository;

#[derive(Clone)]
pub struct BffDeveloperState {
    pub application_repo: Arc<ApplicationRepository>,
    pub openapi_spec_repo: Arc<OpenApiSpecRepository>,
    pub event_type_repo: Arc<EventTypeRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub sync_openapi_use_case: Arc<SyncOpenApiSpecUseCase<crate::usecase::PgUnitOfWork>>,
    /// The platform's own OpenAPI document, captured at server boot from the
    /// utoipa-generated spec. Refreshed in-place is not needed — the value is
    /// compile-time-derived and constant for a given binary.
    pub platform_openapi: Arc<serde_json::Value>,
    /// The application id the platform document is stored against
    /// (`code='platform'`). Resolved at server boot.
    pub platform_application_id: String,
}

// -- DTOs ---------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperApplicationSummary {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub current_version: Option<String>,
    pub current_spec_id: Option<String>,
    pub current_synced_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperApplicationsResponse {
    pub items: Vec<DeveloperApplicationSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiSpecResponse {
    pub id: String,
    pub application_id: String,
    pub version: String,
    pub status: String,
    pub spec: serde_json::Value,
    pub change_notes_text: Option<String>,
    pub change_notes: Option<crate::application_openapi_spec::entity::ChangeNotes>,
    pub synced_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiVersionSummary {
    pub id: String,
    pub version: String,
    pub status: String,
    pub change_notes_text: Option<String>,
    pub has_breaking: bool,
    pub synced_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiVersionsResponse {
    pub items: Vec<OpenApiVersionSummary>,
}

/// A single spec version's body, shipped inline so the Developer portal can
/// render schemas + sample-code without a per-row fetch round trip.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperSpecVersionSummary {
    pub id: String,
    pub version: String,
    pub status: String,
    pub schema: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperEventTypeSummary {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub application: String,
    pub subdomain: String,
    pub aggregate: String,
    pub event_name: String,
    pub spec_versions: Vec<DeveloperSpecVersionSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperEventTypesResponse {
    pub items: Vec<DeveloperEventTypeSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlatformOpenApiResponse {
    pub application_code: String,
    pub spec_id: String,
    pub version: String,
    pub status: String,
    pub archived_prior_version: Option<String>,
    pub has_breaking: bool,
    pub unchanged: bool,
}

// -- Helpers ------------------------------------------------------------------

/// Resolve the set of application ids the caller may see. Anchor users see
/// all active applications; everyone else is restricted to their explicit
/// access grants (`iam_principal_application_access`) — but the 'platform'
/// row is always granted so the developer portal works without per-user
/// grants for the platform itself.
async fn accessible_application_ids(
    state: &BffDeveloperState,
    auth: &AuthContext,
) -> Result<Vec<String>, PlatformError> {
    if auth.is_anchor() || auth.has_permission(crate::permissions::ADMIN_ALL) {
        let apps = state.application_repo.find_active().await?;
        return Ok(apps.into_iter().map(|a| a.id).collect());
    }

    let principal = state
        .principal_repo
        .find_by_id(&auth.principal_id)
        .await?
        .ok_or_else(|| {
            PlatformError::forbidden(format!("Principal {} not found", auth.principal_id))
        })?;
    let mut ids = principal.accessible_application_ids;
    if !ids.contains(&state.platform_application_id) {
        ids.push(state.platform_application_id.clone());
    }
    Ok(ids)
}

async fn require_app_access(
    state: &BffDeveloperState,
    auth: &AuthContext,
    application_id: &str,
) -> Result<(), PlatformError> {
    let ids = accessible_application_ids(state, auth).await?;
    if ids.iter().any(|id| id == application_id) {
        Ok(())
    } else {
        Err(PlatformError::forbidden(format!(
            "No access to application {}",
            application_id
        )))
    }
}

// -- Handlers -----------------------------------------------------------------

/// List the applications the current principal may browse in the developer
/// portal. Each entry carries a snapshot of its CURRENT OpenAPI version (if
/// any) so the list view can show "v2.1.0 · 3 days ago" inline.
pub async fn list_applications(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
) -> Result<Json<DeveloperApplicationsResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_application_openapi(&auth.0)?;

    let ids = accessible_application_ids(&state, &auth.0).await?;
    if ids.is_empty() {
        return Ok(Json(DeveloperApplicationsResponse { items: vec![] }));
    }

    let all_apps = state.application_repo.find_active().await?;
    let mut items = Vec::new();
    for app in all_apps {
        if !ids.contains(&app.id) {
            continue;
        }
        let current = state
            .openapi_spec_repo
            .find_current_by_application(&app.id)
            .await
            .ok()
            .flatten();
        items.push(DeveloperApplicationSummary {
            id: app.id,
            code: app.code,
            name: app.name,
            description: app.description,
            icon_url: app.icon_url,
            current_version: current.as_ref().map(|s| s.version.clone()),
            current_spec_id: current.as_ref().map(|s| s.id.clone()),
            current_synced_at: current.as_ref().map(|s| s.synced_at),
        });
    }
    items.sort_by_key(|a| a.name.to_lowercase());
    Ok(Json(DeveloperApplicationsResponse { items }))
}

pub async fn get_application(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
    Path(app_id): Path<String>,
) -> Result<Json<DeveloperApplicationSummary>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_application_openapi(&auth.0)?;
    require_app_access(&state, &auth.0, &app_id).await?;

    let app = state
        .application_repo
        .find_by_id(&app_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &app_id))?;
    let current = state
        .openapi_spec_repo
        .find_current_by_application(&app_id)
        .await
        .ok()
        .flatten();
    Ok(Json(DeveloperApplicationSummary {
        id: app.id,
        code: app.code,
        name: app.name,
        description: app.description,
        icon_url: app.icon_url,
        current_version: current.as_ref().map(|s| s.version.clone()),
        current_spec_id: current.as_ref().map(|s| s.id.clone()),
        current_synced_at: current.as_ref().map(|s| s.synced_at),
    }))
}

pub async fn get_current_openapi(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
    Path(app_id): Path<String>,
) -> Result<Json<OpenApiSpecResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_application_openapi(&auth.0)?;
    require_app_access(&state, &auth.0, &app_id).await?;

    let spec = state
        .openapi_spec_repo
        .find_current_by_application(&app_id)
        .await?
        .ok_or_else(|| {
            PlatformError::not_found("OpenApiSpec(current)", format!("application_id={}", app_id))
        })?;
    Ok(Json(OpenApiSpecResponse {
        id: spec.id,
        application_id: spec.application_id,
        version: spec.version,
        status: spec.status.as_str().to_string(),
        spec: spec.spec,
        change_notes_text: spec.change_notes_text,
        change_notes: spec.change_notes,
        synced_at: spec.synced_at,
    }))
}

pub async fn list_versions(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
    Path(app_id): Path<String>,
) -> Result<Json<OpenApiVersionsResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_application_openapi(&auth.0)?;
    require_app_access(&state, &auth.0, &app_id).await?;

    let rows = state
        .openapi_spec_repo
        .find_all_by_application(&app_id)
        .await?;
    let items = rows
        .into_iter()
        .map(|s| OpenApiVersionSummary {
            id: s.id,
            version: s.version,
            status: s.status.as_str().to_string(),
            has_breaking: s
                .change_notes
                .as_ref()
                .map(|c| c.has_breaking)
                .unwrap_or(false),
            change_notes_text: s.change_notes_text,
            synced_at: s.synced_at,
        })
        .collect();
    Ok(Json(OpenApiVersionsResponse { items }))
}

pub async fn get_version(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
    Path((app_id, spec_id)): Path<(String, String)>,
) -> Result<Json<OpenApiSpecResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_application_openapi(&auth.0)?;
    require_app_access(&state, &auth.0, &app_id).await?;

    let spec = state
        .openapi_spec_repo
        .find_by_id(&spec_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("OpenApiSpec", &spec_id))?;
    if spec.application_id != app_id {
        return Err(PlatformError::not_found("OpenApiSpec", &spec_id));
    }
    Ok(Json(OpenApiSpecResponse {
        id: spec.id,
        application_id: spec.application_id,
        version: spec.version,
        status: spec.status.as_str().to_string(),
        spec: spec.spec,
        change_notes_text: spec.change_notes_text,
        change_notes: spec.change_notes,
        synced_at: spec.synced_at,
    }))
}

pub async fn list_event_types(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
    Path(app_id): Path<String>,
) -> Result<Json<DeveloperEventTypesResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_application_openapi(&auth.0)?;
    require_app_access(&state, &auth.0, &app_id).await?;

    let app = state
        .application_repo
        .find_by_id(&app_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &app_id))?;
    let event_types = state.event_type_repo.find_by_application(&app.code).await?;
    let items = event_types
        .into_iter()
        .map(|et| DeveloperEventTypeSummary {
            id: et.id,
            code: et.code,
            name: et.name,
            description: et.description,
            status: et.status.as_str().to_string(),
            application: et.application,
            subdomain: et.subdomain,
            aggregate: et.aggregate,
            event_name: et.event_name,
            spec_versions: et
                .spec_versions
                .into_iter()
                .map(|sv| DeveloperSpecVersionSummary {
                    id: sv.id,
                    version: sv.version,
                    status: sv.status.as_str().to_string(),
                    // The schema is serialised as a JSON string here to match
                    // the wire shape SchemaViewerDialog expects (it does
                    // `JSON.parse` on a string field).
                    schema: sv
                        .schema_content
                        .map(|v| serde_json::to_string(&v).unwrap_or_default()),
                })
                .collect(),
        })
        .collect();
    Ok(Json(DeveloperEventTypesResponse { items }))
}

/// Re-sync the platform's own OpenAPI document. Reads the in-process value
/// captured at server boot (so this is an in-memory operation, not an HTTP
/// self-call) and pipes it through `SyncOpenApiSpecUseCase` against the
/// seeded `code='platform'` application row.
pub async fn sync_platform_openapi(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
) -> Result<Json<SyncPlatformOpenApiResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_sync_application_openapi(&auth.0)?;

    let command = SyncOpenApiSpecCommand {
        application_id: state.platform_application_id.clone(),
        application_code: "platform".to_string(),
        spec: (*state.platform_openapi).clone(),
    };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state
        .sync_openapi_use_case
        .run(command, ctx)
        .await
        .into_result()
    {
        Ok(event) => Ok(Json(SyncPlatformOpenApiResponse {
            application_code: event.application_code,
            spec_id: event.spec_id,
            version: event.version,
            status: if event.unchanged {
                "UNCHANGED".to_string()
            } else {
                "CURRENT".to_string()
            },
            archived_prior_version: event.archived_prior_version,
            has_breaking: event.has_breaking,
            unchanged: event.unchanged,
        })),
        Err(err) => Err(err.into()),
    }
}

pub fn bff_developer_router(state: BffDeveloperState) -> Router {
    Router::new()
        .route("/applications", get(list_applications))
        .route("/applications/{app_id}", get(get_application))
        .route(
            "/applications/{app_id}/openapi/current",
            get(get_current_openapi),
        )
        .route(
            "/applications/{app_id}/openapi/versions",
            get(list_versions),
        )
        .route(
            "/applications/{app_id}/openapi/versions/{spec_id}",
            get(get_version),
        )
        .route("/applications/{app_id}/event-types", get(list_event_types))
        .route("/sync-platform-openapi", post(sync_platform_openapi))
        .with_state(state)
}
