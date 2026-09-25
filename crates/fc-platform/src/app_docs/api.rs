//! Documentation routes (Go `docsapi/api.go:47-49`, `sdksync/api.go:103`):
//!
//! - `GET  /api/docs`                                   → `{platform, applications}`
//! - `GET  /api/docs/platform/{slug}`                   → `{slug, title, content}`
//! - `GET  /api/docs/applications/{appCode}/{slug}`     → `{slug, title, content}`
//! - `POST /api/applications/{appCode}/docs/sync`       → `SyncResultResponse`
//!
//! Reading needs `platform:admin:docs:view`; syncing
//! `platform:application-service:docs:sync` and the application in scope.

use axum::{
    extract::{DefaultBodyLimit, Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::operations::sync::{SyncAppDocsCommand, SyncAppDocsUseCase, SyncDocInput};
use super::platform_docs::{platform_doc, platform_docs};
use super::repository::AppDocsRepository;
use crate::shared::authorization_service::{checks, ApplicationAccessService};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::shared::sdk_sync_api::SyncResultResponse;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use crate::ApplicationRepository;

#[derive(Clone)]
pub struct DocsState {
    pub repo: Arc<AppDocsRepository>,
    pub application_repo: Arc<ApplicationRepository>,
    pub app_access: Arc<ApplicationAccessService>,
    pub sync_use_case: Arc<SyncAppDocsUseCase<PgUnitOfWork>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DocEntry {
    pub slug: String,
    pub title: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDocs {
    pub application_code: String,
    pub application_name: String,
    pub docs: Vec<DocEntry>,
}

/// Go `DocListResponse`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocListResponse {
    pub platform: Vec<DocEntry>,
    pub applications: Vec<ApplicationDocs>,
}

/// Go `DocResponse`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocResponse {
    pub slug: String,
    pub title: String,
    pub content: String,
}

/// Go `syncDocInputRequest`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SyncDocInputRequest {
    pub slug: String,
    pub title: Option<String>,
    pub content: String,
}

/// Go `syncDocsRequest`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SyncDocsRequest {
    pub docs: Vec<SyncDocInputRequest>,
}

fn doc_not_found(slug: &str) -> PlatformError {
    PlatformError::not_found_code("Doc", slug)
}

/// The documentation index (Go `listDocs`).
#[utoipa::path(get, path = "/api/docs", tag = "docs", operation_id = "listDocs",
    responses((status = 200, description = "Platform and application pages", body = DocListResponse)),
    security(("bearer_auth" = [])))]
pub async fn list_docs(
    State(state): State<DocsState>,
    auth: Authenticated,
) -> Result<Json<DocListResponse>, PlatformError> {
    checks::require_permission(&auth.0, crate::permissions::admin::DOCS_READ)?;
    let (summaries, apps) =
        tokio::try_join!(state.repo.summaries(), state.application_repo.find_all())?;
    let apps: HashMap<String, crate::Application> =
        apps.into_iter().map(|a| (a.id.clone(), a)).collect();
    let mut groups: Vec<ApplicationDocs> = Vec::new();
    let mut current: Option<String> = None;
    for s in summaries {
        // An application that no longer exists is left out, as in Go.
        let Some(app) = apps.get(&s.application_id) else {
            continue;
        };
        if current.as_deref() != Some(s.application_id.as_str()) {
            current = Some(s.application_id.clone());
            groups.push(ApplicationDocs {
                application_code: app.code.clone(),
                application_name: app.name.clone(),
                docs: Vec::new(),
            });
        }
        if let Some(g) = groups.last_mut() {
            g.docs.push(DocEntry {
                slug: s.slug,
                title: s.title,
            });
        }
    }
    groups.sort_by(|a, b| a.application_name.cmp(&b.application_name));
    Ok(Json(DocListResponse {
        platform: platform_docs()
            .into_iter()
            .map(|d| DocEntry {
                slug: d.slug.to_string(),
                title: d.title.to_string(),
            })
            .collect(),
        applications: groups,
    }))
}

/// A platform page (Go `getPlatformDoc`).
#[utoipa::path(get, path = "/api/docs/platform/{slug}", tag = "docs",
    operation_id = "getPlatformDoc",
    params(("slug" = String, Path, description = "Page slug")),
    responses((status = 200, description = "The page", body = DocResponse),
               (status = 404, description = "No such page")),
    security(("bearer_auth" = [])))]
pub async fn get_platform_doc(
    State(_state): State<DocsState>,
    auth: Authenticated,
    Path(slug): Path<String>,
) -> Result<Json<DocResponse>, PlatformError> {
    checks::require_permission(&auth.0, crate::permissions::admin::DOCS_READ)?;
    let d = platform_doc(&slug).ok_or_else(|| doc_not_found(&slug))?;
    Ok(Json(DocResponse {
        slug: d.slug.to_string(),
        title: d.title.to_string(),
        content: d.content.to_string(),
    }))
}

/// An application's page (Go `getApplicationDoc`).
#[utoipa::path(get, path = "/api/docs/applications/{appCode}/{slug}", tag = "docs",
    operation_id = "getApplicationDoc",
    params(("appCode" = String, Path, description = "Application code"),
           ("slug" = String, Path, description = "Page slug")),
    responses((status = 200, description = "The page", body = DocResponse),
               (status = 404, description = "No such application or page")),
    security(("bearer_auth" = [])))]
pub async fn get_application_doc(
    State(state): State<DocsState>,
    auth: Authenticated,
    Path((app_code, slug)): Path<(String, String)>,
) -> Result<Json<DocResponse>, PlatformError> {
    checks::require_permission(&auth.0, crate::permissions::admin::DOCS_READ)?;
    let app = state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let d = state
        .repo
        .find(&app.id, &slug)
        .await?
        .ok_or_else(|| doc_not_found(&slug))?;
    Ok(Json(DocResponse {
        slug: d.slug,
        title: d.title,
        content: d.content,
    }))
}

/// Replace an application's pages (Go `syncAppDocs`).
#[utoipa::path(post, path = "/api/applications/{appCode}/docs/sync", tag = "sdk-sync",
    operation_id = "syncAppDocs",
    params(("appCode" = String, Path, description = "Application code")),
    request_body = SyncDocsRequest,
    responses((status = 200, description = "Synced", body = SyncResultResponse),
               (status = 400, description = "Invalid slug, too many or too large"),
               (status = 404, description = "Unknown application")),
    security(("bearer_auth" = [])))]
pub async fn sync_app_docs(
    State(state): State<DocsState>,
    auth: Authenticated,
    Path(app_code): Path<String>,
    Json(req): Json<SyncDocsRequest>,
) -> Result<Json<SyncResultResponse>, PlatformError> {
    checks::require_permission(&auth.0, crate::permissions::application_service::DOCS_SYNC)?;
    let app = state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let slugs = req.docs.iter().map(|d| d.slug.trim().to_string()).collect();
    let event = state
        .sync_use_case
        .run(
            SyncAppDocsCommand {
                application_id: app.id,
                application_code: app.code,
                docs: req
                    .docs
                    .into_iter()
                    .map(|d| SyncDocInput {
                        slug: d.slug,
                        title: d.title,
                        content: d.content,
                    })
                    .collect(),
                slugs,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(Json(SyncResultResponse {
        application_code: event.application_code,
        created: event.created,
        updated: event.updated,
        deleted: event.deleted,
        synced_codes: event.synced_codes,
        password_hash_ignored: Vec::new(),
    }))
}

/// Full-path router; merged at the root. The sync accepts Go's 4 MiB of
/// pages plus JSON overhead.
pub fn docs_router(state: DocsState) -> OpenApiRouter {
    let sync = OpenApiRouter::new()
        .routes(routes!(sync_app_docs))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024));
    OpenApiRouter::new()
        .routes(routes!(list_docs))
        .routes(routes!(get_platform_doc))
        .routes(routes!(get_application_doc))
        .merge(sync)
        .with_state(state)
}
