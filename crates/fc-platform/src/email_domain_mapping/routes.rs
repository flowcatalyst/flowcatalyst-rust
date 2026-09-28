//! Email domain mapping routes: `/api/email-domain-mappings` (plain), and
//! Go's lookup and provider-move routes at their full paths
//! (`edm_lookup_router`).

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::{EdmLookupState, EmailDomainMappingsState};
use super::operations::move_provider::MoveMappingToProviderUseCase;
use super::operations::{
    CreateEmailDomainMappingUseCase, DeleteEmailDomainMappingUseCase,
    UpdateEmailDomainMappingUseCase,
};
use super::provider_move_repository::ProviderMoveRepository;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new().merge(edm_lookup_router(edm_lookup_state(ctx))),
        plain: Router::new().nest(
            "/api/email-domain-mappings",
            email_domain_mappings_router(email_domain_mappings_state(ctx)).into(),
        ),
    }
}

pub fn email_domain_mappings_state(ctx: &PlatformContext) -> EmailDomainMappingsState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    EmailDomainMappingsState {
        edm_repo: repos.edm_repo.clone(),
        idp_repo: repos.idp_repo.clone(),
        role_repo: repos.role_repo.clone(),
        create_use_case: Arc::new(CreateEmailDomainMappingUseCase::new(
            repos.edm_repo.clone(),
            repos.idp_repo.clone(),
            uow.clone(),
        )),
        update_use_case: Arc::new(UpdateEmailDomainMappingUseCase::new(
            repos.edm_repo.clone(),
            repos.idp_repo.clone(),
            uow.clone(),
        )),
        delete_use_case: Arc::new(DeleteEmailDomainMappingUseCase::new(
            repos.edm_repo.clone(),
            uow.clone(),
        )),
    }
}

pub fn edm_lookup_state(ctx: &PlatformContext) -> EdmLookupState {
    let repos = &ctx.repos;
    EdmLookupState {
        edm_repo: repos.edm_repo.clone(),
        idp_repo: repos.idp_repo.clone(),
        move_use_case: Arc::new(MoveMappingToProviderUseCase::new(
            repos.edm_repo.clone(),
            repos.idp_repo.clone(),
            repos.principal_repo.clone(),
            Arc::new(ProviderMoveRepository::new(
                &repos.pool,
                repos.principal_repo.clone(),
            )),
            ctx.unit_of_work.clone(),
        )),
    }
}

pub fn email_domain_mappings_router(state: EmailDomainMappingsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::email_domain_mapping::api::create_email_domain_mapping,
            crate::email_domain_mapping::api::list_email_domain_mappings
        ))
        .routes(routes!(
            crate::email_domain_mapping::api::lookup_email_domain_mapping
        ))
        .routes(routes!(
            crate::email_domain_mapping::api::get_email_domain_mapping,
            crate::email_domain_mapping::api::update_email_domain_mapping,
            crate::email_domain_mapping::api::delete_email_domain_mapping
        ))
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn edm_lookup_router(state: EdmLookupState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::email_domain_mapping::api::lookup_email_domain_mapping_by_query
        ))
        .routes(routes!(
            crate::email_domain_mapping::api::get_email_domain_mapping_by_domain
        ))
        .routes(routes!(
            crate::email_domain_mapping::api::move_email_domain_mapping_provider
        ))
        .with_state(state)
}
