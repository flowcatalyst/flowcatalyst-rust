//! Login attempt routes: `/api/login-attempts` (plain).

use axum::routing::get;
use axum::Router;
use utoipa_axum::router::OpenApiRouter;

use super::api::LoginAttemptsState;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new(),
        plain: Router::new().nest(
            "/api/login-attempts",
            login_attempts_router(LoginAttemptsState {
                login_attempt_repo: ctx.repos.login_attempt_repo.clone(),
            }),
        ),
    }
}

pub fn login_attempts_router(state: LoginAttemptsState) -> Router {
    Router::new()
        .route("/", get(crate::login_attempt::api::list_login_attempts))
        .with_state(state)
}
