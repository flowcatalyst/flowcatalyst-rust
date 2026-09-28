//! Role routes: `/api/roles`, Go's per-role permission grants and the
//! permission catalogue writes at their full paths, and
//! `/bff/roles` (plain).

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use std::sync::Arc;

use axum::routing::{delete, get};
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::{RolePermissionsState, RolesState};
use super::operations::{CreateRoleUseCase, DeleteRoleUseCase, UpdateRoleUseCase};
use super::permission_repository::PermissionCatalogRepository;
use crate::role::bff::BffRolesState;
use crate::shared::application_roles_sdk_api::ApplicationRolesSdkState;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .nest("/api/roles", roles_router(roles_state(ctx)))
            .merge(role_permissions_router(RolePermissionsState::new(
                &ctx.repos.pool,
                ctx.repos.role_repo.clone(),
                ctx.unit_of_work.clone(),
            ))),
        plain: Router::new()
            .nest("/bff/roles", bff_roles_router(bff_roles_state(ctx)).into())
            // App-scoped role CRUD for SDKs (`shared::application_roles_sdk_api`).
            .nest(
                "/api/applications",
                application_roles_sdk_router(ApplicationRolesSdkState {
                    app_access: ctx.app_access.clone(),
                    role_repo: ctx.repos.role_repo.clone(),
                    create_use_case: Arc::new(CreateRoleUseCase::new(
                        ctx.repos.role_repo.clone(),
                        ctx.unit_of_work.clone(),
                    )),
                    delete_use_case: Arc::new(DeleteRoleUseCase::new(
                        ctx.repos.role_repo.clone(),
                        ctx.unit_of_work.clone(),
                    )),
                }),
            ),
    }
}

pub fn roles_state(ctx: &PlatformContext) -> RolesState {
    let repo = &ctx.repos.role_repo;
    let uow = &ctx.unit_of_work;
    RolesState {
        role_repo: repo.clone(),
        application_repo: ctx.repos.application_repo.clone(),
        create_use_case: Arc::new(CreateRoleUseCase::new(repo.clone(), uow.clone())),
        update_use_case: Arc::new(UpdateRoleUseCase::new(repo.clone(), uow.clone())),
        delete_use_case: Arc::new(DeleteRoleUseCase::new(repo.clone(), uow.clone())),
        permission_repo: Arc::new(PermissionCatalogRepository::new(&ctx.repos.pool)),
    }
}

pub fn bff_roles_state(ctx: &PlatformContext) -> BffRolesState {
    BffRolesState {
        role_repo: ctx.repos.role_repo.clone(),
        application_repo: ctx.repos.application_repo.clone(),
        unit_of_work: ctx.unit_of_work.clone(),
        role_sync_service: Arc::new(crate::shared::role_sync_service::RoleSyncService::new(
            ctx.repos.role_repo.clone(),
        )),
        permission_repo: Arc::new(PermissionCatalogRepository::new(&ctx.repos.pool)),
    }
}

/// Create roles router
pub fn roles_router(state: RolesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::role::api::create_role,
            crate::role::api::list_roles
        ))
        .routes(routes!(crate::role::api::get_filter_applications))
        .routes(routes!(crate::role::api::list_permissions))
        .routes(routes!(crate::role::api::get_permission))
        .routes(routes!(crate::role::api::get_role_by_code))
        .routes(routes!(crate::role::api::get_roles_by_source))
        .routes(routes!(crate::role::api::get_roles_by_application_id))
        .routes(routes!(
            crate::role::api::get_role,
            crate::role::api::update_role,
            crate::role::api::delete_role
        ))
        // Grant/revoke by role name: `role_permissions_router` (Go's operations).
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn role_permissions_router(state: RolePermissionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::role::api::list_role_permissions,
            crate::role::api::grant_role_permission_by_body
        ))
        .routes(routes!(
            crate::role::api::grant_role_permission,
            crate::role::api::revoke_role_permission
        ))
        .routes(routes!(crate::role::api::delete_catalog_permission))
        .routes(routes!(crate::role::bff::bff_create_permission))
        .with_state(state)
}

/// Create BFF roles router (mounted at `/bff/roles`)
pub fn bff_roles_router(state: BffRolesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::role::bff::create_role,
            crate::role::bff::list_roles
        ))
        .routes(routes!(crate::role::bff::get_filter_applications))
        .routes(routes!(crate::role::bff::list_permissions))
        .routes(routes!(crate::role::bff::get_permission))
        .routes(routes!(crate::role::bff::sync_platform_roles))
        .routes(routes!(
            crate::role::bff::get_role,
            crate::role::bff::update_role,
            crate::role::bff::delete_role
        ))
        .with_state(state)
}

/// Create application roles SDK router
pub fn application_roles_sdk_router(state: ApplicationRolesSdkState) -> Router {
    Router::new()
        .route(
            "/{appCode}/roles",
            get(crate::shared::application_roles_sdk_api::list_roles)
                .post(crate::shared::application_roles_sdk_api::create_role),
        )
        .route(
            "/{appCode}/roles/{roleName}",
            delete(crate::shared::application_roles_sdk_api::delete_role),
        )
        .with_state(state)
}
