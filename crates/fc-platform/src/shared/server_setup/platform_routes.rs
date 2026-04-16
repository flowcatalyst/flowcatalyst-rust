//! Shared builder for `PlatformRoutes`.
//!
//! Constructs the ~38 API state structs every binary needs, wiring
//! them to repositories, auth services, and use cases. Binaries provide
//! a `PlatformRoutesConfig` with the 3 known points of variation:
//!
//! 1. Optional `EventDispatchDeps` for `SdkEventsState.dispatch` (only
//!    fc-dev sets this, because it has a queue publisher embedded in
//!    the same process).
//! 2. Whether the OIDC session cookie's `Secure` flag is set — `true`
//!    in production (fc-server), `false` for dev/no-TLS deployments.
//! 3. Optional static asset directory for SPA serving.
//!
//! In addition, external base URLs for the well-known, OIDC login, and
//! password-reset endpoints are passed directly so each binary can read
//! them from env in whatever style it prefers.

use std::sync::Arc;
use tracing::warn;

use crate::api::{
    ApplicationRolesSdkState, ApplicationsState, AuditLogsState, AuthConfigState, AuthState,
    BffEventTypesState, BffRolesState, CircuitBreakerRegistry, ClientSelectionState, ClientsState,
    ConfigAccessState, ConnectionsState, CorsState, DebugState, DispatchJobsState,
    DispatchPoolsState, DispatchProcessState, EmailDomainMappingsState, EventDispatchDeps,
    EventTypesState, EventsState, FilterOptionsState, IdentityProvidersState, InFlightTracker,
    LeaderState, LoginAttemptsState, MeState, MonitoringState, OAuthClientsState, OAuthState,
    OidcLoginApiState, PasswordResetApiState, PlatformConfigState, PrincipalsState, PublicApiState,
    RolesState, SdkAuditBatchState, SdkDispatchJobsState, SdkEventsState,
    SdkSyncState, ServiceAccountsState, SubscriptionsState,
    WellKnownState,
};
use crate::audit::service::AuditService;
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

/// Per-binary configuration for the points where binaries diverge.
pub struct PlatformRoutesConfig {
    /// When set, `SdkEventsState.dispatch` receives these deps so the SDK
    /// batch endpoint can publish directly to the in-process queue.
    /// `None` for server binaries that delegate to the router.
    pub event_dispatch: Option<EventDispatchDeps>,
    /// `Secure` flag for the OIDC session cookie. `true` in production.
    pub session_cookie_secure: bool,
    /// Optional static asset directory for SPA serving.
    pub static_dir: Option<String>,
    /// External base URL for the OIDC login flow (used for absolute redirect
    /// URLs). Binary pre-resolves from env (usually `FC_EXTERNAL_BASE_URL`).
    pub oidc_login_external_base_url: Option<String>,
    /// External base URL for the `.well-known` endpoints (issuer, JWKS).
    pub well_known_external_base_url: String,
    /// External base URL for password-reset email links.
    pub password_reset_external_base_url: String,
}

/// Build a fully-populated `PlatformRoutes` for the three server binaries.
///
/// Returns the struct, not the router — binaries still call `.build()`
/// and add their own middleware/static layers.
pub fn build_platform_routes(
    repos: &Repositories,
    auth: &AuthServices,
    unit_of_work: &Arc<PgUnitOfWork>,
    config: PlatformRoutesConfig,
) -> PlatformRoutes<PgUnitOfWork> {
    // ── Simple states ─────────────────────────────────────────────────────
    let events_state = EventsState { event_repo: repos.event_repo.clone() };
    let dispatch_jobs_state = DispatchJobsState { dispatch_job_repo: repos.dispatch_job_repo.clone() };
    let filter_options_state = FilterOptionsState {
        client_repo: repos.client_repo.clone(),
        event_type_repo: repos.event_type_repo.clone(),
        subscription_repo: repos.subscription_repo.clone(),
        dispatch_pool_repo: repos.dispatch_pool_repo.clone(),
        application_repo: repos.application_repo.clone(),
    };

    // ── Shared use cases (constructed once, shared between states) ────────
    let sync_event_types_use_case = Arc::new(
        crate::event_type::operations::SyncEventTypesUseCase::new(
            repos.event_type_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let event_types_state = EventTypesState {
        event_type_repo: repos.event_type_repo.clone(),
        sync_use_case: sync_event_types_use_case.clone(),
    };

    let audit_service = Arc::new(AuditService::new(repos.audit_log_repo.clone()));
    let clients_state = ClientsState {
        client_repo: repos.client_repo.clone(),
        application_repo: Some(repos.application_repo.clone()),
        application_client_config_repo: Some(repos.application_client_config_repo.clone()),
        audit_service: Some(audit_service.clone()),
    };
    // Password reset emailer — shared between user-initiated /auth/password-reset/request
    // and admin-initiated /api/principals/{id}/send-password-reset.
    let email_service: Arc<dyn crate::shared::email_service::EmailService> =
        Arc::from(crate::shared::email_service::create_email_service());
    let password_reset_emailer = Arc::new(crate::auth::password_reset_api::PasswordResetEmailer {
        password_reset_repo: repos.password_reset_repo.clone(),
        email_service: email_service.clone(),
        unit_of_work: unit_of_work.clone(),
        external_base_url: config.password_reset_external_base_url.clone(),
    });

    let create_user_use_case = Arc::new(
        crate::principal::operations::CreateUserUseCase::new(
            repos.principal_repo.clone(),
            auth.password.clone(),
            unit_of_work.clone(),
        ),
    );
    let grant_client_access_use_case = Arc::new(
        crate::principal::operations::GrantClientAccessUseCase::new(
            repos.principal_repo.clone(),
            repos.client_repo.clone(),
            repos.client_access_grant_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let reset_password_use_case = Arc::new(
        crate::principal::operations::ResetPasswordUseCase::new(
            repos.principal_repo.clone(),
            auth.password.clone(),
            unit_of_work.clone(),
        ),
    );

    let principals_state = PrincipalsState {
        principal_repo: repos.principal_repo.clone(),
        audit_service: Some(audit_service),
        password_service: Some(auth.password.clone()),
        anchor_domain_repo: Some(repos.anchor_domain_repo.clone()),
        client_auth_config_repo: Some(repos.client_auth_config_repo.clone()),
        email_domain_mapping_repo: Some(repos.edm_repo.clone()),
        identity_provider_repo: Some(repos.idp_repo.clone()),
        application_repo: Some(repos.application_repo.clone()),
        app_client_config_repo: Some(repos.application_client_config_repo.clone()),
        password_reset_emailer: Some(password_reset_emailer.clone()),
        create_user_use_case,
        grant_client_access_use_case,
        reset_password_use_case,
    };
    let roles_state = RolesState {
        role_repo: repos.role_repo.clone(),
        application_repo: Some(repos.application_repo.clone()),
    };

    let sync_subscriptions_use_case = Arc::new(
        crate::subscription::operations::SyncSubscriptionsUseCase::new(
            repos.subscription_repo.clone(),
            repos.connection_repo.clone(),
            repos.dispatch_pool_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let subscriptions_state = SubscriptionsState {
        subscription_repo: repos.subscription_repo.clone(),
        sync_use_case: sync_subscriptions_use_case.clone(),
    };

    let oauth_clients_state = OAuthClientsState { oauth_client_repo: repos.oauth_client_repo.clone() };
    let auth_config_state = AuthConfigState {
        anchor_domain_repo: repos.anchor_domain_repo.clone(),
        client_auth_config_repo: repos.client_auth_config_repo.clone(),
        idp_role_mapping_repo: repos.idp_role_mapping_repo.clone(),
        principal_repo: Some(repos.principal_repo.clone()),
    };

    // ── OIDC login, OAuth, Auth states ────────────────────────────────────
    let oidc_login_state = OidcLoginApiState::new(
        repos.anchor_domain_repo.clone(),
        repos.idp_repo.clone(),
        repos.edm_repo.clone(),
        repos.oidc_login_state_repo.clone(),
        auth.oidc_sync.clone(),
        auth.auth.clone(),
        unit_of_work.clone(),
    )
    .with_session_cookie_settings("fc_session", config.session_cookie_secure, "Lax", 86400);
    let encryption_service = EncryptionService::from_env().map(Arc::new);
    let oidc_login_state = if let Some(enc_svc) = encryption_service {
        oidc_login_state.with_encryption_service(enc_svc)
    } else {
        warn!("FLOWCATALYST_APP_KEY not set — OIDC client secrets cannot be decrypted");
        oidc_login_state
    };
    let oidc_login_state = if let Some(url) = config.oidc_login_external_base_url {
        oidc_login_state.with_external_base_url(url)
    } else {
        oidc_login_state
    };

    let embedded_auth_state = AuthState::new(
        auth.auth.clone(),
        repos.principal_repo.clone(),
        auth.password.clone(),
        repos.refresh_token_repo.clone(),
        repos.edm_repo.clone(),
        repos.idp_repo.clone(),
        repos.login_attempt_repo.clone(),
    );
    let oauth_state = OAuthState::new(
        repos.oauth_client_repo.clone(),
        repos.principal_repo.clone(),
        auth.auth.clone(),
        auth.oidc.clone(),
        repos.auth_code_repo.clone(),
        repos.refresh_token_repo.clone(),
        repos.pending_auth_repo.clone(),
        auth.password.clone(),
        repos.login_attempt_repo.clone(),
    );

    let audit_logs_state = AuditLogsState {
        audit_log_repo: repos.audit_log_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
    };

    // ── Service Account use cases ─────────────────────────────────────────
    let create_sa_use_case = Arc::new(CreateServiceAccountUseCase::new(repos.service_account_repo.clone(), unit_of_work.clone()));
    let update_sa_use_case = Arc::new(UpdateServiceAccountUseCase::new(repos.service_account_repo.clone(), unit_of_work.clone()));
    let delete_sa_use_case = Arc::new(DeleteServiceAccountUseCase::new(repos.service_account_repo.clone(), unit_of_work.clone()));
    let assign_roles_use_case = Arc::new(AssignRolesUseCase::new(repos.service_account_repo.clone(), unit_of_work.clone()));
    let regenerate_token_use_case = Arc::new(RegenerateAuthTokenUseCase::new(repos.service_account_repo.clone(), unit_of_work.clone()));
    let regenerate_secret_use_case = Arc::new(RegenerateSigningSecretUseCase::new(repos.service_account_repo.clone(), unit_of_work.clone()));

    // ── Application use cases ─────────────────────────────────────────────
    let create_app_use_case = Arc::new(CreateApplicationUseCase::new(repos.application_repo.clone(), unit_of_work.clone()));
    let update_app_use_case = Arc::new(UpdateApplicationUseCase::new(repos.application_repo.clone(), unit_of_work.clone()));
    let activate_app_use_case = Arc::new(ActivateApplicationUseCase::new(repos.application_repo.clone(), unit_of_work.clone()));
    let deactivate_app_use_case = Arc::new(DeactivateApplicationUseCase::new(repos.application_repo.clone(), unit_of_work.clone()));

    // ── Dispatch Pool use cases ───────────────────────────────────────────
    let create_pool_use_case = Arc::new(CreateDispatchPoolUseCase::new(repos.dispatch_pool_repo.clone(), unit_of_work.clone()));
    let update_pool_use_case = Arc::new(UpdateDispatchPoolUseCase::new(repos.dispatch_pool_repo.clone(), unit_of_work.clone()));
    let archive_pool_use_case = Arc::new(ArchiveDispatchPoolUseCase::new(repos.dispatch_pool_repo.clone(), unit_of_work.clone()));
    let delete_pool_use_case = Arc::new(DeleteDispatchPoolUseCase::new(repos.dispatch_pool_repo.clone(), unit_of_work.clone()));

    // ── Domain states ─────────────────────────────────────────────────────
    let connections_state = ConnectionsState { connection_repo: repos.connection_repo.clone() };
    let add_cors_use_case = Arc::new(
        crate::cors::operations::AddCorsOriginUseCase::new(
            repos.cors_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let delete_cors_use_case = Arc::new(
        crate::cors::operations::DeleteCorsOriginUseCase::new(
            repos.cors_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let cors_state = CorsState {
        cors_repo: repos.cors_repo.clone(),
        add_use_case: add_cors_use_case,
        delete_use_case: delete_cors_use_case,
    };
    let idp_state = IdentityProvidersState { idp_repo: repos.idp_repo.clone() };
    let edm_state = EmailDomainMappingsState {
        edm_repo: repos.edm_repo.clone(),
        idp_repo: repos.idp_repo.clone(),
    };
    let public_api_state = PublicApiState { config_repo: repos.platform_config_repo.clone() };
    let platform_config_state = PlatformConfigState { config_repo: repos.platform_config_repo.clone() };
    let config_access_state = ConfigAccessState { access_repo: repos.platform_config_access_repo.clone() };
    let login_attempts_state = LoginAttemptsState { login_attempt_repo: repos.login_attempt_repo.clone() };
    let me_state = MeState {
        client_repo: repos.client_repo.clone(),
        application_repo: repos.application_repo.clone(),
        app_client_config_repo: repos.application_client_config_repo.clone(),
    };
    let well_known_state = WellKnownState {
        auth_service: auth.auth.clone(),
        external_base_url: config.well_known_external_base_url,
    };
    let client_selection_state = ClientSelectionState {
        principal_repo: repos.principal_repo.clone(),
        client_repo: repos.client_repo.clone(),
        role_repo: repos.role_repo.clone(),
        grant_repo: repos.client_access_grant_repo.clone(),
        auth_service: auth.auth.clone(),
    };
    let application_roles_sdk_state = ApplicationRolesSdkState {
        application_repo: repos.application_repo.clone(),
        role_repo: repos.role_repo.clone(),
    };

    let password_reset_state = PasswordResetApiState {
        principal_repo: repos.principal_repo.clone(),
        password_service: auth.password.clone(),
        unit_of_work: unit_of_work.clone(),
        emailer: password_reset_emailer,
        password_reset_repo: repos.password_reset_repo.clone(),
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
    };
    let service_accounts_state = ServiceAccountsState {
        repo: repos.service_account_repo.clone(),
        oauth_client_repo: repos.oauth_client_repo.clone(),
        create_use_case: create_sa_use_case,
        update_use_case: update_sa_use_case,
        delete_use_case: delete_sa_use_case,
        assign_roles_use_case,
        regenerate_token_use_case,
        regenerate_secret_use_case,
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
        sync_use_case: sync_dispatch_pools_use_case.clone(),
    };

    let sync_roles_use_case = Arc::new(
        crate::role::operations::SyncRolesUseCase::new(
            repos.role_repo.clone(),
            repos.application_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let sync_principals_use_case = Arc::new(
        crate::principal::operations::SyncPrincipalsUseCase::new(
            repos.principal_repo.clone(),
            repos.application_repo.clone(),
            unit_of_work.clone(),
        ),
    );
    let sdk_sync_state = SdkSyncState {
        sync_roles_use_case,
        sync_event_types_use_case: sync_event_types_use_case.clone(),
        sync_subscriptions_use_case: sync_subscriptions_use_case.clone(),
        sync_dispatch_pools_use_case: sync_dispatch_pools_use_case.clone(),
        sync_principals_use_case,
    };

    let sdk_audit_batch_state = SdkAuditBatchState {
        audit_log_repo: repos.audit_log_repo.clone(),
        application_repo: repos.application_repo.clone(),
        client_repo: repos.client_repo.clone(),
    };
    let sdk_dispatch_jobs_state = SdkDispatchJobsState { dispatch_job_repo: repos.dispatch_job_repo.clone() };

    let sdk_events_state = SdkEventsState {
        event_repo: repos.event_repo.clone(),
        dispatch: config.event_dispatch,
    };
    let debug_state = DebugState {
        event_repo: repos.event_repo.clone(),
        dispatch_job_repo: repos.dispatch_job_repo.clone(),
    };

    let bff_roles_state = BffRolesState {
        role_repo: repos.role_repo.clone(),
        application_repo: Some(repos.application_repo.clone()),
        unit_of_work: unit_of_work.clone(),
    };
    let bff_event_types_state = BffEventTypesState {
        event_type_repo: repos.event_type_repo.clone(),
        application_repo: Some(repos.application_repo.clone()),
        sync_use_case: sync_event_types_use_case.clone(),
        unit_of_work: unit_of_work.clone(),
    };

    let monitoring_state = MonitoringState {
        leader_state: LeaderState::new(uuid::Uuid::new_v4().to_string()),
        circuit_breakers: CircuitBreakerRegistry::new(),
        in_flight: InFlightTracker::new(),
        dispatch_job_repo: repos.dispatch_job_repo.clone(),
        start_time: std::time::Instant::now(),
    };

    PlatformRoutes {
        events: events_state,
        event_types: event_types_state,
        dispatch_jobs: dispatch_jobs_state,
        filter_options: filter_options_state,
        clients: clients_state,
        principals: principals_state,
        roles: roles_state,
        subscriptions: subscriptions_state,
        oauth_clients: oauth_clients_state,
        audit_logs: audit_logs_state,
        monitoring: monitoring_state,
        auth: embedded_auth_state,
        bff_roles: bff_roles_state,
        bff_event_types: bff_event_types_state,
        debug: debug_state,
        auth_config: auth_config_state,
        applications: applications_state,
        dispatch_pools: dispatch_pools_state,
        service_accounts: service_accounts_state,
        connections: connections_state,
        cors: cors_state,
        identity_providers: idp_state,
        email_domain_mappings: edm_state,
        platform_config: platform_config_state,
        config_access: config_access_state,
        login_attempts: login_attempts_state,
        me: me_state,
        sdk_events: sdk_events_state,
        sdk_dispatch_jobs: sdk_dispatch_jobs_state,
        oidc_login: oidc_login_state,
        oauth: oauth_state,
        well_known: well_known_state,
        client_selection: client_selection_state,
        application_roles_sdk: application_roles_sdk_state,
        sdk_sync: sdk_sync_state,
        sdk_audit_batch: sdk_audit_batch_state,
        public: public_api_state,
        password_reset: password_reset_state,
        dispatch_process: Some(DispatchProcessState {
            dispatch_job_repo: repos.dispatch_job_repo.clone(),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
        }),
        static_dir: config.static_dir,
    }
}
