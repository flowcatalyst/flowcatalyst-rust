//! Dispatch job routes: the read routes on `/api/dispatch-jobs` (SDKs) and
//! `/bff/dispatch-jobs` (SPA), and the high-volume ingest
//! `POST /api/dispatch-jobs/batch` (`shared::sdk_dispatch_jobs_api`,
//! platform infrastructure).

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use axum::extract::DefaultBodyLimit;
use axum::routing::post;
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::DispatchJobsState;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use crate::shared::sdk_dispatch_jobs_api::SdkDispatchJobsState;

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let jobs = dispatch_jobs_state(ctx);
    AggregateRoutes {
        // Cursor-paginated read handlers serve both tiers. The API tier
        // leaves out `batch_create_dispatch_jobs` so it doesn't collide with
        // the bulk-insert `POST /api/dispatch-jobs/batch` below.
        documented: OpenApiRouter::new()
            .nest("/api/dispatch-jobs", dispatch_jobs_api_router(jobs.clone()))
            .nest("/bff/dispatch-jobs", dispatch_jobs_router(jobs)),
        plain: Router::new().nest(
            "/api/dispatch-jobs",
            sdk_dispatch_jobs_batch_router(SdkDispatchJobsState {
                dispatch_job_repo: ctx.repos.dispatch_job_repo.clone(),
                signing: ctx.signing_guard.clone(),
            }),
        ),
    }
}

pub fn dispatch_jobs_state(ctx: &PlatformContext) -> DispatchJobsState {
    DispatchJobsState {
        dispatch_job_repo: ctx.repos.dispatch_job_repo.clone(),
        client_repo: ctx.repos.client_repo.clone(),
        signing: ctx.signing_guard.clone(),
    }
}

/// Create dispatch jobs router for the BFF tier (`/bff/dispatch-jobs`).
/// Cookie-auth, used by the SPA. Includes `batch_create_dispatch_jobs` —
/// the SPA-facing batch.
pub fn dispatch_jobs_router(state: DispatchJobsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::dispatch_job::api::list_dispatch_jobs,
            crate::dispatch_job::api::create_dispatch_job
        ))
        .routes(routes!(
            crate::dispatch_job::api::batch_create_dispatch_jobs
        ))
        .routes(routes!(crate::dispatch_job::api::get_filter_options))
        .routes(routes!(crate::dispatch_job::api::list_dispatch_jobs_raw))
        .routes(routes!(crate::dispatch_job::api::get_dispatch_job))
        .routes(routes!(crate::dispatch_job::api::get_dispatch_job_raw))
        .routes(routes!(crate::dispatch_job::api::get_dispatch_job_attempts))
        .routes(routes!(crate::dispatch_job::api::get_jobs_for_event))
        .with_state(state)
}

/// Create dispatch jobs router for the API tier (`/api/dispatch-jobs`).
/// Bearer-auth, used by SDK consumers. **No `batch_create_dispatch_jobs`**
/// — SDK callers use `sdk_dispatch_jobs_batch_router::POST /batch` (the
/// high-volume bulk-insert path). The two routers must not both register
/// `POST /batch` at the same prefix (axum panics on overlap).
pub fn dispatch_jobs_api_router(state: DispatchJobsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::dispatch_job::api::list_dispatch_jobs,
            crate::dispatch_job::api::create_dispatch_job
        ))
        .routes(routes!(crate::dispatch_job::api::get_filter_options))
        .routes(routes!(crate::dispatch_job::api::list_dispatch_jobs_raw))
        .routes(routes!(crate::dispatch_job::api::get_dispatch_job))
        .routes(routes!(crate::dispatch_job::api::get_dispatch_job_raw))
        .routes(routes!(crate::dispatch_job::api::get_dispatch_job_attempts))
        .routes(routes!(crate::dispatch_job::api::get_jobs_for_event))
        .with_state(state)
}

pub fn sdk_dispatch_jobs_batch_router(state: SdkDispatchJobsState) -> Router {
    Router::new()
        .route(
            "/batch",
            post(crate::shared::sdk_dispatch_jobs_api::sdk_batch_create_dispatch_jobs),
        )
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}
