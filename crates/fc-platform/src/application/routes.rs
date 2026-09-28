//! Application routes: `/api/applications` (plain), and Go's
//! service-account attach and client-config read at their full paths
//! (`go_api`).

use std::sync::Arc;

use axum::routing::{get, post, put};
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::ApplicationsState;
use super::go_api::ApplicationGoState;
use super::operations::{
    ActivateApplicationUseCase, AttachServiceAccountToApplicationUseCase, CreateApplicationUseCase,
    DeactivateApplicationUseCase, DisableApplicationForClientUseCase,
    EnableApplicationForClientUseCase, UpdateApplicationClientConfigUseCase,
    UpdateApplicationUseCase,
};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use crate::usecase::{PgUnitOfWork, UnitOfWork};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new().merge(application_go_router(ApplicationGoState {
            principal_repo: ctx.repos.principal_repo.clone(),
            client_config_repo: ctx.repos.application_client_config_repo.clone(),
            attach_use_case: Arc::new(AttachServiceAccountToApplicationUseCase::new(
                ctx.repos.application_repo.clone(),
                ctx.unit_of_work.clone(),
            )),
        })),
        plain: Router::new().nest(
            "/api/applications",
            applications_router(applications_state(ctx)),
        ),
    }
}

pub fn applications_state(ctx: &PlatformContext) -> ApplicationsState<PgUnitOfWork> {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    ApplicationsState {
        application_repo: repos.application_repo.clone(),
        service_account_repo: repos.service_account_repo.clone(),
        role_repo: repos.role_repo.clone(),
        client_config_repo: repos.application_client_config_repo.clone(),
        client_repo: repos.client_repo.clone(),
        create_use_case: Arc::new(CreateApplicationUseCase::new(
            repos.application_repo.clone(),
            uow.clone(),
        )),
        update_use_case: Arc::new(UpdateApplicationUseCase::new(
            repos.application_repo.clone(),
            uow.clone(),
        )),
        activate_use_case: Arc::new(ActivateApplicationUseCase::new(
            repos.application_repo.clone(),
            uow.clone(),
        )),
        deactivate_use_case: Arc::new(DeactivateApplicationUseCase::new(
            repos.application_repo.clone(),
            uow.clone(),
        )),
        enable_for_client_use_case: Arc::new(EnableApplicationForClientUseCase::new(
            repos.application_repo.clone(),
            repos.client_repo.clone(),
            repos.application_client_config_repo.clone(),
            uow.clone(),
        )),
        disable_for_client_use_case: Arc::new(DisableApplicationForClientUseCase::new(
            repos.application_client_config_repo.clone(),
            uow.clone(),
        )),
        update_client_config_use_case: Arc::new(UpdateApplicationClientConfigUseCase::new(
            repos.application_repo.clone(),
            repos.client_repo.clone(),
            repos.application_client_config_repo.clone(),
            uow.clone(),
        )),
        oauth_client_repo: repos.oauth_client_repo.clone(),
        create_oauth_client_use_case: Arc::new(
            crate::auth::operations::CreateOAuthClientUseCase::new(
                repos.oauth_client_repo.clone(),
                uow.clone(),
            ),
        ),
        pg_unit_of_work: uow.clone(),
    }
}

/// Create applications router
pub fn applications_router<U: UnitOfWork + Clone>(state: ApplicationsState<U>) -> Router {
    Router::new()
        .route(
            "/",
            post(crate::application::api::create_application::<U>)
                .get(crate::application::api::list_applications::<U>),
        )
        .route(
            "/{id}",
            get(crate::application::api::get_application::<U>)
                .put(crate::application::api::update_application::<U>)
                .delete(crate::application::api::delete_application::<U>),
        )
        .route(
            "/{id}/activate",
            post(crate::application::api::activate_application::<U>),
        )
        .route(
            "/{id}/deactivate",
            post(crate::application::api::deactivate_application::<U>),
        )
        .route(
            "/{id}/provision-service-account",
            post(crate::application::api::provision_service_account::<U>),
        )
        .route(
            "/{id}/provision-login-client",
            post(crate::application::api::provision_login_client::<U>),
        )
        .route(
            "/{id}/service-account",
            get(crate::application::api::get_application_service_account::<U>),
        )
        .route(
            "/by-id/{id}/roles",
            get(crate::application::api::list_application_roles::<U>),
        )
        .route(
            "/{id}/clients",
            get(crate::application::api::list_client_configs::<U>),
        )
        .route(
            "/{id}/clients/{clientId}",
            put(crate::application::api::update_client_config::<U>),
        )
        .route(
            "/{id}/clients/{clientId}/enable",
            post(crate::application::api::enable_for_client::<U>),
        )
        .route(
            "/{id}/clients/{clientId}/disable",
            post(crate::application::api::disable_for_client::<U>),
        )
        .route(
            "/by-code/{code}",
            get(crate::application::api::get_application_by_code::<U>),
        )
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn application_go_router(state: ApplicationGoState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::application::go_api::attach_application_service_account
        ))
        .routes(routes!(
            crate::application::go_api::get_application_client_config
        ))
        .with_state(state)
}
