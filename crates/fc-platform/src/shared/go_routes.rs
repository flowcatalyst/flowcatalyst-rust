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
}

impl GoRoutesState {
    pub fn build(repos: &Repositories, _auth: &AuthServices, uow: &Arc<PgUnitOfWork>) -> Self {
        Self {
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
}
