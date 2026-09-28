//! Platform config routes: `/api/config` and `/api/config-access` (plain),
//! and Go's property and access routes at their full paths
//! (`go_platform_config_router`).

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::access_api::ConfigAccessState;
use super::api::GoPlatformConfigState;
use super::api::PlatformConfigState;
use super::operations::{
    GrantPlatformConfigAccessUseCase, RevokePlatformConfigAccessUseCase,
    SetPlatformConfigPropertyUseCase,
};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .merge(go_platform_config_router(go_platform_config_state(ctx))),
        plain: Router::new()
            .nest(
                "/api/config",
                admin_platform_config_router(platform_config_state(ctx)).into(),
            )
            .nest(
                "/api/config-access",
                config_access_router(config_access_state(ctx)).into(),
            ),
    }
}

pub fn platform_config_state(ctx: &PlatformContext) -> PlatformConfigState {
    let repos = &ctx.repos;
    PlatformConfigState {
        config_repo: repos.platform_config_repo.clone(),
        access_repo: repos.platform_config_access_repo.clone(),
        app_access: ctx.app_access.clone(),
        set_property_use_case: Arc::new(SetPlatformConfigPropertyUseCase::new(
            repos.platform_config_repo.clone(),
            ctx.unit_of_work.clone(),
            ctx.encryption.clone(),
        )),
    }
}

pub fn config_access_state(ctx: &PlatformContext) -> ConfigAccessState {
    let repos = &ctx.repos;
    ConfigAccessState {
        access_repo: repos.platform_config_access_repo.clone(),
        app_access: ctx.app_access.clone(),
        grant_access_use_case: Arc::new(GrantPlatformConfigAccessUseCase::new(
            repos.platform_config_access_repo.clone(),
            ctx.unit_of_work.clone(),
        )),
        revoke_access_use_case: Arc::new(RevokePlatformConfigAccessUseCase::new(
            repos.platform_config_access_repo.clone(),
            ctx.unit_of_work.clone(),
        )),
    }
}

pub fn go_platform_config_state(ctx: &PlatformContext) -> GoPlatformConfigState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    GoPlatformConfigState {
        config_repo: repos.platform_config_repo.clone(),
        access_repo: repos.platform_config_access_repo.clone(),
        encryption: ctx.encryption.clone(),
        set_property_use_case: Arc::new(SetPlatformConfigPropertyUseCase::new(
            repos.platform_config_repo.clone(),
            uow.clone(),
            ctx.encryption.clone(),
        )),
        grant_access_use_case: Arc::new(GrantPlatformConfigAccessUseCase::new(
            repos.platform_config_access_repo.clone(),
            uow.clone(),
        )),
        revoke_access_use_case: Arc::new(RevokePlatformConfigAccessUseCase::new(
            repos.platform_config_access_repo.clone(),
            uow.clone(),
        )),
        application_repo: repos.application_repo.clone(),
        app_access: ctx.app_access.clone(),
    }
}

pub fn admin_platform_config_router(state: PlatformConfigState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::platform_config::api::list_configs))
        .routes(routes!(crate::platform_config::api::get_section))
        // The property routes are Go's: `go_platform_config_router`.
        .with_state(state)
}

pub fn config_access_router(state: ConfigAccessState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::platform_config::access_api::list_access,
            crate::platform_config::access_api::create_access
        ))
        .routes(routes!(
            crate::platform_config::access_api::update_access,
            crate::platform_config::access_api::delete_access
        ))
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn go_platform_config_router(state: GoPlatformConfigState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::platform_config::api::get_config_property,
            crate::platform_config::api::set_config_property,
            crate::platform_config::api::delete_config_property
        ))
        .routes(routes!(crate::platform_config::api::list_platform_config))
        .routes(routes!(
            crate::platform_config::api::list_platform_config_access,
            crate::platform_config::api::grant_platform_config_access
        ))
        .routes(routes!(
            crate::platform_config::api::revoke_platform_config_access
        ))
        .with_state(state)
}
