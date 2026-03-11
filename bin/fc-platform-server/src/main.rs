//! FlowCatalyst Platform Server
//!
//! Production server for platform REST APIs:
//! - BFF APIs: events, event-types, dispatch-jobs, filter-options
//! - Admin APIs: clients, principals, roles, subscriptions, etc.
//! - Monitoring APIs: health, metrics, leader status
//!
//! ## Environment Variables
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_API_PORT` | `3000` | HTTP API port |
//! | `FC_METRICS_PORT` | `9090` | Metrics/health port |
//! | `FC_DATABASE_URL` | `postgresql://localhost:5432/flowcatalyst` | PostgreSQL connection URL |
//! | `FC_JWT_PRIVATE_KEY_PATH` | - | Path to RSA private key PEM |
//! | `FC_JWT_PUBLIC_KEY_PATH` | - | Path to RSA public key PEM |
//! | `FLOWCATALYST_JWT_PRIVATE_KEY` | - | RSA private key PEM content (env) |
//! | `FLOWCATALYST_JWT_PUBLIC_KEY` | - | RSA public key PEM content (env) |
//! | `FC_JWT_ISSUER` | `flowcatalyst` | JWT issuer claim |
//! | `RUST_LOG` | `info` | Log level |

use std::sync::Arc;
use axum::{
    routing::get,
    response::Json,
    Router,
};
use utoipa_axum::router::OpenApiRouter;
use tower_http::cors::{CorsLayer, AllowOrigin};
use tower_http::trace::TraceLayer;
use tower_http::services::{ServeDir, ServeFile};
use axum::http::{Method, HeaderValue, header as http_header};
use anyhow::Result;
use tracing::info;
use tokio::{signal, net::TcpListener};
use utoipa_swagger_ui::SwaggerUi;

use fc_platform::service::{AuthService, AuthConfig, AuthorizationService, AuditService};
use fc_platform::api::middleware::{AppState, AuthLayer};
use fc_platform::api::{
    EventsState, events_router,
    EventTypesState, event_types_router,
    DispatchJobsState, dispatch_jobs_router,
    FilterOptionsState, filter_options_router, event_type_filters_router,
    ClientsState, clients_router,
    PrincipalsState, principals_router,
    RolesState, roles_router,
    SubscriptionsState, subscriptions_router,
    OAuthClientsState, oauth_clients_router,
    AuthConfigState, anchor_domains_router, client_auth_configs_router, idp_role_mappings_router,
    AuditLogsState, audit_logs_router,
    ApplicationsState, applications_router,
    DispatchPoolsState, dispatch_pools_router,
    MonitoringState, monitoring_router, LeaderState, CircuitBreakerRegistry, InFlightTracker,
    DebugState, debug_events_router, debug_dispatch_jobs_router,
    AuthState, auth_router,
    OAuthState, oauth_router,
    platform_config_router,
    ServiceAccountsState, service_accounts_router,
    // New domain APIs
    ConnectionsState, connections_router,
    CorsState, cors_router,
    IdentityProvidersState, identity_providers_router,
    EmailDomainMappingsState, email_domain_mappings_router,
    PlatformConfigState, admin_platform_config_router,
    ConfigAccessState, config_access_router,
    LoginAttemptsState, login_attempts_router,
    MeState, me_router,
    SdkEventsState, sdk_events_batch_router,
    SdkClientsState, sdk_clients_router,
    SdkPrincipalsState, sdk_principals_router,
    SdkRolesState, sdk_roles_router,
    WellKnownState, well_known_router,
    ClientSelectionState, client_selection_router,
    ApplicationRolesSdkState, application_roles_sdk_router,
    public_router,
    PasswordResetApiState, password_reset_router,
    SdkSyncState, sdk_sync_router,
    SdkAuditBatchState, sdk_audit_batch_router,
    SdkDispatchJobsState, sdk_dispatch_jobs_batch_router,
    BffRolesState, bff_roles_router,
    BffEventTypesState, bff_event_types_router,
};
use fc_platform::repository::{
    EventRepository, EventTypeRepository, DispatchJobRepository, DispatchPoolRepository,
    SubscriptionRepository, ServiceAccountRepository, PrincipalRepository, ClientRepository,
    ApplicationRepository, RoleRepository, OAuthClientRepository,
    AnchorDomainRepository, ClientAuthConfigRepository, ClientAccessGrantRepository, IdpRoleMappingRepository,
    AuditLogRepository, ApplicationClientConfigRepository, OidcLoginStateRepository, RefreshTokenRepository,
    AuthorizationCodeRepository,
    // New repos
    ConnectionRepository, CorsOriginRepository, IdentityProviderRepository,
    EmailDomainMappingRepository, PlatformConfigRepository, PlatformConfigAccessRepository,
    LoginAttemptRepository,
    PasswordResetTokenRepository,
};
use fc_platform::usecase::PgUnitOfWork;
use fc_platform::operations::{
    // Service Account use cases
    CreateServiceAccountUseCase, UpdateServiceAccountUseCase, DeleteServiceAccountUseCase,
    AssignRolesUseCase, RegenerateAuthTokenUseCase, RegenerateSigningSecretUseCase,
    // Application use cases
    CreateApplicationUseCase, UpdateApplicationUseCase,
    ActivateApplicationUseCase, DeactivateApplicationUseCase,
    // Dispatch Pool use cases
    CreateDispatchPoolUseCase, UpdateDispatchPoolUseCase,
    ArchiveDispatchPoolUseCase, DeleteDispatchPoolUseCase,
};
use fc_platform::service::PasswordService;
use fc_platform::service::OidcSyncService;
use fc_platform::service::OidcService;
use fc_platform::api::{OidcLoginApiState, oidc_login_router};
use fc_platform::seed::DevDataSeeder;


fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_or_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> Result<()> {
    fc_common::logging::init_logging("fc-platform-server");

    info!("Starting FlowCatalyst Platform Server");

    // Configuration from environment
    let api_port: u16 = env_or_parse("FC_API_PORT", 3000);
    let metrics_port: u16 = env_or_parse("FC_METRICS_PORT", 9090);
    let database_url = env_or("FC_DATABASE_URL", "postgresql://localhost:5432/flowcatalyst");
    let jwt_issuer = env_or("FC_JWT_ISSUER", "flowcatalyst");

    // Connect to PostgreSQL
    info!("Connecting to PostgreSQL...");
    let pg_db = fc_platform::shared::database::create_connection(&database_url).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    // Run PostgreSQL migrations
    fc_platform::shared::database::run_migrations(&pg_db).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL migrations failed: {}", e))?;

    // Seed development data if in dev mode
    let dev_mode = std::env::var("FC_DEV_MODE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    if dev_mode {
        let seeder = DevDataSeeder::new(pg_db.clone());
        if let Err(e) = seeder.seed().await {
            tracing::warn!("Dev data seeding skipped (data may already exist): {}", e);
        }
    }

    // Initialize repositories
    let event_repo = Arc::new(EventRepository::new(&pg_db));
    let event_type_repo = Arc::new(EventTypeRepository::new(&pg_db));
    let dispatch_job_repo = Arc::new(DispatchJobRepository::new(&pg_db));
    let dispatch_pool_repo = Arc::new(DispatchPoolRepository::new(&pg_db));
    let subscription_repo = Arc::new(SubscriptionRepository::new(&pg_db));
    let service_account_repo = Arc::new(ServiceAccountRepository::new(&pg_db));
    let principal_repo = Arc::new(PrincipalRepository::new(&pg_db));
    let client_repo = Arc::new(ClientRepository::new(&pg_db));
    let application_repo = Arc::new(ApplicationRepository::new(&pg_db));
    let role_repo = Arc::new(RoleRepository::new(&pg_db));
    let oauth_client_repo = Arc::new(OAuthClientRepository::new(&pg_db));
    let anchor_domain_repo = Arc::new(AnchorDomainRepository::new(&pg_db));
    let client_auth_config_repo = Arc::new(ClientAuthConfigRepository::new(&pg_db));
    let client_access_grant_repo = Arc::new(ClientAccessGrantRepository::new(&pg_db));
    let idp_role_mapping_repo = Arc::new(IdpRoleMappingRepository::new(&pg_db));
    let audit_log_repo = Arc::new(AuditLogRepository::new(&pg_db));
    let application_client_config_repo = Arc::new(ApplicationClientConfigRepository::new(&pg_db));
    let oidc_login_state_repo = Arc::new(OidcLoginStateRepository::new(&pg_db));
    let refresh_token_repo = Arc::new(RefreshTokenRepository::new(&pg_db));
    let auth_code_repo = Arc::new(AuthorizationCodeRepository::new(&pg_db));
    // New domain repositories
    let connection_repo = Arc::new(ConnectionRepository::new(&pg_db));
    let cors_repo = Arc::new(CorsOriginRepository::new(&pg_db));
    let idp_repo = Arc::new(IdentityProviderRepository::new(&pg_db));
    let edm_repo = Arc::new(EmailDomainMappingRepository::new(&pg_db));
    let platform_config_repo = Arc::new(PlatformConfigRepository::new(&pg_db));
    let platform_config_access_repo = Arc::new(PlatformConfigAccessRepository::new(&pg_db));
    let login_attempt_repo = Arc::new(LoginAttemptRepository::new(&pg_db));
    let password_reset_repo = Arc::new(PasswordResetTokenRepository::new(&pg_db));
    let pending_auth_repo = Arc::new(fc_platform::PendingAuthRepository::new(&pg_db));
    info!("Repositories initialized");

    // Load CORS allowed origins from database into a shared cache
    let cors_origins_cache: Arc<std::sync::RwLock<std::collections::HashSet<String>>> =
        Arc::new(std::sync::RwLock::new(std::collections::HashSet::new()));
    {
        match cors_repo.get_allowed_origins().await {
            Ok(origins) => {
                let mut cache = cors_origins_cache.write().unwrap();
                for origin in origins {
                    cache.insert(origin);
                }
                info!(count = cache.len(), "CORS origins loaded from database");
            }
            Err(e) => {
                tracing::warn!("Failed to load CORS origins: {}", e);
            }
        }
    }
    // Spawn background task to refresh CORS origins every 60 seconds
    {
        let cache = cors_origins_cache.clone();
        let cors_repo_bg = CorsOriginRepository::new(&pg_db);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.tick().await; // skip first immediate tick
            loop {
                interval.tick().await;
                match cors_repo_bg.get_allowed_origins().await {
                    Ok(origins) => {
                        let mut c = cache.write().unwrap();
                        c.clear();
                        for origin in origins {
                            c.insert(origin);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to refresh CORS origins: {}", e);
                    }
                }
            }
        });
    }

    // Sync code-defined roles to database (always, not just in dev mode)
    {
        let role_sync = fc_platform::service::RoleSyncService::new(
            fc_platform::repository::RoleRepository::new(&pg_db)
        );
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            tracing::warn!("Role sync failed: {}", e);
        }
    }

    // Initialize auth (load or generate RSA keys)
    let private_key_path = std::env::var("FC_JWT_PRIVATE_KEY_PATH").ok();
    let public_key_path = std::env::var("FC_JWT_PUBLIC_KEY_PATH").ok();

    let (private_key, public_key) = AuthConfig::load_or_generate_rsa_keys(
        private_key_path.as_deref(),
        public_key_path.as_deref(),
    )?;

    // Load previous public key for JWT key rotation (optional)
    let previous_public_key = std::env::var("FC_JWT_PUBLIC_KEY_PATH_PREVIOUS").ok()
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .or_else(|| std::env::var("FLOWCATALYST_JWT_PUBLIC_KEY_PREVIOUS").ok());

    let auth_config = AuthConfig {
        rsa_private_key: Some(private_key),
        rsa_public_key: Some(public_key),
        rsa_public_key_previous: previous_public_key,
        secret_key: String::new(),
        issuer: jwt_issuer,
        audience: "flowcatalyst".to_string(),
        access_token_expiry_secs: env_or_parse("FC_ACCESS_TOKEN_EXPIRY_SECS", 3600),
        session_token_expiry_secs: env_or_parse("FC_SESSION_TOKEN_EXPIRY_SECS", 28800),
        refresh_token_expiry_secs: env_or_parse("FC_REFRESH_TOKEN_EXPIRY_SECS", 86400 * 30),
    };
    let auth_service = Arc::new(AuthService::new(auth_config));
    let authz_service = Arc::new(AuthorizationService::new(role_repo.clone()));
    let password_service = Arc::new(PasswordService::default());
    let oidc_sync_service = Arc::new(OidcSyncService::new(
        principal_repo.clone(),
        idp_role_mapping_repo.clone(),
    ));
    let oidc_service = Arc::new(OidcService::new());
    info!("Auth services initialized");

    // Create AppState
    let app_state = AppState {
        auth_service: auth_service.clone(),
        authz_service,
    };

    // Build API states
    let events_state = EventsState { event_repo: event_repo.clone() };
    let sync_event_types_use_case = Arc::new(fc_platform::event_type::operations::SyncEventTypesUseCase::new(event_type_repo.clone()));
    let event_types_state = EventTypesState { event_type_repo: event_type_repo.clone(), sync_use_case: sync_event_types_use_case.clone() };
    let dispatch_jobs_state = DispatchJobsState { dispatch_job_repo: dispatch_job_repo.clone() };
    let sdk_events_state = SdkEventsState { event_repo: event_repo.clone() };
    let sdk_clients_state = SdkClientsState { client_repo: client_repo.clone() };
    let sdk_principals_state = SdkPrincipalsState { principal_repo: principal_repo.clone() };
    let sdk_roles_state = SdkRolesState {
        role_repo: role_repo.clone(),
        application_repo: application_repo.clone(),
    };
    let debug_state = DebugState {
        event_repo,
        dispatch_job_repo: dispatch_job_repo.clone(),
    };
    let filter_options_state = FilterOptionsState {
        client_repo: client_repo.clone(),
        event_type_repo: event_type_repo.clone(),
        subscription_repo: subscription_repo.clone(),
        dispatch_pool_repo: dispatch_pool_repo.clone(),
        application_repo: application_repo.clone(),
    };
    let audit_service = Arc::new(AuditService::new(audit_log_repo.clone()));
    let clients_state = ClientsState {
        client_repo: client_repo.clone(),
        application_repo: Some(application_repo.clone()),
        application_client_config_repo: Some(application_client_config_repo.clone()),
        audit_service: Some(audit_service.clone()),
    };
    let principals_state = PrincipalsState {
        principal_repo: principal_repo.clone(),
        audit_service: Some(audit_service),
        password_service: None,
        anchor_domain_repo: Some(anchor_domain_repo.clone()),
        client_auth_config_repo: Some(client_auth_config_repo.clone()),
        application_repo: Some(application_repo.clone()),
        app_client_config_repo: Some(application_client_config_repo.clone()),
    };
    let roles_state = RolesState { role_repo: role_repo.clone(), application_repo: Some(application_repo.clone()) };
    let sync_subscriptions_use_case = Arc::new(fc_platform::subscription::operations::SyncSubscriptionsUseCase::new(
        subscription_repo.clone(), connection_repo.clone(), dispatch_pool_repo.clone(),
    ));
    let subscriptions_state = SubscriptionsState { subscription_repo, sync_use_case: sync_subscriptions_use_case.clone() };
    let oauth_clients_state = OAuthClientsState { oauth_client_repo: oauth_client_repo.clone() };
    let auth_config_state = AuthConfigState {
        anchor_domain_repo: anchor_domain_repo.clone(),
        client_auth_config_repo: client_auth_config_repo.clone(),
        idp_role_mapping_repo: idp_role_mapping_repo.clone(),
        principal_repo: Some(principal_repo.clone()),
    };
    // Create UnitOfWork for atomic commits with events and audit logs
    let unit_of_work = Arc::new(PgUnitOfWork::new(pg_db.clone()));

    let external_base_url = std::env::var("FC_EXTERNAL_BASE_URL").ok();
    let oidc_login_state = OidcLoginApiState::new(
        anchor_domain_repo,
        idp_repo.clone(),
        edm_repo.clone(),
        oidc_login_state_repo,
        oidc_sync_service,
        auth_service.clone(),
        unit_of_work.clone(),
    ).with_session_cookie_settings("fc_session", false, "Lax", 86400);
    let oidc_login_state = if let Some(url) = external_base_url {
        oidc_login_state.with_external_base_url(url)
    } else {
        oidc_login_state
    };
    let embedded_auth_state = AuthState::new(
        auth_service.clone(),
        principal_repo.clone(),
        password_service.clone(),
        refresh_token_repo.clone(),
        edm_repo.clone(),
        idp_repo.clone(),
        login_attempt_repo.clone(),
    );
    let oauth_state = OAuthState::new(
        oauth_client_repo.clone(),
        principal_repo.clone(),
        auth_service.clone(),
        oidc_service,
        auth_code_repo,
        refresh_token_repo,
        pending_auth_repo,
        password_service.clone(),
    );
    let audit_logs_state = AuditLogsState { audit_log_repo: audit_log_repo.clone(), principal_repo: principal_repo.clone() };

    // Create Service Account use cases
    let create_sa_use_case = Arc::new(CreateServiceAccountUseCase::new(
        service_account_repo.clone(),
        unit_of_work.clone(),
    ));
    let update_sa_use_case = Arc::new(UpdateServiceAccountUseCase::new(
        service_account_repo.clone(),
        unit_of_work.clone(),
    ));
    let delete_sa_use_case = Arc::new(DeleteServiceAccountUseCase::new(
        service_account_repo.clone(),
        unit_of_work.clone(),
    ));
    let assign_roles_use_case = Arc::new(AssignRolesUseCase::new(
        service_account_repo.clone(),
        unit_of_work.clone(),
    ));
    let regenerate_token_use_case = Arc::new(RegenerateAuthTokenUseCase::new(
        service_account_repo.clone(),
        unit_of_work.clone(),
    ));
    let regenerate_secret_use_case = Arc::new(RegenerateSigningSecretUseCase::new(
        service_account_repo.clone(),
        unit_of_work.clone(),
    ));

    // Create Application use cases
    let create_app_use_case = Arc::new(CreateApplicationUseCase::new(
        application_repo.clone(),
        unit_of_work.clone(),
    ));
    let update_app_use_case = Arc::new(UpdateApplicationUseCase::new(
        application_repo.clone(),
        unit_of_work.clone(),
    ));
    let activate_app_use_case = Arc::new(ActivateApplicationUseCase::new(
        application_repo.clone(),
        unit_of_work.clone(),
    ));
    let deactivate_app_use_case = Arc::new(DeactivateApplicationUseCase::new(
        application_repo.clone(),
        unit_of_work.clone(),
    ));

    // Create Dispatch Pool use cases
    let create_pool_use_case = Arc::new(CreateDispatchPoolUseCase::new(
        dispatch_pool_repo.clone(),
        unit_of_work.clone(),
    ));
    let update_pool_use_case = Arc::new(UpdateDispatchPoolUseCase::new(
        dispatch_pool_repo.clone(),
        unit_of_work.clone(),
    ));
    let archive_pool_use_case = Arc::new(ArchiveDispatchPoolUseCase::new(
        dispatch_pool_repo.clone(),
        unit_of_work.clone(),
    ));
    let delete_pool_use_case = Arc::new(DeleteDispatchPoolUseCase::new(
        dispatch_pool_repo.clone(),
        unit_of_work.clone(),
    ));

    // New domain API states (before moves)
    let connections_state = ConnectionsState { connection_repo };
    let cors_state = CorsState { cors_repo };
    let idp_state = IdentityProvidersState { idp_repo: idp_repo.clone() };
    let edm_state = EmailDomainMappingsState { edm_repo, idp_repo };
    let platform_config_state = PlatformConfigState { config_repo: platform_config_repo };
    let config_access_state = ConfigAccessState { access_repo: platform_config_access_repo };
    let login_attempts_state = LoginAttemptsState { login_attempt_repo };
    let me_state = MeState {
        client_repo: client_repo.clone(),
        application_repo: application_repo.clone(),
        app_client_config_repo: application_client_config_repo.clone(),
    };
    let well_known_state = WellKnownState {
        auth_service: auth_service.clone(),
        external_base_url: std::env::var("FC_EXTERNAL_BASE_URL")
            .unwrap_or_else(|_| format!("http://localhost:{}", api_port)),
    };
    let client_selection_state = ClientSelectionState {
        principal_repo: principal_repo.clone(),
        client_repo: client_repo.clone(),
        role_repo: role_repo.clone(),
        grant_repo: client_access_grant_repo,
        auth_service: auth_service.clone(),
    };
    let application_roles_sdk_state = ApplicationRolesSdkState {
        application_repo: application_repo.clone(),
        role_repo: role_repo.clone(),
    };
    let email_service: Arc<dyn fc_platform::shared::email_service::EmailService> =
        Arc::from(fc_platform::shared::email_service::create_email_service());
    let password_reset_state = PasswordResetApiState {
        password_reset_repo,
        principal_repo: principal_repo.clone(),
        password_service: password_service.clone(),
        unit_of_work: unit_of_work.clone(),
        email_service: email_service.clone(),
        external_base_url: std::env::var("FC_EXTERNAL_BASE_URL")
            .unwrap_or_else(|_| format!("http://localhost:{}", api_port)),
    };

    // Build API states with use cases
    let applications_state = ApplicationsState {
        application_repo: application_repo.clone(),
        service_account_repo: service_account_repo.clone(),
        role_repo: role_repo.clone(),
        client_config_repo: application_client_config_repo.clone(),
        client_repo: client_repo.clone(),
        create_use_case: create_app_use_case,
        update_use_case: update_app_use_case,
        activate_use_case: activate_app_use_case,
        deactivate_use_case: deactivate_app_use_case,
    };
    let service_accounts_state = ServiceAccountsState {
        repo: service_account_repo,
        create_use_case: create_sa_use_case,
        update_use_case: update_sa_use_case,
        delete_use_case: delete_sa_use_case,
        assign_roles_use_case,
        regenerate_token_use_case,
        regenerate_secret_use_case,
    };
    let sync_dispatch_pools_use_case = Arc::new(fc_platform::dispatch_pool::operations::SyncDispatchPoolsUseCase::new(dispatch_pool_repo.clone()));
    let dispatch_pools_state = DispatchPoolsState {
        dispatch_pool_repo: dispatch_pool_repo.clone(),
        create_use_case: create_pool_use_case,
        update_use_case: update_pool_use_case,
        archive_use_case: archive_pool_use_case,
        delete_use_case: delete_pool_use_case,
        sync_use_case: sync_dispatch_pools_use_case.clone(),
    };

    // SDK sync state (application-scoped sync endpoints)
    let sync_roles_use_case = Arc::new(fc_platform::role::operations::SyncRolesUseCase::new(role_repo.clone(), application_repo.clone()));
    let sync_principals_use_case = Arc::new(fc_platform::principal::operations::SyncPrincipalsUseCase::new(principal_repo.clone(), application_repo.clone()));
    let sdk_sync_state = SdkSyncState {
        sync_roles_use_case,
        sync_event_types_use_case: sync_event_types_use_case.clone(),
        sync_subscriptions_use_case: sync_subscriptions_use_case.clone(),
        sync_dispatch_pools_use_case: sync_dispatch_pools_use_case.clone(),
        sync_principals_use_case,
    };

    // SDK audit batch state
    let sdk_audit_batch_state = SdkAuditBatchState {
        audit_log_repo: audit_log_repo.clone(),
        application_repo: application_repo.clone(),
        client_repo: client_repo.clone(),
    };

    // SDK dispatch jobs batch state
    let sdk_dispatch_jobs_state = SdkDispatchJobsState {
        dispatch_job_repo: dispatch_job_repo.clone(),
    };

    // BFF states
    let bff_roles_state = BffRolesState {
        role_repo: role_repo.clone(),
        application_repo: Some(application_repo.clone()),
    };
    let bff_event_types_state = BffEventTypesState {
        event_type_repo: event_type_repo.clone(),
        application_repo: Some(application_repo.clone()),
        sync_use_case: sync_event_types_use_case.clone(),
    };

    let monitoring_state = MonitoringState {
        leader_state: LeaderState::new(uuid::Uuid::new_v4().to_string()),
        circuit_breakers: CircuitBreakerRegistry::new(),
        in_flight: InFlightTracker::new(),
        dispatch_job_repo,
        start_time: std::time::Instant::now(),
    };

    // Build platform API router using OpenApiRouter for auto-collected OpenAPI paths
    let (router, mut openapi) = OpenApiRouter::new()
        // BFF APIs (under /bff to match frontend expectations)
        .nest("/bff/events", events_router(events_state))
        .nest("/bff/event-types", event_types_router(event_types_state))
        .nest("/bff/dispatch-jobs", dispatch_jobs_router(dispatch_jobs_state))
        .nest("/bff/filter-options", filter_options_router(filter_options_state.clone()))
        // Admin APIs (under /api/admin to match Java paths)
        .nest("/api/admin/clients", clients_router(clients_state))
        .nest("/api/admin/principals", principals_router(principals_state))
        .nest("/api/admin/roles", roles_router(roles_state))
        .nest("/api/admin/subscriptions", subscriptions_router(subscriptions_state))
        .nest("/api/admin/oauth-clients", oauth_clients_router(oauth_clients_state))
        .nest("/api/admin/audit-logs", audit_logs_router(audit_logs_state))
        // Monitoring APIs
        .nest("/api/monitoring", monitoring_router(monitoring_state))
        // Auth APIs
        .nest("/auth", auth_router(embedded_auth_state))
        .split_for_parts();

    // Add missing schemas that are referenced but not auto-collected (e.g., from #[serde(flatten)])
    use utoipa::openapi::{ObjectBuilder, schema::Type};
    if let Some(components) = openapi.components.as_mut() {
        // PaginationParams is used in query params with #[serde(flatten)]
        components.schemas.insert(
            "PaginationParams".to_string(),
            ObjectBuilder::new()
                .property("page", ObjectBuilder::new().schema_type(Type::Integer))
                .property("limit", ObjectBuilder::new().schema_type(Type::Integer))
                .into(),
        );
    }

    // Update OpenAPI info
    openapi.info.title = "FlowCatalyst Platform API".to_string();
    openapi.info.version = "1.0.0".to_string();
    openapi.info.description = Some("REST APIs for events, subscriptions, and administration".to_string());

    // Add routes that don't use OpenApiRouter (generic routers, legacy routers)
    let app = Router::new()
        .merge(router)
        // Routes that return regular Router (not collected in OpenAPI)
        .nest("/bff/event-types/filters", event_type_filters_router(filter_options_state))
        .nest("/bff/roles", bff_roles_router(bff_roles_state).into())
        .nest("/bff/event-types", bff_event_types_router(bff_event_types_state).into())
        .nest("/bff/debug/events", debug_events_router(debug_state.clone()))
        .nest("/bff/debug/dispatch-jobs", debug_dispatch_jobs_router(debug_state))
        .nest("/api/admin/anchor-domains", anchor_domains_router(auth_config_state.clone()))
        .nest("/api/admin/auth-configs", client_auth_configs_router(auth_config_state.clone()))
        .nest("/api/admin/idp-role-mappings", idp_role_mappings_router(auth_config_state))
        .nest("/api/admin/applications", applications_router(applications_state))
        .nest("/api/admin/dispatch-pools", dispatch_pools_router(dispatch_pools_state))
        .nest("/api/admin/service-accounts", service_accounts_router(service_accounts_state))
        // New domain routes
        .nest("/api/admin/connections", connections_router(connections_state).into())
        .nest("/api/admin/platform/cors", cors_router(cors_state))
        .nest("/api/admin/identity-providers", identity_providers_router(idp_state))
        .nest("/api/admin/email-domain-mappings", email_domain_mappings_router(edm_state).into())
        .nest("/api/admin/config", admin_platform_config_router(platform_config_state).into())
        .nest("/api/admin/config-access", config_access_router(config_access_state).into())
        .nest("/api/admin/login-attempts", login_attempts_router(login_attempts_state))
        .nest("/api/me", me_router(me_state))
        .nest("/api/sdk/events", sdk_events_batch_router(sdk_events_state))
        .nest("/api/sdk/clients", sdk_clients_router(sdk_clients_state))
        .nest("/api/sdk/principals", sdk_principals_router(sdk_principals_state))
        .nest("/api/sdk/roles", sdk_roles_router(sdk_roles_state))
        .nest("/api/sdk/dispatch-jobs", sdk_dispatch_jobs_batch_router(sdk_dispatch_jobs_state))
        .nest("/auth", oidc_login_router(oidc_login_state))
        .nest("/oauth", oauth_router(oauth_state))
        .nest("/.well-known", well_known_router(well_known_state))
        .nest("/auth/client", client_selection_router(client_selection_state))
        .nest("/api/applications", application_roles_sdk_router(application_roles_sdk_state))
        .nest("/api/applications", sdk_sync_router(sdk_sync_state))
        .nest("/api/audit-logs", sdk_audit_batch_router(sdk_audit_batch_state))
        .nest("/api/config", platform_config_router())
        // Public routes (no auth required)
        .nest("/api/public", public_router())
        .nest("/auth/password-reset", password_reset_router(password_reset_state))
        // Health check on API port (for load balancers / K8s probes)
        .route("/health", get(health_handler))
        // OpenAPI / Swagger UI with auto-collected paths
        .merge(SwaggerUi::new("/swagger-ui").url("/q/openapi", openapi))
        // Auth middleware
        .layer(AuthLayer::new(app_state))
        .layer(TraceLayer::new_for_http())
        .layer({
            let cache = cors_origins_cache.clone();
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _parts| {
                    let origin_str = match origin.to_str() {
                        Ok(s) => s,
                        Err(_) => return false,
                    };
                    let origins = cache.read().unwrap();
                    // Check exact match first
                    if origins.contains(origin_str) {
                        return true;
                    }
                    // Check wildcard patterns (e.g., https://*.example.com)
                    for pattern in origins.iter() {
                        if pattern.contains('*') {
                            let regex_str = format!(
                                "^{}$",
                                regex::escape(pattern).replace(r"\*", "[a-zA-Z0-9-]+")
                            );
                            if let Ok(re) = regex::Regex::new(&regex_str) {
                                if re.is_match(origin_str) {
                                    return true;
                                }
                            }
                        }
                    }
                    false
                }))
                .allow_methods([
                    Method::GET, Method::POST, Method::PUT, Method::PATCH,
                    Method::DELETE, Method::OPTIONS, Method::HEAD,
                ])
                .allow_headers([
                    http_header::AUTHORIZATION,
                    http_header::CONTENT_TYPE,
                    http_header::ACCEPT,
                    http_header::ORIGIN,
                    http_header::HeaderName::from_static("x-requested-with"),
                    http_header::HeaderName::from_static("x-client-id"),
                ])
                .allow_credentials(true)
                .max_age(std::time::Duration::from_secs(86400))
        });

    // Static frontend serving (SPA fallback)
    let app = if let Ok(static_dir) = std::env::var("FC_STATIC_DIR") {
        let index_path = std::path::PathBuf::from(&static_dir).join("index.html");
        info!(dir = %static_dir, "Serving static frontend files");
        app.fallback_service(
            ServeDir::new(&static_dir).not_found_service(ServeFile::new(index_path))
        )
    } else {
        app
    };

    // Start API server
    let api_addr = format!("0.0.0.0:{}", api_port);
    info!("API server listening on http://{}", api_addr);

    let api_listener = TcpListener::bind(&api_addr).await?;
    let api_task = tokio::spawn(async move {
        axum::serve(api_listener, app).await.unwrap();
    });

    // Start metrics server
    let metrics_addr = format!("0.0.0.0:{}", metrics_port);
    info!("Metrics server listening on http://{}/metrics", metrics_addr);

    let metrics_app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler));

    let metrics_listener = TcpListener::bind(&metrics_addr).await?;
    let metrics_task = tokio::spawn(async move {
        axum::serve(metrics_listener, metrics_app).await.unwrap();
    });

    info!("FlowCatalyst Platform Server started");
    info!("Press Ctrl+C to shutdown");

    // Wait for shutdown
    shutdown_signal().await;
    info!("Shutdown signal received...");

    api_task.abort();
    metrics_task.abort();

    info!("FlowCatalyst Platform Server shutdown complete");
    Ok(())
}

async fn metrics_handler() -> &'static str {
    "# HELP fc_platform_up Platform is up\n# TYPE fc_platform_up gauge\nfc_platform_up 1\n"
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "UP",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn ready_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "READY"
    }))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
