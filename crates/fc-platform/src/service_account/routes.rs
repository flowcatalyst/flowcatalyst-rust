//! Service account routes: `/api/service-accounts` (plain), and Go's
//! deactivate and token-mint routes at their full paths (`admin_api`).

use std::sync::Arc;

use axum::routing::{get, post, put};
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::admin_api::ServiceAccountAdminState;
use super::api::ServiceAccountsState;
use super::operations::mint_token::RecordServiceAccountTokenMintUseCase;
use super::operations::{
    AssignRolesUseCase, CreateServiceAccountUseCase, DeactivateServiceAccountUseCase,
    DeleteServiceAccountUseCase, RegenerateAuthTokenUseCase, RegenerateSigningSecretUseCase,
    UpdateServiceAccountUseCase,
};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use crate::usecase::{PgUnitOfWork, UnitOfWork};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new().merge(service_account_admin_router(
            service_account_admin_state(ctx),
        )),
        plain: Router::new().nest(
            "/api/service-accounts",
            service_accounts_router(service_accounts_state(ctx)),
        ),
    }
}

pub fn service_accounts_state(ctx: &PlatformContext) -> ServiceAccountsState<PgUnitOfWork> {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    let encryption = &ctx.encryption;
    ServiceAccountsState {
        repo: repos.service_account_repo.clone(),
        role_repo: repos.role_repo.clone(),
        create_use_case: Arc::new(CreateServiceAccountUseCase::new(
            repos.service_account_repo.clone(),
            repos.client_repo.clone(),
            uow.clone(),
            encryption.clone(),
        )),
        update_use_case: Arc::new(UpdateServiceAccountUseCase::new(
            repos.service_account_repo.clone(),
            repos.client_repo.clone(),
            uow.clone(),
            encryption.clone(),
        )),
        delete_use_case: Arc::new(DeleteServiceAccountUseCase::new(
            repos.service_account_repo.clone(),
            uow.clone(),
        )),
        assign_roles_use_case: Arc::new(AssignRolesUseCase::new(
            repos.service_account_repo.clone(),
            uow.clone(),
        )),
        regenerate_token_use_case: Arc::new(RegenerateAuthTokenUseCase::new(
            repos.service_account_repo.clone(),
            uow.clone(),
            encryption.clone(),
        )),
        regenerate_secret_use_case: Arc::new(RegenerateSigningSecretUseCase::new(
            repos.service_account_repo.clone(),
            uow.clone(),
            encryption.clone(),
        )),
        create_oauth_client_use_case: Arc::new(
            crate::auth::operations::CreateOAuthClientUseCase::new(
                repos.oauth_client_repo.clone(),
                uow.clone(),
            ),
        ),
        oauth_client_repo: repos.oauth_client_repo.clone(),
        app_access: ctx.app_access.clone(),
    }
}

pub fn service_account_admin_state(ctx: &PlatformContext) -> ServiceAccountAdminState {
    let repos = &ctx.repos;
    ServiceAccountAdminState {
        repo: repos.service_account_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        auth_service: ctx.auth.auth.clone(),
        deactivate_use_case: Arc::new(DeactivateServiceAccountUseCase::new(
            repos.service_account_repo.clone(),
            ctx.unit_of_work.clone(),
        )),
        record_mint_use_case: Arc::new(RecordServiceAccountTokenMintUseCase::new(
            ctx.unit_of_work.clone(),
        )),
    }
}

/// Create the service accounts router
pub fn service_accounts_router<U: UnitOfWork + Clone>(state: ServiceAccountsState<U>) -> Router {
    Router::new()
        .route(
            "/",
            get(crate::service_account::api::list_service_accounts::<U>)
                .post(crate::service_account::api::create_service_account::<U>),
        )
        .route(
            "/{id}",
            get(crate::service_account::api::get_service_account::<U>)
                .put(crate::service_account::api::update_service_account::<U>)
                .delete(crate::service_account::api::delete_service_account::<U>),
        )
        .route(
            "/code/{code}",
            get(crate::service_account::api::get_service_account_by_code::<U>),
        )
        .route(
            "/{id}/auth-token",
            put(crate::service_account::api::update_auth_token::<U>),
        )
        .route(
            "/{id}/regenerate-auth-token",
            post(crate::service_account::api::regenerate_auth_token::<U>),
        )
        .route(
            "/{id}/regenerate-signing-secret",
            post(crate::service_account::api::regenerate_signing_secret::<U>),
        )
        // Go's shorter spellings of the two (serviceaccount/api/api.go:71-82).
        .route(
            "/{id}/regenerate-token",
            post(crate::service_account::api::regenerate_auth_token::<U>),
        )
        .route(
            "/{id}/regenerate-secret",
            post(crate::service_account::api::regenerate_signing_secret::<U>),
        )
        .route(
            "/{id}/roles",
            get(crate::service_account::api::get_roles::<U>)
                .put(crate::service_account::api::assign_roles::<U>),
        )
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn service_account_admin_router(state: ServiceAccountAdminState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::service_account::admin_api::deactivate_service_account
        ))
        .routes(routes!(
            crate::service_account::admin_api::mint_service_account_token
        ))
        .with_state(state)
}
