//! CORS origin routes: `/api/platform/cors` (plain).

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use utoipa_axum::router::OpenApiRouter;

use super::api::CorsState;
use super::operations::{AddCorsOriginUseCase, DeleteCorsOriginUseCase};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new(),
        plain: Router::new().nest("/api/platform/cors", cors_router(cors_state(ctx))),
    }
}

pub fn cors_state(ctx: &PlatformContext) -> CorsState {
    let repo = &ctx.repos.cors_repo;
    CorsState {
        cors_repo: repo.clone(),
        add_use_case: Arc::new(AddCorsOriginUseCase::new(
            repo.clone(),
            ctx.unit_of_work.clone(),
        )),
        delete_use_case: Arc::new(DeleteCorsOriginUseCase::new(
            repo.clone(),
            ctx.unit_of_work.clone(),
        )),
    }
}

pub fn cors_router(state: CorsState) -> Router {
    Router::new()
        .route(
            "/",
            post(crate::cors::api::create_cors_origin).get(crate::cors::api::list_cors_origins),
        )
        .route("/allowed", get(crate::cors::api::get_allowed_origins))
        .route(
            "/{id}",
            get(crate::cors::api::get_cors_origin).delete(crate::cors::api::delete_cors_origin),
        )
        .with_state(state)
}
