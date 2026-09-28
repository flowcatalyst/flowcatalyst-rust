//! Shared builder for `PlatformRoutes`.
//!
//! Constructs the ~38 API state structs every binary needs, wiring
//! them to repositories, auth services, and use cases. Binaries provide
//! a `PlatformRoutesConfig` with the points of variation:
//!
//! 1. Whether the OIDC session cookie's `Secure` flag is set — `true`
//!    in production (fc-server), `false` for dev/no-TLS deployments.
//! 2. Optional static asset directory for SPA serving.
//!
//! Event fan-out (subscriptions → dispatch jobs → queue) runs out-of-band
//! in the stream processor (fc-stream::EventFanOutService); the request
//! path no longer needs queue/dispatch deps wired in here.
//!
//! In addition, external base URLs for the well-known, OIDC login, and
//! password-reset endpoints are passed directly so each binary can read
//! them from env in whatever style it prefers.

use std::sync::Arc;

use crate::api::{
    ApplicationRolesSdkState, CircuitBreakerRegistry, ClientSelectionState, DebugState,
    DispatchProcessState, FilterOptionsState, InFlightTracker, LeaderState, MeState,
    MonitoringState, PublicApiState, SdkSyncState, WellKnownState,
};
use crate::repository::Repositories;
use crate::router::PlatformRoutes;
use crate::usecase::PgUnitOfWork;

use super::AuthServices;

use crate::shared::platform_context::PlatformContext;
pub use crate::shared::platform_context::PlatformRoutesConfig;

/// Build a fully-populated `PlatformRoutes` for the three server binaries.
///
/// Returns the struct, not the router — binaries still call `.build()`
/// and add their own middleware/static layers.
pub fn build_platform_routes(
    repos: &Repositories,
    auth: &AuthServices,
    unit_of_work: &Arc<PgUnitOfWork>,
    config: PlatformRoutesConfig,
    platform_application_id: String,
) -> PlatformRoutes {
    let ctx = PlatformContext::new(repos, auth, unit_of_work, config, platform_application_id);
    let _signing_guard = ctx.signing_guard.clone();
    let filter_options_state = FilterOptionsState {
        client_repo: repos.client_repo.clone(),
        event_type_repo: repos.event_type_repo.clone(),
        subscription_repo: repos.subscription_repo.clone(),
        dispatch_pool_repo: repos.dispatch_pool_repo.clone(),
        application_repo: repos.application_repo.clone(),
    };

    // ── Shared use cases (constructed once, shared between states) ────────
    let sync_event_types_use_case =
        Arc::new(crate::event_type::operations::SyncEventTypesUseCase::new(
            repos.event_type_repo.clone(),
            unit_of_work.clone(),
        ));

    // ── Process documentation (use cases + API state) ────────────────────
    let sync_processes_use_case = Arc::new(crate::process::operations::SyncProcessesUseCase::new(
        repos.process_repo.clone(),
        unit_of_work.clone(),
    ));

    let _email_service = ctx.email_service.clone();
    let _password_reset_emailer = ctx.password_reset_emailer.clone();

    let _reset_password_use_case =
        Arc::new(crate::principal::operations::ResetPasswordUseCase::new(
            repos.principal_repo.clone(),
            auth.password.clone(),
            unit_of_work.clone(),
        ));

    let app_access = ctx.app_access.clone();

    let sync_subscriptions_use_case = Arc::new(
        crate::subscription::operations::SyncSubscriptionsUseCase::new(
            repos.subscription_repo.clone(),
            repos.connection_repo.clone(),
            repos.dispatch_pool_repo.clone(),
            unit_of_work.clone(),
        ),
    );

    let _session_cookie = ctx.session_cookie.clone();
    let _encryption_service = ctx.encryption.clone();
    let _secret_resolver = ctx.secret_resolver.clone();

    let _backoff_policy = ctx.backoff_policy.clone();
    let _two_factor = ctx.two_factor.clone();
    let _portal_state = ctx.portal.clone();

    let public_api_state = PublicApiState {
        config_repo: repos.platform_config_repo.clone(),
        client_repo: repos.client_repo.clone(),
    };
    let me_state = MeState {
        client_repo: repos.client_repo.clone(),
        application_repo: repos.application_repo.clone(),
        app_client_config_repo: repos.application_client_config_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        auth_service: auth.auth.clone(),
    };
    let well_known_state = WellKnownState {
        auth_service: auth.auth.clone(),
        external_base_url: ctx.config.well_known_external_base_url.clone(),
    };
    let client_selection_state = ClientSelectionState {
        principal_repo: repos.principal_repo.clone(),
        client_repo: repos.client_repo.clone(),
        role_repo: repos.role_repo.clone(),
        grant_repo: repos.client_access_grant_repo.clone(),
        auth_service: auth.auth.clone(),
    };
    let create_role_use_case = Arc::new(crate::role::operations::CreateRoleUseCase::new(
        repos.role_repo.clone(),
        unit_of_work.clone(),
    ));
    let delete_role_use_case = Arc::new(crate::role::operations::DeleteRoleUseCase::new(
        repos.role_repo.clone(),
        unit_of_work.clone(),
    ));
    let application_roles_sdk_state = ApplicationRolesSdkState {
        app_access: app_access.clone(),
        role_repo: repos.role_repo.clone(),
        create_use_case: create_role_use_case,
        delete_use_case: delete_role_use_case,
    };

    let sync_dispatch_pools_use_case = Arc::new(
        crate::dispatch_pool::operations::SyncDispatchPoolsUseCase::new(
            repos.dispatch_pool_repo.clone(),
            unit_of_work.clone(),
        ),
    );

    let sync_roles_use_case = Arc::new(crate::role::operations::SyncRolesUseCase::new(
        repos.role_repo.clone(),
        repos.application_repo.clone(),
        unit_of_work.clone(),
    ));
    let sync_scheduled_jobs_use_case = Arc::new(
        crate::scheduled_job::operations::SyncScheduledJobsUseCase::new(
            repos.scheduled_job_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let openapi_spec_repo = Arc::new(
        crate::application_openapi_spec::repository::OpenApiSpecRepository::new(&repos.pool),
    );
    let sync_openapi_use_case = Arc::new(
        crate::application_openapi_spec::operations::SyncOpenApiSpecUseCase::new(
            openapi_spec_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let sdk_sync_state = SdkSyncState {
        sync_roles_use_case,
        sync_event_types_use_case: sync_event_types_use_case.clone(),
        sync_subscriptions_use_case: sync_subscriptions_use_case.clone(),
        sync_dispatch_pools_use_case: sync_dispatch_pools_use_case.clone(),
        sync_processes_use_case: sync_processes_use_case.clone(),
        sync_scheduled_jobs_use_case,
        sync_openapi_use_case: sync_openapi_use_case.clone(),
        app_access: app_access.clone(),
        trigger_objects: repos.function_trigger_object_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        application_repo: repos.application_repo.clone(),
        client_repo: repos.client_repo.clone(),
        unit_of_work: unit_of_work.clone(),
    };

    let debug_state = DebugState {
        event_repo: repos.event_repo.clone(),
        dispatch_job_repo: repos.dispatch_job_repo.clone(),
    };

    let outbound_credentials = ctx.outbound_credentials.clone();

    let monitoring_state = MonitoringState {
        leader_state: LeaderState::new(uuid::Uuid::new_v4().to_string()),
        circuit_breakers: CircuitBreakerRegistry::new(),
        in_flight: InFlightTracker::new(),
        dispatch_job_repo: repos.dispatch_job_repo.clone(),
        pool: repos.pool.clone(),
        start_time: std::time::Instant::now(),
    };

    let bff_dashboard_state = crate::shared::bff_dashboard_api::BffDashboardState {
        pool: repos.pool.clone(),
    };

    PlatformRoutes {
        filter_options: filter_options_state,
        monitoring: monitoring_state,
        bff_dashboard: bff_dashboard_state,
        debug: debug_state,
        me: me_state,
        well_known: well_known_state,
        client_selection: client_selection_state,
        application_roles_sdk: application_roles_sdk_state,
        sdk_sync: sdk_sync_state,
        public: public_api_state,
        // The router's delivery callback. Fail closed as Go does: without
        // FLOWCATALYST_APP_KEY no token can be verified, so it is not mounted.
        dispatch_process: match crate::scheduler::DispatchAuthService::from_env() {
            Some(auth) => Some(DispatchProcessState {
                dispatch_job_repo: repos.dispatch_job_repo.clone(),
                http_client: crate::shared::dispatch_process_api::delivery_http_client(),
                credentials: Some(Arc::new(
                    crate::dispatch_job::delivery_credentials::DeliveryCredentials::new(
                        repos.subscription_repo.clone(),
                        repos.connection_repo.clone(),
                        repos.application_repo.clone(),
                        outbound_credentials.clone(),
                    ),
                )),
                auth,
                client_codes: Some(Arc::new(
                    crate::shared::dispatch_process_api::ClientCodeResolver::new(
                        repos.client_repo.clone(),
                    ),
                )),
            }),
            None => {
                tracing::warn!(
                    "dispatch-processing callback not mounted: FLOWCATALYST_APP_KEY is not set, \
                     so the router's job tokens cannot be verified"
                );
                None
            }
        },
        bff_developer: crate::router::BffDeveloperDeps {
            application_repo: repos.application_repo.clone(),
            openapi_spec_repo: openapi_spec_repo.clone(),
            event_type_repo: repos.event_type_repo.clone(),
            principal_repo: repos.principal_repo.clone(),
            sync_openapi_use_case,
            platform_application_id: ctx.platform_application_id.clone(),
        },
        go_routes: crate::shared::go_routes::GoRoutesState::build(
            repos,
            unit_of_work,
            app_access.clone(),
        ),
        static_dir: ctx.config.static_dir.clone(),
        rate_limit_store: ctx.config.rate_limit_store.clone(),
        rate_limit_policies: ctx.config.rate_limit_policies.clone(),
        ctx,
    }
}
