//! FlowCatalyst Development Monolith
//!
//! All-in-one binary for local development containing:
//! - Message Router (with embedded SQLite queue)
//! - API Server (for publishing messages)
//! - Outbox Processor (configurable database backend)
//! - Platform APIs (events, subscriptions, auth, etc.)
//! - Metrics endpoint

use anyhow::Result;
use axum::http::header::CACHE_CONTROL;
use axum::http::HeaderValue;
use axum::{response::Json, routing::get, Router};
use clap::Parser;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

use rust_embed::Embed;

use fc_common::{PoolConfig, QueueConfig, RouterConfig};

/// Embedded frontend static files (compiled into the binary from frontend/dist/).
/// In dev, set FC_STATIC_DIR to override with a live directory.
#[derive(Embed)]
#[folder = "../../frontend/dist/"]
#[prefix = ""]
struct FrontendAssets;
use fc_outbox::enhanced_processor::{EnhancedOutboxProcessor, EnhancedProcessorConfig};
use fc_outbox::http_dispatcher::HttpDispatcherConfig;
use fc_outbox::postgres::PostgresOutboxRepository;
use fc_queue::postgres::PostgresQueue;
use fc_queue::EmbeddedQueue;
use fc_router::{
    api::create_router as create_api_router,
    CircuitBreakerRegistry as RouterCircuitBreakerRegistry, HealthService, HealthServiceConfig,
    HttpMediatorConfig, LifecycleConfig, LifecycleManager, QueueManager, WarningService,
    WarningServiceConfig,
};

// Platform imports
use fc_platform::api::event_type_filters_router;
use fc_platform::api::middleware::{AppState, AuthLayer};
use fc_platform::repository::{Repositories, RoleRepository};
use fc_platform::usecase::PgUnitOfWork;

/// FlowCatalyst Development Monolith — top-level CLI.
///
/// Default invocation (`fc-dev` with flags) runs the dev server. The
/// `upgrade` subcommand replaces the binary with the latest GitHub release.
#[derive(Parser, Debug)]
#[command(name = "fc-dev")]
#[command(version)]
#[command(about = "FlowCatalyst Development Monolith - All components in one binary")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    run: RunArgs,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Run the dev monolith. Identical to invoking `fc-dev` with no
    /// subcommand — kept for discoverability.
    Start(RunArgs),

    /// Run the FlowCatalyst MCP server (read-only access to event types
    /// and subscriptions for AI agents).
    ///
    /// Reads `FLOWCATALYST_URL`, `FLOWCATALYST_CLIENT_ID`, and
    /// `FLOWCATALYST_CLIENT_SECRET` from the environment.
    Mcp(McpArgs),

    /// Download the latest fc-dev release and replace this binary.
    Upgrade(UpgradeArgs),
}

#[derive(clap::Args, Debug)]
struct UpgradeArgs {
    /// Re-install even if the running binary is already on the latest version.
    #[arg(long)]
    force: bool,

    /// Check for a newer version without downloading.
    #[arg(long)]
    check: bool,
}

#[derive(clap::Args, Debug)]
struct McpArgs {
    /// Run as a streamable HTTP server instead of stdio.
    #[arg(long)]
    http: bool,

    /// Bind address for `--http` mode.
    #[arg(long, env = "FC_MCP_BIND", default_value = "127.0.0.1:3100")]
    bind: std::net::SocketAddr,
}

/// Flags for the (default) run-server path. Flattened into `Cli` so existing
/// invocations like `fc-dev --api-port 3000` keep working unchanged.
#[derive(clap::Args, Debug)]
struct RunArgs {
    /// API server port. Matches the project-wide convention used by the
    /// justfile and .env.development; production binaries also use 8080.
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
    #[arg(
        long,
        env = "FC_DATABASE_URL",
        default_value = "postgresql://localhost:5432/flowcatalyst"
    )]
    database_url: String,

    /// Start an embedded PostgreSQL instead of connecting to `--database-url`.
    /// First run downloads a ~80MB pg binary to `~/.cache/flowcatalyst-dev/pgdata/`.
    /// Set to `false` (or pass `--embedded-db=false`) to connect to an
    /// existing Postgres (e.g. the one you run in Docker). Only available
    /// when compiled with the `embedded-db` feature.
    #[cfg(feature = "embedded-db")]
    #[arg(long, env = "FC_EMBEDDED_DB", default_value = "true")]
    embedded_db: bool,

    /// Wipe the embedded Postgres data directory before starting. Only
    /// honoured when `--embedded-db` is active.
    #[cfg(feature = "embedded-db")]
    #[arg(long, env = "FC_RESET_DB", default_value = "false")]
    reset_db: bool,
}

#[cfg(feature = "embedded-db")]
mod embedded_pg {
    //! Bundles a PostgreSQL binary into fc-dev so a fresh clone can
    //! `./fc-dev` without running any database separately.
    //!
    //! The `bundled` feature on `postgresql_embedded` pulls the pg binary
    //! at **build** time and embeds it into the fc-dev executable. First
    //! run extracts from the exe itself — no runtime network call, no
    //! second binary for EDR or corporate allowlisting to review.

    use anyhow::{Context, Result};
    use postgresql_embedded::{PostgreSQL, Settings};
    use std::path::PathBuf;
    use tracing::info;

    pub struct EmbeddedDb {
        pub postgresql: PostgreSQL,
        pub url: String,
    }

    pub fn data_dir() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("flowcatalyst-dev")
            .join("pgdata")
    }

    pub async fn start(reset: bool) -> Result<EmbeddedDb> {
        let data_dir = data_dir();

        if reset && data_dir.exists() {
            info!(path = %data_dir.display(), "Resetting embedded Postgres data dir");
            std::fs::remove_dir_all(&data_dir)
                .context("Failed to remove embedded Postgres data dir")?;
        }

        // Pin the password so the data dir and the connection URL stay
        // consistent across restarts. `Settings::default()` generates a
        // *fresh* random password every process start and does NOT
        // persist it — initdb'd data from a previous run then no longer
        // matches, and connections fail.
        //
        // Username is pinned to "postgres" because postgresql_embedded
        // hardcodes that as the initdb bootstrap superuser regardless of
        // what we pass. Setting it to anything else only changes what
        // appears in `settings().url()`, not the actual role pg creates —
        // and we want the URL to match an existing role.
        //
        // Changing these later after a data dir exists requires
        // `--reset-db` (initdb only runs once).
        let settings = Settings {
            data_dir: data_dir.clone(),
            // Deterministic port so the connection string is stable across
            // restarts. 15432 avoids colliding with a native-installed Postgres.
            port: 15432,
            username: "postgres".to_string(),
            password: "flowcatalyst".to_string(),
            temporary: false,
            ..Settings::default()
        };

        info!(
            data_dir = %data_dir.display(),
            port = settings.port,
            "Starting embedded Postgres (binary is bundled into fc-dev)"
        );

        let mut postgresql = PostgreSQL::new(settings);
        postgresql
            .setup()
            .await
            .context("embedded Postgres setup failed")?;
        postgresql
            .start()
            .await
            .context("embedded Postgres start failed")?;

        let database_name = "flowcatalyst";
        // create_database is not idempotent across restarts; the second run
        // returns an "already exists" error that we can safely ignore.
        if let Err(e) = postgresql.create_database(database_name).await {
            let msg = e.to_string();
            if !msg.contains("already exists") {
                return Err(anyhow::anyhow!(
                    "failed to create flowcatalyst database: {}",
                    e
                ));
            }
        }

        let url = postgresql.settings().url(database_name);
        info!("Embedded Postgres ready at {}", url);

        Ok(EmbeddedDb { postgresql, url })
    }

    pub async fn stop(db: &mut EmbeddedDb) {
        info!("Stopping embedded Postgres");
        if let Err(e) = db.postgresql.stop().await {
            tracing::warn!(error = %e, "Failed to stop embedded Postgres cleanly");
        }
    }
}

mod banner;
mod mcp_bootstrap;
mod upgrade;
mod version_check;

#[tokio::main]
async fn main() -> Result<()> {
    // Subcommand fast path — handle the ones that don't need a database,
    // env vars, or anything else expensive before booting the dev server.
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Upgrade(opts)) => {
            fc_common::logging::init_logging("fc-dev");
            return upgrade::run(&opts).await;
        }
        Some(Command::Mcp(opts)) => {
            // MCP over stdio uses stdout for JSON-RPC, so we MUST send tracing
            // to stderr regardless of what the rest of fc-dev does.
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info".into()),
                )
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .init();
            let config = fc_mcp::Config::from_env()?;
            return if opts.http {
                fc_mcp::run_http(config, opts.bind).await
            } else {
                fc_mcp::run_stdio(config).await
            };
        }
        _ => {}
    }

    // Load .env.development (or .env) if present
    let _ = dotenvy::from_filename(".env.development").or_else(|_| dotenvy::dotenv());

    // Set dev defaults for env vars that aren't set
    // These make fc-dev zero-config (only DB URL needed).
    if std::env::var("FLOWCATALYST_APP_KEY").is_err() {
        std::env::set_var(
            "FLOWCATALYST_APP_KEY",
            "MpU3dI07kjZmZGROrElYfDXQgab30e3wr0KTnxQbePg=",
        );
    }
    if std::env::var("FC_DEV_MODE").is_err() {
        std::env::set_var("FC_DEV_MODE", "true");
    }

    // Anchor the JWT keypair to an absolute dev-cache path so sessions
    // survive across launches regardless of CWD. Without this, the keys
    // land in `./.jwt-keys/` relative to wherever fc-dev was invoked —
    // so `cargo run -p fc-dev` vs `./target/release/fc-dev` generate
    // separate keys, and any existing `fc_session` cookie signed with
    // the other set fails validation and kicks the user back to login.
    if std::env::var("FC_JWT_PRIVATE_KEY_PATH").is_err()
        && std::env::var("FC_JWT_PUBLIC_KEY_PATH").is_err()
    {
        let keys_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("flowcatalyst-dev")
            .join("jwt-keys");
        std::env::set_var("FC_JWT_PRIVATE_KEY_PATH", keys_dir.join("private.key"));
        std::env::set_var("FC_JWT_PUBLIC_KEY_PATH", keys_dir.join("public.key"));
    }

    // Initialize logging (JSON if LOG_FORMAT=json, text otherwise)
    fc_common::logging::init_logging("fc-dev");

    // `fc-dev` (bare) and `fc-dev start` are equivalent — `start` exists for
    // discoverability. Subcommand args take precedence if both forms are
    // mixed; matters only for `start --foo`, which clap routes here.
    #[allow(unused_mut)]
    let mut args = match cli.command {
        Some(Command::Start(start_args)) => start_args,
        _ => cli.run,
    };

    info!("Starting FlowCatalyst Dev Monolith (Rust)");
    info!(
        "API port: {}, Metrics port: {}",
        args.api_port, args.metrics_port
    );

    // Best-effort, non-blocking startup version check. Spawned (not awaited)
    // so it never delays boot; result is logged + exposed via /health.
    version_check::spawn();

    // 0. If embedded-pg is enabled, start it before anything else touches
    //    the database and override the URL that downstream code will use.
    #[cfg(feature = "embedded-db")]
    let mut embedded_db = if args.embedded_db {
        let db = embedded_pg::start(args.reset_db).await?;
        args.database_url = db.url.clone();
        std::env::set_var("FC_DATABASE_URL", &db.url);
        Some(db)
    } else {
        None
    };

    // Setup shutdown signal
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // 1. Connect to Postgres early — the queue, control plane, stream
    //    processor, and unit-of-work all share the same pool.
    info!("Connecting to PostgreSQL...");
    let pg_pool = fc_platform::shared::database::create_pool(&args.database_url)
        .await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    fc_platform::shared::database::run_migrations(
        &pg_pool,
        fc_platform::shared::database::MigrationProfile::Embedded,
    )
    .await
    .map_err(|e| anyhow::anyhow!("PostgreSQL migrations failed: {}", e))?;

    fc_platform::shared::database::seed_builtin_roles(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Built-in role seeding failed: {}", e))?;

    fc_platform::shared::database::seed_platform_application(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Platform application seeding failed: {}", e))?;

    fc_platform::shared::default_processes::seed_default_processes(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Default processes seeding failed: {}", e))?;

    // Referential-integrity scan — warns when any aggregate delete path has
    // left orphan junction rows behind. Non-fatal; operator-visible.
    fc_platform::shared::integrity_scan::run(&pg_pool).await;

    // 2. Initialise the embedded queue on the same Postgres pool. Queue
    //    tables live alongside the control-plane tables — one DB to back
    //    up, one dialect to reason about.
    let queue = Arc::new(PostgresQueue::new(
        pg_pool.clone(),
        "dev-queue".to_string(),
        30, // visibility timeout
    ));
    queue.init_schema().await?;
    info!("Embedded Postgres queue initialized");

    // 3. Warning + Health services (constructed first so the QueueManager
    //    can thread warning_service into each per-pool HttpMediator).
    let warning_service = Arc::new(WarningService::new(WarningServiceConfig::default()));
    let health_service = Arc::new(HealthService::new(
        HealthServiceConfig::default(),
        warning_service.clone(),
    ));

    // 4. Create QueueManager. Mediator *config* is passed (not a singleton);
    //    each pool gets its own HttpMediator + connection pool.
    let mut queue_manager_inner = QueueManager::new(HttpMediatorConfig::dev());
    queue_manager_inner.set_warning_service(warning_service.clone());
    let queue_manager = Arc::new(queue_manager_inner);
    queue_manager.add_consumer(queue.clone()).await;

    // 5. Apply router configuration
    let router_config = RouterConfig {
        processing_pools: vec![PoolConfig {
            code: "DEFAULT".to_string(),
            concurrency: args.pool_concurrency,
            rate_limit_per_minute: None,
        }],
        queues: vec![QueueConfig {
            name: "dev-queue".to_string(),
            uri: args.database_url.clone(),
            connections: 1,
            visibility_timeout: 30,
        }],
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
    let outbox_pool: Option<sqlx::PgPool> =
        if args.outbox_enabled && args.outbox_db_type == "postgres" {
            let outbox_db_url = args.outbox_db_url.as_deref().unwrap_or(&args.database_url);
            info!(
                db_type = %args.outbox_db_type,
                db_url = %outbox_db_url,
                poll_interval_ms = args.outbox_poll_interval_ms,
                "Connecting to outbox database"
            );
            Some(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(5)
                    .connect(outbox_db_url)
                    .await
                    .map_err(|e| anyhow::anyhow!("Outbox PostgreSQL connection failed: {}", e))?,
            )
        } else {
            None
        };

    // 8. Setup platform services and APIs
    info!("Initializing platform services...");

    // Seed development data (the pool + migrations were set up in step 1).
    let seeder = fc_platform::seed::DevDataSeeder::new(pg_pool.clone());
    if let Err(e) = seeder.seed().await {
        tracing::warn!("Dev data seeding skipped (data may already exist): {}", e);
    }

    // 8c. Initialize all repositories
    let repos = Repositories::new(&pg_pool);
    info!("Platform repositories initialized");

    // 8c.1 Auto-provision OAuth credentials for `fc-dev mcp`. Best-effort
    // (a failure here doesn't block fc-dev from serving); gated on
    // FC_DEV_MODE so production binaries never run it.
    if let Err(e) = mcp_bootstrap::run(&repos).await {
        warn!(error = %e, "MCP credential bootstrap skipped — `fc-dev mcp` may need manual setup");
    }

    // 8b1.5 Start CQRS stream processor (projects msg_events → msg_events_read, etc.)
    let stream_handle = {
        let config = fc_stream::StreamProcessorConfig {
            events_enabled: true,
            events_batch_size: 100,
            dispatch_jobs_enabled: true,
            dispatch_jobs_batch_size: 100,
            fan_out_enabled: true,
            fan_out_batch_size: 200,
            fan_out_subscription_refresh_secs: 5,
            // fc-dev's embedded postgres now runs the partitioning migrations
            // (019/022 are core, not production-only) so dev mirrors prod's
            // partitioned table shape. The Rust partition manager handles
            // forward+retention here; in production migration 023 hands the
            // job to pg_partman_bgw and the manager auto-defers.
            partition_manager_enabled: true,
        };
        let (handle, _health) = fc_stream::start_stream_processor(pg_pool.clone(), config);
        info!("Stream processor started (event + dispatch job + fan-out projections)");
        handle
    };

    // 8b2. Create UnitOfWork for atomic commits
    let unit_of_work = Arc::new(PgUnitOfWork::new(pg_pool.clone()));

    // Sync code-defined roles to database
    {
        let role_sync =
            fc_platform::service::RoleSyncService::new(Arc::new(RoleRepository::new(&pg_pool)));
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            tracing::warn!("Role sync failed: {}", e);
        }
    }

    // 8c. Initialize auth services (auto-generate RSA keys for dev, like Java)
    let auth_services = fc_platform::shared::server_setup::init_auth_services(
        &repos,
        fc_platform::shared::server_setup::AuthInitConfig::from_env("http://localhost:8080"),
    )
    .expect("Failed to initialize auth services");
    info!("Auth services initialized");

    // 7b. Start outbox processor now that AuthService is ready — generate a
    //     long-lived internal service token so the outbox HTTP dispatcher can
    //     authenticate against the SDK batch endpoints.
    let outbox_handle: Option<tokio::task::JoinHandle<()>> = if let Some(pool) = outbox_pool {
        use fc_platform::principal::entity::Principal;

        let internal_principal =
            Principal::new_service("outbox-processor", "Outbox Processor (internal)");
        let token = auth_services
            .auth
            .generate_access_token(&internal_principal)
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
                .map_err(|e| anyhow::anyhow!("Failed to create outbox processor: {}", e))?,
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
        let scheduler = Arc::new(DispatchScheduler::new(
            config,
            pg_pool.clone(),
            queue.clone(),
        ));

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

    // 7d. Start scheduled-job scheduler (cron-driven instance creation +
    // webhook delivery). Independent of the dispatch_job scheduler above.
    let _scheduled_job_scheduler: Option<tokio::task::JoinHandle<()>> = {
        use fc_platform::scheduled_job::scheduler::{
            ScheduledJobSchedulerConfig, ScheduledJobSchedulerService,
        };
        let svc = ScheduledJobSchedulerService::new(
            ScheduledJobSchedulerConfig::from_env(),
            repos.scheduled_job_repo.clone(),
            repos.scheduled_job_instance_repo.clone(),
        );
        let (poller_h, dispatcher_h) = svc.start();
        let mut shutdown_rx = shutdown_tx.subscribe();
        let svc_arc = Arc::new(svc);
        let svc_clone = svc_arc.clone();
        let handle = tokio::spawn(async move {
            let _ = shutdown_rx.recv().await;
            info!("Scheduled-job scheduler received shutdown signal");
            svc_clone.shutdown();
            let _ = poller_h.await;
            let _ = dispatcher_h.await;
        });
        info!("Scheduled-job scheduler started (cron poller + dispatcher)");
        Some(handle)
    };

    // 8d. Create AppState for authentication middleware
    let app_state = AppState {
        auth_service: auth_services.auth.clone(),
        authz_service: auth_services.authz.clone(),
    };

    // Resolve the seeded `platform` application id.
    let platform_application_id = repos
        .application_repo
        .find_by_code("platform")
        .await?
        .ok_or_else(|| anyhow::anyhow!("platform application row missing after seeding"))?
        .id;

    // 8e. Build platform API router via shared builder (handles ~38 state structs).
    // Event fan-out runs as a background service (started below); the request
    // path doesn't need the queue/dispatch deps wired in here.
    let routes = fc_platform::shared::server_setup::build_platform_routes(
        &repos,
        &auth_services,
        &unit_of_work,
        fc_platform::shared::server_setup::PlatformRoutesConfig {
            session_cookie_secure: false,
            session_cookie_same_site:
                fc_platform::shared::server_setup::PlatformRoutesConfig::DEFAULT_SAME_SITE
                    .to_string(),
            session_token_expiry_secs:
                fc_platform::shared::server_setup::PlatformRoutesConfig::DEFAULT_SESSION_EXPIRY_SECS,
            static_dir: None, // fc-dev handles SPA serving itself (embedded or FC_STATIC_DIR)
            oidc_login_external_base_url: Some(
                std::env::var("FC_EXTERNAL_BASE_URL")
                    .unwrap_or_else(|_| "http://localhost:4200".to_string()),
            ),
            well_known_external_base_url: format!("http://localhost:{}", args.api_port),
            password_reset_external_base_url: format!("http://localhost:{}", args.api_port),
        },
        platform_application_id,
    );

    // Event fan-out runs inside the stream processor (fc-stream) configured
    // above; nothing to start here.
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

    // Dev-specific extra routes.
    //
    // `POST /api/dispatch-jobs/batch` is already registered by
    // `PlatformRoutes` via `sdk_dispatch_jobs_batch_router`, so we do NOT
    // re-nest the full `dispatch_jobs_router` here — doing so double-
    // registers the /batch handler and axum panics at startup. Dispatch
    // job list/get endpoints remain available under `/bff/dispatch-jobs/*`.
    let _ = dispatch_jobs_state; // kept for future expansion; not mounted
    let platform_router = platform_app
        .nest(
            "/api/event-types/filters",
            event_type_filters_router(filter_options_state),
        )
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
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

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
                .route(
                    "/auth/forgot-password",
                    axum::routing::get(embedded_spa_handler),
                )
                .route(
                    "/auth/reset-password",
                    axum::routing::get(embedded_spa_handler),
                )
                .nest_service("/assets", assets_service)
                .fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(index_path)))
        } else {
            warn!(dir = %static_dir, "FC_STATIC_DIR set but index.html not found — using embedded assets");
            api_app.fallback(axum::routing::get(embedded_asset_handler))
        }
    } else {
        info!("Serving embedded frontend (compiled into binary)");
        api_app
            .route("/auth/login", axum::routing::get(embedded_spa_handler))
            .route(
                "/auth/forgot-password",
                axum::routing::get(embedded_spa_handler),
            )
            .route(
                "/auth/reset-password",
                axum::routing::get(embedded_spa_handler),
            )
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
    info!(
        "Metrics server listening on http://{}/metrics",
        metrics_addr
    );

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

    banner::print(args.api_port, args.metrics_port);
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
    })
    .await;

    // Stop embedded Postgres last — repositories / pools will have been
    // shut down by the timeout above, so closing the server is safe.
    #[cfg(feature = "embedded-db")]
    if let Some(ref mut db) = embedded_db {
        embedded_pg::stop(db).await;
    }

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
        )
            .into_response(),
    }
}

use axum::response::IntoResponse;
