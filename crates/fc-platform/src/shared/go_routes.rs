//! Routes Go serves that the Rust platform lacked (parity run 1,
//! `docs/parity/api-run-1.md` root cause 4). Each group's handlers live in
//! their domain module; this file only builds their states and merges their
//! full-path routers, so `router.rs` and `platform_routes.rs` each carry a
//! single line for all of them.

use std::sync::Arc;
use utoipa_axum::router::OpenApiRouter;

use crate::repository::Repositories;
use crate::usecase::PgUnitOfWork;

/// Every state the Go-parity routes need.
#[derive(Clone)]
pub struct GoRoutesState {
    pub router_config: crate::shared::router_config_api::RouterConfigState,
    pub read_aliases: crate::shared::go_read_aliases_api::ReadAliasesState,
    pub sdk_sync: crate::shared::sdk_sync_go_api::SdkSyncGoState,
}

impl GoRoutesState {
    pub fn build(
        repos: &Repositories,
        uow: &Arc<PgUnitOfWork>,
        app_access: Arc<crate::shared::authorization_service::ApplicationAccessService>,
    ) -> Self {
        // Go resolves the queue settings once at boot and refuses to start
        // on a bad SQS configuration (internal/server/run.go:69).
        let queue_settings = crate::shared::dispatch_queue::QueueSettings::from_env()
            .unwrap_or_else(|e| panic!("dispatch queue settings: {e}"));
        let signing = Arc::new(crate::dispatch_job::signing_guard::SigningGuard::new(
            repos.subscription_repo.clone(),
            repos.connection_repo.clone(),
            repos.service_account_repo.clone(),
            repos.application_repo.clone(),
            repos.principal_repo.clone(),
        ));
        Self {
            sdk_sync: crate::shared::sdk_sync_go_api::SdkSyncGoState {
                app_access: app_access.clone(),
                client_repo: repos.client_repo.clone(),
                sync_connections_use_case: Arc::new(
                    crate::connection::operations::sync::SyncConnectionsUseCase::new(
                        repos.connection_repo.clone(),
                        repos.application_repo.clone(),
                        repos.subscription_repo.clone(),
                        uow.clone(),
                    ),
                ),
                sync_processes_use_case: Arc::new(
                    crate::process::operations::SyncProcessesUseCase::new(
                        repos.process_repo.clone(),
                        uow.clone(),
                    ),
                ),
            },
            read_aliases: crate::shared::go_read_aliases_api::ReadAliasesState {
                events: crate::event::api::EventsState {
                    event_repo: repos.event_repo.clone(),
                    signing: signing.clone(),
                },
                dispatch_jobs: crate::dispatch_job::api::DispatchJobsState {
                    dispatch_job_repo: repos.dispatch_job_repo.clone(),
                    client_repo: repos.client_repo.clone(),
                    signing,
                },
            },
            router_config: crate::shared::router_config_api::RouterConfigState {
                repo: Arc::new(
                    crate::dispatch_pool::router_config_repository::RouterConfigRepository::new(
                        &repos.pool,
                    ),
                ),
                settings: Arc::new(queue_settings),
            },
        }
    }
}

/// All Go-parity routes, at their full paths.
pub fn go_routes_router(state: GoRoutesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .merge(crate::shared::sdk_sync_go_api::sdk_sync_go_router(
            state.sdk_sync,
        ))
        .merge(crate::shared::go_read_aliases_api::read_aliases_router(
            state.read_aliases,
        ))
        .merge(crate::shared::router_config_api::router_config_router(
            state.router_config,
        ))
}
