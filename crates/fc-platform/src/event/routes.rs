//! Event routes: the read routes on `/api/events` (SDKs, bearer) and
//! `/bff/events` (SPA, cookie), and the high-volume ingest
//! `POST /api/events/batch` (`shared::batch_api`, platform infrastructure).

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use axum::extract::DefaultBodyLimit;
use axum::routing::post;
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::EventsState;
use crate::shared::batch_api::SdkEventsState;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let events = events_state(ctx);
    AggregateRoutes {
        // Same cursor-paginated read handlers on both tiers. The API tier
        // leaves out `batch_create_events`: SDK callers use the bulk-insert
        // `POST /api/events/batch` below, and axum panics if both register it.
        documented: OpenApiRouter::new()
            .nest("/api/events", events_api_router(events.clone()))
            .nest("/bff/events", events_router(events)),
        plain: Router::new().nest(
            "/api/events",
            sdk_events_batch_router(SdkEventsState {
                event_repo: ctx.repos.event_repo.clone(),
                client_repo: ctx.repos.client_repo.clone(),
                signing: ctx.signing_guard.clone(),
            }),
        ),
    }
}

pub fn events_state(ctx: &PlatformContext) -> EventsState {
    EventsState {
        event_repo: ctx.repos.event_repo.clone(),
        signing: ctx.signing_guard.clone(),
    }
}

/// Create events router for the BFF tier (`/bff/events`). Cookie-auth, used
/// by the SPA. Includes `batch_create_events` — the SPA-facing batch that
/// fans out events to subscriptions.
pub fn events_router(state: EventsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::event::api::create_event,
            crate::event::api::list_events
        ))
        .routes(routes!(crate::event::api::batch_create_events))
        .routes(routes!(crate::event::api::list_events_raw))
        .routes(routes!(crate::event::api::event_filter_options))
        .routes(routes!(crate::event::api::get_event))
        .with_state(state)
}

/// Create events router for the API tier (`/api/events`). Bearer-auth, used
/// by SDK consumers. **No `batch_create_events`** — SDK callers use
/// `sdk_events_batch_router::POST /batch` (different handler, optimized for
/// high-volume insert without per-event fan-out). The two routers must not
/// both register `POST /batch` against the same prefix.
pub fn events_api_router(state: EventsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::event::api::create_event,
            crate::event::api::list_events
        ))
        .routes(routes!(crate::event::api::list_events_raw))
        .routes(routes!(crate::event::api::event_filter_options))
        .routes(routes!(crate::event::api::get_event))
        .with_state(state)
}

pub fn sdk_events_batch_router(state: SdkEventsState) -> Router {
    Router::new()
        .route("/batch", post(crate::shared::batch_api::batch_events))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}
