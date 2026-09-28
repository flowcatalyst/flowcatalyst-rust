//! Dispatch pool routes: `/api/dispatch-pools` (plain).

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use utoipa_axum::router::OpenApiRouter;

use super::api::DispatchPoolsState;
use super::operations::{
    ActivateDispatchPoolUseCase, ArchiveDispatchPoolUseCase, CreateDispatchPoolUseCase,
    DeleteDispatchPoolUseCase, SuspendDispatchPoolUseCase, UpdateDispatchPoolUseCase,
};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use crate::usecase::{PgUnitOfWork, UnitOfWork};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new(),
        plain: Router::new().nest(
            "/api/dispatch-pools",
            dispatch_pools_router(dispatch_pools_state(ctx)),
        ),
    }
}

pub fn dispatch_pools_state(ctx: &PlatformContext) -> DispatchPoolsState<PgUnitOfWork> {
    let repo = &ctx.repos.dispatch_pool_repo;
    let uow = &ctx.unit_of_work;
    DispatchPoolsState {
        dispatch_pool_repo: repo.clone(),
        create_use_case: Arc::new(CreateDispatchPoolUseCase::new(repo.clone(), uow.clone())),
        update_use_case: Arc::new(UpdateDispatchPoolUseCase::new(repo.clone(), uow.clone())),
        archive_use_case: Arc::new(ArchiveDispatchPoolUseCase::new(repo.clone(), uow.clone())),
        delete_use_case: Arc::new(DeleteDispatchPoolUseCase::new(repo.clone(), uow.clone())),
        suspend_use_case: Arc::new(SuspendDispatchPoolUseCase::new(repo.clone(), uow.clone())),
        activate_use_case: Arc::new(ActivateDispatchPoolUseCase::new(repo.clone(), uow.clone())),
    }
}

/// Create dispatch pools router
pub fn dispatch_pools_router<U: UnitOfWork + Clone>(state: DispatchPoolsState<U>) -> Router {
    Router::new()
        .route(
            "/",
            post(crate::dispatch_pool::api::create_dispatch_pool::<U>)
                .get(crate::dispatch_pool::api::list_dispatch_pools::<U>),
        )
        .route(
            "/{id}",
            get(crate::dispatch_pool::api::get_dispatch_pool::<U>)
                .put(crate::dispatch_pool::api::update_dispatch_pool::<U>)
                .delete(crate::dispatch_pool::api::delete_dispatch_pool::<U>),
        )
        .route(
            "/{id}/archive",
            post(crate::dispatch_pool::api::archive_dispatch_pool::<U>),
        )
        .route(
            "/{id}/suspend",
            post(crate::dispatch_pool::api::suspend_dispatch_pool::<U>),
        )
        .route(
            "/{id}/activate",
            post(crate::dispatch_pool::api::activate_dispatch_pool::<U>),
        )
        .with_state(state)
}
