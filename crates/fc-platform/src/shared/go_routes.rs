//! Routes Go serves that the Rust platform lacked (parity run 1,
//! `docs/parity/api-run-1.md` root cause 4). Each group's handlers live in
//! their domain module; this file only builds their states and merges their
//! full-path routers, so `router.rs` and `platform_routes.rs` each carry a
//! single line for all of them.

use std::sync::Arc;
use utoipa_axum::router::OpenApiRouter;

use crate::repository::Repositories;
use crate::shared::server_setup::AuthServices;
use crate::usecase::PgUnitOfWork;

/// Every state the Go-parity routes need.
#[derive(Clone)]
pub struct GoRoutesState {
    pub role_permissions: crate::role::permission_api::RolePermissionsState,
    pub router_config: crate::shared::router_config_api::RouterConfigState,
    pub edm_lookup: crate::email_domain_mapping::lookup_api::EdmLookupState,
    pub principals: crate::principal::go_api::PrincipalGoState,
    pub applications: crate::application::go_api::ApplicationGoState,
    pub event_types: crate::event_type::go_api::EventTypeGoState,
    pub read_aliases: crate::shared::go_read_aliases_api::ReadAliasesState,
    pub service_account_admin: crate::service_account::admin_api::ServiceAccountAdminState,
    pub client_search: crate::client::search_api::ClientSearchState,
    pub platform_config: crate::platform_config::go_api::GoPlatformConfigState,
}

impl GoRoutesState {
    pub fn build(
        repos: &Repositories,
        auth: &AuthServices,
        uow: &Arc<PgUnitOfWork>,
        emailer: Arc<crate::auth::password_reset_api::PasswordResetEmailer>,
    ) -> Self {
        let encryption =
            crate::shared::encryption_service::EncryptionService::from_env().map(Arc::new);
        // Go resolves the queue settings once at boot and refuses to start
        // on a bad SQS configuration (internal/server/run.go:69).
        let queue_settings = crate::shared::dispatch_queue::QueueSettings::from_env()
            .unwrap_or_else(|e| panic!("dispatch queue settings: {e}"));
        let edm_move_repo = Arc::new(
            crate::email_domain_mapping::provider_move_repository::ProviderMoveRepository::new(
                &repos.pool,
                repos.principal_repo.clone(),
            ),
        );
        let signing = Arc::new(crate::dispatch_job::signing_guard::SigningGuard::new(
            repos.subscription_repo.clone(),
            repos.connection_repo.clone(),
            repos.service_account_repo.clone(),
            repos.application_repo.clone(),
            repos.principal_repo.clone(),
        ));
        Self {
            event_types: crate::event_type::go_api::EventTypeGoState {
                event_type_repo: repos.event_type_repo.clone(),
                add_schema_use_case: Arc::new(
                    crate::event_type::operations::AddSchemaUseCase::new(
                        repos.event_type_repo.clone(),
                        uow.clone(),
                    ),
                ),
                bff: crate::shared::bff_event_types_api::BffEventTypesState {
                    event_type_repo: repos.event_type_repo.clone(),
                    sync_use_case: Arc::new(
                        crate::event_type::operations::SyncEventTypesUseCase::new(
                            repos.event_type_repo.clone(),
                            uow.clone(),
                        ),
                    ),
                    unit_of_work: uow.clone(),
                },
            },
            read_aliases: crate::shared::go_read_aliases_api::ReadAliasesState {
                events: crate::event::api::EventsState {
                    event_repo: repos.event_repo.clone(),
                    signing: signing.clone(),
                },
                dispatch_jobs: crate::dispatch_job::api::DispatchJobsState {
                    dispatch_job_repo: repos.dispatch_job_repo.clone(),
                    signing,
                },
            },
            applications: crate::application::go_api::ApplicationGoState {
                principal_repo: repos.principal_repo.clone(),
                client_config_repo: repos.application_client_config_repo.clone(),
                attach_use_case: Arc::new(
                    crate::application::operations::AttachServiceAccountToApplicationUseCase::new(
                        repos.application_repo.clone(),
                        uow.clone(),
                    ),
                ),
            },
            principals: crate::principal::go_api::PrincipalGoState {
                principal_repo: repos.principal_repo.clone(),
                role_repo: repos.role_repo.clone(),
                edm_repo: repos.edm_repo.clone(),
                idp_repo: repos.idp_repo.clone(),
                client_config_repo: repos.application_client_config_repo.clone(),
                emailer,
                create_user_use_case: Arc::new(
                    crate::principal::operations::CreateUserUseCase::new(
                        repos.principal_repo.clone(),
                        auth.password.clone(),
                        uow.clone(),
                    ),
                ),
                assign_roles_use_case: Arc::new(
                    crate::principal::operations::AssignUserRolesUseCase::new(
                        repos.principal_repo.clone(),
                        repos.role_repo.clone(),
                        uow.clone(),
                    ),
                ),
                set_client_association_use_case: Arc::new(
                    crate::principal::operations::set_client_association::SetClientAssociationUseCase::new(
                        repos.principal_repo.clone(),
                        repos.client_repo.clone(),
                        uow.clone(),
                    ),
                ),
            },
            edm_lookup: crate::email_domain_mapping::lookup_api::EdmLookupState {
                edm_repo: repos.edm_repo.clone(),
                idp_repo: repos.idp_repo.clone(),
                move_use_case: Arc::new(
                    crate::email_domain_mapping::operations::move_provider::MoveMappingToProviderUseCase::new(
                        repos.edm_repo.clone(),
                        repos.idp_repo.clone(),
                        repos.principal_repo.clone(),
                        edm_move_repo,
                        uow.clone(),
                    ),
                ),
            },
            router_config: crate::shared::router_config_api::RouterConfigState {
                repo: Arc::new(
                    crate::dispatch_pool::router_config_repository::RouterConfigRepository::new(
                        &repos.pool,
                    ),
                ),
                settings: Arc::new(queue_settings),
            },
            platform_config: crate::platform_config::go_api::GoPlatformConfigState {
                config_repo: repos.platform_config_repo.clone(),
                access_repo: repos.platform_config_access_repo.clone(),
                encryption: encryption.clone(),
                set_property_use_case: Arc::new(
                    crate::platform_config::operations::SetPlatformConfigPropertyUseCase::new(
                        repos.platform_config_repo.clone(),
                        uow.clone(),
                        encryption,
                    ),
                ),
                grant_access_use_case: Arc::new(
                    crate::platform_config::operations::GrantPlatformConfigAccessUseCase::new(
                        repos.platform_config_access_repo.clone(),
                        uow.clone(),
                    ),
                ),
                revoke_access_use_case: Arc::new(
                    crate::platform_config::operations::RevokePlatformConfigAccessUseCase::new(
                        repos.platform_config_access_repo.clone(),
                        uow.clone(),
                    ),
                ),
            },
            client_search: crate::client::search_api::ClientSearchState {
                client_repo: repos.client_repo.clone(),
            },
            service_account_admin: crate::service_account::admin_api::ServiceAccountAdminState {
                repo: repos.service_account_repo.clone(),
                principal_repo: repos.principal_repo.clone(),
                role_repo: repos.role_repo.clone(),
                auth_service: auth.auth.clone(),
                deactivate_use_case: Arc::new(
                    crate::service_account::operations::DeactivateServiceAccountUseCase::new(
                        repos.service_account_repo.clone(),
                        uow.clone(),
                    ),
                ),
                record_mint_use_case: Arc::new(
                    crate::service_account::operations::mint_token::RecordServiceAccountTokenMintUseCase::new(
                        uow.clone(),
                    ),
                ),
            },
            role_permissions: crate::role::permission_api::RolePermissionsState::new(
                &repos.pool,
                repos.role_repo.clone(),
                uow.clone(),
            ),
        }
    }
}

/// All Go-parity routes, at their full paths.
pub fn go_routes_router(state: GoRoutesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .merge(crate::event_type::go_api::event_type_go_router(
            state.event_types,
        ))
        .merge(crate::shared::go_read_aliases_api::read_aliases_router(
            state.read_aliases,
        ))
        .merge(crate::application::go_api::application_go_router(
            state.applications,
        ))
        .merge(crate::principal::go_api::principal_go_router(
            state.principals,
        ))
        .merge(crate::email_domain_mapping::lookup_api::edm_lookup_router(
            state.edm_lookup,
        ))
        .merge(crate::shared::router_config_api::router_config_router(
            state.router_config,
        ))
        .merge(crate::role::permission_api::role_permissions_router(
            state.role_permissions,
        ))
        .merge(
            crate::service_account::admin_api::service_account_admin_router(
                state.service_account_admin,
            ),
        )
        .merge(crate::client::search_api::client_search_router(
            state.client_search,
        ))
        .merge(crate::platform_config::go_api::go_platform_config_router(
            state.platform_config,
        ))
}
