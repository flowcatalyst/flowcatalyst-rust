//! Event type routes: `/api/event-types`, `/bff/event-types` (plain), and
//! Go's `POST /api/event-types/{id}/schemas` and `PUT /bff/event-types/{id}`
//! at their full paths.

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::EventTypesState;
use super::go_api::EventTypeGoState;
use super::operations::{
    AddSchemaUseCase, CreateEventTypeUseCase, DeleteEventTypeUseCase, SyncEventTypesUseCase,
    UpdateEventTypeUseCase,
};
use crate::event_type::bff::BffEventTypesState;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let repo = &ctx.repos.event_type_repo;
    let uow = &ctx.unit_of_work;
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .nest(
                "/api/event-types",
                event_types_router(event_types_state(ctx)),
            )
            .merge(event_type_go_router(EventTypeGoState {
                event_type_repo: repo.clone(),
                add_schema_use_case: Arc::new(AddSchemaUseCase::new(repo.clone(), uow.clone())),
                bff: bff_event_types_state(ctx),
            })),
        plain: Router::new().nest(
            "/bff/event-types",
            bff_event_types_router(bff_event_types_state(ctx)).into(),
        ),
    }
}

pub fn event_types_state(ctx: &PlatformContext) -> EventTypesState {
    let repo = &ctx.repos.event_type_repo;
    let uow = &ctx.unit_of_work;
    EventTypesState {
        event_type_repo: repo.clone(),
        create_use_case: Arc::new(CreateEventTypeUseCase::new(repo.clone(), uow.clone())),
        update_use_case: Arc::new(UpdateEventTypeUseCase::new(repo.clone(), uow.clone())),
        delete_use_case: Arc::new(DeleteEventTypeUseCase::new(repo.clone(), uow.clone())),
        add_schema_use_case: Arc::new(AddSchemaUseCase::new(repo.clone(), uow.clone())),
    }
}

pub fn bff_event_types_state(ctx: &PlatformContext) -> BffEventTypesState {
    let repo = &ctx.repos.event_type_repo;
    BffEventTypesState {
        event_type_repo: repo.clone(),
        sync_use_case: Arc::new(SyncEventTypesUseCase::new(
            repo.clone(),
            ctx.unit_of_work.clone(),
        )),
        unit_of_work: ctx.unit_of_work.clone(),
    }
}

/// Create event types router
pub fn event_types_router(state: EventTypesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::event_type::api::create_event_type,
            crate::event_type::api::list_event_types
        ))
        .routes(routes!(
            crate::event_type::api::get_event_type,
            crate::event_type::api::update_event_type,
            crate::event_type::api::delete_event_type
        ))
        .routes(routes!(crate::event_type::api::get_event_type_by_code))
        .routes(routes!(crate::event_type::api::add_schema_version))
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn event_type_go_router(state: EventTypeGoState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::event_type::go_api::add_event_type_schema))
        .routes(routes!(crate::event_type::go_api::bff_put_event_type))
        .with_state(state)
}

/// Create BFF event types router (mounted at `/bff/event-types`)
pub fn bff_event_types_router(state: BffEventTypesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::event_type::bff::create_event_type,
            crate::event_type::bff::list_event_types
        ))
        .routes(routes!(crate::event_type::bff::sync_platform))
        .routes(routes!(crate::event_type::bff::get_filter_applications))
        .routes(routes!(crate::event_type::bff::get_filter_subdomains))
        .routes(routes!(crate::event_type::bff::get_filter_aggregates))
        .routes(routes!(
            crate::event_type::bff::get_event_type,
            crate::event_type::bff::update_event_type,
            crate::event_type::bff::delete_event_type
        ))
        .routes(routes!(crate::event_type::bff::archive_event_type))
        .routes(routes!(crate::event_type::bff::add_schema))
        .routes(routes!(crate::event_type::bff::finalise_schema))
        .routes(routes!(crate::event_type::bff::deprecate_schema))
        .with_state(state)
}
