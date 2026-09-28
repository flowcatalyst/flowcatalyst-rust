//! Identity provider routes: `/api/identity-providers` (plain).

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use utoipa_axum::router::OpenApiRouter;

use super::api::IdentityProvidersState;
use super::operations::{
    CreateIdentityProviderUseCase, DeleteIdentityProviderUseCase, DomainDeps,
    UpdateIdentityProviderUseCase,
};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new(),
        plain: Router::new().nest(
            "/api/identity-providers",
            identity_providers_router(identity_providers_state(ctx)),
        ),
    }
}

pub fn identity_providers_state(ctx: &PlatformContext) -> IdentityProvidersState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    let domains = DomainDeps {
        edm_repo: repos.edm_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        move_repo: Arc::new(
            crate::email_domain_mapping::provider_move_repository::ProviderMoveRepository::new(
                &repos.pool,
                repos.principal_repo.clone(),
            ),
        ),
    };
    IdentityProvidersState {
        idp_repo: repos.idp_repo.clone(),
        domains: domains.clone(),
        role_repo: repos.role_repo.clone(),
        pg_unit_of_work: uow.clone(),
        create_use_case: Arc::new(CreateIdentityProviderUseCase::new(
            repos.idp_repo.clone(),
            domains.clone(),
            uow.clone(),
            repos.role_repo.clone(),
        )),
        update_use_case: Arc::new(UpdateIdentityProviderUseCase::new(
            repos.idp_repo.clone(),
            domains,
            uow.clone(),
            repos.role_repo.clone(),
        )),
        delete_use_case: Arc::new(DeleteIdentityProviderUseCase::new(
            repos.idp_repo.clone(),
            repos.edm_repo.clone(),
            uow.clone(),
        )),
        encryption_service: ctx.encryption.clone(),
    }
}

pub fn identity_providers_router(state: IdentityProvidersState) -> Router {
    Router::new()
        .route(
            "/",
            post(crate::identity_provider::api::create_identity_provider)
                .get(crate::identity_provider::api::list_identity_providers),
        )
        .route(
            "/{id}",
            get(crate::identity_provider::api::get_identity_provider)
                .put(crate::identity_provider::api::update_identity_provider)
                .delete(crate::identity_provider::api::delete_identity_provider),
        )
        .with_state(state)
}
