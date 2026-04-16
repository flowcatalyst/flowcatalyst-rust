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
use tokio::sync::broadcast;
use tokio::net::TcpListener;
use anyhow::Result;
use tracing::{info, warn, error};
use axum::{
    routing::get,
    response::Json,
    Router,
};
use tower_http::cors::{CorsLayer, Any};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tower_http::services::{ServeDir, ServeFile};
use axum::http::header::CACHE_CONTROL;
use axum::http::HeaderValue;

use rust_embed::Embed;

use fc_common::{RouterConfig, PoolConfig, QueueConfig};

/// Embedded frontend static files (compiled into the binary from frontend/dist/).
/// In dev, set FC_STATIC_DIR to override with a live directory.
#[derive(Embed)]
#[folder = "../../frontend/dist/"]
#[prefix = ""]
struct FrontendAssets;
use fc_router::{
    QueueManager, HttpMediator, LifecycleManager, LifecycleConfig,
    WarningService, WarningServiceConfig, HealthService, HealthServiceConfig,
    CircuitBreakerRegistry as RouterCircuitBreakerRegistry,
    api::create_router as create_api_router,
};
use fc_queue::sqlite::SqliteQueue;
use fc_queue::EmbeddedQueue;
use fc_outbox::enhanced_processor::{EnhancedOutboxProcessor, EnhancedProcessorConfig};
use fc_outbox::http_dispatcher::HttpDispatcherConfig;
use fc_outbox::postgres::PostgresOutboxRepository;

// Platform imports
use fc_platform::api::middleware::{AppState, AuthLayer};
use fc_platform::api::{event_type_filters_router, dispatch_jobs_router};
use fc_platform::repository::{
    Repositories,
    RoleRepository,
};
use fc_platform::usecase::PgUnitOfWork;

use sqlx::sqlite::SqlitePoolOptions;

/// FlowCatalyst Development Server
#[derive(Parser, Debug)]
#[command(name = "fc-dev")]
#[command(about = "FlowCatalyst Development Monolith - All components in one binary")]
struct Args {
    /// API server port
    #[arg(long, env = "FC_API_PORT", default_value = "3000")]
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

    /// Enable dispatch scheduler (polls PENDING jobs and queues them)
    #[arg(long, env = "FC_SCHEDULER_ENABLED", default_value = "true")]
    scheduler_enabled: bool,

    /// Enable outbox processor
    #[arg(long, env = "FC_OUTBOX_ENABLED", default_value = "false")]
    outbox_enabled: bool,

    /// Outbox poll interval in milliseconds
    #[arg(long, env = "FC_OUTBOX_POLL_INTERVAL_MS", default_value = "1000")]
    outbox_poll_interval_ms: u64,

    // Platform configuration

    /// PostgreSQL database URL
    #[arg(long, env = "FC_DATABASE_URL", default_value = "postgresql://localhost:5432/flowcatalyst")]
    database_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env.development (or .env) if present
    let _ = dotenvy::from_filename(".env.development")
        .or_else(|_| dotenvy::dotenv());

    // Set dev defaults for env vars that aren't set
    // These make fc-dev zero-config (only DB URL needed).
    if std::env::var("FLOWCATALYST_APP_KEY").is_err() {
        std::env::set_var("FLOWCATALYST_APP_KEY", "MpU3dI07kjZmZGROrElYfDXQgab30e3wr0KTnxQbePg=");
    }
    if std::env::var("FC_DEV_MODE").is_err() {
        std::env::set_var("FC_DEV_MODE", "true");
    }

    // Initialize logging (JSON if LOG_FORMAT=json, text otherwise)
    fc_common::logging::init_logging("fc-dev");

    let args = Args::parse();

    info!("Starting FlowCatalyst Dev Monolith (Rust)");
    info!("API port: {}, Metrics port: {}", args.api_port, args.metrics_port);

    // Setup shutdown signal
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // 1. Setup SQLite for embedded queue (file-based so it persists across restarts)
    let queue_pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite:fc_dev_queue.db?mode=rwc")
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
                uri: "sqlite:fc_dev_queue.db?mode=rwc".to_string(),
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

    // 7. Outbox processor — deferred until after AuthService is ready (needs a service token).
    //    We store the config now and start it after step 8c.
    let outbox_pool: Option<sqlx::PgPool> = if args.outbox_enabled && args.outbox_db_type == "postgres" {
        let outbox_db_url = args.outbox_db_url.as_deref()
            .unwrap_or(&args.database_url);
        info!(
            db_type = %args.outbox_db_type,
            db_url = %outbox_db_url,
            poll_interval_ms = args.outbox_poll_interval_ms,
            "Connecting to outbox database"
        );
        Some(sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(outbox_db_url)
            .await
            .map_err(|e| anyhow::anyhow!("Outbox PostgreSQL connection failed: {}", e))?)
    } else {
        None
    };

    // 8. Setup platform services and APIs
    info!("Initializing platform services...");

    // 8a. Connect to PostgreSQL
    info!("Connecting to PostgreSQL...");
    let pg_pool = fc_platform::shared::database::create_pool(&args.database_url).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    // Run PostgreSQL migrations
    fc_platform::shared::database::run_migrations(&pg_pool).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL migrations failed: {}", e))?;

    // Seed development data
    let seeder = fc_platform::seed::DevDataSeeder::new(pg_pool.clone());
    if let Err(e) = seeder.seed().await {
        tracing::warn!("Dev data seeding skipped (data may already exist): {}", e);
    }

    // 8c. Initialize all repositories
    let repos = Repositories::new(&pg_pool);
    info!("Platform repositories initialized");

    // 8b1.5 Start CQRS stream processor (projects msg_events → msg_events_read, etc.)
    let stream_handle = {
        let config = fc_stream::StreamProcessorConfig {
            events_enabled: true,
            events_batch_size: 100,
            dispatch_jobs_enabled: true,
            dispatch_jobs_batch_size: 100,
        };
        let (handle, _health) = fc_stream::start_stream_processor(pg_pool.clone(), config);
        info!("Stream processor started (event + dispatch job projections)");
        handle
    };

    // 8b2. Create UnitOfWork for atomic commits
    let unit_of_work = Arc::new(PgUnitOfWork::new(pg_pool.clone()));

    // Sync code-defined roles to database
    {
        let role_sync = fc_platform::service::RoleSyncService::new(
            RoleRepository::new(&pg_pool)
        );
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            tracing::warn!("Role sync failed: {}", e);
        }
    }

    // 8c. Initialize auth services (auto-generate RSA keys for dev, like Java)
    let auth_services = fc_platform::shared::server_setup::init_auth_services(
        &repos,
        fc_platform::shared::server_setup::AuthInitConfig::from_env("http://localhost:8080"),
    ).expect("Failed to initialize auth services");
    info!("Auth services initialized");

    // 7b. Start outbox processor now that AuthService is ready — generate a
    //     long-lived internal service token so the outbox HTTP dispatcher can
    //     authenticate against the SDK batch endpoints.
    let outbox_handle: Option<tokio::task::JoinHandle<()>> = if let Some(pool) = outbox_pool {
        use fc_platform::principal::entity::Principal;

        let internal_principal = Principal::new_service("outbox-processor", "Outbox Processor (internal)");
        let token = auth_services.auth.generate_access_token(&internal_principal)
            .map_err(|e| anyhow::anyhow!("Failed to generate outbox service token: {}", e))?;
        info!("Generated internal service token for outbox processor");

        let repository = Arc::new(PostgresOutboxRepository::new(pool));
        let api_base_url = format!("http://localhost:{}", args.api_port);

        let config = EnhancedProcessorConfig {
            poll_interval: Duration::from_millis(args.outbox_poll_interval_ms),
            http_config: HttpDispatcherConfig {
                api_base_url,
                api_token: Some(token),
                ..Default::default()
            },
            ..Default::default()
        };

        let processor = Arc::new(
            EnhancedOutboxProcessor::new(config, repository)
                .map_err(|e| anyhow::anyhow!("Failed to create outbox processor: {}", e))?
        );

        let proc_clone = processor.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = processor.start() => {}
                _ = shutdown_rx.recv() => {
                    info!("Outbox processor received shutdown signal");
                    proc_clone.stop();
                }
            }
        });

        info!("Outbox processor started");
        Some(handle)
    } else {
        None
    };

    // 7c. Start dispatch scheduler (polls PENDING jobs → publishes to queue → router delivers)
    let _scheduler_handle: Option<tokio::task::JoinHandle<()>> = if args.scheduler_enabled {
        use fc_platform::scheduler::{DispatchScheduler, SchedulerConfig};

        let config = SchedulerConfig {
            processing_endpoint: format!("http://localhost:{}/api/dispatch/process", args.api_port),
            ..SchedulerConfig::default()
        };

        // Pass the SQLite queue publisher directly — no bridge needed
        let scheduler = Arc::new(DispatchScheduler::new(config, pg_pool.clone(), queue.clone()));

        let mut shutdown_rx = shutdown_tx.subscribe();
        let sched_clone = scheduler.clone();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = scheduler.start() => {}
                _ = shutdown_rx.recv() => {
                    info!("Dispatch scheduler received shutdown signal");
                    sched_clone.stop().await;
                }
            }
        });

        info!("Dispatch scheduler started (polling PENDING jobs)");
        Some(handle)
    } else {
        None
    };

    // 8d. Create AppState for authentication middleware
    let app_state = AppState {
        auth_service: auth_services.auth.clone(),
        authz_service: auth_services.authz.clone(),
    };

    // 8e. Build platform API router via shared builder (handles ~38 state structs).
    // fc-dev embeds the queue, so `event_dispatch` is populated with deps pointing
    // at the in-process queue publisher.
    let routes = fc_platform::shared::server_setup::build_platform_routes(
        &repos,
        &auth_services,
        &unit_of_work,
        fc_platform::shared::server_setup::PlatformRoutesConfig {
            event_dispatch: Some(fc_platform::api::EventDispatchDeps {
                subscription_repo: repos.subscription_repo.clone(),
                dispatch_job_repo: repos.dispatch_job_repo.clone(),
                queue_publisher: queue.clone(),
                dispatch_process_url: format!("http://localhost:{}/api/dispatch/process", args.api_port),
            }),
            session_cookie_secure: false,
            static_dir: None, // fc-dev handles SPA serving itself (embedded or FC_STATIC_DIR)
            oidc_login_external_base_url: Some(
                std::env::var("FC_EXTERNAL_BASE_URL")
                    .unwrap_or_else(|_| "http://localhost:4200".to_string()),
            ),
            well_known_external_base_url: format!("http://localhost:{}", args.api_port),
            password_reset_external_base_url: format!("http://localhost:{}", args.api_port),
        },
    );
    let (platform_app, _openapi) = routes.build();

    // Dev-specific extra route states (the shared builder doesn't wire
    // /api/dispatch-jobs or /api/event-types/filters — fc-dev does
    // this itself as compatibility for the generated frontend client).
    let dispatch_jobs_state = fc_platform::api::DispatchJobsState {
        dispatch_job_repo: repos.dispatch_job_repo.clone(),
    };
    let filter_options_state = fc_platform::api::FilterOptionsState {
        client_repo: repos.client_repo.clone(),
        event_type_repo: repos.event_type_repo.clone(),
        subscription_repo: repos.subscription_repo.clone(),
        dispatch_pool_repo: repos.dispatch_pool_repo.clone(),
        application_repo: repos.application_repo.clone(),
    };

    // Dev-specific extra routes: API-surface mirrors of BFF routes.
    // NOTE: /api/events is now provided by PlatformRoutes via admin_events_router.
    let platform_router = platform_app
        .nest("/api/dispatch-jobs", dispatch_jobs_router(dispatch_jobs_state).into())
        .nest("/api/event-types/filters", event_type_filters_router(filter_options_state).into())
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
        .nest("/q/router", router_api)
        .merge(platform_router)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any));

    // Static frontend serving — uses FC_STATIC_DIR if set (for live reload),
    // otherwise serves from the embedded frontend assets compiled into the binary.
    let api_app = if let Ok(static_dir) = std::env::var("FC_STATIC_DIR") {
        let index_path = std::path::PathBuf::from(&static_dir).join("index.html");
        if index_path.exists() {
            info!(dir = %static_dir, "Serving frontend from filesystem (live reload)");
            let assets_dir = std::path::PathBuf::from(&static_dir).join("assets");
            let assets_service = tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::overriding(
                    CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                ))
                .service(ServeDir::new(&assets_dir));

            api_app
                .route("/auth/login", axum::routing::get(embedded_spa_handler))
                .route("/auth/forgot-password", axum::routing::get(embedded_spa_handler))
                .route("/auth/reset-password", axum::routing::get(embedded_spa_handler))
                .nest_service("/assets", assets_service)
                .fallback_service(
                    ServeDir::new(&static_dir)
                        .fallback(ServeFile::new(index_path))
                )
        } else {
            warn!(dir = %static_dir, "FC_STATIC_DIR set but index.html not found — using embedded assets");
            api_app.fallback(axum::routing::get(embedded_asset_handler))
        }
    } else {
        info!("Serving embedded frontend (compiled into binary)");
        api_app
            .route("/auth/login", axum::routing::get(embedded_spa_handler))
            .route("/auth/forgot-password", axum::routing::get(embedded_spa_handler))
            .route("/auth/reset-password", axum::routing::get(embedded_spa_handler))
            .fallback(axum::routing::get(embedded_asset_handler))
    };

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
    fc_platform::shared::server_setup::wait_for_shutdown_signal().await;
    info!("Shutdown signal received, initiating graceful shutdown...");

    // Broadcast shutdown to all components
    let _ = shutdown_tx.send(());

    // Stop lifecycle manager and stream processor
    lifecycle.shutdown().await;
    stream_handle.stop().await;

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

/// Serve embedded frontend assets. Handles all GET requests that don't match API routes.
/// For HTML requests or root, serves index.html (SPA fallback).
/// For asset requests, serves the matching embedded file with correct MIME type.
async fn embedded_asset_handler(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // Try exact path first (for assets like /assets/index-BKjElYp6.js)
    if let Some(file) = FrontendAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            mime.as_ref().parse().unwrap(),
        );
        // Immutable cache for hashed assets
        if path.starts_with("assets/") {
            headers.insert(
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".parse().unwrap(),
            );
        }
        return (headers, file.data.to_vec()).into_response();
    }

    // SPA fallback: serve index.html for all other paths
    embedded_spa_handler().await.into_response()
}

/// Serve the embedded index.html (SPA entry point).
async fn embedded_spa_handler() -> impl axum::response::IntoResponse {
    match FrontendAssets::get("index.html") {
        Some(file) => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                "text/html; charset=utf-8".parse().unwrap(),
            );
            (headers, file.data.to_vec()).into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            "Frontend not embedded in this build",
        ).into_response(),
    }
}

use axum::response::IntoResponse;
