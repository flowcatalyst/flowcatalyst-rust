//! Scheduled job routes: `/api/scheduled-jobs` (definitions and the SDK's
//! instance callbacks) and the read-only `/bff/scheduled-jobs` (plain).

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::ScheduledJobsState;
use super::operations::{
    ArchiveScheduledJobUseCase, CreateScheduledJobUseCase, DeleteScheduledJobUseCase,
    FireScheduledJobUseCase, PauseScheduledJobUseCase, ResumeScheduledJobUseCase,
    UpdateScheduledJobUseCase,
};
use crate::shared::bff_scheduled_jobs_api::BffScheduledJobsState;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new().nest(
            "/api/scheduled-jobs",
            scheduled_jobs_router(scheduled_jobs_state(ctx)),
        ),
        plain: Router::new().nest(
            "/bff/scheduled-jobs",
            bff_scheduled_jobs_router(BffScheduledJobsState {
                repo: ctx.repos.scheduled_job_repo.clone(),
                instance_repo: ctx.repos.scheduled_job_instance_repo.clone(),
                client_repo: ctx.repos.client_repo.clone(),
                application_repo: ctx.repos.application_repo.clone(),
            }),
        ),
    }
}

pub fn scheduled_jobs_state(ctx: &PlatformContext) -> ScheduledJobsState {
    let repo = &ctx.repos.scheduled_job_repo;
    let uow = &ctx.unit_of_work;
    ScheduledJobsState {
        repo: repo.clone(),
        instance_repo: ctx.repos.scheduled_job_instance_repo.clone(),
        create_use_case: Arc::new(CreateScheduledJobUseCase::new(repo.clone(), uow.clone())),
        update_use_case: Arc::new(UpdateScheduledJobUseCase::new(repo.clone(), uow.clone())),
        pause_use_case: Arc::new(PauseScheduledJobUseCase::new(repo.clone(), uow.clone())),
        resume_use_case: Arc::new(ResumeScheduledJobUseCase::new(repo.clone(), uow.clone())),
        archive_use_case: Arc::new(ArchiveScheduledJobUseCase::new(repo.clone(), uow.clone())),
        delete_use_case: Arc::new(DeleteScheduledJobUseCase::new(repo.clone(), uow.clone())),
        fire_use_case: Arc::new(FireScheduledJobUseCase::new(
            repo.clone(),
            ctx.repos.scheduled_job_instance_repo.clone(),
            uow.clone(),
        )),
    }
}

pub fn scheduled_jobs_router(state: ScheduledJobsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::scheduled_job::api::create_scheduled_job,
            crate::scheduled_job::api::list_scheduled_jobs
        ))
        .routes(routes!(
            crate::scheduled_job::api::get_scheduled_job,
            crate::scheduled_job::api::update_scheduled_job,
            crate::scheduled_job::api::delete_scheduled_job
        ))
        .routes(routes!(
            crate::scheduled_job::api::get_scheduled_job_by_code
        ))
        .routes(routes!(crate::scheduled_job::api::pause_scheduled_job))
        .routes(routes!(crate::scheduled_job::api::resume_scheduled_job))
        .routes(routes!(crate::scheduled_job::api::archive_scheduled_job))
        .routes(routes!(crate::scheduled_job::api::fire_scheduled_job))
        .routes(routes!(crate::scheduled_job::api::list_instances_for_job))
        .routes(routes!(crate::scheduled_job::api::get_instance))
        .routes(routes!(crate::scheduled_job::api::list_instance_logs))
        .routes(routes!(crate::scheduled_job::api::post_instance_log))
        .routes(routes!(crate::scheduled_job::api::post_instance_complete))
        .with_state(state)
}

pub fn bff_scheduled_jobs_router(state: BffScheduledJobsState) -> Router {
    Router::new()
        .route("/", get(crate::shared::bff_scheduled_jobs_api::list_jobs))
        .route(
            "/filter-options",
            get(crate::shared::bff_scheduled_jobs_api::filter_options),
        )
        .route("/{id}", get(crate::shared::bff_scheduled_jobs_api::get_job))
        .route(
            "/{id}/instances",
            get(crate::shared::bff_scheduled_jobs_api::list_instances),
        )
        .route(
            "/instances/{instanceId}",
            get(crate::shared::bff_scheduled_jobs_api::get_instance),
        )
        .route(
            "/instances/{instanceId}/logs",
            get(crate::shared::bff_scheduled_jobs_api::list_instance_logs),
        )
        .with_state(state)
}
