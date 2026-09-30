//! Routes of the shared, cross-aggregate features: filter options, the
//! monitoring API, the SDK sync endpoints, Go's raw-list aliases, the
//! platform-owned `/api/dispatch/*` (router config, and the delivery
//! callback when an app key is set), the dashboard and debug BFF grids,
//! `/api/me`, the client switch (`/auth/client`, behind the `/auth`
//! per-IP limit), `/.well-known`, the public platform info, and the
//! developer portal (which serves the platform's own OpenAPI document, so
//! `router::build` mounts it once the document exists:
//! [`developer_portal_routes`]).

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::bff_dashboard_api::BffDashboardState;
use super::bff_developer_api::BffDeveloperState;
use super::client_selection_api::ClientSelectionState;
use super::debug_api::DebugState;
use super::dispatch_process_api::DispatchProcessState;
use super::filter_options_api::FilterOptionsState;
use super::go_read_aliases_api::ReadAliasesState;
use super::health_api::HealthState;
use super::me_api::MeState;
use super::monitoring_api::{
    CircuitBreakerRegistry, InFlightTracker, LeaderState, MonitoringState,
};
use super::platform_context::{AggregateRoutes, PlatformContext};
use super::public_api::PublicApiState;
use super::rate_limit_middleware::rate_limit_per_ip;
use super::router_config_api::RouterConfigState;
use super::sdk_sync_api::SdkSyncState;
use super::sdk_sync_go_api::SdkSyncGoState;
use super::well_known_api::WellKnownState;

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let repos = &ctx.repos;
    let auth_rate_limit =
        axum::middleware::from_fn_with_state(ctx.auth_ip_limit.clone(), rate_limit_per_ip);
    let debug = DebugState {
        event_repo: repos.event_repo.clone(),
        dispatch_job_repo: repos.dispatch_job_repo.clone(),
    };
    let public = PublicApiState {
        config_repo: repos.platform_config_repo.clone(),
        client_repo: repos.client_repo.clone(),
    };
    let plain = Router::new()
        .nest(
            "/bff/dashboard",
            bff_dashboard_router(BffDashboardState {
                pool: repos.pool.clone(),
            }),
        )
        .nest("/bff/debug/events", debug_events_router(debug.clone()))
        .nest(
            "/bff/debug/dispatch-jobs",
            debug_dispatch_jobs_router(debug),
        )
        .nest("/api/me", me_router(me_state(ctx)))
        .nest(
            "/.well-known",
            well_known_router(WellKnownState {
                auth_service: ctx.auth.auth.clone(),
                external_base_url: ctx.config.well_known_external_base_url.clone(),
            }),
        )
        .nest(
            "/auth/client",
            client_selection_router(client_selection_state(ctx)).layer(auth_rate_limit),
        )
        // Go's SPA-bootstrap alias of `/api/public/platform`.
        .nest("/api/config", platform_info_router(public.clone()))
        .nest("/api/public", public_router(public));
    // The router's delivery callback, only when its job tokens can be
    // verified (see `dispatch_process_state`).
    let plain = match dispatch_process_state(ctx) {
        Some(state) => plain.nest("/api/dispatch", dispatch_process_router(state)),
        None => plain,
    };
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .nest(
                "/bff/filter-options",
                filter_options_router(filter_options_state(ctx)),
            )
            .nest("/api/monitoring", monitoring_router(monitoring_state(ctx)))
            // SDK-facing app-scoped sync routes, in the OpenAPI document so
            // the SDK code generators produce typed bindings for them.
            .nest("/api/applications", sdk_sync_router(sdk_sync_state(ctx)))
            .merge(sdk_sync_go_router(sdk_sync_go_state(ctx)))
            .merge(read_aliases_router(read_aliases_state(ctx)))
            .merge(router_config_router(router_config_state_for(ctx))),
        plain,
    }
}

/// `/bff/developer/*`: the developer portal. It serves the platform's own
/// OpenAPI document (`platform_openapi`, computed from the other routes),
/// so it is built after them.
pub fn developer_portal_routes(
    ctx: &PlatformContext,
    platform_openapi: Arc<serde_json::Value>,
) -> Router {
    let repos = &ctx.repos;
    let openapi_spec_repo = Arc::new(
        crate::application_openapi_spec::repository::OpenApiSpecRepository::new(&repos.pool),
    );
    Router::new().nest(
        "/bff/developer",
        bff_developer_router(BffDeveloperState {
            application_repo: repos.application_repo.clone(),
            openapi_spec_repo: openapi_spec_repo.clone(),
            event_type_repo: repos.event_type_repo.clone(),
            principal_repo: repos.principal_repo.clone(),
            sync_openapi_use_case: Arc::new(
                crate::application_openapi_spec::operations::SyncOpenApiSpecUseCase::new(
                    openapi_spec_repo,
                    ctx.unit_of_work.clone(),
                ),
            ),
            platform_openapi,
            platform_application_id: ctx.platform_application_id.clone(),
        }),
    )
}

pub fn filter_options_state(ctx: &PlatformContext) -> FilterOptionsState {
    let repos = &ctx.repos;
    FilterOptionsState {
        client_repo: repos.client_repo.clone(),
        event_type_repo: repos.event_type_repo.clone(),
        subscription_repo: repos.subscription_repo.clone(),
        dispatch_pool_repo: repos.dispatch_pool_repo.clone(),
        application_repo: repos.application_repo.clone(),
    }
}

fn monitoring_state(ctx: &PlatformContext) -> MonitoringState {
    MonitoringState {
        leader_state: LeaderState::new(uuid::Uuid::new_v4().to_string()),
        circuit_breakers: CircuitBreakerRegistry::new(),
        in_flight: InFlightTracker::new(),
        dispatch_job_repo: ctx.repos.dispatch_job_repo.clone(),
        pool: ctx.repos.pool.clone(),
        start_time: std::time::Instant::now(),
    }
}

pub fn sdk_sync_state(ctx: &PlatformContext) -> SdkSyncState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    SdkSyncState {
        sync_roles_use_case: Arc::new(crate::role::operations::SyncRolesUseCase::new(
            repos.role_repo.clone(),
            repos.application_repo.clone(),
            uow.clone(),
        )),
        sync_event_types_use_case: Arc::new(
            crate::event_type::operations::SyncEventTypesUseCase::new(
                repos.event_type_repo.clone(),
                uow.clone(),
            ),
        ),
        sync_subscriptions_use_case: Arc::new(
            crate::subscription::operations::SyncSubscriptionsUseCase::new(
                repos.subscription_repo.clone(),
                repos.connection_repo.clone(),
                repos.dispatch_pool_repo.clone(),
                uow.clone(),
            ),
        ),
        sync_dispatch_pools_use_case: Arc::new(
            crate::dispatch_pool::operations::SyncDispatchPoolsUseCase::new(
                repos.dispatch_pool_repo.clone(),
                uow.clone(),
            ),
        ),
        sync_processes_use_case: Arc::new(crate::process::operations::SyncProcessesUseCase::new(
            repos.process_repo.clone(),
            uow.clone(),
        )),
        sync_scheduled_jobs_use_case: Arc::new(
            crate::scheduled_job::operations::SyncScheduledJobsUseCase::new(
                repos.scheduled_job_repo.clone(),
                uow.clone(),
            ),
        ),
        sync_openapi_use_case: Arc::new(
            crate::application_openapi_spec::operations::SyncOpenApiSpecUseCase::new(
                Arc::new(
                    crate::application_openapi_spec::repository::OpenApiSpecRepository::new(
                        &repos.pool,
                    ),
                ),
                uow.clone(),
            ),
        ),
        app_access: ctx.app_access.clone(),
        trigger_objects: repos.function_trigger_object_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        application_repo: repos.application_repo.clone(),
        client_repo: repos.client_repo.clone(),
        unit_of_work: uow.clone(),
    }
}

fn sdk_sync_go_state(ctx: &PlatformContext) -> SdkSyncGoState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    SdkSyncGoState {
        app_access: ctx.app_access.clone(),
        client_repo: repos.client_repo.clone(),
        sync_connections_use_case: Arc::new(
            crate::connection::operations::sync::SyncConnectionsUseCase::new(
                repos.connection_repo.clone(),
                repos.application_repo.clone(),
                repos.subscription_repo.clone(),
                uow.clone(),
            ),
        ),
        sync_processes_use_case: Arc::new(crate::process::operations::SyncProcessesUseCase::new(
            repos.process_repo.clone(),
            uow.clone(),
        )),
    }
}

fn read_aliases_state(ctx: &PlatformContext) -> ReadAliasesState {
    ReadAliasesState {
        events: crate::event::routes::events_state(ctx),
        dispatch_jobs: crate::dispatch_job::routes::dispatch_jobs_state(ctx),
    }
}

fn router_config_state_for(ctx: &PlatformContext) -> RouterConfigState {
    // Go resolves the queue settings once at boot and refuses to start on a
    // bad SQS configuration (internal/server/run.go:69).
    let queue_settings = crate::shared::dispatch_queue::QueueSettings::from_env()
        .unwrap_or_else(|e| panic!("dispatch queue settings: {e}"));
    RouterConfigState {
        repo: Arc::new(
            crate::dispatch_pool::router_config_repository::RouterConfigRepository::new(
                &ctx.repos.pool,
            ),
        ),
        settings: Arc::new(queue_settings),
    }
}

pub fn me_state(ctx: &PlatformContext) -> MeState {
    let repos = &ctx.repos;
    MeState {
        client_repo: repos.client_repo.clone(),
        application_repo: repos.application_repo.clone(),
        app_client_config_repo: repos.application_client_config_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        auth_service: ctx.auth.auth.clone(),
    }
}

fn client_selection_state(ctx: &PlatformContext) -> ClientSelectionState {
    let repos = &ctx.repos;
    ClientSelectionState {
        principal_repo: repos.principal_repo.clone(),
        client_repo: repos.client_repo.clone(),
        role_repo: repos.role_repo.clone(),
        grant_repo: repos.client_access_grant_repo.clone(),
        auth_service: ctx.auth.auth.clone(),
    }
}

/// The router's delivery callback. Fail closed as Go does: without
/// FLOWCATALYST_APP_KEY no token can be verified, so it is not mounted.
fn dispatch_process_state(ctx: &PlatformContext) -> Option<DispatchProcessState> {
    let repos = &ctx.repos;
    match crate::scheduler::DispatchAuthService::from_env() {
        Some(auth) => Some(DispatchProcessState {
            dispatch_job_repo: repos.dispatch_job_repo.clone(),
            http_client: super::dispatch_process_api::delivery_http_client(
                fc_common::netguard::default_policy(),
            ),
            delivery_policy: fc_common::netguard::default_policy(),
            credentials: Some(Arc::new(
                crate::dispatch_job::delivery_credentials::DeliveryCredentials::new(
                    repos.subscription_repo.clone(),
                    repos.connection_repo.clone(),
                    repos.application_repo.clone(),
                    ctx.outbound_credentials.clone(),
                ),
            )),
            auth,
            client_codes: Some(Arc::new(
                super::dispatch_process_api::ClientCodeResolver::new(repos.client_repo.clone()),
            )),
        }),
        None => {
            tracing::warn!(
                "dispatch-processing callback not mounted: FLOWCATALYST_APP_KEY is not set, \
                 so the router's job tokens cannot be verified"
            );
            None
        }
    }
}

/// Create filter options router
pub fn filter_options_router(state: FilterOptionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::shared::filter_options_api::get_all_options))
        .routes(routes!(
            crate::shared::filter_options_api::get_client_options
        ))
        .routes(routes!(
            crate::shared::filter_options_api::get_event_type_options
        ))
        .routes(routes!(
            crate::shared::filter_options_api::get_subscription_options
        ))
        .routes(routes!(
            crate::shared::filter_options_api::get_dispatch_pool_options
        ))
        .routes(routes!(
            crate::shared::filter_options_api::get_events_filter_options
        ))
        .routes(routes!(
            crate::shared::filter_options_api::get_dispatch_jobs_filter_options
        ))
        .routes(routes!(
            crate::shared::filter_options_api::get_event_type_applications
        ))
        .routes(routes!(
            crate::shared::filter_options_api::get_event_type_subdomains
        ))
        .routes(routes!(
            crate::shared::filter_options_api::get_event_type_aggregates
        ))
        .with_state(state)
}

/// Create event-type filters router (for mounting at /bff/event-types/filters)
/// This provides the same endpoints as filter_options_router but at a different path
/// to maintain backwards compatibility with frontend expectations.
pub fn event_type_filters_router(state: FilterOptionsState) -> Router {
    Router::new()
        .route(
            "/applications",
            get(crate::shared::filter_options_api::get_event_type_applications),
        )
        .route(
            "/subdomains",
            get(crate::shared::filter_options_api::get_event_type_subdomains),
        )
        .route(
            "/aggregates",
            get(crate::shared::filter_options_api::get_event_type_aggregates),
        )
        .with_state(state)
}

/// Create monitoring router
pub fn monitoring_router(state: MonitoringState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::shared::monitoring_api::get_standby_status))
        .routes(routes!(crate::shared::monitoring_api::get_dashboard))
        .routes(routes!(crate::shared::monitoring_api::get_circuit_breakers))
        .routes(routes!(
            crate::shared::monitoring_api::get_in_flight_messages
        ))
        .routes(routes!(crate::shared::monitoring_api::get_pool_stats))
        .with_state(state)
}

/// Create SDK sync router
///
/// Mounts application-scoped sync routes:
/// - POST /{appCode}/roles/sync
/// - POST /{appCode}/event-types/sync
/// - POST /{appCode}/subscriptions/sync
/// - POST /{appCode}/dispatch-pools/sync
/// - POST /{appCode}/principals/sync
/// - POST /{appCode}/processes/sync
/// - POST /{appCode}/scheduled-jobs/sync
/// - POST /{appCode}/openapi/sync
pub fn sdk_sync_router(state: SdkSyncState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::shared::sdk_sync_api::sync_roles))
        .routes(routes!(crate::shared::sdk_sync_api::sync_event_types))
        .routes(routes!(crate::shared::sdk_sync_api::sync_subscriptions))
        .routes(routes!(crate::shared::sdk_sync_api::sync_dispatch_pools))
        .routes(routes!(crate::shared::sdk_sync_api::sync_principals))
        .routes(routes!(crate::shared::sdk_sync_api::sync_processes))
        .routes(routes!(crate::shared::sdk_sync_api::sync_scheduled_jobs))
        .routes(routes!(crate::shared::sdk_sync_api::sync_openapi))
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn sdk_sync_go_router(state: SdkSyncGoState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::shared::sdk_sync_go_api::sync_connections))
        .routes(routes!(
            crate::shared::sdk_sync_go_api::sync_processes_by_body
        ))
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn read_aliases_router(state: ReadAliasesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::shared::go_read_aliases_api::api_list_events_raw
        ))
        .routes(routes!(
            crate::shared::go_read_aliases_api::bff_list_events_raw
        ))
        .routes(routes!(
            crate::shared::go_read_aliases_api::api_list_dispatch_jobs_raw
        ))
        .routes(routes!(
            crate::shared::go_read_aliases_api::bff_list_dispatch_jobs_raw
        ))
        .routes(routes!(
            crate::shared::go_read_aliases_api::api_dispatch_jobs_by_event
        ))
        .routes(routes!(
            crate::shared::go_read_aliases_api::bff_dispatch_jobs_by_event
        ))
        .with_state(state)
}

pub fn router_config_router(state: RouterConfigState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::shared::router_config_api::get_router_config))
        .with_state(state)
}

pub fn bff_dashboard_router(state: BffDashboardState) -> Router {
    Router::new()
        .route(
            "/stats",
            get(crate::shared::bff_dashboard_api::get_dashboard_stats),
        )
        .with_state(state)
}

/// Create debug events router
pub fn debug_events_router(state: DebugState) -> Router {
    Router::new()
        .route("/", get(crate::shared::debug_api::list_raw_events))
        .route("/{id}", get(crate::shared::debug_api::get_raw_event))
        .with_state(state)
}

/// Create debug dispatch jobs router
pub fn debug_dispatch_jobs_router(state: DebugState) -> Router {
    Router::new()
        .route("/", get(crate::shared::debug_api::list_raw_dispatch_jobs))
        .route("/{id}", get(crate::shared::debug_api::get_raw_dispatch_job))
        .with_state(state)
}

pub fn me_router(state: MeState) -> Router {
    Router::new()
        .route("/", get(crate::shared::me_api::whoami))
        .route(
            "/applications",
            get(crate::shared::me_api::list_my_applications),
        )
        .route("/clients", get(crate::shared::me_api::list_my_clients))
        .route(
            "/clients/{clientId}",
            get(crate::shared::me_api::get_my_client),
        )
        .route(
            "/clients/{clientId}/applications",
            get(crate::shared::me_api::list_my_client_applications),
        )
        .with_state(state)
}

/// Create the well-known router
pub fn well_known_router(state: WellKnownState) -> Router {
    Router::new()
        .route(
            "/openid-configuration",
            get(crate::shared::well_known_api::get_openid_configuration),
        )
        .route("/jwks.json", get(crate::shared::well_known_api::get_jwks))
        .with_state(state)
}

/// Create client selection router
pub fn client_selection_router(state: ClientSelectionState) -> Router {
    Router::new()
        .route(
            "/accessible",
            get(crate::shared::client_selection_api::list_accessible_clients),
        )
        .route(
            "/switch",
            post(crate::shared::client_selection_api::switch_client),
        )
        .route(
            "/current",
            get(crate::shared::client_selection_api::get_current_client),
        )
        .with_state(state)
}

/// `/platform` alone, for Go's SPA-bootstrap alias `/api/config/platform`.
pub fn platform_info_router(state: PublicApiState) -> Router {
    Router::new()
        .route(
            "/platform",
            get(crate::shared::public_api::get_platform_info),
        )
        .with_state(state)
}

pub fn public_router(state: PublicApiState) -> Router {
    Router::new()
        .route(
            "/platform",
            get(crate::shared::public_api::get_platform_info),
        )
        .route(
            "/login-theme",
            get(crate::shared::public_api::get_login_theme),
        )
        .with_state(state)
}

/// `/process` and `/settled`, nested under `/api/dispatch`. Outside the
/// platform's bearer middleware: both authenticate per job with the
/// scheduler's token.
pub fn dispatch_process_router(state: DispatchProcessState) -> Router {
    Router::new()
        .route(
            "/process",
            post(crate::shared::dispatch_process_api::process_dispatch),
        )
        .route(
            "/settled",
            post(crate::shared::dispatch_process_api::settled),
        )
        .with_state(state)
}

pub fn bff_developer_router(state: BffDeveloperState) -> Router {
    Router::new()
        .route(
            "/applications",
            get(crate::shared::bff_developer_api::list_applications),
        )
        .route(
            "/applications/{app_id}",
            get(crate::shared::bff_developer_api::get_application),
        )
        .route(
            "/applications/{app_id}/openapi/current",
            get(crate::shared::bff_developer_api::get_current_openapi),
        )
        .route(
            "/applications/{app_id}/openapi/versions",
            get(crate::shared::bff_developer_api::list_versions),
        )
        .route(
            "/applications/{app_id}/openapi/versions/{spec_id}",
            get(crate::shared::bff_developer_api::get_version),
        )
        .route(
            "/applications/{app_id}/event-types",
            get(crate::shared::bff_developer_api::list_event_types),
        )
        .route(
            "/sync-platform-openapi",
            post(crate::shared::bff_developer_api::sync_platform_openapi),
        )
        .with_state(state)
}

/// Create the health router
pub fn health_router(state: HealthState) -> Router {
    Router::new()
        .route("/", get(crate::shared::health_api::get_health))
        .route("/live", get(crate::shared::health_api::get_liveness))
        .route("/ready", get(crate::shared::health_api::get_readiness))
        .route("/startup", get(crate::shared::health_api::get_startup))
        .with_state(state)
}

/// Create the platform config router
pub fn platform_config_router() -> Router {
    Router::new().route(
        "/platform",
        get(crate::shared::platform_config_api::get_platform_config),
    )
}
