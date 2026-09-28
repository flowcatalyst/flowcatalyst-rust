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
    ApplicationRolesSdkState, ApplicationsState, AuthConfigState, AuthState,
    CircuitBreakerRegistry, ClientSelectionState, ConfigAccessState, DebugState,
    DispatchPoolsState, DispatchProcessState, EmailDomainMappingsState, FilterOptionsState,
    InFlightTracker, LeaderState, MeState, MonitoringState, OAuthClientsState, OAuthState,
    OidcLoginApiState, PasswordResetApiState, PlatformConfigState, PrincipalsState, PublicApiState,
    SdkSyncState, ServiceAccountsState, WellKnownState,
};
use crate::audit::service::AuditService;
use crate::auth::session_cookie::SessionCookieConfig;
use crate::operations::{
    ActivateApplicationUseCase, ArchiveDispatchPoolUseCase, AssignRolesUseCase,
    CreateApplicationUseCase, CreateDispatchPoolUseCase, CreateServiceAccountUseCase,
    DeactivateApplicationUseCase, DeleteDispatchPoolUseCase, DeleteServiceAccountUseCase,
    RegenerateAuthTokenUseCase, RegenerateSigningSecretUseCase, UpdateApplicationUseCase,
    UpdateDispatchPoolUseCase, UpdateServiceAccountUseCase,
};
use crate::repository::Repositories;
use crate::router::PlatformRoutes;
use crate::shared::encryption_service::EncryptionService;
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
) -> PlatformRoutes<PgUnitOfWork> {
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

    let audit_service = Arc::new(AuditService::new(repos.audit_log_repo.clone()));
    let email_service = ctx.email_service.clone();
    let password_reset_emailer = ctx.password_reset_emailer.clone();

    let create_user_use_case = Arc::new(crate::principal::operations::CreateUserUseCase::new(
        repos.principal_repo.clone(),
        auth.password.clone(),
        unit_of_work.clone(),
    ));
    let grant_client_access_use_case =
        Arc::new(crate::principal::operations::GrantClientAccessUseCase::new(
            repos.principal_repo.clone(),
            repos.client_repo.clone(),
            repos.client_access_grant_repo.clone(),
            unit_of_work.clone(),
        ));
    let reset_password_use_case =
        Arc::new(crate::principal::operations::ResetPasswordUseCase::new(
            repos.principal_repo.clone(),
            auth.password.clone(),
            unit_of_work.clone(),
        ));
    let activate_user_use_case = Arc::new(crate::principal::operations::ActivateUserUseCase::new(
        repos.principal_repo.clone(),
        unit_of_work.clone(),
    ));
    let deactivate_user_use_case =
        Arc::new(crate::principal::operations::DeactivateUserUseCase::new(
            repos.principal_repo.clone(),
            unit_of_work.clone(),
        ));
    let delete_user_use_case = Arc::new(crate::principal::operations::DeleteUserUseCase::new(
        repos.principal_repo.clone(),
        unit_of_work.clone(),
    ));
    let update_user_use_case = Arc::new(crate::principal::operations::UpdateUserUseCase::new(
        repos.principal_repo.clone(),
        unit_of_work.clone(),
    ));
    let assign_user_roles_use_case =
        Arc::new(crate::principal::operations::AssignUserRolesUseCase::new(
            repos.principal_repo.clone(),
            repos.role_repo.clone(),
            unit_of_work.clone(),
        ));
    let revoke_client_access_use_case = Arc::new(
        crate::principal::operations::RevokeClientAccessUseCase::new(
            repos.principal_repo.clone(),
            repos.client_access_grant_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let assign_app_access_use_case = Arc::new(
        crate::principal::operations::AssignApplicationAccessUseCase::new(
            repos.principal_repo.clone(),
            repos.application_repo.clone(),
            unit_of_work.clone(),
        ),
    );

    let app_access = ctx.app_access.clone();
    let principals_state = PrincipalsState {
        mfa_repo: Arc::new(crate::mfa::MfaRepository::new(&repos.pool)),
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        client_repo: repos.client_repo.clone(),
        audit_service,
        anchor_domain_repo: repos.anchor_domain_repo.clone(),
        email_domain_mapping_repo: repos.edm_repo.clone(),
        identity_provider_repo: repos.idp_repo.clone(),
        application_repo: repos.application_repo.clone(),
        app_client_config_repo: repos.application_client_config_repo.clone(),
        client_access_grant_repo: repos.client_access_grant_repo.clone(),
        password_reset_emailer: password_reset_emailer.clone(),
        new_user_notifier: Some(crate::mfa::notify::Notifier {
            email: email_service.clone(),
            name: crate::mfa::notify::PlatformName {
                configs: Some(repos.platform_config_repo.clone()),
            },
        }),
        create_user_use_case,
        grant_client_access_use_case,
        reset_password_use_case: reset_password_use_case.clone(),
        activate_use_case: activate_user_use_case,
        deactivate_use_case: deactivate_user_use_case,
        delete_use_case: delete_user_use_case,
        update_use_case: update_user_use_case,
        assign_roles_use_case: assign_user_roles_use_case,
        revoke_client_access_use_case,
        assign_app_access_use_case,
        app_access: app_access.clone(),
        unit_of_work: unit_of_work.clone(),
    };

    let sync_subscriptions_use_case = Arc::new(
        crate::subscription::operations::SyncSubscriptionsUseCase::new(
            repos.subscription_repo.clone(),
            repos.connection_repo.clone(),
            repos.dispatch_pool_repo.clone(),
            unit_of_work.clone(),
        ),
    );

    let create_oauth_client_use_case =
        Arc::new(crate::auth::operations::CreateOAuthClientUseCase::new(
            repos.oauth_client_repo.clone(),
            unit_of_work.clone(),
        ));
    let update_oauth_client_use_case =
        Arc::new(crate::auth::operations::UpdateOAuthClientUseCase::new(
            repos.oauth_client_repo.clone(),
            unit_of_work.clone(),
        ));
    let delete_oauth_client_use_case =
        Arc::new(crate::auth::operations::DeleteOAuthClientUseCase::new(
            repos.oauth_client_repo.clone(),
            unit_of_work.clone(),
        ));
    let activate_oauth_client_use_case =
        Arc::new(crate::auth::operations::ActivateOAuthClientUseCase::new(
            repos.oauth_client_repo.clone(),
            unit_of_work.clone(),
        ));
    let deactivate_oauth_client_use_case =
        Arc::new(crate::auth::operations::DeactivateOAuthClientUseCase::new(
            repos.oauth_client_repo.clone(),
            unit_of_work.clone(),
        ));
    let rotate_oauth_client_secret_use_case = Arc::new(
        crate::auth::operations::RotateOAuthClientSecretUseCase::new(
            repos.oauth_client_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let revoke_oauth_client_previous_secret_use_case = Arc::new(
        crate::auth::operations::RevokeOAuthClientPreviousSecretUseCase::new(
            repos.oauth_client_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let oauth_clients_state = OAuthClientsState {
        application_repo: repos.application_repo.clone(),
        oauth_client_repo: repos.oauth_client_repo.clone(),
        portal_apps: Arc::new(crate::portal::repository::PortalAppRepository::new(
            &repos.pool,
        )),
        principal_repo: repos.principal_repo.clone(),
        create_oauth_client_use_case,
        update_oauth_client_use_case,
        delete_oauth_client_use_case,
        activate_oauth_client_use_case,
        deactivate_oauth_client_use_case,
        rotate_oauth_client_secret_use_case,
        revoke_oauth_client_previous_secret_use_case,
    };
    let create_anchor_domain_use_case =
        Arc::new(crate::auth::operations::CreateAnchorDomainUseCase::new(
            repos.anchor_domain_repo.clone(),
            unit_of_work.clone(),
        ));
    let update_anchor_domain_use_case =
        Arc::new(crate::auth::operations::UpdateAnchorDomainUseCase::new(
            repos.anchor_domain_repo.clone(),
            unit_of_work.clone(),
        ));
    let delete_anchor_domain_use_case =
        Arc::new(crate::auth::operations::DeleteAnchorDomainUseCase::new(
            repos.anchor_domain_repo.clone(),
            unit_of_work.clone(),
        ));
    let create_auth_config_use_case =
        Arc::new(crate::auth::operations::CreateAuthConfigUseCase::new(
            repos.client_auth_config_repo.clone(),
            unit_of_work.clone(),
        ));
    let update_auth_config_use_case =
        Arc::new(crate::auth::operations::UpdateAuthConfigUseCase::new(
            repos.client_auth_config_repo.clone(),
            unit_of_work.clone(),
        ));
    let delete_auth_config_use_case =
        Arc::new(crate::auth::operations::DeleteAuthConfigUseCase::new(
            repos.client_auth_config_repo.clone(),
            unit_of_work.clone(),
        ));
    let create_idp_role_mapping_use_case =
        Arc::new(crate::auth::operations::CreateIdpRoleMappingUseCase::new(
            repos.idp_role_mapping_repo.clone(),
            unit_of_work.clone(),
        ));
    let delete_idp_role_mapping_use_case =
        Arc::new(crate::auth::operations::DeleteIdpRoleMappingUseCase::new(
            repos.idp_role_mapping_repo.clone(),
            unit_of_work.clone(),
        ));
    let auth_config_state = AuthConfigState {
        anchor_domain_repo: repos.anchor_domain_repo.clone(),
        client_auth_config_repo: repos.client_auth_config_repo.clone(),
        idp_role_mapping_repo: repos.idp_role_mapping_repo.clone(),
        role_repo: repos.role_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        unit_of_work: unit_of_work.clone(),
        encryption_service: EncryptionService::from_env().map(Arc::new),
        create_anchor_domain_use_case,
        update_anchor_domain_use_case,
        delete_anchor_domain_use_case,
        create_auth_config_use_case,
        update_auth_config_use_case,
        delete_auth_config_use_case,
        create_idp_role_mapping_use_case,
        delete_idp_role_mapping_use_case,
    };

    let session_cookie = ctx.session_cookie.clone();
    let encryption_service = ctx.encryption.clone();
    let secret_resolver = ctx.secret_resolver.clone();
    let oidc_login_state = OidcLoginApiState {
        anchor_domain_repo: repos.anchor_domain_repo.clone(),
        identity_provider_repo: repos.idp_repo.clone(),
        email_domain_mapping_repo: repos.edm_repo.clone(),
        oidc_login_state_repo: repos.oidc_login_state_repo.clone(),
        oidc_sync_service: auth.oidc_sync.clone(),
        auth_service: auth.auth.clone(),
        jwks_cache: ctx.jwks_cache.clone(),
        unit_of_work: unit_of_work.clone(),
        oauth_client_repo: repos.oauth_client_repo.clone(),
        external_base_url: ctx.config.oidc_login_external_base_url.clone(),
        session_cookie: session_cookie.clone(),
        secret_resolver: secret_resolver.clone(),
        password_setup_hint: Some(crate::auth::oidc_login_api::PasswordSetupHint {
            principal_repo: repos.principal_repo.clone(),
            login_attempt_repo: repos.login_attempt_repo.clone(),
            backoff_policy: Arc::new(crate::auth::login_backoff::BackoffPolicy::from_env()),
        }),
    };

    let backoff_policy = ctx.backoff_policy.clone();
    let two_factor = ctx.two_factor.clone();
    let embedded_auth_state = AuthState {
        auth_service: auth.auth.clone(),
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        password_service: auth.password.clone(),
        refresh_token_repo: repos.refresh_token_repo.clone(),
        email_domain_mapping_repo: repos.edm_repo.clone(),
        identity_provider_repo: repos.idp_repo.clone(),
        login_attempt_repo: repos.login_attempt_repo.clone(),
        backoff_policy: backoff_policy.clone(),
        session_cookie: SessionCookieConfig::password_login(ctx.config.session_cookie_secure),
        two_factor: Some(two_factor.clone()),
    };
    let portal_state = ctx.portal.clone();
    let oauth_state = OAuthState {
        service_account_repo: repos.service_account_repo.clone(),
        oauth_client_repo: repos.oauth_client_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        auth_service: auth.auth.clone(),
        auth_code_repo: repos.auth_code_repo.clone(),
        refresh_token_repo: repos.refresh_token_repo.clone(),
        pending_auth_repo: repos.pending_auth_repo.clone(),
        password_service: auth.password.clone(),
        login_attempt_repo: repos.login_attempt_repo.clone(),
        client_token_rate_limit: crate::shared::rate_limit_middleware::IpRateLimiterState::new(
            &crate::shared::rate_limit_middleware::RateLimitConfig::oauth_token_per_client_from_env(
            ),
        ),
        rate_limit_store: ctx.config.rate_limit_store.clone(),
        rate_limit_policies: ctx.config.rate_limit_policies.clone(),
        encryption_service: encryption_service.clone(),
        portal: Some(portal_state.clone()),
    };

    // ── Service Account use cases ─────────────────────────────────────────
    let create_sa_use_case = Arc::new(CreateServiceAccountUseCase::new(
        repos.service_account_repo.clone(),
        repos.client_repo.clone(),
        unit_of_work.clone(),
        encryption_service.clone(),
    ));
    let update_sa_use_case = Arc::new(UpdateServiceAccountUseCase::new(
        repos.service_account_repo.clone(),
        repos.client_repo.clone(),
        unit_of_work.clone(),
        encryption_service.clone(),
    ));
    let delete_sa_use_case = Arc::new(DeleteServiceAccountUseCase::new(
        repos.service_account_repo.clone(),
        unit_of_work.clone(),
    ));
    let assign_roles_use_case = Arc::new(AssignRolesUseCase::new(
        repos.service_account_repo.clone(),
        unit_of_work.clone(),
    ));
    let regenerate_token_use_case = Arc::new(RegenerateAuthTokenUseCase::new(
        repos.service_account_repo.clone(),
        unit_of_work.clone(),
        encryption_service.clone(),
    ));
    let regenerate_secret_use_case = Arc::new(RegenerateSigningSecretUseCase::new(
        repos.service_account_repo.clone(),
        unit_of_work.clone(),
        encryption_service.clone(),
    ));

    // ── Application use cases ─────────────────────────────────────────────
    let create_app_use_case = Arc::new(CreateApplicationUseCase::new(
        repos.application_repo.clone(),
        unit_of_work.clone(),
    ));
    let update_app_use_case = Arc::new(UpdateApplicationUseCase::new(
        repos.application_repo.clone(),
        unit_of_work.clone(),
    ));
    let activate_app_use_case = Arc::new(ActivateApplicationUseCase::new(
        repos.application_repo.clone(),
        unit_of_work.clone(),
    ));
    let deactivate_app_use_case = Arc::new(DeactivateApplicationUseCase::new(
        repos.application_repo.clone(),
        unit_of_work.clone(),
    ));
    let enable_for_client_use_case = Arc::new(
        crate::application::operations::EnableApplicationForClientUseCase::new(
            repos.application_repo.clone(),
            repos.client_repo.clone(),
            repos.application_client_config_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let disable_for_client_use_case = Arc::new(
        crate::application::operations::DisableApplicationForClientUseCase::new(
            repos.application_client_config_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let update_client_config_use_case = Arc::new(
        crate::application::operations::UpdateApplicationClientConfigUseCase::new(
            repos.application_repo.clone(),
            repos.client_repo.clone(),
            repos.application_client_config_repo.clone(),
            unit_of_work.clone(),
        ),
    );

    // ── Dispatch Pool use cases ───────────────────────────────────────────
    let create_pool_use_case = Arc::new(CreateDispatchPoolUseCase::new(
        repos.dispatch_pool_repo.clone(),
        unit_of_work.clone(),
    ));
    let update_pool_use_case = Arc::new(UpdateDispatchPoolUseCase::new(
        repos.dispatch_pool_repo.clone(),
        unit_of_work.clone(),
    ));
    let archive_pool_use_case = Arc::new(ArchiveDispatchPoolUseCase::new(
        repos.dispatch_pool_repo.clone(),
        unit_of_work.clone(),
    ));
    let delete_pool_use_case = Arc::new(DeleteDispatchPoolUseCase::new(
        repos.dispatch_pool_repo.clone(),
        unit_of_work.clone(),
    ));

    let create_edm_use_case = Arc::new(
        crate::email_domain_mapping::operations::CreateEmailDomainMappingUseCase::new(
            repos.edm_repo.clone(),
            repos.idp_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let update_edm_use_case = Arc::new(
        crate::email_domain_mapping::operations::UpdateEmailDomainMappingUseCase::new(
            repos.edm_repo.clone(),
            repos.idp_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let delete_edm_use_case = Arc::new(
        crate::email_domain_mapping::operations::DeleteEmailDomainMappingUseCase::new(
            repos.edm_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let edm_state = EmailDomainMappingsState {
        edm_repo: repos.edm_repo.clone(),
        idp_repo: repos.idp_repo.clone(),
        role_repo: repos.role_repo.clone(),
        create_use_case: create_edm_use_case,
        update_use_case: update_edm_use_case,
        delete_use_case: delete_edm_use_case,
    };
    let public_api_state = PublicApiState {
        config_repo: repos.platform_config_repo.clone(),
        client_repo: repos.client_repo.clone(),
    };
    let set_platform_config_property_use_case = Arc::new(
        crate::platform_config::operations::SetPlatformConfigPropertyUseCase::new(
            repos.platform_config_repo.clone(),
            unit_of_work.clone(),
            encryption_service.clone(),
        ),
    );
    let grant_platform_config_access_use_case = Arc::new(
        crate::platform_config::operations::GrantPlatformConfigAccessUseCase::new(
            repos.platform_config_access_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let revoke_platform_config_access_use_case = Arc::new(
        crate::platform_config::operations::RevokePlatformConfigAccessUseCase::new(
            repos.platform_config_access_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let platform_config_state = PlatformConfigState {
        config_repo: repos.platform_config_repo.clone(),
        access_repo: repos.platform_config_access_repo.clone(),
        app_access: app_access.clone(),
        set_property_use_case: set_platform_config_property_use_case,
    };
    let config_access_state = ConfigAccessState {
        access_repo: repos.platform_config_access_repo.clone(),
        app_access: app_access.clone(),
        grant_access_use_case: grant_platform_config_access_use_case,
        revoke_access_use_case: revoke_platform_config_access_use_case,
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

    let reset_approvals_state = crate::mfa::reset_approval_api::ResetApprovalsState {
        approvals: Arc::new(crate::mfa::reset_approval::ResetApprovalRepository::new(
            &repos.pool,
        )),
        principal_repo: repos.principal_repo.clone(),
        emailer: password_reset_emailer.clone(),
    };
    let password_reset_state = PasswordResetApiState {
        principal_repo: repos.principal_repo.clone(),
        password_service: auth.password.clone(),
        unit_of_work: unit_of_work.clone(),
        emailer: password_reset_emailer.clone(),
        password_reset_repo: repos.password_reset_repo.clone(),
        reset_password_use_case: reset_password_use_case.clone(),
        two_factor: Some(two_factor.clone()),
        refresh_token_repo: repos.refresh_token_repo.clone(),
        rate_limit_store: ctx.config.rate_limit_store.clone(),
        rate_limit_policies: ctx.config.rate_limit_policies.clone(),
    };

    let applications_state = ApplicationsState {
        application_repo: repos.application_repo.clone(),
        service_account_repo: repos.service_account_repo.clone(),
        role_repo: repos.role_repo.clone(),
        client_config_repo: repos.application_client_config_repo.clone(),
        client_repo: repos.client_repo.clone(),
        create_use_case: create_app_use_case,
        update_use_case: update_app_use_case,
        activate_use_case: activate_app_use_case,
        deactivate_use_case: deactivate_app_use_case,
        enable_for_client_use_case,
        disable_for_client_use_case,
        update_client_config_use_case,
        oauth_client_repo: repos.oauth_client_repo.clone(),
        create_oauth_client_use_case: oauth_clients_state.create_oauth_client_use_case.clone(),
        pg_unit_of_work: unit_of_work.clone(),
    };
    let service_accounts_state = ServiceAccountsState {
        repo: repos.service_account_repo.clone(),
        role_repo: repos.role_repo.clone(),
        create_use_case: create_sa_use_case,
        update_use_case: update_sa_use_case,
        delete_use_case: delete_sa_use_case,
        assign_roles_use_case,
        regenerate_token_use_case,
        regenerate_secret_use_case,
        create_oauth_client_use_case: oauth_clients_state.create_oauth_client_use_case.clone(),
        oauth_client_repo: repos.oauth_client_repo.clone(),
        app_access: app_access.clone(),
    };

    let sync_dispatch_pools_use_case = Arc::new(
        crate::dispatch_pool::operations::SyncDispatchPoolsUseCase::new(
            repos.dispatch_pool_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let dispatch_pools_state = DispatchPoolsState {
        dispatch_pool_repo: repos.dispatch_pool_repo.clone(),
        create_use_case: create_pool_use_case,
        update_use_case: update_pool_use_case,
        archive_use_case: archive_pool_use_case,
        delete_use_case: delete_pool_use_case,
        suspend_use_case: Arc::new(
            crate::dispatch_pool::operations::SuspendDispatchPoolUseCase::new(
                repos.dispatch_pool_repo.clone(),
                unit_of_work.clone(),
            ),
        ),
        activate_use_case: Arc::new(
            crate::dispatch_pool::operations::ActivateDispatchPoolUseCase::new(
                repos.dispatch_pool_repo.clone(),
                unit_of_work.clone(),
            ),
        ),
    };

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
    // ── Function registry ─────────────────────────────────────────────────
    // Java reads the FC_FN_DEFAULT_* limits once at startup and refuses to
    // start on a non-positive one (Env.java:600-605).
    let function_limits = crate::function::FunctionLimits::from_env()
        .unwrap_or_else(|e| panic!("invalid function limits: {e}"));
    let function_settings = Arc::new(
        crate::function::settings_repository::FunctionSettingsRepository::new(
            &repos.pool,
            encryption_service.clone(),
        ),
    );
    // FC_FN_ARTIFACT_STORE and FC_FN_SIGNATURES/FC_FN_TRUST_ROOT are resolved
    // once; an unrecognised store or signatures off outside dev mode refuse to
    // start, as in Java (ArtifactBlobStores.configure, Signatures.resolve).
    let function_artifacts = crate::function::artifact::store_from_env()
        .unwrap_or_else(|e| panic!("invalid function artifact store: {e}"));
    let function_signatures = crate::function::artifact::signatures_from_env()
        .unwrap_or_else(|e| panic!("invalid function signature settings: {e}"));
    // FC_FN_POOL_URL is resolved once; a bad template refuses to start, as
    // in Java (Env.java, PoolUrlTemplate).
    let function_pool_url = crate::function::PoolUrlTemplate::from_env()
        .unwrap_or_else(|e| panic!("invalid function pool URL: {e}"));
    let trigger_sync = crate::function::operations::TriggerSync::from_repositories(
        repos,
        function_settings.clone(),
        function_pool_url,
    );
    let functions_state = crate::function::api::FunctionsState {
        functions: repos.function_repo.clone(),
        versions: repos.function_version_repo.clone(),
        hosts: repos.function_host_repo.clone(),
        settings: function_settings.clone(),
        policies: repos.function_policy_repo.clone(),
        domains: repos.function_domain_repo.clone(),
        routes: repos.function_route_repo.clone(),
        trigger_objects: repos.function_trigger_object_repo.clone(),
        app_access: app_access.clone(),
        limits: function_limits,
        ops: crate::function::operations::FunctionOperations {
            functions: repos.function_repo.clone(),
            versions: repos.function_version_repo.clone(),
            applications: repos.application_repo.clone(),
            clients: repos.client_repo.clone(),
            settings: function_settings,
            policies: repos.function_policy_repo.clone(),
            domains: repos.function_domain_repo.clone(),
            routes: repos.function_route_repo.clone(),
            trigger_sync,
            limits: function_limits,
            signatures: function_signatures,
            artifacts: function_artifacts,
            publish_checks: crate::function::operations::PublishChecks {
                event_types: repos.event_type_repo.clone(),
                service_accounts: repos.service_account_repo.clone(),
                versions: repos.function_version_repo.clone(),
                functions: repos.function_repo.clone(),
                domains: repos.function_domain_repo.clone(),
                routes: repos.function_route_repo.clone(),
                hosts: repos.function_host_repo.clone(),
                limits: function_limits,
            },
            unit_of_work: unit_of_work.clone(),
        },
    };

    // The host control plane reads what the function routes write, with the
    // same artifact store and the same credentials resolver (read fresh).
    let function_control_state = crate::function::control_api::FunctionControlState {
        desired: Arc::new(crate::function::desired_state::DesiredStateBuilder {
            functions: repos.function_repo.clone(),
            versions: repos.function_version_repo.clone(),
            hosts: repos.function_host_repo.clone(),
            settings: functions_state.settings.clone(),
            routes: repos.function_route_repo.clone(),
            credentials: outbound_credentials.clone(),
        }),
        functions: repos.function_repo.clone(),
        versions: repos.function_version_repo.clone(),
        hosts: repos.function_host_repo.clone(),
        applications: repos.application_repo.clone(),
        event_types: repos.event_type_repo.clone(),
        events: repos.event_repo.clone(),
        artifacts: functions_state.ops.artifacts.clone(),
        unit_of_work: functions_state.ops.unit_of_work.clone(),
    };

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

    let webauthn_credential_repo =
        Arc::new(crate::webauthn::repository::WebauthnCredentialRepository::new(&repos.pool));
    let webauthn_ceremony_repo = Arc::new(crate::webauthn::WebauthnCeremonyRepository::new(
        &repos.pool,
    ));
    let webauthn_service = Arc::new(
        crate::webauthn::WebauthnService::from_env()
            .expect("FC_WEBAUTHN_RP_ID/FC_WEBAUTHN_ORIGINS misconfigured"),
    );
    let webauthn_state = crate::webauthn::WebauthnApiState {
        credential_repo: webauthn_credential_repo,
        ceremony_repo: webauthn_ceremony_repo,
        principal_repo: repos.principal_repo.clone(),
        email_domain_mapping_repo: repos.edm_repo.clone(),
        login_attempt_repo: repos.login_attempt_repo.clone(),
        webauthn_service,
        auth_service: auth.auth.clone(),
        backoff_policy: backoff_policy.clone(),
        unit_of_work: unit_of_work.clone(),
        session_cookie,
    };

    PlatformRoutes {
        functions: functions_state,
        function_control: function_control_state,
        filter_options: filter_options_state,
        principals: principals_state,
        oauth_clients: oauth_clients_state,
        monitoring: monitoring_state,
        auth: embedded_auth_state,
        bff_dashboard: bff_dashboard_state,
        debug: debug_state,
        auth_config: auth_config_state,
        applications: applications_state,
        dispatch_pools: dispatch_pools_state,
        service_accounts: service_accounts_state,
        email_domain_mappings: edm_state,
        platform_config: platform_config_state,
        config_access: config_access_state,
        me: me_state,
        oidc_login: oidc_login_state,
        oauth: oauth_state,
        well_known: well_known_state,
        client_selection: client_selection_state,
        application_roles_sdk: application_roles_sdk_state,
        sdk_sync: sdk_sync_state,
        public: public_api_state,
        password_reset: password_reset_state,
        portal: portal_state,
        webauthn: webauthn_state,
        reset_approvals: reset_approvals_state,
        developer_credentials: crate::developer_credential::api::DeveloperCredentialsState {
            principal_repo: repos.principal_repo.clone(),
            set_use_case: Arc::new(
                crate::developer_credential::operations::SetDeveloperCredentialUseCase {
                    principal_repo: repos.principal_repo.clone(),
                    unit_of_work: unit_of_work.clone(),
                },
            ),
            revoke_use_case: Arc::new(
                crate::developer_credential::operations::RevokeDeveloperCredentialUseCase {
                    principal_repo: repos.principal_repo.clone(),
                    unit_of_work: unit_of_work.clone(),
                },
            ),
            encryption: encryption_service.clone(),
        },
        account: Arc::new(crate::mfa::AccountState {
            two_factor: two_factor.clone(),
            password_service: auth.password.clone(),
            refresh_token_repo: repos.refresh_token_repo.clone(),
        }),
        two_factor,
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
            auth,
            unit_of_work,
            password_reset_emailer,
            app_access.clone(),
        ),
        static_dir: ctx.config.static_dir.clone(),
        rate_limit_store: ctx.config.rate_limit_store.clone(),
        rate_limit_policies: ctx.config.rate_limit_policies.clone(),
        ctx,
    }
}
