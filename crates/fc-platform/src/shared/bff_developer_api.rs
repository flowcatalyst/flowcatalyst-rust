//! /bff/developer — frontend Developer portal endpoints
//!
//! Cookie-authenticated reads of an application's OpenAPI document and its
//! event types, plus a write endpoint that re-syncs the platform's own
//! OpenAPI document (the dynamic utoipa-generated spec captured at boot).
//!
//! Who sees what (owner decision #37, `docs/owner-decisions-2026-09-25.md`):
//! - an anchor caller holding `platform:developer:application-openapi:view`
//!   sees every active application, as Go's `CanReadDeveloperPortal`
//!   (shared/bff/developer.go);
//! - a non-anchor caller holding the view or manage permission (an
//!   application-scoped developer) sees the applications it can access (Go's
//!   `CanAccessApplication`: every application with `all_applications`, else
//!   its grants) plus the seeded `platform` application; any other
//!   application is 404, as an out-of-scope application is everywhere;
//! - anyone else is refused as Go refuses them.
//!
//! Response shapes are Go's. The platform sync needs anchor and `…:sync`.

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
use crate::shared::authorization_service::{ApplicationScope, AuthContext};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_spec_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_notes_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_notes: Option<crate::application_openapi_spec::entity::ChangeNotes>,
    pub synced_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiVersionSummary {
    pub id: String,
    pub version: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperEventTypeSummary {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    pub application: String,
    pub subdomain: String,
    pub aggregate: String,
    pub event_name: String,
    /// `null` for an event type with no versions (Go appends to a nil
    /// slice).
    pub spec_versions: Option<Vec<DeveloperSpecVersionSummary>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_prior_version: Option<String>,
    pub has_breaking: bool,
    pub unchanged: bool,
}

// -- Helpers ------------------------------------------------------------------

/// The applications the caller may browse (module docs, decision #37).
///
/// A non-anchor caller holding the view or manage permission is an
/// application-scoped developer, confined to its application access plus the
/// `platform` application. Everyone else answers to Go's
/// `CanReadDeveloperPortal` (anchor scope, then `…:view`) and, when admitted,
/// sees every application; a caller Go refuses is refused with Go's answer.
async fn portal_reach(
    state: &BffDeveloperState,
    auth: &AuthContext,
) -> Result<ApplicationScope, PlatformError> {
    use crate::shared::authorization_service::checks;
    if !auth.is_anchor() && checks::can_read_application_openapi(auth).is_ok() {
        let binding = state
            .principal_repo
            .find_application_binding(&auth.principal_id)
            .await?;
        let mut scope = ApplicationScope::from_binding(binding);
        if let ApplicationScope::Only(ids) = &mut scope {
            ids.insert(state.platform_application_id.clone());
        }
        return Ok(scope);
    }
    checks::can_read_developer_portal(auth)?;
    Ok(ApplicationScope::All)
}

/// [`portal_reach`], then 404 for an application outside it (the owner
/// ruling: an out-of-scope application answers as a missing one).
async fn require_app_reach(
    state: &BffDeveloperState,
    auth: &AuthContext,
    app_id: &str,
) -> Result<(), PlatformError> {
    if portal_reach(state, auth).await?.allows(app_id) {
        Ok(())
    } else {
        Err(PlatformError::not_found("Application", app_id))
    }
}

fn summary(
    app: crate::application::entity::Application,
    current: Option<crate::application_openapi_spec::repository::CurrentSpecRef>,
) -> DeveloperApplicationSummary {
    DeveloperApplicationSummary {
        id: app.id,
        code: app.code,
        name: app.name,
        description: app.description,
        icon_url: app.icon_url,
        current_version: current.as_ref().map(|s| s.version.clone()),
        current_spec_id: current.as_ref().map(|s| s.id.clone()),
        current_synced_at: current.as_ref().map(|s| s.synced_at),
    }
}

fn spec_response(
    spec: crate::application_openapi_spec::entity::OpenApiSpec,
) -> OpenApiSpecResponse {
    OpenApiSpecResponse {
        id: spec.id,
        application_id: spec.application_id,
        version: spec.version,
        status: spec.status.as_str().to_string(),
        spec: spec.spec,
        change_notes_text: spec.change_notes_text,
        change_notes: spec.change_notes,
        synced_at: spec.synced_at,
    }
}

// -- Handlers -----------------------------------------------------------------

/// Every active application the caller can browse, ordered by code, each
/// with a snapshot of its CURRENT OpenAPI version if any (Go
/// `listApplications`).
pub async fn list_applications(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
) -> Result<Json<DeveloperApplicationsResponse>, PlatformError> {
    let reach = portal_reach(&state, &auth.0).await?;

    let mut apps = state.application_repo.find_active().await?;
    apps.retain(|a| reach.allows(&a.id));
    apps.sort_by(|a, b| a.code.cmp(&b.code));
    let ids: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
    let mut current = state
        .openapi_spec_repo
        .find_current_refs_by_applications(&ids)
        .await?;
    let items = apps
        .into_iter()
        .map(|app| {
            let spec = current.remove(&app.id);
            summary(app, spec)
        })
        .collect();
    Ok(Json(DeveloperApplicationsResponse { items }))
}

pub async fn get_application(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
    Path(app_id): Path<String>,
) -> Result<Json<DeveloperApplicationSummary>, PlatformError> {
    require_app_reach(&state, &auth.0, &app_id).await?;

    let app = state
        .application_repo
        .find_by_id(&app_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &app_id))?;
    let current = state
        .openapi_spec_repo
        .find_current_refs_by_applications(std::slice::from_ref(&app.id))
        .await?
        .remove(&app.id);
    Ok(Json(summary(app, current)))
}

pub async fn get_current_openapi(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
    Path(app_id): Path<String>,
) -> Result<Json<OpenApiSpecResponse>, PlatformError> {
    require_app_reach(&state, &auth.0, &app_id).await?;

    let spec = state
        .openapi_spec_repo
        .find_current_by_application(&app_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("OpenApiSpec", &app_id))?;
    Ok(Json(spec_response(spec)))
}

pub async fn list_versions(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
    Path(app_id): Path<String>,
) -> Result<Json<OpenApiVersionsResponse>, PlatformError> {
    require_app_reach(&state, &auth.0, &app_id).await?;

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
    require_app_reach(&state, &auth.0, &app_id).await?;

    let spec = state
        .openapi_spec_repo
        .find_by_id(&spec_id)
        .await?
        .filter(|s| s.application_id == app_id)
        .ok_or_else(|| PlatformError::not_found("OpenApiSpec", &spec_id))?;
    Ok(Json(spec_response(spec)))
}

pub async fn list_event_types(
    State(state): State<BffDeveloperState>,
    auth: Authenticated,
    Path(app_id): Path<String>,
) -> Result<Json<DeveloperEventTypesResponse>, PlatformError> {
    require_app_reach(&state, &auth.0, &app_id).await?;

    let app = state
        .application_repo
        .find_by_id(&app_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &app_id))?;
    let mut event_types = state.event_type_repo.find_by_application(&app.code).await?;
    event_types.sort_by(|a, b| a.code.cmp(&b.code));
    let items = event_types
        .into_iter()
        .map(|et| {
            let spec_versions: Vec<DeveloperSpecVersionSummary> = et
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
                        .as_ref()
                        .map(crate::shared::jsonb_text::jsonb_text),
                })
                .collect();
            DeveloperEventTypeSummary {
                id: et.id,
                code: et.code,
                name: et.name,
                description: et.description,
                status: et.status.as_str().to_string(),
                application: et.application,
                subdomain: et.subdomain,
                aggregate: et.aggregate,
                event_name: et.event_name,
                spec_versions: Some(spec_versions).filter(|v| !v.is_empty()),
            }
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
    crate::shared::authorization_service::checks::can_sync_platform_openapi(&auth.0)?;

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
