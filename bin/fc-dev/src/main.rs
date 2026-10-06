//! FlowCatalyst Development Monolith
//!
//! All-in-one binary for local development containing:
//! - Message Router (with embedded SQLite queue)
//! - API Server (for publishing messages)
//! - Outbox Processor (configurable database backend)
//! - Platform APIs (events, subscriptions, auth, etc.)
//! - Metrics endpoint

use anyhow::Result;
use axum::{response::Json, routing::get, Router};
use clap::Parser;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

use rust_embed::Embed;

use axum::http::header;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::response::Response;
use fc_common::config;
use fc_common::diagnostics;
use fc_common::diagnostics::Exposition;
use fc_common::logging;
use fc_common::netguard;
use fc_common::{PoolConfig, QueueConfig, RouterConfig};
use fc_platform::api::DispatchJobsState;
use fc_platform::api::FilterOptionsState;
#[cfg(feature = "web")]
use fc_platform::auth::routes;
#[cfg(feature = "web")]
use fc_platform::developer_credential::routes::developer_credentials_state;
use fc_platform::dispatch_job::reaper;
use fc_platform::dispatch_job::signing_guard::SigningGuard;
use fc_platform::principal::entity::UserScope;
#[cfg(feature = "web")]
use fc_platform::principal::routes::{principal_go_state, principals_state};
use fc_platform::role::entity::roles;
use fc_platform::router;
use fc_platform::seed::platform_event_types;
use fc_platform::service::RoleSyncService;
use fc_platform::service_account::outbound_credentials::OutboundCredentialsResolver;
use fc_platform::shared::database;
use fc_platform::shared::database::MigrationProfile;
use fc_platform::shared::default_processes;
use fc_platform::shared::encryption_service::EncryptionService;
use fc_platform::shared::integrity_scan;
use fc_platform::shared::rate_limit_store;
use fc_platform::shared::rate_limit_store::RateLimitPolicies;
use fc_platform::shared::secret_backfill;
use fc_platform::shared::server_setup;
use fc_platform::shared::server_setup::AuthInitConfig;
use fc_platform::shared::server_setup::PlatformContext;
use fc_platform::shared::server_setup::PlatformRoutesConfig;
use fc_router::{
    api::create_router as create_api_router, HealthService, HealthServiceConfig,
    HttpMediatorConfig, LifecycleConfig, LifecycleManager, QueueManager, WarningService,
    WarningServiceConfig,
};
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use tokio::task::{JoinError, JoinHandle};
use tokio::time;
use tokio_util::sync::CancellationToken;

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

    /// Bootstrap a fresh application on this local fc-dev:
    /// admin user (if none exists), Default Client, Application,
    /// Service Account, OAuth client, and a `.env` written to the
    /// project root. Replaces the per-SDK init commands.
    Init(init::InitArgs),

    /// Truncate every FlowCatalyst table in the database (preserves the
    /// schema + the migration tracker). Used to start over without
    /// reinstalling or re-migrating. Refuses to run without explicit
    /// confirmation.
    Fresh(fresh::FreshArgs),

    /// Run the FlowCatalyst MCP server (read-only access to event types
    /// and subscriptions for AI agents): stdio by default, `--http` to
    /// listen (Go's `fcdev mcp`).
    ///
    /// Reads `FLOWCATALYST_URL`, `FLOWCATALYST_CLIENT_ID`, and
    /// `FLOWCATALYST_CLIENT_SECRET` from the environment, else the
    /// credentials file a running fc-dev writes; the flags override them.
    Mcp(McpArgs),

    /// Standalone outbox poller (Go's `fcdev outbox`). Polls an external
    /// app's `outbox_messages` Postgres table and forwards to a
    /// FlowCatalyst platform API; `create-table` provisions the table. Use
    /// when the app's database can't be the embedded one (e.g. PostGIS in
    /// Docker).
    Outbox(outbox::OutboxArgs),

    /// Stop a running fcdev (Rust, Go or Java) via the shared PID file:
    /// SIGTERM, so it drains and stops its embedded Postgres.
    Stop(stop::StopArgs),

    /// Download the latest fc-dev release and replace this binary.
    Upgrade(UpgradeArgs),

    /// Functions: scaffold, build, publish, deploy and invoke them against
    /// a running fc-dev (credentials from the fn-cli.json it writes).
    Fn(fn_cli::FnArgs),
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
    /// Run as a streamable HTTP server instead of stdio, at `--bind`, or at
    /// the address given (Go's `--http 127.0.0.1:8090`).
    #[arg(long, value_name = "BIND", num_args = 0..=1, default_missing_value = "")]
    http: Option<String>,

    /// Bind address for `--http` mode: `host:port`, or a host with port
    /// 3100.
    #[arg(long, env = "FC_MCP_BIND", default_value = "127.0.0.1:3100")]
    bind: String,

    /// Platform base URL (overrides `FLOWCATALYST_URL`).
    #[arg(long)]
    platform_url: Option<String>,

    /// OAuth client id (overrides `FLOWCATALYST_CLIENT_ID`).
    #[arg(long)]
    client_id: Option<String>,

    /// OAuth client secret (overrides `FLOWCATALYST_CLIENT_SECRET`).
    #[arg(long)]
    client_secret: Option<String>,
}

/// The port an MCP bind without one listens on (`fc-dev mcp --http` and
/// `fc-dev start --mcp`). Go's fcdev uses 8090, which is fc-dev's function
/// host port.
const DEV_MCP_PORT: u16 = 3100;

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

    /// Run the MCP HTTP server beside the platform (Go's `fcdev start
    /// --mcp`), at `FC_MCP_BIND` (default `127.0.0.1`) : `FC_MCP_PORT`
    /// (default 3100), authenticated with the credentials fc-dev
    /// provisions for `fc-dev mcp`.
    #[arg(
        long = "mcp",
        env = "FC_MCP_ENABLED",
        default_value = "false",
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true"
    )]
    mcp_enabled: bool,

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

    /// Start the embedded PostgreSQL 18 cluster shared with Go's and Java's
    /// fcdev instead of connecting to `--database-url`. Set to `false`
    /// (`--embedded-db=false` / `FC_EMBEDDED_DB=false`) to use an existing
    /// Postgres. Only available when compiled with the `embedded-db` feature.
    #[cfg(feature = "embedded-db")]
    #[arg(
        long,
        env = "FC_EMBEDDED_DB",
        default_value = "true",
        // Go's `--embedded-db=false` form (and bare `--embedded-db`).
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true"
    )]
    embedded_db: bool,

    /// Wipe the embedded Postgres directory (`--embedded-db-path`, the whole
    /// directory, as Go does) before starting. `--reset-db` / `FC_RESET_DB`
    /// is the older fc-dev spelling. The shared default cluster also needs
    /// `--confirm-shared-db-reset`.
    #[cfg(feature = "embedded-db")]
    #[arg(
        long = "embedded-db-reset",
        alias = "reset-db",
        env = "FC_EMBEDDED_DB_RESET",
        default_value = "false",
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true"
    )]
    embedded_db_reset: bool,

    /// Confirm that `--embedded-db-reset` may delete the shared default
    /// cluster (the one Go's and Java's fcdev use too).
    #[cfg(feature = "embedded-db")]
    #[arg(
        long,
        env = "FC_CONFIRM_SHARED_DB_RESET",
        default_value = "false",
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true"
    )]
    confirm_shared_db_reset: bool,

    /// Embedded cluster location, port and PostGIS source.
    #[cfg(feature = "embedded-db")]
    #[command(flatten)]
    embedded: embedded_pg::EmbeddedDbArgs,

    /// PID file written while running, shared with Go's and Java's fcdev;
    /// used by `fc-dev stop` and to refuse a second instance.
    /// [default: <userDataDir>/flowcatalyst/fcdev.pid]
    #[arg(long, env = "FC_DEV_PID_FILE", value_name = "FILE")]
    pid_file: Option<PathBuf>,

    /// The in-process function host (on by default).
    #[command(flatten)]
    functions: functions::FunctionArgs,
}

mod banner;
mod dev_paths;
#[cfg(feature = "embedded-db")]
mod embedded_pg;
mod fn_cli;
mod fresh;
mod functions;
mod init;
mod instance_guard;
mod mcp_bootstrap;
mod outbox;
#[cfg(feature = "embedded-db")]
mod pg_extensions;
mod stop;
mod upgrade;
mod version_check;

#[tokio::main]
#[expect(
    clippy::expect_used,
    reason = "start-up: fc-dev cannot run without its auth keys"
)]
#[expect(
    clippy::let_underscore_must_use,
    reason = "a missing .env file is normal; the process environment is the source of truth; any outcome (a value, lag or a closed channel) is the signal being waited for; best-effort cleanup: a file already gone, or one that cannot be removed, changes nothing the caller relies on; the receiver may already be gone (shutdown, or an abandoned caller): nobody is left to notify"
)]
async fn main() -> Result<()> {
    // Load .env BEFORE Cli::parse() so clap's `#[arg(env = "…")]` fallbacks
    // pick up values from the project's `.env.development` / `.env`. Without
    // this, env vars only resolve from the actual shell environment, and the
    // common case of "I set FC_OUTBOX_TOKEN in .env" silently doesn't work.
    //
    // Not for `fc-dev fn`: a project's `.env` holds its application's
    // service account, which must not shadow the fn CLI's own credentials.
    if env::args_os().nth(1).is_none_or(|a| a != "fn") {
        let _ = dotenvy::from_filename(".env.development").or_else(|_| dotenvy::dotenv());
    }

    // Developers run webhook receivers on their own machine and network, so
    // the delivery policy allows loopback and private targets here unless the
    // environment says otherwise. Cloud metadata and link-local addresses stay
    // blocked (Go: cmd/fcdev/envcfg.go).
    let delivery_policy = netguard::default_policy();
    if env::var_os("FC_DELIVERY_ALLOW_LOOPBACK").is_none() {
        delivery_policy.set_allow_loopback(true);
    }
    if env::var_os("FC_DELIVERY_ALLOW_PRIVATE").is_none() {
        delivery_policy.set_allow_private(true);
    }

    // Subcommand fast path — handle the ones that don't need a database,
    // env vars, or anything else expensive before booting the dev server.
    let cli = Cli::parse();

    // The dev app key, before the subcommands: `fc-dev init` seals the
    // secrets it mints with it, and they must open under the server's.
    let state_dir = shared_state_dir(&cli);
    apply_dev_app_key(state_dir.as_deref());

    match cli.command {
        Some(Command::Upgrade(opts)) => {
            logging::init_logging("fc-dev");
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
                .with_writer(io::stderr)
                .with_ansi(false)
                .init();
            // Flags override the environment (Go's order).
            for (key, value) in [
                ("FLOWCATALYST_URL", &opts.platform_url),
                ("FLOWCATALYST_CLIENT_ID", &opts.client_id),
                ("FLOWCATALYST_CLIENT_SECRET", &opts.client_secret),
            ] {
                if let Some(v) = value.as_deref().filter(|v| !v.is_empty()) {
                    env::set_var(key, v);
                }
            }
            let config = fc_mcp::Config::from_env()?;
            return match opts.http.as_deref() {
                None => fc_mcp::run_stdio(config).await,
                Some("") => {
                    fc_mcp::run_http(config, fc_mcp::resolve_bind(&opts.bind, DEV_MCP_PORT)?).await
                }
                Some(bind) => {
                    fc_mcp::run_http(config, fc_mcp::resolve_bind(bind, DEV_MCP_PORT)?).await
                }
            };
        }
        Some(Command::Init(args)) => {
            logging::init_logging("fc-dev init");
            return init::run(args).await;
        }
        Some(Command::Fresh(args)) => {
            logging::init_logging("fc-dev fresh");
            return fresh::run(args).await;
        }
        Some(Command::Outbox(args)) => {
            logging::init_logging("fc-dev outbox");
            return outbox::run(args).await;
        }
        Some(Command::Fn(args)) => {
            process::exit(fn_cli::run(args).await);
        }
        Some(Command::Stop(args)) => {
            return stop::run(args);
        }
        _ => {}
    }

    // Set dev defaults for env vars that aren't set
    // These make fc-dev zero-config (only DB URL needed).
    if env::var("FC_DEV_MODE").is_err() {
        env::set_var("FC_DEV_MODE", "true");
    }

    // WebAuthn / passkeys default to localhost in fc-dev so the browser
    // accepts the credentials without TLS. Override either by exporting
    // the env var or by putting it in `.env.development`.
    //   RP_ID must be the bare hostname (no scheme, no port).
    //   ORIGINS is a comma-separated allow-list of full origins — Vite
    //   on :5173 and the fc-dev API on :8080 cover both the SPA dev
    //   server and the production-served frontend on the same port as
    //   the API.
    if env::var("FC_WEBAUTHN_RP_ID").is_err() {
        env::set_var("FC_WEBAUTHN_RP_ID", "localhost");
    }
    if env::var("FC_WEBAUTHN_ORIGINS").is_err() {
        env::set_var(
            "FC_WEBAUTHN_ORIGINS",
            "http://localhost:5173,http://localhost:8080",
        );
    }

    // Anchor the JWT keypair to an absolute dev-cache path so sessions
    // survive across launches regardless of CWD. Without this, the keys
    // land in `./.jwt-keys/` relative to wherever fc-dev was invoked —
    // so `cargo run -p fc-dev` vs `./target/release/fc-dev` generate
    // separate keys, and any existing `fc_session` cookie signed with
    // the other set fails validation and kicks the user back to login.
    //
    // On the shared cluster, Go's and Java's signing key
    // (`<userDataDir>/flowcatalyst/jwt-signing-key.pem`) is used when it
    // exists, so a session survives switching binaries.
    let go_signing_key = state_dir
        .as_ref()
        .map(|d| d.join("jwt-signing-key.pem"))
        .filter(|p| p.is_file());
    let jwt_configured = [
        "FC_JWT_PRIVATE_KEY_PATH",
        "FC_JWT_PUBLIC_KEY_PATH",
        "FC_JWT_SIGNING_KEY_PATH",
    ]
    .iter()
    .any(|k| env::var_os(k).is_some_and(|v| !v.is_empty()));
    if !jwt_configured && go_signing_key.is_some() {
        if let Some(key) = &go_signing_key {
            env::set_var("FC_JWT_SIGNING_KEY_PATH", key);
        }
    } else if !jwt_configured {
        let keys_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("flowcatalyst-dev")
            .join("jwt-keys");
        env::set_var("FC_JWT_PRIVATE_KEY_PATH", keys_dir.join("private.key"));
        env::set_var("FC_JWT_PUBLIC_KEY_PATH", keys_dir.join("public.key"));
    }

    // Initialize logging (JSON if LOG_FORMAT=json, text otherwise), with
    // the panic hook; then the process's one Prometheus registry.
    logging::init_logging("fc-dev");
    fc_router::init_prometheus_recorder();

    // `fc-dev` (bare) and `fc-dev start` are equivalent — `start` exists for
    // discoverability. Subcommand args take precedence if both forms are
    // mixed; matters only for `start --foo`, which clap routes here.
    #[allow(unused_mut)]
    let mut args = match cli.command {
        Some(Command::Start(start_args)) => start_args,
        _ => cli.run,
    };

    // Function publishing works with nothing configured, as Java's
    // `fcdev start`: dev mode on, uploaded artifacts kept in the dev
    // cache, signatures off, and the pool URL pointing at fc-dev's own
    // function host (see `functions`).
    let data_dir = functions::data_dir();
    functions::apply_platform_defaults(&args.functions, &data_dir);

    info!("Starting FlowCatalyst Dev Monolith (Rust)");
    info!(
        "API port: {}, Metrics port: {}",
        args.api_port, args.metrics_port
    );

    // Best-effort, non-blocking startup version check. Spawned (not awaited)
    // so it never delays boot; result is logged + exposed via /health.
    version_check::spawn();

    // 0. One fcdev at a time (Rust, Go or Java): the shared PID file.
    let pid_file = args
        .pid_file
        .clone()
        .unwrap_or_else(dev_paths::default_pid_file);
    let _pid_file_guard = instance_guard::claim_pid_file(&pid_file)?;

    //    If embedded-pg is enabled, start it before anything else touches
    //    the database and override the URL that downstream code will use.
    #[cfg(feature = "embedded-db")]
    let mut embedded_db = if args.embedded_db {
        if env::var_os("FC_DATABASE_URL").is_some() {
            info!(
                "Using the embedded Postgres; FC_DATABASE_URL / --database-url is ignored \
                 (pass --embedded-db=false to connect to it instead)"
            );
        }
        let reset = embedded_pg::Reset {
            requested: args.embedded_db_reset || env_flag("FC_RESET_DB"),
            confirmed: args.confirm_shared_db_reset,
        };
        let db = embedded_pg::start(
            &args.embedded,
            reset,
            embedded_pg::Mode::Exclusive,
            &pid_file,
        )
        .await?;
        args.database_url = db.url.clone();
        env::set_var("FC_DATABASE_URL", &db.url);
        Some(db)
    } else {
        None
    };

    // Setup shutdown signal
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // 1. Connect to Postgres early — the queue, control plane, stream
    //    processor, and unit-of-work all share the same pool.
    info!("Connecting to PostgreSQL...");
    let pg_pool = database::create_pool(&args.database_url)
        .await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    database::run_migrations(&pg_pool, MigrationProfile::Embedded)
        .await
        .map_err(|e| anyhow::anyhow!("PostgreSQL migrations failed: {}", e))?;

    database::seed_builtin_roles(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Built-in role seeding failed: {}", e))?;

    database::seed_platform_application(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Platform application seeding failed: {}", e))?;

    // Go seeds the platform event-type catalogue on every start.
    database::seed_platform_event_types(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Platform event type seeding failed: {}", e))?;

    default_processes::seed_default_processes(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Default processes seeding failed: {}", e))?;

    // Referential-integrity scan — warns when any aggregate delete path has
    // left orphan junction rows behind. Non-fatal; operator-visible.
    integrity_scan::run(&pg_pool).await;

    // Dev databases may hold secrets (e.g. IDP client secrets) stored in
    // plaintext before encrypt-on-write; reads now refuse those. Encrypt
    // them with the dev key — but only in the embedded Postgres fc-dev
    // started itself. The default --database-url (localhost:5432) is also
    // where an SSH tunnel to a real database usually lands, and encrypting
    // its secrets with the dev key would make them unreadable to the real
    // server. Anywhere else this is a dry run that only reports counts.
    #[cfg(feature = "embedded-db")]
    let own_database = embedded_db.is_some();
    #[cfg(not(feature = "embedded-db"))]
    let own_database = false;
    if let Some(enc) = EncryptionService::from_env() {
        match secret_backfill::backfill_secrets(&pg_pool, &enc, own_database).await {
            Ok(reports) if own_database => {
                for r in reports.iter().filter(|r| r.encrypted > 0) {
                    info!(column = %r.column, encrypted = r.encrypted, "Encrypted plaintext secrets");
                }
            }
            Ok(reports) => {
                for r in reports.iter().filter(|r| r.unencrypted > 0) {
                    warn!(
                        column = %r.column,
                        unencrypted = r.unencrypted,
                        "Plaintext secrets in an external database; not touching them from fc-dev \
                         (run `fc-server backfill-secrets` with that deployment's key)"
                    );
                }
            }
            Err(e) => {
                warn!(error = %e, "Secret backfill failed; plaintext secrets stay unreadable")
            }
        }
    }

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
    let queue_manager = Arc::new(
        QueueManager::builder(HttpMediatorConfig::dev())
            .warning_service(warning_service.clone())
            .build(),
    );
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
    let lifecycle_config = LifecycleConfig::default();
    let cb_max_idle = lifecycle_config.circuit_breaker_max_idle;
    let mut lifecycle = LifecycleManager::start(
        queue_manager.clone(),
        warning_service.clone(),
        health_service.clone(),
        lifecycle_config,
    );
    // Wire periodic idle-eviction against the manager's shared breaker registry
    // (see fc-router main.rs) — otherwise shared breakers grow unbounded.
    lifecycle.spawn_circuit_breaker_eviction(
        queue_manager.circuit_breaker_registry().clone(),
        cb_max_idle,
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
                PgPoolOptions::new()
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

    // No auto-seeded dev data — use `fc-dev init` to bootstrap an
    // admin + application + service account interactively. Built-in
    // roles + platform application + default processes are seeded
    // unconditionally in step 1 above.

    // 8c. Initialize all repositories
    let repos = Repositories::new(&pg_pool);
    info!("Platform repositories initialized");

    // 8c.1 Auto-provision OAuth credentials for `fc-dev mcp`. Best-effort
    // (a failure here doesn't block fc-dev from serving); gated on
    // FC_DEV_MODE so production binaries never run it.
    if let Err(e) = mcp_bootstrap::run(&repos).await {
        warn!(error = %e, "MCP credential bootstrap skipped — `fc-dev mcp` may need manual setup");
    }

    // 8c.2 The function host's and the fn CLI's OAuth clients, with fresh
    // secrets. A failure leaves fc-dev running without functions.
    let fn_identities = if args.functions.enabled() {
        match functions::bootstrap_identities(&repos).await {
            Ok(identities) => Some(identities),
            Err(e) => {
                warn!(error = %e, "Function clients not provisioned; starting without a function host");
                None
            }
        }
    } else {
        None
    };
    let fn_host = functions::HostSlot::default();

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
        let role_sync = RoleSyncService::new(Arc::new(RoleRepository::new(&pg_pool)));
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            tracing::warn!("Role sync failed: {}", e);
        }
    }

    // 8c. Initialize auth services (auto-generate RSA keys for dev, like Java)
    let auth_services =
        server_setup::init_auth_services(&repos, AuthInitConfig::from_env("http://localhost:8080"))
            .expect("Failed to initialize auth services");
    info!("Auth services initialized");

    // 7b. Start outbox processor now that AuthService is ready — generate a
    //     long-lived internal service token so the outbox HTTP dispatcher can
    //     authenticate against the SDK batch endpoints.
    let outbox_handle: Option<JoinHandle<()>> = if let Some(pool) = outbox_pool {
        use fc_platform::principal::entity::Principal;

        // Anchor: the outbox forwards every client's messages. Go's fcdev
        // gives its internal router principal the same scope.
        let mut internal_principal = Principal::new_service(
            "outbox-processor",
            "Outbox Processor (internal)",
            UserScope::Anchor,
        );
        // The ingest routes need `platform:messaging:batch:*-write`, and an
        // event or job of any application's type may be ingested only by a
        // caller that may sign as it. The dev outbox forwards every
        // application's messages, so it holds the built-in super-admin role
        // (seeded before serving), as Go's fcdev bootstrap principal does.
        internal_principal.assign_role(roles::super_admin().name);
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

    // 7c. Start dispatch scheduler (claims PENDING jobs → publishes to the
    //     embedded queue → router → /api/dispatch/process). fc-dev's router
    //     consumes exactly one queue, so every job goes to it rather than to
    //     per-tenant queues.
    let _scheduler_handle: Option<JoinHandle<()>> = if args.scheduler_enabled {
        use fc_platform::scheduler::{
            DispatchAuthService, DispatchScheduler, PoolCodeResolver, SchedulerConfig,
            SingleQueuePublisher,
        };

        let config = SchedulerConfig {
            processing_endpoint: format!("http://localhost:{}/api/dispatch/process", args.api_port),
            ..SchedulerConfig::default()
        }
        .with_env_overrides();
        // The platform's own callback is exempt from the delivery policy, for a
        // developer who has turned loopback delivery off.
        netguard::default_policy().allow_url(&config.processing_endpoint);
        // FLOWCATALYST_APP_KEY is always set by this point (see above).
        let auth = DispatchAuthService::from_env().ok_or_else(|| {
            anyhow::anyhow!("FLOWCATALYST_APP_KEY is required to sign dispatch tokens")
        })?;
        // The scheduler's own pool, so its claim and status updates do not
        // compete with the API for connections.
        let scheduler_pool =
            database::create_scheduler_pool(&args.database_url, config.db_max_connections())
                .await
                .map_err(|e| anyhow::anyhow!("dispatch scheduler PG pool failed: {e}"))?;
        let pool_codes = Arc::new(PoolCodeResolver::new(
            scheduler_pool.clone(),
            config.paused_cache_ttl,
        ));
        let scheduler = DispatchScheduler::new(
            config,
            scheduler_pool,
            Arc::new(SingleQueuePublisher::new(queue.clone())),
            auth,
            pool_codes,
        );

        let cancel = CancellationToken::new();
        let mut shutdown_rx = shutdown_tx.subscribe();
        let stop = cancel.clone();
        tokio::spawn(async move {
            let _ = shutdown_rx.recv().await;
            info!("Dispatch scheduler received shutdown signal");
            stop.cancel();
        });
        let handle = tokio::spawn(async move {
            scheduler.run(Arc::new(|| true), cancel).await;
        });

        // The stranded-sibling reaper (Go's A-01 backstop).
        {
            let cancel = CancellationToken::new();
            let stop = cancel.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            tokio::spawn(async move {
                let _ = shutdown_rx.recv().await;
                stop.cancel();
            });
            tokio::spawn(reaper::run_reaper(
                Arc::new(fc_platform::DispatchJobRepository::new(&pg_pool)),
                reaper::DEFAULT_REAPER_INTERVAL,
                reaper::DEFAULT_PROCESSING_LIVE_AFTER,
                cancel,
            ));
        }

        info!("Dispatch scheduler started (polling PENDING jobs)");
        Some(handle)
    } else {
        None
    };

    // 7d. Start scheduled-job scheduler (cron-driven instance creation +
    // webhook delivery). Independent of the dispatch_job scheduler above.
    let _scheduled_job_scheduler: Option<JoinHandle<()>> = {
        use fc_platform::scheduled_job::scheduler::{
            ScheduledJobSchedulerConfig, ScheduledJobSchedulerService,
        };
        // Firings are signed with each job's application's credentials
        // (Java JobDispatcher).
        let credentials = Arc::new(OutboundCredentialsResolver::new(
            repos.service_account_repo.clone(),
            EncryptionService::from_env().map(Arc::new),
        ));
        let svc = ScheduledJobSchedulerService::new(
            ScheduledJobSchedulerConfig::from_env(),
            repos.scheduled_job_repo.clone(),
            repos.scheduled_job_instance_repo.clone(),
        )
        .with_credentials(credentials);
        let (poller_h, dispatcher_h) = svc.start();
        let mut shutdown_rx = shutdown_tx.subscribe();
        let svc_arc = Arc::new(svc);
        let svc_clone = svc_arc.clone();
        let handle = tokio::spawn(async move {
            let _ = shutdown_rx.recv().await;
            info!("Scheduled-job scheduler received shutdown signal");
            svc_clone.shutdown();
            log_join("scheduled-job poller", poller_h.await);
            log_join("scheduled-job dispatcher", dispatcher_h.await);
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
    let rate_limit_store = rate_limit_store::build_rate_limit_store(repos.pool.clone()).await;
    let rate_limit_policies = Arc::new(RateLimitPolicies::from_env());

    let ctx = PlatformContext::new(
        &repos,
        &auth_services,
        &unit_of_work,
        PlatformRoutesConfig {
            rate_limit_store: rate_limit_store.clone(),
            rate_limit_policies: rate_limit_policies.clone(),
            session_cookie_secure: false,
            session_cookie_same_site: PlatformRoutesConfig::DEFAULT_SAME_SITE.to_string(),
            session_token_expiry_secs: PlatformRoutesConfig::DEFAULT_SESSION_EXPIRY_SECS,
            static_dir: None, // fc-dev handles SPA serving itself (embedded or FC_STATIC_DIR)
            oidc_login_external_base_url: Some(
                env::var("FC_EXTERNAL_BASE_URL")
                    .unwrap_or_else(|_| "http://localhost:4200".to_string()),
            ),
            well_known_external_base_url: format!("http://localhost:{}", args.api_port),
            password_reset_external_base_url: format!("http://localhost:{}", args.api_port),
        },
        platform_application_id.clone(),
    );

    // Go's auth purger (expired auth rows, lapsed OAuth secret overlaps,
    // login-attempts partitions), every minute.
    server_setup::spawn_auth_purger(&repos.pool, repos.oauth_client_repo.clone());

    // Background prune for the Postgres rate-limit table (no-op for Redis).
    // Runs hourly; keeps `iam_rate_limit_events` from growing past peak QPS
    // × max policy window.
    {
        let store = rate_limit_store.clone();
        let max_window = rate_limit_policies.max_window();
        tokio::spawn(async move {
            let mut tick = time::interval(Duration::from_secs(3600));
            tick.tick().await; // skip the immediate-fire tick
            loop {
                tick.tick().await;
                match store.prune(max_window).await {
                    Ok(n) if n > 0 => tracing::debug!(rows = n, "rate_limit_events prune"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "rate_limit_events prune failed"),
                }
            }
        });
    }

    // Event fan-out runs inside the stream processor (fc-stream) configured
    // above; nothing to start here.

    // The Topcoat UI trial signs in through the same states `/auth/login`
    // and `/auth/check-domain` use.
    #[cfg(feature = "web")]
    let web_auth = (
        routes::auth_state(&ctx),
        routes::oidc_login_state(&ctx).password_setup_hint,
    );
    // The users section runs the principal API's own handler bodies, so it
    // takes the states those handlers were built with.
    #[cfg(feature = "web")]
    let web_users = fc_web::UserAdminStates {
        principals: principals_state(&ctx),
        principal_go: principal_go_state(&ctx),
        two_factor: ctx.two_factor.clone(),
        developer_credentials: developer_credentials_state(&ctx),
    };
    let (platform_app, platform_openapi) = router::build(&ctx);

    // Dev-only auto-sync of the Developer portal artefacts. Idempotent —
    // event-types sync writes only deltas, and OpenAPI sync no-ops when
    // the spec hash matches CURRENT. Best-effort: failures warn but don't
    // block fc-dev from serving.
    if let Err(e) = auto_sync_developer_portal(
        &pg_pool,
        unit_of_work.clone(),
        repos.event_type_repo.clone(),
        repos.principal_repo.clone(),
        platform_application_id.clone(),
        serde_json::to_value(&platform_openapi).unwrap_or(serde_json::Value::Null),
    )
    .await
    {
        warn!(error = %e, "Developer-portal auto-sync skipped");
    }

    // Dev-specific extra route states (the shared builder doesn't wire
    // /api/dispatch-jobs or /api/event-types/filters — fc-dev does
    // this itself as compatibility for the generated frontend client).
    let dispatch_jobs_state = DispatchJobsState {
        dispatch_job_repo: repos.dispatch_job_repo.clone(),
        client_repo: repos.client_repo.clone(),
        signing: Arc::new(SigningGuard::new(
            repos.subscription_repo.clone(),
            repos.connection_repo.clone(),
            repos.service_account_repo.clone(),
            repos.application_repo.clone(),
            repos.principal_repo.clone(),
        )),
    };
    let filter_options_state = FilterOptionsState {
        client_repo: repos.client_repo.clone(),
        event_type_repo: repos.event_type_repo.clone(),
        subscription_repo: repos.subscription_repo.clone(),
        dispatch_pool_repo: repos.dispatch_pool_repo.clone(),
        application_repo: repos.application_repo.clone(),
    };

    // Dev-specific extra routes.
    //
    // `POST /api/dispatch-jobs/batch` is already registered by
    // `dispatch_job::routes` via `sdk_dispatch_jobs_batch_router`, so we do NOT
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
    let platform_router = if fn_identities.is_some() {
        functions::nudge_on_function_writes(platform_router, fn_host.clone())
    } else {
        platform_router
    };

    info!("Platform APIs configured");

    // 9. Start API server (merge router API with platform APIs)
    // Shared registry: the monitoring API must read the same breakers the
    // pools record into (was a disconnected RouterCircuitBreakerRegistry::default()).
    let router_circuit_breaker = queue_manager.circuit_breaker_registry().clone();
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

    // The Topcoat UI trial (`--features web`): it sits in front of the SPA
    // fallback, claims `/ui/*` and `/_topcoat/*`, and hands everything else
    // on to the SPA service below.
    #[cfg(feature = "web")]
    let mut web_deps = Some(fc_web::WebDeps::new(
        &repos,
        &auth_services,
        unit_of_work.clone(),
        web_auth.0,
        web_auth.1,
        web_users,
    ));
    #[cfg(feature = "web")]
    info!("Topcoat UI trial mounted at /ui");

    // Static frontend serving — uses FC_STATIC_DIR if set (for live reload),
    // otherwise serves from the embedded frontend assets compiled into the binary.
    let api_app = if let Ok(static_dir) = env::var("FC_STATIC_DIR") {
        let index_path = PathBuf::from(&static_dir).join("index.html");
        if index_path.exists() {
            info!(dir = %static_dir, "Serving frontend from filesystem (live reload)");
            let app = router::serve_spa(api_app, &static_dir);
            #[cfg(feature = "web")]
            let app = app.fallback_service(fc_web::service(
                web_deps.take().expect("web deps used once"),
                router::serve_spa(Router::new(), &static_dir),
            ));
            app
        } else {
            warn!(dir = %static_dir, "FC_STATIC_DIR set but index.html not found — using embedded assets");
            api_app.fallback(get(embedded_asset_handler))
        }
    } else {
        info!("Serving embedded frontend (compiled into binary)");
        let app = api_app
            .route("/auth/login", get(embedded_spa_handler))
            .route("/auth/forgot-password", get(embedded_spa_handler))
            .route("/auth/reset-password", get(embedded_spa_handler));
        #[cfg(feature = "web")]
        let app = app.fallback_service(fc_web::service(
            web_deps.take().expect("web deps used once"),
            Router::new().fallback(get(embedded_asset_handler)),
        ));
        #[cfg(not(feature = "web"))]
        let app = app.fallback(get(embedded_asset_handler));
        app
    };

    let api_addr = format!("0.0.0.0:{}", args.api_port);
    info!("API server listening on http://{}", api_addr);

    let api_listener = TcpListener::bind(&api_addr).await?;
    #[cfg(feature = "web")]
    if let Ok(addr) = api_listener.local_addr() {
        tokio::spawn(fc_web::notify_dev_ready(addr));
    }
    let api_handle = {
        let mut shutdown_rx = shutdown_tx.subscribe();
        // Keep-alive idle 75 s, 30 s to read a request (owner ruling 10).
        tokio::spawn(async move {
            router::serve_api(api_listener, api_app, async move {
                let _ = shutdown_rx.recv().await;
                info!("API server shutting down");
            })
            .await;
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

    // 12. The function host, once the platform is accepting connections:
    //     its first reconcile calls it.
    let fn_cli_file = functions::cli_file_path();
    if let Some(identities) = &fn_identities {
        match functions::start_host(
            &args.functions,
            &format!("http://localhost:{}", args.api_port),
            &identities.host,
            &data_dir.join("fn-cache"),
        )
        .await
        {
            Ok(host) => {
                fn_host.set(host);
                let file = functions::cli_file(&args.functions, args.api_port, &identities.cli);
                if let Err(e) = functions::write_cli_file(&fn_cli_file, &file) {
                    warn!(error = %e, "Could not write the fn CLI's credentials");
                }
            }
            Err(e) => warn!(
                error = %e,
                "Function host not started; fc-dev runs without one (--no-functions skips it)"
            ),
        }
    }

    // 13. The in-process MCP server (`--mcp`), after the credential
    //     bootstrap above wrote the file it reads.
    let mcp_handle = if args.mcp_enabled {
        match start_mcp(args.api_port, shutdown_tx.subscribe()).await {
            Ok(handle) => Some(handle),
            Err(e) => {
                warn!(error = %e, "MCP server not started; fc-dev runs without it");
                None
            }
        }
    } else {
        None
    };

    let fn_ports = fn_host
        .is_running()
        .then_some((args.functions.fn_port, args.functions.fn_public_port));
    banner::print(args.api_port, args.metrics_port, fn_ports);
    info!("Press Ctrl+C to shutdown");

    // Wait for shutdown signal
    server_setup::wait_for_shutdown_signal().await;
    info!("Shutdown signal received, initiating graceful shutdown...");

    // The function host stops first, so its DRAINING heartbeat still
    // reaches the platform.
    if fn_host.is_running() {
        if time::timeout(Duration::from_secs(30), fn_host.close())
            .await
            .is_err()
        {
            warn!("The function host did not close within 30s");
        }
        // Best effort: a leftover file only makes `fc-dev fn` try a dead host.
        let _ = fs::remove_file(&fn_cli_file);
    }

    // Broadcast shutdown to all components
    let _ = shutdown_tx.send(());

    // Stop lifecycle manager and stream processor
    lifecycle.shutdown().await;
    stream_handle.stop().await;

    // Wait for all handles with timeout
    let shutdown_timeout = Duration::from_secs(30);
    let drained = time::timeout(shutdown_timeout, async {
        log_join("API server", api_handle.await);
        log_join("metrics server", metrics_handle.await);
        log_join("queue manager", manager_handle.await);
        if let Some(h) = outbox_handle {
            log_join("outbox processor", h.await);
        }
        if let Some(h) = mcp_handle {
            log_join("MCP server", h.await);
        }
    })
    .await;
    if drained.is_err() {
        warn!("Tasks did not stop within {shutdown_timeout:?} of the shutdown signal");
    }

    // Stop embedded Postgres last — repositories / pools will have been
    // shut down by the timeout above, so closing the server is safe.
    #[cfg(feature = "embedded-db")]
    if let Some(ref mut db) = embedded_db {
        embedded_pg::stop(db).await;
    }

    info!("FlowCatalyst Dev Monolith shutdown complete");
    logging::shutdown();
    Ok(())
}

/// `fc-dev start --mcp`: the MCP server on `FC_MCP_BIND`:`FC_MCP_PORT`
/// against this fc-dev's API, until shutdown.
#[expect(
    clippy::let_underscore_must_use,
    reason = "any outcome (a value, lag or a closed channel) is the signal being waited for"
)]
async fn start_mcp(
    api_port: u16,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<JoinHandle<()>> {
    let addr = fc_mcp::resolve_bind(
        &env::var("FC_MCP_BIND").unwrap_or_else(|_| "127.0.0.1".to_string()),
        config::env_or_parse("FC_MCP_PORT", DEV_MCP_PORT),
    )?;
    let config = fc_mcp::Config::from_env_or_base(&format!("http://localhost:{api_port}"))?;
    let listener = TcpListener::bind(addr).await?;
    Ok(tokio::spawn(async move {
        let stop = async move {
            let _ = shutdown_rx.recv().await;
        };
        if let Err(e) = fc_mcp::serve_http(config, listener, stop).await {
            warn!(error = %e, "MCP server stopped with an error");
        }
    }))
}

/// A boolean environment flag (`true`/`1`/`yes`).
#[allow(dead_code)]
fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(false)
}

/// The directory holding the state shared with Go's and Java's fcdev
/// (`<userDataDir>/flowcatalyst`, beside the embedded cluster), when the
/// command uses the embedded cluster.
fn shared_state_dir(cli: &Cli) -> Option<PathBuf> {
    #[cfg(feature = "embedded-db")]
    {
        let embedded = match &cli.command {
            None => cli.run.embedded_db.then_some(&cli.run.embedded),
            Some(Command::Start(a)) => a.embedded_db.then_some(&a.embedded),
            Some(Command::Init(a)) => a.embedded_db.then_some(&a.embedded),
            Some(Command::Fresh(a)) => a.embedded_db.then_some(&a.embedded),
            _ => None,
        }?;
        Some(dev_paths::state_dir_for(&embedded.path()))
    }
    #[cfg(not(feature = "embedded-db"))]
    {
        let _ = cli;
        None
    }
}

/// `FLOWCATALYST_APP_KEY` when the environment doesn't set it. On the
/// shared cluster: Go's and Java's key file (`<state>/app-key`, created as
/// Go creates it), so secrets any of the three binaries stored stay
/// readable by the others. Otherwise (an external database, or a command
/// without one): the fixed dev key fc-dev has always used, so secrets in an
/// existing external dev database stay readable.
fn apply_dev_app_key(state_dir: Option<&Path>) {
    if env::var_os("FLOWCATALYST_APP_KEY").is_some_and(|v| !v.is_empty()) {
        return;
    }
    if let Some(dir) = state_dir {
        let path = dir.join("app-key");
        match dev_paths::ensure_app_key_file(&path) {
            Ok(key) => {
                env::set_var("FLOWCATALYST_APP_KEY", key);
                return;
            }
            Err(e) => eprintln!(
                "fc-dev: could not read or create {} ({e}); using the fixed dev key — \
                 secrets Go/Java fcdev stored will not decrypt",
                path.display()
            ),
        }
    }
    env::set_var(
        "FLOWCATALYST_APP_KEY",
        "MpU3dI07kjZmZGROrElYfDXQgab30e3wr0KTnxQbePg=",
    );
}

/// The process's Prometheus registry (the scheduler's and stream
/// processor's series), the tokio runtime and process series, and `fc_up`.
async fn metrics_handler() -> String {
    let mut out = fc_router::init_prometheus_recorder().render();
    diagnostics::render_prometheus(&mut out, None, Exposition::Prometheus);
    out.push_str("# HELP fc_up FlowCatalyst is up\n# TYPE fc_up gauge\nfc_up 1\n");
    out
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
async fn embedded_asset_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // The shell asked for by name is the shell: never cacheable.
    if path.is_empty() || path == "index.html" {
        return embedded_spa_handler().await.into_response();
    }

    // Try exact path first (for assets like /assets/index-BKjElYp6.js)
    if let Some(file) = FrontendAssets::get(path) {
        return embedded_file_response(path, file.data.to_vec());
    }

    // SPA fallback: serve index.html for all other paths
    embedded_spa_handler().await.into_response()
}

/// Logs a task that panicked or was cancelled while the process shut down.
fn log_join<T>(task: &str, joined: Result<T, JoinError>) {
    if let Err(e) = joined {
        warn!(task, error = %e, "task ended abnormally during shutdown");
    }
}

/// An embedded file other than the shell: hashed `/assets/*` are
/// immutable; anything else keeps default caching.
#[expect(
    clippy::expect_used,
    reason = "mime_guess yields registered MIME types, which are always valid header values"
)]
fn embedded_file_response(path: &str, data: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        mime.as_ref()
            .parse()
            .expect("a MIME type is a valid header value"),
    );
    if path.starts_with("assets/") {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    (headers, data).into_response()
}

/// Serve the embedded index.html (SPA entry point). Never cacheable
/// (Java 8fd35a8b), so a browser never keeps a stale shell after an upgrade.
async fn embedded_spa_handler() -> impl IntoResponse {
    match FrontendAssets::get("index.html") {
        Some(file) => embedded_shell_response(file.data.to_vec()),
        None => (StatusCode::NOT_FOUND, "Frontend not embedded in this build").into_response(),
    }
}

fn embedded_shell_response(html: Vec<u8>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(router::SPA_SHELL_CACHE_CONTROL),
    );
    (headers, html).into_response()
}

use axum::response::IntoResponse;
use fc_platform::event_type::operations::SyncEventTypeInput;

/// Dev-only auto-sync of the Developer portal artefacts for the
/// `platform` application. Mirrors the two BFF handlers
/// (`POST /bff/event-types/sync-platform` and
/// `POST /bff/developer/sync-platform-openapi`) but skips HTTP — we
/// already have the use cases + the captured OpenAPI in scope at this
/// point in startup.
///
/// Idempotent in both directions: event-types sync only writes deltas,
/// and OpenAPI sync no-ops when the hash matches the CURRENT row. Safe
/// to run on every `fc-dev` start.
///
/// Best-effort. Logs and returns Ok on per-step failure rather than
/// aborting startup — these are dev affordances, not load-bearing.
async fn auto_sync_developer_portal(
    pg_pool: &sqlx::PgPool,
    unit_of_work: Arc<PgUnitOfWork>,
    event_type_repo: Arc<fc_platform::EventTypeRepository>,
    principal_repo: Arc<fc_platform::PrincipalRepository>,
    platform_application_id: String,
    platform_openapi: serde_json::Value,
) -> anyhow::Result<()> {
    use fc_platform::application_openapi_spec::operations::{
        SyncOpenApiSpecCommand, SyncOpenApiSpecUseCase,
    };
    use fc_platform::application_openapi_spec::repository::OpenApiSpecRepository;
    use fc_platform::event_type::operations::{SyncEventTypesCommand, SyncEventTypesUseCase};
    use fc_platform::usecase::{ExecutionContext, UseCase};

    // Attribute the sync to the seeded bootstrap admin so it has a real
    // principal_id (and so audit logs / `synced_by` show a human, not a
    // synthetic system actor). `synced_by` is VARCHAR(17) — a TSID id
    // fits, an arbitrary string usually doesn't.
    let admin_email = env::var("FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL")
        .unwrap_or_else(|_| "admin@flowcatalyst.local".to_string());
    let principal_id = match principal_repo.find_by_email(&admin_email).await {
        Ok(Some(p)) => p.id.into_string(),
        Ok(None) => {
            info!(
                "Developer-portal auto-sync skipped: no admin principal yet \
                 (run `fc-dev fresh` or set FLOWCATALYST_BOOTSTRAP_ADMIN_* env vars)."
            );
            return Ok(());
        }
        Err(e) => {
            warn!(error = %e, "Developer-portal auto-sync: principal lookup failed");
            return Ok(());
        }
    };
    let ctx = ExecutionContext::system(principal_id);

    // ── Event types + schemas for the `platform` application ──────────
    // Only definitions that are new or differ from what is stored go to the
    // sync: the sync (like Go's) records an "updated" event and audit row for
    // every listed type that exists, changed or not, and this runs on every
    // start — on a shared developer cluster that was 131 events per start.
    let all_definitions = platform_event_types::definitions();
    let event_types_total = all_definitions.len();
    let stored = match event_type_repo.find_by_application("platform").await {
        Ok(rows) => rows,
        Err(e) => {
            warn!(error = %e, "Developer-portal auto-sync: reading platform event types failed");
            Vec::new()
        }
    };
    let definitions = changed_event_type_definitions(all_definitions, &stored);
    if definitions.is_empty() {
        info!(
            total = event_types_total,
            "Developer-portal auto-sync: platform event types up to date"
        );
    }
    let cmd = SyncEventTypesCommand {
        application_code: "platform".to_string(),
        event_types: definitions,
        remove_unlisted: false,
    };
    let sync_event_types = SyncEventTypesUseCase::new(event_type_repo, unit_of_work.clone());
    let sync_result = if cmd.event_types.is_empty() {
        None
    } else {
        Some(sync_event_types.run(cmd, ctx.clone()).await.into_result())
    };
    match sync_result {
        None => {}
        Some(result) => match result {
            Ok(event) => {
                info!(
                    total = event_types_total,
                    created = event.created,
                    updated = event.updated,
                    deleted = event.deleted,
                    "Developer-portal auto-sync: platform event types"
                );
            }
            Err(err) => {
                warn!(error = ?err, "Developer-portal auto-sync: platform event types failed");
            }
        },
    }

    // ── Platform's own OpenAPI document into the developer portal ─────
    if !platform_openapi.is_object() {
        warn!("Developer-portal auto-sync: OpenAPI spec is not a JSON object, skipping");
        return Ok(());
    }
    let openapi_repo = Arc::new(OpenApiSpecRepository::new(pg_pool));
    let sync_openapi = SyncOpenApiSpecUseCase::new(openapi_repo, unit_of_work);
    let cmd = SyncOpenApiSpecCommand {
        application_id: platform_application_id,
        application_code: "platform".to_string(),
        spec: platform_openapi,
    };
    match sync_openapi.run(cmd, ctx).await.into_result() {
        Ok(event) => {
            info!(
                version = %event.version,
                unchanged = event.unchanged,
                has_breaking = event.has_breaking,
                "Developer-portal auto-sync: platform OpenAPI"
            );
        }
        Err(err) => {
            warn!(error = ?err, "Developer-portal auto-sync: platform OpenAPI failed");
        }
    }

    Ok(())
}

/// The platform event-type definitions worth syncing: those not stored yet,
/// or whose name, description or 1.0 schema differs from the stored row.
fn changed_event_type_definitions(
    definitions: Vec<SyncEventTypeInput>,
    stored: &[fc_platform::EventType],
) -> Vec<SyncEventTypeInput> {
    definitions
        .into_iter()
        .filter(|def| match stored.iter().find(|et| et.code == def.code) {
            None => true,
            Some(et) => {
                et.name != def.name
                    || et.description != def.description
                    || def.schema.as_ref().is_some_and(|schema| {
                        et.spec_versions
                            .iter()
                            .find(|sv| sv.version == "1.0")
                            .and_then(|sv| sv.schema_content.as_ref())
                            != Some(schema)
                    })
            }
        })
        .collect()
}

#[cfg(test)]
mod spa_cache_tests {
    use super::*;
    use axum::http::header::CACHE_CONTROL;
    use axum::response::Response;
    use fc_platform::router;

    fn cache_control(res: &Response) -> Option<&str> {
        res.headers()
            .get(CACHE_CONTROL)
            .map(|v| v.to_str().unwrap())
    }

    /// Java 8fd35a8b: the shell is never cacheable; hashed assets stay
    /// immutable; other embedded files keep default caching.
    #[test]
    fn the_embedded_shell_is_never_cacheable() {
        let shell = embedded_shell_response(b"<html></html>".to_vec());
        assert_eq!(cache_control(&shell), Some(router::SPA_SHELL_CACHE_CONTROL));
        let asset = embedded_file_response("assets/index-abc.js", vec![]);
        assert_eq!(
            cache_control(&asset),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(
            cache_control(&embedded_file_response("favicon.ico", vec![])),
            None
        );
    }

    #[tokio::test]
    async fn index_html_by_name_and_the_fallback_are_the_shell() {
        if FrontendAssets::get("index.html").is_none() {
            return; // a build without the frontend embeds nothing to serve
        }
        for path in ["/", "/index.html", "/applications/app_1"] {
            let res = embedded_asset_handler(path.parse().unwrap())
                .await
                .into_response();
            assert_eq!(
                cache_control(&res),
                Some(router::SPA_SHELL_CACHE_CONTROL),
                "{path}"
            );
        }
    }
}

#[cfg(test)]
mod auto_sync_tests {
    use super::changed_event_type_definitions;
    use fc_platform::event_type::entity::EventTypeCode;
    use fc_platform::event_type::operations::SyncEventTypeInput;
    use fc_platform::{EventType, SpecVersion};
    use serde_json::json;

    fn def(code: &str, name: &str, schema: Option<serde_json::Value>) -> SyncEventTypeInput {
        SyncEventTypeInput {
            code: code.into(),
            name: name.into(),
            description: None,
            schema,
        }
    }

    fn stored(code: &str, name: &str, schema: Option<serde_json::Value>) -> EventType {
        let code = EventTypeCode::parse(code).expect("valid code");
        let mut et = EventType::new(code, name);
        if schema.is_some() {
            et.spec_versions = vec![SpecVersion::new(&et.id, "1.0", schema)];
        }
        et
    }

    /// Only new or changed definitions are synced, so an unchanged start
    /// records no events (it recorded one per platform type before).
    #[test]
    fn only_new_or_changed_definitions_are_synced() {
        let schema = json!({ "type": "object" });
        let rows = vec![
            stored("platform:a:b:same", "Same", Some(schema.clone())),
            stored("platform:a:b:renamed", "Old name", None),
            stored(
                "platform:a:b:schema",
                "Schema",
                Some(json!({ "type": "string" })),
            ),
        ];
        let defs = vec![
            def("platform:a:b:same", "Same", Some(schema.clone())),
            def("platform:a:b:renamed", "New name", None),
            def("platform:a:b:schema", "Schema", Some(schema.clone())),
            def("platform:a:b:new", "New", None),
        ];
        let codes: Vec<String> = changed_event_type_definitions(defs, &rows)
            .into_iter()
            .map(|d| d.code)
            .collect();
        assert_eq!(
            codes,
            vec![
                "platform:a:b:renamed",
                "platform:a:b:schema",
                "platform:a:b:new"
            ]
        );
    }
}
