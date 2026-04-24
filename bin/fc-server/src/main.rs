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
use tower_http::cors::{CorsLayer, AllowOrigin};
// SetResponseHeaderLayer moved to PlatformRoutes
use tower_http::trace::TraceLayer;
// CACHE_CONTROL moved to PlatformRoutes
// SPA serving is handled by PlatformRoutes::build()
use axum::http::{Method, HeaderValue, header as http_header};
use anyhow::Result;
use tracing::{info, warn, error};
use tokio::{net::TcpListener, sync::watch};

use fc_platform::api::middleware::{AppState, AuthLayer};
use fc_platform::repository::{
    Repositories,
    CorsOriginRepository,
};
use fc_platform::usecase::PgUnitOfWork;
use fc_platform::seed::DevDataSeeder;

use fc_common::config::{env_or, env_or_alias, env_or_parse, env_or_alias_parse, env_bool, env_bool_alias};

/// Resolve database URL and (optionally) the live `SecretProvider` it came from.
///
/// Supports three modes:
/// 1. `FC_DATABASE_URL` / `DATABASE_URL` — full connection string (preferred for Rust)
/// 2. `DB_HOST` + `DB_NAME` + `DB_SECRET_ARN` — AWS Secrets Manager (TS compatibility)
/// 3. `DB_HOST` + `DB_NAME` + `DB_USERNAME` + `DB_PASSWORD` — explicit credentials
///
/// When mode 2 is used the returned `SecretProvider` is also returned so the
/// caller can spawn the background credential-refresh task.
async fn resolve_database_url() -> Result<(String, Option<Arc<dyn fc_platform::shared::database::SecretProvider>>)> {
    // Mode 1: Full connection string
    if let Ok(url) = std::env::var("FC_DATABASE_URL").or_else(|_| std::env::var("DATABASE_URL")) {
        return Ok((url, None));
    }

    // Mode 2/3: Build from components
    let host = std::env::var("DB_HOST")
        .map_err(|_| anyhow::anyhow!("No database config found. Set FC_DATABASE_URL or DB_HOST+DB_NAME"))?;
    let name = env_or("DB_NAME", "flowcatalyst");
    let port = env_or("DB_PORT", "5432");

    // Try AWS Secrets Manager
    if let Ok(secret_arn) = std::env::var("DB_SECRET_ARN") {
        let provider_kind = env_or("DB_SECRET_PROVIDER", "aws");
        if provider_kind == "aws" {
            info!(secret_arn = %secret_arn, "Resolving database credentials from AWS Secrets Manager");
            let provider = Arc::new(fc_platform::shared::database::AwsSecretProvider::new(
                secret_arn,
                host.clone(),
                name.clone(),
                port.clone(),
            ));
            let url = fc_platform::shared::database::SecretProvider::get_db_url(provider.as_ref()).await?;
            info!("Database URL resolved from Secrets Manager (host: {}, db: {})", host, name);
            return Ok((url, Some(provider as Arc<dyn fc_platform::shared::database::SecretProvider>)));
        }
    }

    // Mode 3: Explicit credentials
    let username = env_or("DB_USERNAME", "postgres");
    let password = env_or("DB_PASSWORD", "");
    let host_port = if host.contains(':') {
        host.clone()
    } else {
        format!("{}:{}", host, port)
    };
    let url = if password.is_empty() {
        format!("postgresql://{}@{}/{}", username, host_port, name)
    } else {
        let password_encoded = urlencoding::encode(&password);
        format!("postgresql://{}:{}@{}/{}", username, password_encoded, host_port, name)
    };
    Ok((url, None))
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    fc_common::logging::init_logging("fc-server");

    info!("Starting FlowCatalyst Unified Server");

    // ── Configuration ────────────────────────────────────────────────────────
    // Each env var supports both the Rust name (FC_ prefix) and the TS name for
    // compatibility with existing ECS task definitions.
    let api_port: u16 = env_or_alias_parse("FC_API_PORT", "PORT", 3000);
    let metrics_port: u16 = env_or_parse("FC_METRICS_PORT", 9090);
    let (database_url, secret_provider) = resolve_database_url().await?;
    // JWT issuer should be the external base URL per OIDC spec
    let jwt_issuer = std::env::var("FC_JWT_ISSUER")
        .or_else(|_| std::env::var("FC_EXTERNAL_BASE_URL"))
        .or_else(|_| std::env::var("EXTERNAL_BASE_URL"))
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    // Subsystem toggles (TS names: PLATFORM_ENABLED, MESSAGE_ROUTER_ENABLED, etc.)
    let platform_enabled = env_bool_alias("FC_PLATFORM_ENABLED", "PLATFORM_ENABLED", true);
    let router_enabled = env_bool_alias("FC_ROUTER_ENABLED", "MESSAGE_ROUTER_ENABLED", false);
    let scheduler_enabled = env_bool_alias("FC_SCHEDULER_ENABLED", "DISPATCH_SCHEDULER_ENABLED", false);
    let stream_enabled = env_bool_alias("FC_STREAM_PROCESSOR_ENABLED", "STREAM_PROCESSOR_ENABLED", false);
    let outbox_enabled = env_bool_alias("FC_OUTBOX_ENABLED", "OUTBOX_PROCESSOR_ENABLED", false);

    // Standby / HA
    let standby_enabled = env_bool_alias("FC_STANDBY_ENABLED", "STANDBY_ENABLED", false);
    let standby_redis_url = env_or_alias("FC_STANDBY_REDIS_URL", "REDIS_URL", "redis://127.0.0.1:6379");
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
    let pg_pool = fc_platform::shared::database::create_pool(&database_url).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    fc_platform::shared::database::run_migrations(&pg_pool).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL migrations failed: {}", e))?;

    // Referential-integrity scan — warns about orphaned junction rows.
    fc_platform::shared::integrity_scan::run(&pg_pool).await;

    // Dev mode seeding
    if env_bool("FC_DEV_MODE", false) {
        let seeder = DevDataSeeder::new(pg_pool.clone());
        if let Err(e) = seeder.seed().await {
            warn!("Dev data seeding skipped: {}", e);
        }
    }

    // ── DB credential refresh (AWS Secrets Manager rotation) ─────────────────
    // When credentials come from a secret provider, poll it on an interval and
    // update the pool's connect options when the password rotates. This avoids
    // the failure mode where AWS rotates the password and the pool keeps using
    // the now-stale credentials. Mirrors the TS implementation.
    if let Some(provider) = secret_provider {
        let interval_ms: u64 = env_or_parse("DB_SECRET_REFRESH_INTERVAL_MS", 300_000);
        fc_platform::shared::database::start_secret_refresh(
            provider,
            pg_pool.clone(),
            database_url.clone(),
            std::time::Duration::from_millis(interval_ms),
        );
    }

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

    let repos = Repositories::new(&pg_pool);
    info!("Repositories initialized");

    // CORS origins cache
    let cors_origins_cache: Arc<std::sync::RwLock<std::collections::HashSet<String>>> =
        Arc::new(std::sync::RwLock::new(std::collections::HashSet::new()));
    {
        match repos.cors_repo.get_allowed_origins().await {
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
        let cors_repo_bg = CorsOriginRepository::new(&pg_pool);
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
            fc_platform::repository::RoleRepository::new(&pg_pool),
        );
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            warn!("Role sync failed: {}", e);
        }
    }

    // Auth services
    let auth_init_config = fc_platform::shared::server_setup::AuthInitConfig {
        issuer: jwt_issuer,
        ..fc_platform::shared::server_setup::AuthInitConfig::from_env("http://localhost:3000")
    };
    let auth_services = fc_platform::shared::server_setup::init_auth_services(&repos, auth_init_config)?;
    info!("Auth services initialized");

    let unit_of_work = Arc::new(PgUnitOfWork::new(pg_pool.clone()));

    // ── Build HTTP app ───────────────────────────────────────────────────────
    let app = if platform_enabled {
        build_platform_app(
            api_port,
            &auth_services,
            &unit_of_work,
            &repos,
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
        spawn_scheduler(&pg_pool, active_rx.clone()).await?;
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
    fc_platform::shared::server_setup::wait_for_shutdown_signal().await;
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

fn build_platform_app(
    api_port: u16,
    auth_services: &fc_platform::shared::server_setup::AuthServices,
    unit_of_work: &Arc<PgUnitOfWork>,
    repos: &Repositories,
    cors_origins_cache: &Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    _standby_enabled: bool,
) -> Router {
    let app_state = AppState {
        auth_service: auth_services.auth.clone(),
        authz_service: auth_services.authz.clone(),
    };

    // Build platform API router via shared builder (handles ~38 state structs)
    let routes = fc_platform::shared::server_setup::build_platform_routes(
        repos,
        auth_services,
        unit_of_work,
        fc_platform::shared::server_setup::PlatformRoutesConfig {
            event_dispatch: None,
            session_cookie_secure: true,
            static_dir: std::env::var("FC_STATIC_DIR").ok(),
            oidc_login_external_base_url: std::env::var("FC_EXTERNAL_BASE_URL")
                .or_else(|_| std::env::var("EXTERNAL_BASE_URL"))
                .ok(),
            well_known_external_base_url: std::env::var("FC_EXTERNAL_BASE_URL")
                .or_else(|_| std::env::var("EXTERNAL_BASE_URL"))
                .unwrap_or_else(|_| format!("http://localhost:{}", api_port)),
            password_reset_external_base_url: std::env::var("FC_EXTERNAL_BASE_URL")
                .or_else(|_| std::env::var("EXTERNAL_BASE_URL"))
                .unwrap_or_else(|_| format!("http://localhost:{}", api_port)),
        },
    );
    let (app, _openapi) = routes.build();

    // Add middleware layers
    let app = app
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

    // SPA serving is now handled by PlatformRoutes::build() via the static_dir field.
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
    pg_pool: &sqlx::PgPool,
    mut active_rx: watch::Receiver<bool>,
) -> Result<()> {
    use fc_platform::scheduler::DispatchScheduler;

    struct NoopQueuePublisher;

    #[async_trait::async_trait]
    impl fc_queue::QueuePublisher for NoopQueuePublisher {
        fn identifier(&self) -> &str { "noop-scheduler" }
        async fn publish(&self, message: fc_common::Message) -> fc_queue::Result<String> {
            info!(id = %message.id, "Scheduler: message published (noop)");
            Ok(message.id)
        }
        async fn publish_batch(&self, messages: Vec<fc_common::Message>) -> fc_queue::Result<Vec<String>> {
            let ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
            for m in &messages {
                info!(id = %m.id, "Scheduler: message published (noop)");
            }
            Ok(ids)
        }
    }

    let config = load_scheduler_config();
    let queue_publisher: Arc<dyn fc_queue::QueuePublisher> = Arc::new(NoopQueuePublisher);
    let scheduler = Arc::new(DispatchScheduler::new(config, pg_pool.clone(), queue_publisher));

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

fn load_scheduler_config() -> fc_platform::scheduler::SchedulerConfig {
    let config = fc_config::AppConfig::load().unwrap_or_default();
    fc_platform::scheduler::SchedulerConfig {
        enabled: config.scheduler.enabled,
        poll_interval: Duration::from_millis(config.scheduler.poll_interval_ms),
        batch_size: config.scheduler.batch_size,
        stale_threshold: Duration::from_secs(config.scheduler.stale_threshold_minutes * 60),
        default_dispatch_mode: fc_common::DispatchMode::from_str(&config.scheduler.default_dispatch_mode),
        default_pool_code: env_or("FC_SCHEDULER_DEFAULT_POOL_CODE", "DISPATCH-POOL"),
        processing_endpoint: env_or_alias("FC_SCHEDULER_PROCESSING_ENDPOINT", "DISPATCH_SCHEDULER_PROCESSING_ENDPOINT", "http://localhost:8080/api/dispatch/process"),
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

