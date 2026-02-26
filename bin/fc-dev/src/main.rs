//! FlowCatalyst Development Monolith
//!
//! All-in-one binary for local development containing:
//! - Message Router (with embedded SQLite queue)
//! - API Server (for publishing messages)
//! - Outbox Processor (configurable database backend)
//! - Platform APIs (events, subscriptions, auth, etc.)
//! - Metrics endpoint

use std::sync::Arc;
use std::time::Duration;
use clap::Parser;
use tokio::signal;
use tokio::sync::broadcast;
use tokio::net::TcpListener;
use anyhow::Result;
use tracing::{info, error};
use axum::{
    routing::get,
    response::Json,
    Router,
};
use tower_http::cors::{CorsLayer, Any};
use tower_http::trace::TraceLayer;

use fc_common::{RouterConfig, PoolConfig, QueueConfig};
use fc_router::{
    QueueManager, HttpMediator, LifecycleManager, LifecycleConfig,
    WarningService, WarningServiceConfig, HealthService, HealthServiceConfig,
    CircuitBreakerRegistry as RouterCircuitBreakerRegistry,
    api::create_router as create_api_router,
};
use fc_queue::sqlite::SqliteQueue;
use fc_queue::{QueuePublisher, EmbeddedQueue};
use fc_outbox::{OutboxProcessor, OutboxRepository};

// Platform imports
use fc_platform::service::{AuthService, AuthConfig, AuthorizationService, AuditService};
use fc_platform::api::middleware::{AppState, AuthLayer};
use fc_platform::api::{
    EventsState, events_router,
    EventTypesState, event_types_router,
    DispatchJobsState, dispatch_jobs_router,
    FilterOptionsState, filter_options_router,
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
    ServiceAccountsState, service_accounts_router,
};
use fc_platform::repository::{
    EventRepository, EventTypeRepository, DispatchJobRepository, DispatchPoolRepository,
    SubscriptionRepository, ServiceAccountRepository, PrincipalRepository, ClientRepository,
    ApplicationRepository, RoleRepository, OAuthClientRepository,
    AnchorDomainRepository, ClientAuthConfigRepository, ClientAccessGrantRepository, IdpRoleMappingRepository,
    AuditLogRepository, ApplicationClientConfigRepository,
};
use fc_platform::usecase::MongoUnitOfWork;
use fc_platform::operations::{
    // Application use cases
    CreateApplicationUseCase, UpdateApplicationUseCase,
    ActivateApplicationUseCase, DeactivateApplicationUseCase,
    // Service Account use cases
    CreateServiceAccountUseCase, UpdateServiceAccountUseCase, DeleteServiceAccountUseCase,
    AssignRolesUseCase, RegenerateAuthTokenUseCase, RegenerateSigningSecretUseCase,
    // Dispatch Pool use cases
    CreateDispatchPoolUseCase, UpdateDispatchPoolUseCase,
    ArchiveDispatchPoolUseCase, DeleteDispatchPoolUseCase,
};

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::postgres::PgPoolOptions;

/// FlowCatalyst Development Server
#[derive(Parser, Debug)]
#[command(name = "fc-dev")]
#[command(about = "FlowCatalyst Development Monolith - All components in one binary")]
struct Args {
    /// API server port
    #[arg(long, env = "FC_API_PORT", default_value = "8080")]
    api_port: u16,

    /// Metrics server port
    #[arg(long, env = "FC_METRICS_PORT", default_value = "9090")]
    metrics_port: u16,

    /// Outbox database type: sqlite, postgres, mongo
    #[arg(long, env = "FC_OUTBOX_DB_TYPE", default_value = "sqlite")]
    outbox_db_type: String,

    /// Outbox database URL (for postgres/mongo)
    #[arg(long, env = "FC_OUTBOX_DB_URL")]
    outbox_db_url: Option<String>,

    /// MongoDB database name (when using mongo outbox)
    #[arg(long, env = "FC_OUTBOX_MONGO_DB", default_value = "flowcatalyst")]
    outbox_mongo_db: String,

    /// MongoDB collection name for outbox
    #[arg(long, env = "FC_OUTBOX_MONGO_COLLECTION", default_value = "outbox")]
    outbox_mongo_collection: String,

    /// Default pool concurrency
    #[arg(long, env = "FC_POOL_CONCURRENCY", default_value = "10")]
    pool_concurrency: u32,

    /// Enable outbox processor
    #[arg(long, env = "FC_OUTBOX_ENABLED", default_value = "false")]
    outbox_enabled: bool,

    /// Outbox poll interval in milliseconds
    #[arg(long, env = "FC_OUTBOX_POLL_INTERVAL_MS", default_value = "1000")]
    outbox_poll_interval_ms: u64,

    // Platform configuration

    /// MongoDB URL for platform database
    #[arg(long, env = "FC_MONGO_URL", default_value = "mongodb://localhost:27017")]
    mongo_url: String,

    /// MongoDB database name for platform
    #[arg(long, env = "FC_MONGO_DB", default_value = "flowcatalyst")]
    mongo_db: String,

    /// PostgreSQL database URL (for migrated repositories)
    #[arg(long, env = "FC_DATABASE_URL", default_value = "postgresql://localhost:5432/flowcatalyst")]
    database_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging (JSON if LOG_FORMAT=json, text otherwise)
    fc_common::logging::init_logging("fc-dev");

    let args = Args::parse();

    info!("Starting FlowCatalyst Dev Monolith (Rust)");
    info!("API port: {}, Metrics port: {}", args.api_port, args.metrics_port);

    // Setup shutdown signal
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // 1. Setup SQLite for embedded queue
    let queue_pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite::memory:")
        .await?;

    // 2. Initialize embedded queue (SQLite-based, mimics SQS FIFO)
    let queue = Arc::new(SqliteQueue::new(
        queue_pool.clone(),
        "dev-queue".to_string(),
        30, // visibility timeout
    ));
    queue.init_schema().await?;
    info!("Embedded SQLite queue initialized");

    // 3. Initialize HTTP Mediator (dev mode: HTTP/1.1, shorter timeout)
    let mediator = Arc::new(HttpMediator::dev());

    // 4. Create QueueManager (central orchestrator)
    let queue_manager = Arc::new(QueueManager::new(mediator.clone()));
    queue_manager.add_consumer(queue.clone()).await;

    // 4b. Create Warning and Health services
    let warning_service = Arc::new(WarningService::new(WarningServiceConfig::default()));
    let health_service = Arc::new(HealthService::new(
        HealthServiceConfig::default(),
        warning_service.clone(),
    ));

    // 5. Apply router configuration
    let router_config = RouterConfig {
        processing_pools: vec![
            PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: args.pool_concurrency,
                rate_limit_per_minute: None,
            },
        ],
        queues: vec![
            QueueConfig {
                name: "dev-queue".to_string(),
                uri: "sqlite::memory:".to_string(),
                connections: 1,
                visibility_timeout: 30,
            },
        ],
    };
    queue_manager.apply_config(router_config).await?;

    // 6. Start lifecycle manager (visibility extension, health checks)
    let lifecycle = LifecycleManager::start(
        queue_manager.clone(),
        warning_service.clone(),
        health_service.clone(),
        LifecycleConfig::default(),
    );

    // 7. Setup outbox processor if enabled
    let outbox_handle = if args.outbox_enabled {
        let outbox_repo = create_outbox_repository(&args).await?;
        let outbox_publisher = OutboxQueuePublisher::new(queue.clone());

        let processor = OutboxProcessor::new(
            outbox_repo,
            Arc::new(outbox_publisher),
            Duration::from_millis(args.outbox_poll_interval_ms),
            100, // batch size
        );

        let mut shutdown_rx = shutdown_tx.subscribe();
        Some(tokio::spawn(async move {
            tokio::select! {
                _ = processor.start() => {}
                _ = shutdown_rx.recv() => {
                    info!("Outbox processor shutting down");
                }
            }
        }))
    } else {
        None
    };

    // 8. Setup platform services and APIs
    info!("Initializing platform services...");

    // 8a. Connect to MongoDB
    let mongo_client = mongodb::Client::with_uri_str(&args.mongo_url).await?;
    let platform_db = mongo_client.database(&args.mongo_db);
    info!("Connected to MongoDB: {}/{}", args.mongo_url, args.mongo_db);

    // 8a2. Connect to PostgreSQL (for migrated repositories)
    info!("Connecting to PostgreSQL...");
    let pg_db = fc_platform::shared::database::create_connection(&args.database_url).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    // 8b. Initialize all repositories
    let event_repo = Arc::new(EventRepository::new(&platform_db));
    let event_type_repo = Arc::new(EventTypeRepository::new(&platform_db));
    let dispatch_job_repo = Arc::new(DispatchJobRepository::new(&platform_db));
    let dispatch_pool_repo = Arc::new(DispatchPoolRepository::new(&platform_db));
    let subscription_repo = Arc::new(SubscriptionRepository::new(&platform_db));
    let service_account_repo = Arc::new(ServiceAccountRepository::new(&pg_db));
    let principal_repo = Arc::new(PrincipalRepository::new(&pg_db));
    let client_repo = Arc::new(ClientRepository::new(&pg_db));
    let application_repo = Arc::new(ApplicationRepository::new(&platform_db));
    let role_repo = Arc::new(RoleRepository::new(&pg_db));
    let oauth_client_repo = Arc::new(OAuthClientRepository::new(&platform_db));
    let anchor_domain_repo = Arc::new(AnchorDomainRepository::new(&platform_db));
    let client_auth_config_repo = Arc::new(ClientAuthConfigRepository::new(&platform_db));
    let _client_access_grant_repo = Arc::new(ClientAccessGrantRepository::new(&pg_db));
    let idp_role_mapping_repo = Arc::new(IdpRoleMappingRepository::new(&platform_db));
    let audit_log_repo = Arc::new(AuditLogRepository::new(&platform_db));
    let application_client_config_repo = Arc::new(ApplicationClientConfigRepository::new(&platform_db));
    info!("Platform repositories initialized");

    // 8b2. Create UnitOfWork for atomic commits
    let unit_of_work = Arc::new(MongoUnitOfWork::new(mongo_client.clone(), platform_db.clone()));

    // 8b3. Create use cases
    let create_application_use_case = Arc::new(CreateApplicationUseCase::new(application_repo.clone(), unit_of_work.clone()));
    let update_application_use_case = Arc::new(UpdateApplicationUseCase::new(application_repo.clone(), unit_of_work.clone()));
    let activate_application_use_case = Arc::new(ActivateApplicationUseCase::new(application_repo.clone(), unit_of_work.clone()));
    let deactivate_application_use_case = Arc::new(DeactivateApplicationUseCase::new(application_repo.clone(), unit_of_work.clone()));

    let create_service_account_use_case = Arc::new(CreateServiceAccountUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let update_service_account_use_case = Arc::new(UpdateServiceAccountUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let delete_service_account_use_case = Arc::new(DeleteServiceAccountUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let assign_roles_use_case = Arc::new(AssignRolesUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let regenerate_token_use_case = Arc::new(RegenerateAuthTokenUseCase::new(service_account_repo.clone(), unit_of_work.clone()));
    let regenerate_secret_use_case = Arc::new(RegenerateSigningSecretUseCase::new(service_account_repo.clone(), unit_of_work.clone()));

    let create_dispatch_pool_use_case = Arc::new(CreateDispatchPoolUseCase::new(dispatch_pool_repo.clone(), unit_of_work.clone()));
    let update_dispatch_pool_use_case = Arc::new(UpdateDispatchPoolUseCase::new(dispatch_pool_repo.clone(), unit_of_work.clone()));
    let archive_dispatch_pool_use_case = Arc::new(ArchiveDispatchPoolUseCase::new(dispatch_pool_repo.clone(), unit_of_work.clone()));
    let delete_dispatch_pool_use_case = Arc::new(DeleteDispatchPoolUseCase::new(dispatch_pool_repo.clone(), unit_of_work.clone()));
    info!("Use cases initialized");

    // Sync code-defined roles to database
    {
        let role_sync = fc_platform::service::RoleSyncService::new(
            RoleRepository::new(&pg_db)
        );
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            tracing::warn!("Role sync failed: {}", e);
        }
    }

    // 8c. Initialize auth services (auto-generate RSA keys for dev, like Java)
    let (private_key, public_key) = AuthConfig::load_or_generate_rsa_keys(None, None)
        .expect("Failed to initialize JWT keys");

    let auth_config = AuthConfig {
        rsa_private_key: Some(private_key),
        rsa_public_key: Some(public_key),
        secret_key: String::new(),
        issuer: "flowcatalyst".to_string(),
        audience: "flowcatalyst".to_string(),
        access_token_expiry_secs: 3600,      // 1 hour (PT1H)
        session_token_expiry_secs: 28800,    // 8 hours (PT8H)
        refresh_token_expiry_secs: 86400 * 30, // 30 days (P30D)
    };
    let auth_service = Arc::new(AuthService::new(auth_config));
    let authz_service = Arc::new(AuthorizationService::new(role_repo.clone()));
    info!("Auth services initialized");

    // 8d. Create AppState for authentication middleware
    let app_state = AppState {
        auth_service: auth_service.clone(),
        authz_service: authz_service.clone(),
    };

    // 8e. Build API states
    let events_state = EventsState { event_repo: event_repo.clone() };
    let event_types_state = EventTypesState { event_type_repo: event_type_repo.clone() };
    let dispatch_jobs_state = DispatchJobsState { dispatch_job_repo: dispatch_job_repo.clone() };
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
    };
    let roles_state = RolesState { role_repo: role_repo.clone(), application_repo: Some(application_repo.clone()) };
    let subscriptions_state = SubscriptionsState { subscription_repo: subscription_repo.clone() };
    let oauth_clients_state = OAuthClientsState { oauth_client_repo: oauth_client_repo.clone() };
    let auth_config_state = AuthConfigState {
        anchor_domain_repo: anchor_domain_repo.clone(),
        client_auth_config_repo: client_auth_config_repo.clone(),
        idp_role_mapping_repo: idp_role_mapping_repo.clone(),
        principal_repo: Some(principal_repo.clone()),
    };
    let audit_logs_state = AuditLogsState { audit_log_repo: audit_log_repo.clone() };
    let applications_state = ApplicationsState {
        application_repo: application_repo.clone(),
        service_account_repo: service_account_repo.clone(),
        role_repo: role_repo.clone(),
        client_config_repo: application_client_config_repo.clone(),
        client_repo: client_repo.clone(),
        create_use_case: create_application_use_case,
        update_use_case: update_application_use_case,
        activate_use_case: activate_application_use_case,
        deactivate_use_case: deactivate_application_use_case,
    };
    let dispatch_pools_state = DispatchPoolsState {
        dispatch_pool_repo: dispatch_pool_repo.clone(),
        create_use_case: create_dispatch_pool_use_case,
        update_use_case: update_dispatch_pool_use_case,
        archive_use_case: archive_dispatch_pool_use_case,
        delete_use_case: delete_dispatch_pool_use_case,
    };
    let service_accounts_state = ServiceAccountsState {
        repo: service_account_repo.clone(),
        create_use_case: create_service_account_use_case,
        update_use_case: update_service_account_use_case,
        delete_use_case: delete_service_account_use_case,
        assign_roles_use_case,
        regenerate_token_use_case,
        regenerate_secret_use_case,
    };
    let debug_state = DebugState {
        event_repo: event_repo.clone(),
        dispatch_job_repo: dispatch_job_repo.clone(),
    };

    // Monitoring state with leader election and circuit breakers
    let monitoring_state = MonitoringState {
        leader_state: LeaderState::new(uuid::Uuid::new_v4().to_string()),
        circuit_breakers: CircuitBreakerRegistry::new(),
        in_flight: InFlightTracker::new(),
        dispatch_job_repo: dispatch_job_repo.clone(),
        start_time: std::time::Instant::now(),
    };

    // 8f. Build platform API router with all endpoints
    let platform_router = Router::new()
        // BFF APIs (under /bff to match frontend expectations)
        .nest("/bff/events", events_router(events_state).into())
        .nest("/bff/event-types", event_types_router(event_types_state).into())
        .nest("/bff/dispatch-jobs", dispatch_jobs_router(dispatch_jobs_state).into())
        .nest("/bff/filter-options", filter_options_router(filter_options_state).into())
        .nest("/bff/roles", roles_router(roles_state.clone()).into())
        // Debug BFF APIs (raw data access)
        .nest("/bff/debug/events", debug_events_router(debug_state.clone()).into())
        .nest("/bff/debug/dispatch-jobs", debug_dispatch_jobs_router(debug_state).into())
        // Admin APIs (under /api/admin to match Java paths)
        .nest("/api/admin/clients", clients_router(clients_state).into())
        .nest("/api/admin/principals", principals_router(principals_state).into())
        .nest("/api/admin/roles", roles_router(roles_state).into())
        .nest("/api/admin/subscriptions", subscriptions_router(subscriptions_state).into())
        .nest("/api/admin/oauth-clients", oauth_clients_router(oauth_clients_state).into())
        .nest("/api/admin/anchor-domains", anchor_domains_router(auth_config_state.clone()).into())
        .nest("/api/admin/auth-configs", client_auth_configs_router(auth_config_state.clone()).into())
        .nest("/api/admin/idp-role-mappings", idp_role_mappings_router(auth_config_state).into())
        .nest("/api/admin/audit-logs", audit_logs_router(audit_logs_state).into())
        .nest("/api/admin/applications", applications_router(applications_state).into())
        .nest("/api/admin/dispatch-pools", dispatch_pools_router(dispatch_pools_state).into())
        .nest("/api/admin/service-accounts", service_accounts_router(service_accounts_state).into())
        // Monitoring APIs
        .nest("/api/monitoring", monitoring_router(monitoring_state).into())
        // Add auth middleware
        .layer(AuthLayer::new(app_state));

    info!("Platform APIs configured");

    // 9. Start API server (merge router API with platform APIs)
    let router_circuit_breaker = Arc::new(RouterCircuitBreakerRegistry::default());
    let router_api = create_api_router(
        queue.clone(),
        queue_manager.clone(),
        warning_service.clone(),
        health_service.clone(),
        router_circuit_breaker,
    );

    let api_app = Router::new()
        .merge(router_api)
        .merge(platform_router)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any));

    let api_addr = format!("0.0.0.0:{}", args.api_port);
    info!("API server listening on http://{}", api_addr);

    let api_listener = TcpListener::bind(&api_addr).await?;
    let api_handle = {
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let server = axum::serve(api_listener, api_app);
            tokio::select! {
                result = server => {
                    if let Err(e) = result {
                        error!("API server error: {}", e);
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("API server shutting down");
                }
            }
        })
    };

    // 10. Start metrics server
    let metrics_addr = format!("0.0.0.0:{}", args.metrics_port);
    info!("Metrics server listening on http://{}/metrics", metrics_addr);

    let metrics_app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler));

    let metrics_listener = TcpListener::bind(&metrics_addr).await?;
    let metrics_handle = {
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let server = axum::serve(metrics_listener, metrics_app);
            tokio::select! {
                result = server => {
                    if let Err(e) = result {
                        error!("Metrics server error: {}", e);
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Metrics server shutting down");
                }
            }
        })
    };

    // 11. Start QueueManager (blocking - runs consumer loops)
    let manager_handle = {
        let manager = queue_manager.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            tokio::select! {
                result = manager.clone().start() => {
                    if let Err(e) = result {
                        error!("QueueManager error: {}", e);
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("QueueManager received shutdown signal");
                    manager.shutdown().await;
                }
            }
        })
    };

    info!("FlowCatalyst Dev Monolith started successfully");
    info!("Press Ctrl+C to shutdown");

    // Wait for shutdown signal
    shutdown_signal().await;
    info!("Shutdown signal received, initiating graceful shutdown...");

    // Broadcast shutdown to all components
    let _ = shutdown_tx.send(());

    // Stop lifecycle manager
    lifecycle.shutdown().await;

    // Wait for all handles with timeout
    let shutdown_timeout = Duration::from_secs(30);
    let _ = tokio::time::timeout(shutdown_timeout, async {
        let _ = api_handle.await;
        let _ = metrics_handle.await;
        let _ = manager_handle.await;
        if let Some(h) = outbox_handle {
            let _ = h.await;
        }
    }).await;

    info!("FlowCatalyst Dev Monolith shutdown complete");
    Ok(())
}

async fn create_outbox_repository(args: &Args) -> Result<Arc<dyn OutboxRepository>> {
    match args.outbox_db_type.as_str() {
        "sqlite" => {
            let url = args.outbox_db_url.as_deref().unwrap_or("sqlite::memory:");
            let pool = SqlitePoolOptions::new()
                .max_connections(2)
                .connect(url)
                .await?;
            let repo = fc_outbox::sqlite::SqliteOutboxRepository::new(pool);
            repo.init_schema().await?;
            info!("Outbox using SQLite: {}", url);
            Ok(Arc::new(repo))
        }
        "postgres" => {
            let url = args.outbox_db_url.as_ref()
                .ok_or_else(|| anyhow::anyhow!("FC_OUTBOX_DB_URL required for postgres"))?;
            let pool = PgPoolOptions::new()
                .max_connections(5)
                .connect(url)
                .await?;
            let repo = fc_outbox::postgres::PostgresOutboxRepository::new(pool);
            repo.init_schema().await?;
            info!("Outbox using PostgreSQL");
            Ok(Arc::new(repo))
        }
        "mongo" => {
            let url = args.outbox_db_url.as_ref()
                .ok_or_else(|| anyhow::anyhow!("FC_OUTBOX_DB_URL required for mongo"))?;
            let client = mongodb::Client::with_uri_str(url).await?;
            let repo = fc_outbox::mongo::MongoOutboxRepository::new(
                client,
                &args.outbox_mongo_db,
            );
            info!("Outbox using MongoDB: {} (collections: outbox_events, outbox_dispatch_jobs)", args.outbox_mongo_db);
            Ok(Arc::new(repo))
        }
        other => {
            Err(anyhow::anyhow!("Unknown outbox database type: {}. Use sqlite, postgres, or mongo", other))
        }
    }
}

/// Adapter to use QueuePublisher as outbox publisher
struct OutboxQueuePublisher {
    queue: Arc<dyn QueuePublisher>,
}

impl OutboxQueuePublisher {
    fn new(queue: Arc<dyn QueuePublisher>) -> Self {
        Self { queue }
    }
}

#[async_trait::async_trait]
impl fc_outbox::QueuePublisher for OutboxQueuePublisher {
    async fn publish(&self, message: fc_common::Message) -> Result<()> {
        self.queue.publish(message).await
            .map_err(|e| anyhow::anyhow!("Queue publish error: {}", e))?;
        Ok(())
    }
}

async fn metrics_handler() -> &'static str {
    // In a real implementation, you'd use metrics-exporter-prometheus
    // For now, return basic Prometheus format
    "# HELP fc_up FlowCatalyst is up\n# TYPE fc_up gauge\nfc_up 1\n"
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "UP",
        "version": env!("CARGO_PKG_VERSION"),
        "components": {
            "queue": "UP",
            "router": "UP"
        }
    }))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
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
