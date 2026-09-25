//! Routes Go serves that the Rust platform lacked (parity run 1,
//! `docs/parity/api-run-1.md` root cause 4). Each group's handlers live in
//! their domain module; this file only builds their states and merges their
//! full-path routers, so `router.rs` and `platform_routes.rs` each carry a
//! single line for all of them.

use std::sync::Arc;
use utoipa_axum::router::OpenApiRouter;

use crate::repository::Repositories;
use crate::shared::server_setup::AuthServices;
use crate::usecase::PgUnitOfWork;

/// Every state the Go-parity routes need.
#[derive(Clone)]
pub struct GoRoutesState {
    pub role_permissions: crate::role::permission_api::RolePermissionsState,
    pub service_account_admin: crate::service_account::admin_api::ServiceAccountAdminState,
    pub client_search: crate::client::search_api::ClientSearchState,
}

impl GoRoutesState {
    pub fn build(repos: &Repositories, auth: &AuthServices, uow: &Arc<PgUnitOfWork>) -> Self {
        Self {
            client_search: crate::client::search_api::ClientSearchState {
                client_repo: repos.client_repo.clone(),
            },
            service_account_admin: crate::service_account::admin_api::ServiceAccountAdminState {
                repo: repos.service_account_repo.clone(),
                principal_repo: repos.principal_repo.clone(),
                role_repo: repos.role_repo.clone(),
                auth_service: auth.auth.clone(),
                deactivate_use_case: Arc::new(
                    crate::service_account::operations::DeactivateServiceAccountUseCase::new(
                        repos.service_account_repo.clone(),
                        uow.clone(),
                    ),
                ),
                record_mint_use_case: Arc::new(
                    crate::service_account::operations::mint_token::RecordServiceAccountTokenMintUseCase::new(
                        uow.clone(),
                    ),
                ),
            },
            role_permissions: crate::role::permission_api::RolePermissionsState::new(
                &repos.pool,
                repos.role_repo.clone(),
                uow.clone(),
            ),
        }
    }
}

/// All Go-parity routes, at their full paths.
pub fn go_routes_router(state: GoRoutesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .merge(crate::role::permission_api::role_permissions_router(
            state.role_permissions,
        ))
        .merge(
            crate::service_account::admin_api::service_account_admin_router(
                state.service_account_admin,
            ),
        )
        .merge(crate::client::search_api::client_search_router(
            state.client_search,
        ))
}
