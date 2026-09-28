//! Developer credential routes, nested under `/api/principals`
//! (`/developer-users`, `/{id}/developer-credential`).

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::DeveloperCredentialsState;
use super::operations::{RevokeDeveloperCredentialUseCase, SetDeveloperCredentialUseCase};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new().nest(
            "/api/principals",
            developer_credentials_router(developer_credentials_state(ctx)),
        ),
        plain: Router::new(),
    }
}

pub fn developer_credentials_state(ctx: &PlatformContext) -> DeveloperCredentialsState {
    DeveloperCredentialsState {
        principal_repo: ctx.repos.principal_repo.clone(),
        set_use_case: Arc::new(SetDeveloperCredentialUseCase::new(
            ctx.repos.principal_repo.clone(),
            ctx.unit_of_work.clone(),
        )),
        revoke_use_case: Arc::new(RevokeDeveloperCredentialUseCase::new(
            ctx.repos.principal_repo.clone(),
            ctx.unit_of_work.clone(),
        )),
        encryption: ctx.encryption.clone(),
    }
}

/// Nested at `/api/principals` beside the principal routes.
pub fn developer_credentials_router(state: DeveloperCredentialsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::developer_credential::api::list_developer_users
        ))
        .routes(routes!(
            crate::developer_credential::api::set_developer_credential,
            crate::developer_credential::api::revoke_developer_credential
        ))
        .with_state(state)
}
