//! Process documentation routes: `/api/processes` and `/bff/processes`
//! (the same handlers on both tiers).

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::ProcessesState;
use super::operations::{
    ArchiveProcessUseCase, CreateProcessUseCase, DeleteProcessUseCase, UpdateProcessUseCase,
};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let state = processes_state(ctx);
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .nest("/api/processes", processes_router(state.clone()))
            .nest("/bff/processes", processes_router(state)),
        plain: Router::new(),
    }
}

pub fn processes_state(ctx: &PlatformContext) -> ProcessesState {
    let repo = &ctx.repos.process_repo;
    let uow = &ctx.unit_of_work;
    ProcessesState {
        process_repo: repo.clone(),
        create_use_case: Arc::new(CreateProcessUseCase::new(repo.clone(), uow.clone())),
        update_use_case: Arc::new(UpdateProcessUseCase::new(repo.clone(), uow.clone())),
        archive_use_case: Arc::new(ArchiveProcessUseCase::new(repo.clone(), uow.clone())),
        delete_use_case: Arc::new(DeleteProcessUseCase::new(repo.clone(), uow.clone())),
    }
}

pub fn processes_router(state: ProcessesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::process::api::create_process,
            crate::process::api::list_processes
        ))
        .routes(routes!(
            crate::process::api::get_process,
            crate::process::api::update_process,
            crate::process::api::delete_process
        ))
        .routes(routes!(crate::process::api::get_process_by_code))
        .routes(routes!(crate::process::api::archive_process))
        .with_state(state)
}
