//! Connection routes: `/api/connections` (plain: not in the OpenAPI
//! document).

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::ConnectionsState;
use super::operations::{
    CreateConnectionUseCase, DeleteConnectionUseCase, UpdateConnectionUseCase,
};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new(),
        plain: Router::new().nest(
            "/api/connections",
            connections_router(connections_state(ctx)).into(),
        ),
    }
}

pub fn connections_state(ctx: &PlatformContext) -> ConnectionsState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    ConnectionsState {
        connection_repo: repos.connection_repo.clone(),
        app_access: ctx.app_access.clone(),
        create_use_case: Arc::new(CreateConnectionUseCase::new(
            repos.connection_repo.clone(),
            repos.service_account_repo.clone(),
            uow.clone(),
        )),
        update_use_case: Arc::new(UpdateConnectionUseCase::new(
            repos.connection_repo.clone(),
            uow.clone(),
        )),
        delete_use_case: Arc::new(DeleteConnectionUseCase::new(
            repos.connection_repo.clone(),
            repos.subscription_repo.clone(),
            uow.clone(),
        )),
    }
}

/// Create connections router
pub fn connections_router(state: ConnectionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::connection::api::create_connection,
            crate::connection::api::list_connections
        ))
        .routes(routes!(
            crate::connection::api::get_connection,
            crate::connection::api::update_connection,
            crate::connection::api::delete_connection
        ))
        .routes(routes!(crate::connection::api::pause_connection))
        .routes(routes!(crate::connection::api::activate_connection))
        .with_state(state)
}
