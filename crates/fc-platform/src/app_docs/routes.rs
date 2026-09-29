//! Application docs routes (Go's docs plane), at their full paths:
//! `/api/docs*` and `POST /api/applications/{appCode}/docs/sync`.

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::DocsState;
use super::operations::sync::SyncAppDocsUseCase;
use super::repository::AppDocsRepository;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let repo = Arc::new(AppDocsRepository::new(&ctx.repos.pool));
    AggregateRoutes {
        documented: OpenApiRouter::new().merge(docs_router(DocsState {
            repo: repo.clone(),
            application_repo: ctx.repos.application_repo.clone(),
            app_access: ctx.app_access.clone(),
            sync_use_case: Arc::new(SyncAppDocsUseCase::new(repo, ctx.unit_of_work.clone())),
        })),
        plain: Router::new(),
    }
}

/// Full-path router; merged at the root. The sync accepts Go's 4 MiB of
/// pages plus JSON overhead.
pub fn docs_router(state: DocsState) -> OpenApiRouter {
    let sync = OpenApiRouter::new()
        .routes(routes!(crate::app_docs::api::sync_app_docs))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024));
    OpenApiRouter::new()
        .routes(routes!(crate::app_docs::api::list_docs))
        .routes(routes!(crate::app_docs::api::get_platform_doc))
        .routes(routes!(crate::app_docs::api::get_application_doc))
        .merge(sync)
        .with_state(state)
}
