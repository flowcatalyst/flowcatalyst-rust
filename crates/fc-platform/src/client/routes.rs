//! Client routes: `/api/clients`, plus Go's `POST /api/clients/search`
//! at its full path.

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::{ClientSearchState, ClientsState};
use super::operations::{
    ActivateClientUseCase, AddClientNoteUseCase, CreateClientUseCase, DeleteClientUseCase,
    SuspendClientUseCase, UpdateClientUseCase,
};
use crate::application::operations::{
    DisableApplicationForClientUseCase, EnableApplicationForClientUseCase,
    UpdateClientApplicationsUseCase,
};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .nest("/api/clients", clients_router(clients_state(ctx)))
            .merge(client_search_router(ClientSearchState {
                client_repo: ctx.repos.client_repo.clone(),
            })),
        plain: Router::new(),
    }
}

pub fn clients_state(ctx: &PlatformContext) -> ClientsState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    ClientsState {
        client_repo: repos.client_repo.clone(),
        application_repo: repos.application_repo.clone(),
        application_client_config_repo: repos.application_client_config_repo.clone(),
        create_use_case: Arc::new(CreateClientUseCase::new(
            repos.client_repo.clone(),
            uow.clone(),
        )),
        update_use_case: Arc::new(UpdateClientUseCase::new(
            repos.client_repo.clone(),
            uow.clone(),
        )),
        delete_use_case: Arc::new(DeleteClientUseCase::new(
            repos.client_repo.clone(),
            uow.clone(),
        )),
        activate_use_case: Arc::new(ActivateClientUseCase::new(
            repos.client_repo.clone(),
            uow.clone(),
        )),
        suspend_use_case: Arc::new(SuspendClientUseCase::new(
            repos.client_repo.clone(),
            uow.clone(),
        )),
        add_note_use_case: Arc::new(AddClientNoteUseCase::new(
            repos.client_repo.clone(),
            uow.clone(),
        )),
        update_applications_use_case: Arc::new(UpdateClientApplicationsUseCase::new(
            repos.application_repo.clone(),
            repos.client_repo.clone(),
            repos.application_client_config_repo.clone(),
            uow.clone(),
        )),
        enable_application_use_case: Arc::new(EnableApplicationForClientUseCase::new(
            repos.application_repo.clone(),
            repos.client_repo.clone(),
            repos.application_client_config_repo.clone(),
            uow.clone(),
        )),
        disable_application_use_case: Arc::new(DisableApplicationForClientUseCase::new(
            repos.application_client_config_repo.clone(),
            uow.clone(),
        )),
    }
}

/// Create clients router
pub fn clients_router(state: ClientsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::client::api::create_client,
            crate::client::api::list_clients
        ))
        .routes(routes!(crate::client::api::search_clients))
        .routes(routes!(crate::client::api::get_client_by_identifier))
        .routes(routes!(
            crate::client::api::get_client,
            crate::client::api::update_client,
            crate::client::api::delete_client
        ))
        .routes(routes!(crate::client::api::activate_client))
        .routes(routes!(crate::client::api::suspend_client))
        .routes(routes!(crate::client::api::deactivate_client))
        .routes(routes!(crate::client::api::add_note))
        .routes(routes!(
            crate::client::api::get_client_applications,
            crate::client::api::update_client_applications
        ))
        .routes(routes!(crate::client::api::enable_application))
        .routes(routes!(crate::client::api::disable_application))
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn client_search_router(state: ClientSearchState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::client::api::search_clients_by_body))
        .with_state(state)
}
