//! Portal identity plane routes (Go wire_routes.go:261-291): the admin
//! surface (`/api/portal-users`, `/api/portal-apps`) and the public portal
//! login surface (`/portal/*`), behind the OIDC bridge's per-IP limit
//! (FC_OIDC_RATE_PER_MIN / FC_OIDC_BURST). The plane's hooks on the shared
//! reset-token and OIDC routes are mounted by `auth::routes`.

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use axum::routing::{delete, get, post};
use axum::Router;
use utoipa_axum::router::OpenApiRouter;

use super::login_api::PortalLoginState;
use super::PortalState;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use crate::shared::rate_limit_middleware::{rate_limit_per_ip, IpRateLimiterState};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let portal_rate_limit = axum::middleware::from_fn_with_state(
        IpRateLimiterState::new(&super::login_api::portal_ip_rate_config()),
        rate_limit_per_ip,
    );
    AggregateRoutes {
        documented: OpenApiRouter::new(),
        plain: Router::new()
            .nest("/api/portal-users", portal_users_router(ctx.portal.clone()))
            .nest("/api/portal-apps", portal_apps_router(ctx.portal.clone()))
            .nest(
                "/portal",
                portal_login_router(portal_login_state(ctx)).layer(portal_rate_limit),
            ),
    }
}

/// The portal login routes' state; the portal hook on the OIDC callback
/// route (`auth::routes`) runs on it too.
pub fn portal_login_state(ctx: &PlatformContext) -> PortalLoginState {
    PortalLoginState {
        portal: ctx.portal.clone(),
        oidc: crate::auth::routes::oidc_login_state(ctx),
    }
}

/// `/api/portal-users` routes.
pub fn portal_users_router(state: PortalState) -> Router {
    Router::new()
        .route(
            "/",
            post(crate::portal::api::ensure_portal_user).get(crate::portal::api::list_portal_users),
        )
        .route("/{id}", delete(crate::portal::api::delete_portal_user))
        .route(
            "/{id}/activate",
            post(crate::portal::api::activate_portal_user),
        )
        .route(
            "/{id}/deactivate",
            post(crate::portal::api::deactivate_portal_user),
        )
        .route(
            "/{id}/apps",
            post(crate::portal::api::grant_portal_user_app),
        )
        .route(
            "/{id}/apps/{portal_app_code}",
            delete(crate::portal::api::revoke_portal_user_app),
        )
        .with_state(state)
}

/// `/api/portal-apps` routes.
pub fn portal_apps_router(state: PortalState) -> Router {
    Router::new()
        .route(
            "/",
            get(crate::portal::api::list_portal_apps).post(crate::portal::api::create_portal_app),
        )
        .route(
            "/{id}",
            axum::routing::put(crate::portal::api::update_portal_app)
                .delete(crate::portal::api::delete_portal_app),
        )
        .route(
            "/{id}/assign-unassigned",
            post(crate::portal::api::assign_unassigned_portal_users),
        )
        .with_state(state)
}

/// `/portal` routes (public; per-IP limited by the router).
pub fn portal_login_router(state: PortalLoginState) -> Router {
    Router::new()
        .route("/authorize", get(crate::portal::login_api::authorize))
        .route(
            "/auth/check-domain",
            post(crate::portal::login_api::check_domain),
        )
        .route(
            "/auth/login",
            post(crate::portal::login_api::password_login),
        )
        .route(
            "/auth/password-reset",
            post(crate::portal::login_api::request_password_reset),
        )
        .route(
            "/auth/oidc/login",
            get(crate::portal::login_api::portal_oidc_login),
        )
        .with_state(state)
}
