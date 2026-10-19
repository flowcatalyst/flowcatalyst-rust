//! Application users and invitations (Go F20): the six routes under
//! `/api/applications/{appCode}/users` and `/invitations`, which an
//! application's own admin pages call as its service account. The model and
//! handlers are fc-platform-iam's; this is the wiring.

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use std::sync::Arc;

use utoipa_axum::{router::OpenApiRouter, routes};

use super::{AppUsersRepository, AppUsersState};
use crate::principal::operations::{DeactivateUserUseCase, InviteApplicationUserUseCase};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .nest("/api/applications", app_users_router(app_users_state(ctx))),
        plain: axum::Router::new(),
    }
}

pub fn app_users_state(ctx: &PlatformContext) -> AppUsersState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    AppUsersState {
        repo: Arc::new(AppUsersRepository::new(&repos.pool)),
        principal_repo: repos.principal_repo.clone(),
        app_access: ctx.app_access.clone(),
        client_repo: repos.client_repo.clone(),
        client_config_repo: repos.application_client_config_repo.clone(),
        email_domain_mapping_repo: repos.edm_repo.clone(),
        identity_provider_repo: repos.idp_repo.clone(),
        emailer: ctx.password_reset_emailer.clone(),
        invite_use_case: Arc::new(InviteApplicationUserUseCase::new(
            repos.principal_repo.clone(),
            repos.role_repo.clone(),
            repos.application_repo.clone(),
            uow.clone(),
        )),
        deactivate_use_case: Arc::new(DeactivateUserUseCase::new(
            repos.principal_repo.clone(),
            uow.clone(),
        )),
    }
}

/// The application-scoped user and invitation routes, nested at
/// `/api/applications`.
pub fn app_users_router(state: AppUsersState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::appusers::api::list_users,
            crate::appusers::api::invite_user
        ))
        .routes(routes!(crate::appusers::api::get_user))
        .routes(routes!(crate::appusers::api::list_invitations))
        .routes(routes!(crate::appusers::api::resend_invitation))
        .routes(routes!(crate::appusers::api::revoke_invitation))
        .with_state(state)
}
