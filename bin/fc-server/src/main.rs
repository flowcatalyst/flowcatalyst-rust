//! FlowCatalyst Unified Production Server
//!
//! Single binary combining all subsystems, toggled via environment variables.
//! Background processors (router, scheduler, stream, outbox) can optionally
//! run in standby mode with Redis leader election — only the leader processes.
//!
//! ## Environment Variables
//!
//! ### Core
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_API_PORT` | `3000` | HTTP API port |
//! | `FC_METRICS_PORT` | `9090` | Metrics/health port |
//! | `FC_DATABASE_URL` | `postgresql://localhost:5432/flowcatalyst` | PostgreSQL URL |
//!
//! ### Subsystem Toggles
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_PLATFORM_ENABLED` | `true` | Run the platform API server |
//! | `FC_ROUTER_ENABLED` | `false` | Run the SQS message router |
//! | `FC_SCHEDULER_ENABLED` | `false` | Run the dispatch scheduler |
//! | `FC_STREAM_PROCESSOR_ENABLED` | `false` | Run the CQRS stream processor |
//! | `FC_OUTBOX_ENABLED` | `false` | Run the outbox processor |
//!
//! ### Standby / HA
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_STANDBY_ENABLED` | `false` | Enable Redis leader election |
//! | `FC_STANDBY_REDIS_URL` | `redis://127.0.0.1:6379` | Redis URL |
//! | `FC_STANDBY_LOCK_KEY` | `fc:server:leader` | Redis lock key |
//!
//! ### ALB (requires `alb` feature)
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_ALB_ENABLED` | `false` | Register router with ALB when leader |
//! | `FC_ALB_TARGET_GROUP_ARN` | - | ALB target group ARN |
//! | `FC_ALB_TARGET_ID` | - | Target ID (instance ID or IP) |
//! | `FC_ALB_TARGET_PORT` | `8080` | Port for ALB health checks |

use std::sync::Arc;
use std::time::Duration;

use axum::{
    routing::get,
    response::Json,
    Router,
};
use utoipa_axum::router::OpenApiRouter;
use tower_http::cors::{CorsLayer, AllowOrigin};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use axum::http::header::CACHE_CONTROL;
use tower_http::services::{ServeDir, ServeFile};
use axum::http::{Method, HeaderValue, header as http_header};
use anyhow::Result;
use tracing::{info, warn, error};
use tokio::{signal, net::TcpListener, sync::watch};
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
    public_router, PublicApiState,
    PasswordResetApiState, password_reset_router,
    SdkSyncState, sdk_sync_router,
    SdkAuditBatchState, sdk_audit_batch_router,
    SdkDispatchJobsState, sdk_dispatch_jobs_batch_router,
    BffRolesState, bff_roles_router,
    BffEventTypesState, bff_event_types_router,
};
use fc_platform::api::{OidcLoginApiState, oidc_login_router};
use fc_platform::repository::{
    EventRepository, EventTypeRepository, DispatchJobRepository, DispatchPoolRepository,
    SubscriptionRepository, ServiceAccountRepository, PrincipalRepository, ClientRepository,
    ApplicationRepository, RoleRepository, OAuthClientRepository,
    AnchorDomainRepository, ClientAuthConfigRepository, ClientAccessGrantRepository, IdpRoleMappingRepository,
    AuditLogRepository, ApplicationClientConfigRepository, OidcLoginStateRepository, RefreshTokenRepository,
    AuthorizationCodeRepository,
    ConnectionRepository, CorsOriginRepository, IdentityProviderRepository,
    EmailDomainMappingRepository, PlatformConfigRepository, PlatformConfigAccessRepository,
    LoginAttemptRepository,
    PasswordResetTokenRepository,
};
use fc_platform::usecase::PgUnitOfWork;
use fc_platform::shared::encryption_service::EncryptionService;
use fc_platform::operations::{
    CreateServiceAccountUseCase, UpdateServiceAccountUseCase, DeleteServiceAccountUseCase,
    AssignRolesUseCase, RegenerateAuthTokenUseCase, RegenerateSigningSecretUseCase,
    CreateApplicationUseCase, UpdateApplicationUseCase,
    ActivateApplicationUseCase, DeactivateApplicationUseCase,
    CreateDispatchPoolUseCase, UpdateDispatchPoolUseCase,
    ArchiveDispatchPoolUseCase, DeleteDispatchPoolUseCase,
};
use fc_platform::service::PasswordService;
use fc_platform::service::OidcSyncService;
use fc_platform::service::OidcService;
use fc_platform::seed::DevDataSeeder;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_or_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| v == "true" || v == "1")
        .unwrap_or(default)
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    fc_common::logging::init_logging("fc-server");

    info!("Starting FlowCatalyst Unified Server");

    // ── Configuration ────────────────────────────────────────────────────────
    let api_port: u16 = env_or_parse("FC_API_PORT", 3000);
    let metrics_port: u16 = env_or_parse("FC_METRICS_PORT", 9090);
    let database_url = env_or("FC_DATABASE_URL", "postgresql://localhost:5432/flowcatalyst");
    let jwt_issuer = env_or("FC_JWT_ISSUER", "flowcatalyst");

    // Subsystem toggles
    let platform_enabled = env_bool("FC_PLATFORM_ENABLED", true);
    let router_enabled = env_bool("FC_ROUTER_ENABLED", false);
    let scheduler_enabled = env_bool("FC_SCHEDULER_ENABLED", false);
    let stream_enabled = env_bool("FC_STREAM_PROCESSOR_ENABLED", false);
    let outbox_enabled = env_bool("FC_OUTBOX_ENABLED", false);

    // Standby / HA
    let standby_enabled = env_bool("FC_STANDBY_ENABLED", false);
    let standby_redis_url = env_or("FC_STANDBY_REDIS_URL", "redis://127.0.0.1:6379");
    let standby_lock_key = env_or("FC_STANDBY_LOCK_KEY", "fc:server:leader");

    info!(
        platform = platform_enabled,
        router = router_enabled,
        scheduler = scheduler_enabled,
        stream = stream_enabled,
        outbox = outbox_enabled,
        standby = standby_enabled,
        "Subsystem configuration"
    );

    // ── Database ─────────────────────────────────────────────────────────────
    info!("Connecting to PostgreSQL...");
    let pg_db = fc_platform::shared::database::create_connection(&database_url).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    fc_platform::shared::database::run_migrations(&pg_db).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL migrations failed: {}", e))?;

    // Dev mode seeding
    if env_bool("FC_DEV_MODE", false) {
        let seeder = DevDataSeeder::new(pg_db.clone());
        if let Err(e) = seeder.seed().await {
            warn!("Dev data seeding skipped: {}", e);
        }
    }

    let pg_pool = fc_platform::shared::database::create_pool(&database_url).await
        .map_err(|e| anyhow::anyhow!("SQLx pool creation failed: {}", e))?;

    // ── Leader Election ──────────────────────────────────────────────────────
    // Shared watch channel: true = active (process), false = standby (pause)
    let (active_tx, active_rx) = watch::channel(!standby_enabled); // if standby disabled, always active

    let leader_election: Option<Arc<fc_standby::LeaderElection>> = if standby_enabled {
        info!(redis_url = %standby_redis_url, lock_key = %standby_lock_key, "Initializing leader election");
        let config = fc_standby::LeaderElectionConfig::new(standby_redis_url)
            .with_lock_key(standby_lock_key);
        let election = Arc::new(fc_standby::LeaderElection::new(config).await
            .map_err(|e| anyhow::anyhow!("Leader election init failed: {}", e))?);
        election.clone().start().await
            .map_err(|e| anyhow::anyhow!("Leader election start failed: {}", e))?;

        // Bridge leadership status changes to the active watch channel
        let mut status_rx = election.subscribe();
        let active_tx_clone = active_tx.clone();
        tokio::spawn(async move {
            loop {
                if status_rx.changed().await.is_err() {
                    break;
                }
                let is_leader = *status_rx.borrow() == fc_standby::LeadershipStatus::Leader;
                let _ = active_tx_clone.send(is_leader);
            }
        });

        Some(election)
    } else {
        None
    };

    let is_leader = move || {
        leader_election.as_ref().map_or(true, |e| e.is_leader())
    };

    // ── Platform API ─────────────────────────────────────────────────────────
    // Repositories and auth are always initialized (needed by health checks and
    // potentially by background processors).

    let event_repo = Arc::new(EventRepository::new(&pg_pool));
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

    // CORS origins cache
    let cors_origins_cache: Arc<std::sync::RwLock<std::collections::HashSet<String>>> =
        Arc::new(std::sync::RwLock::new(std::collections::HashSet::new()));
    {
        match cors_repo.get_allowed_origins().await {
            Ok(origins) => {
                let mut cache = cors_origins_cache.write().unwrap();
                for origin in origins {
                    cache.insert(origin);
                }
                info!(count = cache.len(), "CORS origins loaded");
            }
            Err(e) => warn!("Failed to load CORS origins: {}", e),
        }
    }
    {
        let cache = cors_origins_cache.clone();
        let cors_repo_bg = CorsOriginRepository::new(&pg_db);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await;
            loop {
                interval.tick().await;
                match cors_repo_bg.get_allowed_origins().await {
                    Ok(origins) => {
                        let mut c = cache.write().unwrap();
                        c.clear();
                        for origin in origins { c.insert(origin); }
                    }
                    Err(e) => warn!("Failed to refresh CORS origins: {}", e),
                }
            }
        });
    }

    // Sync code-defined roles
    {
        let role_sync = fc_platform::service::RoleSyncService::new(
            fc_platform::repository::RoleRepository::new(&pg_db),
        );
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            warn!("Role sync failed: {}", e);
        }
    }

    // Auth services
    let private_key_path = std::env::var("FC_JWT_PRIVATE_KEY_PATH").ok();
    let public_key_path = std::env::var("FC_JWT_PUBLIC_KEY_PATH").ok();
    let (private_key, public_key) = AuthConfig::load_or_generate_rsa_keys(
        private_key_path.as_deref(),
        public_key_path.as_deref(),
    )?;

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

    let unit_of_work = Arc::new(PgUnitOfWork::new(pg_db.clone()));

    // ── Build HTTP app ───────────────────────────────────────────────────────
    let app = if platform_enabled {
        build_platform_app(
            api_port,
            &pg_db,
            &auth_service,
            &authz_service,
            &password_service,
            &oidc_sync_service,
            &oidc_service,
            &unit_of_work,
            &event_repo,
            &event_type_repo,
            &dispatch_job_repo,
            &dispatch_pool_repo,
            &subscription_repo,
            &service_account_repo,
            &principal_repo,
            &client_repo,
            &application_repo,
            &role_repo,
            &oauth_client_repo,
            &anchor_domain_repo,
            &client_auth_config_repo,
            &client_access_grant_repo,
            &idp_role_mapping_repo,
            &audit_log_repo,
            &application_client_config_repo,
            &oidc_login_state_repo,
            &refresh_token_repo,
            &auth_code_repo,
            &connection_repo,
            &cors_repo,
            &idp_repo,
            &edm_repo,
            &platform_config_repo,
            &platform_config_access_repo,
            &login_attempt_repo,
            &password_reset_repo,
            &pending_auth_repo,
            &cors_origins_cache,
            standby_enabled,
        )
    } else {
        // Minimal app with just health + metrics
        Router::new()
            .route("/health", get(health_handler))
            .layer(TraceLayer::new_for_http())
    };

    // Collect handles for graceful shutdown
    let mut shutdown_handles: Vec<Box<dyn std::any::Any + Send>> = Vec::new();

    // ── Background Processors ────────────────────────────────────────────────

    // Router (SQS message processing)
    if router_enabled {
        info!("Starting message router subsystem...");
        let router_active_rx = active_rx.clone();
        let router_handle = spawn_router(router_active_rx).await;
        if let Some(handle) = router_handle {
            shutdown_handles.push(Box::new(handle));
        }
    }

    // Scheduler (dispatch job polling)
    if scheduler_enabled {
        info!("Starting scheduler subsystem...");
        spawn_scheduler(&pg_db, active_rx.clone()).await?;
    }

    // Stream processor (CQRS projections)
    let _stream_handle = if stream_enabled {
        info!("Starting stream processor subsystem...");
        Some(spawn_stream_processor(&database_url, active_rx.clone()).await?)
    } else {
        None
    };

    // Outbox processor
    if outbox_enabled {
        info!("Starting outbox processor subsystem...");
        spawn_outbox_processor(active_rx.clone()).await?;
    }

    // ── ALB Traffic Watcher ──────────────────────────────────────────────────
    #[cfg(feature = "alb")]
    if env_bool("FC_ALB_ENABLED", false) && router_enabled {
        if let Some(ref election) = leader_election {
            let status_rx = election.subscribe();
            let alb_config = fc_router::AlbTrafficConfig {
                target_group_arn: std::env::var("FC_ALB_TARGET_GROUP_ARN")
                    .expect("FC_ALB_TARGET_GROUP_ARN required when FC_ALB_ENABLED=true"),
                target_id: std::env::var("FC_ALB_TARGET_ID")
                    .expect("FC_ALB_TARGET_ID required when FC_ALB_ENABLED=true"),
                target_port: env_or_parse("FC_ALB_TARGET_PORT", 8080),
            };
            let aws_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            let strategy = Arc::new(fc_router::AwsAlbTrafficStrategy::new(alb_config, &aws_config));
            fc_router::spawn_traffic_watcher(strategy, status_rx);
            info!("ALB traffic watcher started");
        } else {
            warn!("FC_ALB_ENABLED=true but FC_STANDBY_ENABLED=false — ALB watcher requires standby mode");
        }
    }

    // ── Start HTTP Servers ───────────────────────────────────────────────────
    let api_addr = format!("0.0.0.0:{}", api_port);
    info!("API server listening on http://{}", api_addr);
    let api_listener = TcpListener::bind(&api_addr).await?;
    let api_task = tokio::spawn(async move {
        axum::serve(api_listener, app).await.unwrap();
    });

    let metrics_addr = format!("0.0.0.0:{}", metrics_port);
    info!("Metrics server listening on http://{}/metrics", metrics_addr);

    let is_leader_for_health = is_leader.clone();
    let health_state = HealthState {
        platform_enabled,
        router_enabled,
        scheduler_enabled,
        stream_enabled,
        outbox_enabled,
        is_leader: Arc::new(move || is_leader_for_health()),
    };

    let metrics_app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get({
            let state = health_state.clone();
            move || combined_health_handler(state.clone())
        }))
        .route("/ready", get(ready_handler));

    let metrics_listener = TcpListener::bind(&metrics_addr).await?;
    let metrics_task = tokio::spawn(async move {
        axum::serve(metrics_listener, metrics_app).await.unwrap();
    });

    // ── Startup Summary ──────────────────────────────────────────────────────
    info!("=== FlowCatalyst Unified Server Started ===");
    info!("  Platform API: {}", if platform_enabled { "ENABLED" } else { "DISABLED" });
    info!("  Router:       {}", if router_enabled { "ENABLED" } else { "DISABLED" });
    info!("  Scheduler:    {}", if scheduler_enabled { "ENABLED" } else { "DISABLED" });
    info!("  Stream:       {}", if stream_enabled { "ENABLED" } else { "DISABLED" });
    info!("  Outbox:       {}", if outbox_enabled { "ENABLED" } else { "DISABLED" });
    if standby_enabled {
        info!("  HA Mode:      STANDBY (Redis leader election)");
        info!("  Leader:       {}", is_leader());
    } else {
        info!("  HA Mode:      DISABLED (always active)");
    }
    info!("=============================================");

    // ── Shutdown ─────────────────────────────────────────────────────────────
    shutdown_signal().await;
    info!("Shutdown signal received...");

    // Signal all background processors to stop via the active channel
    let _ = active_tx.send(false);

    api_task.abort();
    metrics_task.abort();

    // Shutdown stream processor if running
    if let Some(handle) = _stream_handle {
        handle.stop().await;
    }

    info!("FlowCatalyst Unified Server shutdown complete");
    Ok(())
}

// ── Platform App Builder ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn build_platform_app(
    api_port: u16,
    _pg_db: &sea_orm::DatabaseConnection,
    auth_service: &Arc<AuthService>,
    authz_service: &Arc<AuthorizationService>,
    password_service: &Arc<PasswordService>,
    oidc_sync_service: &Arc<OidcSyncService>,
    oidc_service: &Arc<OidcService>,
    unit_of_work: &Arc<PgUnitOfWork>,
    event_repo: &Arc<EventRepository>,
    event_type_repo: &Arc<EventTypeRepository>,
    dispatch_job_repo: &Arc<DispatchJobRepository>,
    dispatch_pool_repo: &Arc<DispatchPoolRepository>,
    subscription_repo: &Arc<SubscriptionRepository>,
    service_account_repo: &Arc<ServiceAccountRepository>,
    principal_repo: &Arc<PrincipalRepository>,
    client_repo: &Arc<ClientRepository>,
    application_repo: &Arc<ApplicationRepository>,
    role_repo: &Arc<RoleRepository>,
    oauth_client_repo: &Arc<OAuthClientRepository>,
    anchor_domain_repo: &Arc<AnchorDomainRepository>,
    client_auth_config_repo: &Arc<ClientAuthConfigRepository>,
    client_access_grant_repo: &Arc<ClientAccessGrantRepository>,
    idp_role_mapping_repo: &Arc<IdpRoleMappingRepository>,
    audit_log_repo: &Arc<AuditLogRepository>,
    application_client_config_repo: &Arc<ApplicationClientConfigRepository>,
    oidc_login_state_repo: &Arc<OidcLoginStateRepository>,
    refresh_token_repo: &Arc<RefreshTokenRepository>,
    auth_code_repo: &Arc<AuthorizationCodeRepository>,
    connection_repo: &Arc<ConnectionRepository>,
    cors_repo: &Arc<CorsOriginRepository>,
    idp_repo: &Arc<IdentityProviderRepository>,
    edm_repo: &Arc<EmailDomainMappingRepository>,
    platform_config_repo: &Arc<PlatformConfigRepository>,
    platform_config_access_repo: &Arc<PlatformConfigAccessRepository>,
    login_attempt_repo: &Arc<LoginAttemptRepository>,
    password_reset_repo: &Arc<PasswordResetTokenRepository>,
    pending_auth_repo: &Arc<fc_platform::PendingAuthRepository>,
    cors_origins_cache: &Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    _standby_enabled: bool,
) -> Router {
    let app_state = AppState {
        auth_service: auth_service.clone(),
        authz_service: authz_service.clone(),
    };

    // Build API states
    let events_state = EventsState { event_repo: event_repo.clone() };
    let sync_event_types_use_case = Arc::new(fc_platform::event_type::operations::SyncEventTypesUseCase::new(event_type_repo.clone()));
    let event_types_state = EventTypesState { event_type_repo: event_type_repo.clone(), sync_use_case: sync_event_types_use_case.clone() };
    let dispatch_jobs_state = DispatchJobsState { dispatch_job_repo: dispatch_job_repo.clone() };
    let sdk_events_state = SdkEventsState { event_repo: event_repo.clone() };
    let sdk_clients_state = SdkClientsState { client_repo: client_repo.clone(), unit_of_work: unit_of_work.clone() };
    let sdk_principals_state = SdkPrincipalsState { principal_repo: principal_repo.clone(), role_repo: role_repo.clone(), unit_of_work: unit_of_work.clone() };
    let sdk_roles_state = SdkRolesState {
        role_repo: role_repo.clone(),
        application_repo: application_repo.clone(),
    };
    let debug_state = DebugState {
        event_repo: event_repo.clone(),
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
    let subscriptions_state = SubscriptionsState { subscription_repo: subscription_repo.clone(), sync_use_case: sync_subscriptions_use_case.clone() };
    let oauth_clients_state = OAuthClientsState { oauth_client_repo: oauth_client_repo.clone() };
    let auth_config_state = AuthConfigState {
        anchor_domain_repo: anchor_domain_repo.clone(),
        client_auth_config_repo: client_auth_config_repo.clone(),
        idp_role_mapping_repo: idp_role_mapping_repo.clone(),
        principal_repo: Some(principal_repo.clone()),
    };

    let external_base_url = std::env::var("FC_EXTERNAL_BASE_URL").ok();
    let oidc_login_state = OidcLoginApiState::new(
        anchor_domain_repo.clone(),
        idp_repo.clone(),
        edm_repo.clone(),
        oidc_login_state_repo.clone(),
        oidc_sync_service.clone(),
        auth_service.clone(),
        unit_of_work.clone(),
    ).with_session_cookie_settings("fc_session", false, "Lax", 86400);
    let encryption_service = EncryptionService::from_env().map(Arc::new);
    let oidc_login_state = if let Some(enc_svc) = encryption_service {
        oidc_login_state.with_encryption_service(enc_svc)
    } else {
        warn!("FLOWCATALYST_APP_KEY not set — OIDC client secrets cannot be decrypted");
        oidc_login_state
    };
    let oidc_login_state = if let Some(ref url) = external_base_url {
        oidc_login_state.with_external_base_url(url.clone())
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
        oidc_service.clone(),
        auth_code_repo.clone(),
        refresh_token_repo.clone(),
        pending_auth_repo.clone(),
        password_service.clone(),
        login_attempt_repo.clone(),
    );
    let audit_logs_state = AuditLogsState { audit_log_repo: audit_log_repo.clone(), principal_repo: principal_repo.clone() };

    // Service Account use cases
    let create_sa_use_case = Arc::new(CreateServiceAccountUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let update_sa_use_case = Arc::new(UpdateServiceAccountUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let delete_sa_use_case = Arc::new(DeleteServiceAccountUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let assign_roles_use_case = Arc::new(AssignRolesUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let regenerate_token_use_case = Arc::new(RegenerateAuthTokenUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let regenerate_secret_use_case = Arc::new(RegenerateSigningSecretUseCase::new(service_account_repo.clone(), unit_of_work.clone()));

    // Application use cases
    let create_app_use_case = Arc::new(CreateApplicationUseCase::new(application_repo.clone(), unit_of_work.clone()));
    let update_app_use_case = Arc::new(UpdateApplicationUseCase::new(application_repo.clone(), unit_of_work.clone()));
    let activate_app_use_case = Arc::new(ActivateApplicationUseCase::new(application_repo.clone(), unit_of_work.clone()));
    let deactivate_app_use_case = Arc::new(DeactivateApplicationUseCase::new(application_repo.clone(), unit_of_work.clone()));

    // Dispatch Pool use cases
    let create_pool_use_case = Arc::new(CreateDispatchPoolUseCase::new(dispatch_pool_repo.clone(), unit_of_work.clone()));
    let update_pool_use_case = Arc::new(UpdateDispatchPoolUseCase::new(dispatch_pool_repo.clone(), unit_of_work.clone()));
    let archive_pool_use_case = Arc::new(ArchiveDispatchPoolUseCase::new(dispatch_pool_repo.clone(), unit_of_work.clone()));
    let delete_pool_use_case = Arc::new(DeleteDispatchPoolUseCase::new(dispatch_pool_repo.clone(), unit_of_work.clone()));

    // Domain API states
    let connections_state = ConnectionsState { connection_repo: connection_repo.clone() };
    let cors_state = CorsState { cors_repo: cors_repo.clone() };
    let idp_state = IdentityProvidersState { idp_repo: idp_repo.clone() };
    let edm_state = EmailDomainMappingsState { edm_repo: edm_repo.clone(), idp_repo: idp_repo.clone() };
    let public_api_state = PublicApiState { config_repo: platform_config_repo.clone() };
    let platform_config_state = PlatformConfigState { config_repo: platform_config_repo.clone() };
    let config_access_state = ConfigAccessState { access_repo: platform_config_access_repo.clone() };
    let login_attempts_state = LoginAttemptsState { login_attempt_repo: login_attempt_repo.clone() };
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
        grant_repo: client_access_grant_repo.clone(),
        auth_service: auth_service.clone(),
    };
    let application_roles_sdk_state = ApplicationRolesSdkState {
        application_repo: application_repo.clone(),
        role_repo: role_repo.clone(),
    };
    let email_service: Arc<dyn fc_platform::shared::email_service::EmailService> =
        Arc::from(fc_platform::shared::email_service::create_email_service());
    let password_reset_state = PasswordResetApiState {
        password_reset_repo: password_reset_repo.clone(),
        principal_repo: principal_repo.clone(),
        password_service: password_service.clone(),
        unit_of_work: unit_of_work.clone(),
        email_service,
        external_base_url: std::env::var("FC_EXTERNAL_BASE_URL")
            .unwrap_or_else(|_| format!("http://localhost:{}", api_port)),
    };

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
        repo: service_account_repo.clone(),
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

    let sync_roles_use_case = Arc::new(fc_platform::role::operations::SyncRolesUseCase::new(role_repo.clone(), application_repo.clone()));
    let sync_principals_use_case = Arc::new(fc_platform::principal::operations::SyncPrincipalsUseCase::new(principal_repo.clone(), application_repo.clone()));
    let sdk_sync_state = SdkSyncState {
        sync_roles_use_case,
        sync_event_types_use_case: sync_event_types_use_case.clone(),
        sync_subscriptions_use_case: sync_subscriptions_use_case.clone(),
        sync_dispatch_pools_use_case: sync_dispatch_pools_use_case.clone(),
        sync_principals_use_case,
    };
    let sdk_audit_batch_state = SdkAuditBatchState {
        audit_log_repo: audit_log_repo.clone(),
        application_repo: application_repo.clone(),
        client_repo: client_repo.clone(),
    };
    let sdk_dispatch_jobs_state = SdkDispatchJobsState {
        dispatch_job_repo: dispatch_job_repo.clone(),
    };
    let bff_roles_state = BffRolesState {
        role_repo: role_repo.clone(),
        application_repo: Some(application_repo.clone()),
        unit_of_work: unit_of_work.clone(),
    };
    let bff_event_types_state = BffEventTypesState {
        event_type_repo: event_type_repo.clone(),
        application_repo: Some(application_repo.clone()),
        sync_use_case: sync_event_types_use_case.clone(),
        unit_of_work: unit_of_work.clone(),
    };
    let monitoring_state = MonitoringState {
        leader_state: LeaderState::new(uuid::Uuid::new_v4().to_string()),
        circuit_breakers: CircuitBreakerRegistry::new(),
        in_flight: InFlightTracker::new(),
        dispatch_job_repo: dispatch_job_repo.clone(),
        start_time: std::time::Instant::now(),
    };

    // Build OpenAPI router
    let (router, mut openapi) = OpenApiRouter::new()
        .nest("/bff/events", events_router(events_state))
        .nest("/bff/event-types", event_types_router(event_types_state))
        .nest("/bff/dispatch-jobs", dispatch_jobs_router(dispatch_jobs_state))
        .nest("/bff/filter-options", filter_options_router(filter_options_state.clone()))
        .nest("/api/admin/clients", clients_router(clients_state))
        .nest("/api/admin/principals", principals_router(principals_state))
        .nest("/api/admin/roles", roles_router(roles_state))
        .nest("/api/admin/subscriptions", subscriptions_router(subscriptions_state))
        .nest("/api/admin/oauth-clients", oauth_clients_router(oauth_clients_state))
        .nest("/api/admin/audit-logs", audit_logs_router(audit_logs_state))
        .nest("/api/monitoring", monitoring_router(monitoring_state))
        .nest("/auth", auth_router(embedded_auth_state))
        .split_for_parts();

    use utoipa::openapi::{ObjectBuilder, schema::Type};
    if let Some(components) = openapi.components.as_mut() {
        components.schemas.insert(
            "PaginationParams".to_string(),
            ObjectBuilder::new()
                .property("page", ObjectBuilder::new().schema_type(Type::Integer))
                .property("limit", ObjectBuilder::new().schema_type(Type::Integer))
                .into(),
        );
    }
    openapi.info.title = "FlowCatalyst Platform API".to_string();
    openapi.info.version = "1.0.0".to_string();
    openapi.info.description = Some("REST APIs for events, subscriptions, and administration".to_string());

    let app = Router::new()
        .merge(router)
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
        .nest("/api/public", public_router(public_api_state))
        .nest("/auth/password-reset", password_reset_router(password_reset_state))
        .route("/health", get(health_handler))
        .merge(SwaggerUi::new("/swagger-ui").url("/q/openapi", openapi))
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
                    if origins.contains(origin_str) {
                        return true;
                    }
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
                .max_age(Duration::from_secs(86400))
        });

    // Static frontend serving
    if let Ok(static_dir) = std::env::var("FC_STATIC_DIR") {
        let index_path = std::path::PathBuf::from(&static_dir).join("index.html");
        if index_path.exists() {
            info!(dir = %static_dir, "Serving static frontend files with SPA fallback");
            let assets_dir = std::path::PathBuf::from(&static_dir).join("assets");
            let assets_service = tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::overriding(
                    CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                ))
                .service(ServeDir::new(&assets_dir));

            return app
                .nest_service("/assets", assets_service)
                .fallback_service(
                    ServeDir::new(&static_dir)
                        .fallback(ServeFile::new(index_path))
                );
        }
        warn!(dir = %static_dir, "FC_STATIC_DIR set but index.html not found");
    }

    app
}

// ── Background Processor Spawners ────────────────────────────────────────────

/// Spawn the SQS message router, gated on leadership.
async fn spawn_router(
    mut active_rx: watch::Receiver<bool>,
) -> Option<tokio::task::JoinHandle<()>> {
    use fc_router::{
        QueueManager, HttpMediator,
        WarningService, WarningServiceConfig,
        HealthService, HealthServiceConfig,
    };
    use fc_queue::sqs::SqsQueueConsumer;

    let dev_mode = env_bool("FLOWCATALYST_DEV_MODE", false);

    let sqs_client = if dev_mode {
        let endpoint_url = env_or("LOCALSTACK_ENDPOINT", "http://localhost:4566");
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .endpoint_url(&endpoint_url)
            .load()
            .await;
        aws_sdk_sqs::Client::new(&config)
    } else {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        aws_sdk_sqs::Client::new(&config)
    };

    let config_url = std::env::var("FLOWCATALYST_CONFIG_URL").ok();
    if config_url.is_none() && !dev_mode {
        error!("FC_ROUTER_ENABLED=true but FLOWCATALYST_CONFIG_URL not set and not in dev mode");
        return None;
    }

    let warning_service = Arc::new(WarningService::new(WarningServiceConfig::default()));
    let health_service = Arc::new(HealthService::new(
        HealthServiceConfig::default(),
        warning_service.clone(),
    ));
    let mediator = Arc::new(HttpMediator::production());
    let mut queue_manager_inner = QueueManager::new(mediator);
    queue_manager_inner.set_health_service(health_service.clone());
    let queue_manager = Arc::new(queue_manager_inner);

    // Load configuration
    let router_config = if dev_mode {
        use fc_common::{RouterConfig, PoolConfig, QueueConfig};
        let sqs_host = env_or("LOCALSTACK_SQS_HOST", "http://sqs.eu-west-1.localhost.localstack.cloud:4566");
        RouterConfig {
            processing_pools: vec![
                PoolConfig { code: "DEFAULT".to_string(), concurrency: 10, rate_limit_per_minute: None },
            ],
            queues: vec![
                QueueConfig {
                    name: "fc-default.fifo".to_string(),
                    uri: format!("{}/000000000000/fc-default.fifo", sqs_host),
                    connections: 2,
                    visibility_timeout: 120,
                },
            ],
        }
    } else {
        let config_url = config_url.unwrap();
        let config_sync_config = fc_router::ConfigSyncConfig::new(config_url);
        let sync_service = Arc::new(fc_router::ConfigSyncService::new(
            config_sync_config,
            queue_manager.clone(),
            warning_service.clone(),
        ));
        match sync_service.initial_sync().await {
            Ok(config) => config,
            Err(e) => {
                error!("Router config sync failed: {}", e);
                return None;
            }
        }
    };

    // Add SQS consumers
    for queue_config in &router_config.queues {
        let consumer = Arc::new(SqsQueueConsumer::from_queue_url(
            sqs_client.clone(),
            queue_config.uri.clone(),
            queue_config.visibility_timeout as i32,
        ).await);
        queue_manager.add_consumer(consumer).await;
    }

    let manager = queue_manager.clone();
    let handle = tokio::spawn(async move {
        loop {
            // Wait until we're active (leader)
            if !*active_rx.borrow() {
                info!("Router: waiting for leadership...");
                loop {
                    if active_rx.changed().await.is_err() { return; }
                    if *active_rx.borrow() { break; }
                }
                info!("Router: acquired leadership, starting processing");
            }

            // Process until leadership lost or shutdown
            let mut lost_rx = active_rx.clone();
            tokio::select! {
                result = manager.clone().start() => {
                    if let Err(e) = result {
                        error!("QueueManager error: {}", e);
                    }
                }
                _ = async {
                    loop {
                        if lost_rx.changed().await.is_err() { return; }
                        if !*lost_rx.borrow() { return; }
                    }
                } => {
                    warn!("Router: lost leadership, pausing");
                    manager.shutdown().await;
                }
            }
        }
    });

    Some(handle)
}

/// Spawn the dispatch scheduler, gated on leadership.
async fn spawn_scheduler(
    pg_db: &sea_orm::DatabaseConnection,
    mut active_rx: watch::Receiver<bool>,
) -> Result<()> {
    use fc_scheduler::{DispatchScheduler, QueuePublisher, QueueMessage, SchedulerError};

    struct NoopQueuePublisher;

    #[async_trait::async_trait]
    impl QueuePublisher for NoopQueuePublisher {
        async fn publish(&self, message: QueueMessage) -> std::result::Result<(), SchedulerError> {
            info!(id = %message.id, "Scheduler: message published");
            Ok(())
        }
        fn is_healthy(&self) -> bool { true }
    }

    let config = load_scheduler_config();
    let queue_publisher: Arc<dyn QueuePublisher> = Arc::new(NoopQueuePublisher);
    let scheduler = Arc::new(DispatchScheduler::new(config, pg_db.clone(), queue_publisher));

    tokio::spawn(async move {
        loop {
            // Wait until active
            if !*active_rx.borrow() {
                info!("Scheduler: waiting for leadership...");
                loop {
                    if active_rx.changed().await.is_err() { return; }
                    if *active_rx.borrow() { break; }
                }
                info!("Scheduler: acquired leadership, starting");
            }

            scheduler.start().await;

            // Watch for leadership loss
            let mut lost_rx = active_rx.clone();
            loop {
                if lost_rx.changed().await.is_err() {
                    scheduler.stop().await;
                    return;
                }
                if !*lost_rx.borrow() {
                    info!("Scheduler: lost leadership, stopping");
                    scheduler.stop().await;
                    break;
                }
            }
        }
    });

    Ok(())
}

fn load_scheduler_config() -> fc_scheduler::SchedulerConfig {
    let config = fc_config::AppConfig::load().unwrap_or_default();
    fc_scheduler::SchedulerConfig {
        enabled: config.scheduler.enabled,
        poll_interval: Duration::from_millis(config.scheduler.poll_interval_ms),
        batch_size: config.scheduler.batch_size,
        stale_threshold: Duration::from_secs(config.scheduler.stale_threshold_minutes * 60),
        default_dispatch_mode: config.scheduler.default_dispatch_mode.as_str().into(),
        default_pool_code: env_or("FC_SCHEDULER_DEFAULT_POOL_CODE", "DISPATCH-POOL"),
        processing_endpoint: env_or("FC_SCHEDULER_PROCESSING_ENDPOINT", "http://localhost:8080/api/dispatch/process"),
        app_key: if config.scheduler.app_key.is_empty() { None } else { Some(config.scheduler.app_key.clone()) },
        max_concurrent_groups: env_or_parse("FC_SCHEDULER_MAX_CONCURRENT_GROUPS", 10),
        connection_filter_enabled: true,
    }
}

/// Spawn the CQRS stream processor, gated on leadership.
async fn spawn_stream_processor(
    database_url: &str,
    mut active_rx: watch::Receiver<bool>,
) -> Result<StreamProcessorShutdown> {
    use fc_stream::{StreamProcessorConfig, start_stream_processor};

    let config = StreamProcessorConfig {
        events_enabled: env_bool("FC_STREAM_EVENTS_ENABLED", true),
        events_batch_size: env_or_parse("FC_STREAM_EVENTS_BATCH_SIZE", 100),
        dispatch_jobs_enabled: env_bool("FC_STREAM_DISPATCH_JOBS_ENABLED", true),
        dispatch_jobs_batch_size: env_or_parse("FC_STREAM_DISPATCH_JOBS_BATCH_SIZE", 100),
    };

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .idle_timeout(Duration::from_secs(20))
        .acquire_timeout(Duration::from_secs(30))
        .connect(database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Stream processor PG pool failed: {}", e))?;

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let pool_clone = pool.clone();

    tokio::spawn(async move {
        let mut current_handle: Option<fc_stream::StreamProcessorHandle>;
        let mut stop_rx = stop_rx;

        loop {
            // Wait until active
            if !*active_rx.borrow() {
                info!("Stream processor: waiting for leadership...");
                loop {
                    tokio::select! {
                        result = active_rx.changed() => {
                            if result.is_err() { return; }
                            if *active_rx.borrow() { break; }
                        }
                        _ = &mut stop_rx => {
                            return;
                        }
                    }
                }
                info!("Stream processor: acquired leadership, starting projections");
            }

            // Start projections
            let cfg = StreamProcessorConfig {
                events_enabled: config.events_enabled,
                events_batch_size: config.events_batch_size,
                dispatch_jobs_enabled: config.dispatch_jobs_enabled,
                dispatch_jobs_batch_size: config.dispatch_jobs_batch_size,
            };
            let (handle, _health_service) = start_stream_processor(pool_clone.clone(), cfg);
            current_handle = Some(handle);

            // Wait for leadership loss or shutdown
            loop {
                tokio::select! {
                    result = active_rx.changed() => {
                        if result.is_err() {
                            if let Some(h) = current_handle.take() { h.stop().await; }
                            return;
                        }
                        if !*active_rx.borrow() {
                            info!("Stream processor: lost leadership, stopping projections");
                            if let Some(h) = current_handle.take() { h.stop().await; }
                            break;
                        }
                    }
                    _ = &mut stop_rx => {
                        if let Some(h) = current_handle.take() { h.stop().await; }
                        return;
                    }
                }
            }
        }
    });

    Ok(StreamProcessorShutdown { _stop_tx: Some(stop_tx) })
}

/// Handle for stopping the stream processor from the main shutdown path.
struct StreamProcessorShutdown {
    _stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl StreamProcessorShutdown {
    async fn stop(mut self) {
        // Dropping the sender signals the spawned task
        self._stop_tx.take();
    }
}

/// Spawn the outbox processor, gated on leadership.
async fn spawn_outbox_processor(
    mut active_rx: watch::Receiver<bool>,
) -> Result<()> {
    use fc_outbox::{EnhancedOutboxProcessor, EnhancedProcessorConfig};
    use fc_outbox::http_dispatcher::HttpDispatcherConfig;
    use fc_outbox::repository::{OutboxRepository, OutboxTableConfig};

    let db_type = env_or("FC_OUTBOX_DB_TYPE", "postgres");
    let poll_interval_ms: u64 = env_or_parse("FC_OUTBOX_POLL_INTERVAL_MS", 1000);

    let table_config = OutboxTableConfig {
        events_table: env_or("FC_OUTBOX_EVENTS_TABLE", "outbox_messages"),
        dispatch_jobs_table: env_or("FC_OUTBOX_DISPATCH_JOBS_TABLE", "outbox_messages"),
        audit_logs_table: env_or("FC_OUTBOX_AUDIT_LOGS_TABLE", "outbox_messages"),
    };

    let outbox_repo: Arc<dyn OutboxRepository> = match db_type.as_str() {
        "sqlite" => {
            let url = std::env::var("FC_OUTBOX_DB_URL")
                .map_err(|_| anyhow::anyhow!("FC_OUTBOX_DB_URL required for sqlite outbox"))?;
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(5)
                .connect(&url)
                .await?;
            let repo = fc_outbox::sqlite::SqliteOutboxRepository::with_config(pool, table_config);
            repo.init_schema().await?;
            Arc::new(repo)
        }
        "postgres" => {
            let url = std::env::var("FC_OUTBOX_DB_URL")
                .map_err(|_| anyhow::anyhow!("FC_OUTBOX_DB_URL required for postgres outbox"))?;
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(10)
                .connect(&url)
                .await?;
            let repo = fc_outbox::postgres::PostgresOutboxRepository::with_config(pool, table_config);
            repo.init_schema().await?;
            Arc::new(repo)
        }
        other => return Err(anyhow::anyhow!("Unknown outbox DB type: {}", other)),
    };

    let api_base_url = env_or("FC_API_BASE_URL", "http://localhost:8080");
    let api_token = std::env::var("FC_API_TOKEN").ok();

    let config = EnhancedProcessorConfig {
        poll_interval: Duration::from_millis(poll_interval_ms),
        poll_batch_size: env_or_parse("FC_OUTBOX_BATCH_SIZE", 500),
        api_batch_size: env_or_parse("FC_API_BATCH_SIZE", 100),
        max_concurrent_groups: env_or_parse("FC_MAX_CONCURRENT_GROUPS", 10),
        global_buffer_size: env_or_parse("FC_GLOBAL_BUFFER_SIZE", 1000),
        max_in_flight: env_or_parse("FC_MAX_IN_FLIGHT", 5000),
        http_config: HttpDispatcherConfig {
            api_base_url,
            api_token,
            ..Default::default()
        },
        ..Default::default()
    };

    let processor = Arc::new(EnhancedOutboxProcessor::new(config, outbox_repo)?);

    tokio::spawn(async move {
        loop {
            // Wait until active
            if !*active_rx.borrow() {
                info!("Outbox: waiting for leadership...");
                loop {
                    if active_rx.changed().await.is_err() { return; }
                    if *active_rx.borrow() { break; }
                }
                info!("Outbox: acquired leadership, starting");
            }

            let proc = processor.clone();
            let mut lost_rx = active_rx.clone();
            tokio::select! {
                _ = proc.start() => {}
                _ = async {
                    loop {
                        if lost_rx.changed().await.is_err() { return; }
                        if !*lost_rx.borrow() { return; }
                    }
                } => {
                    info!("Outbox: lost leadership, stopping");
                    processor.stop();
                }
            }
        }
    });

    Ok(())
}

// ── Health Endpoints ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct HealthState {
    platform_enabled: bool,
    router_enabled: bool,
    scheduler_enabled: bool,
    stream_enabled: bool,
    outbox_enabled: bool,
    is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
}

async fn combined_health_handler(state: HealthState) -> Json<serde_json::Value> {
    let leader = (state.is_leader)();
    Json(serde_json::json!({
        "status": "UP",
        "leader": leader,
        "version": env!("CARGO_PKG_VERSION"),
        "components": {
            "platform": if state.platform_enabled { "UP" } else { "DISABLED" },
            "router": if state.router_enabled { if leader { "UP" } else { "STANDBY" } } else { "DISABLED" },
            "scheduler": if state.scheduler_enabled { if leader { "UP" } else { "STANDBY" } } else { "DISABLED" },
            "stream_processor": if state.stream_enabled { if leader { "UP" } else { "STANDBY" } } else { "DISABLED" },
            "outbox": if state.outbox_enabled { if leader { "UP" } else { "STANDBY" } } else { "DISABLED" },
        }
    }))
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "UP",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn metrics_handler() -> &'static str {
    "# HELP fc_server_up Server is up\n# TYPE fc_server_up gauge\nfc_server_up 1\n"
}

async fn ready_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "READY" }))
}

// ── Shutdown Signal ──────────────────────────────────────────────────────────

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
