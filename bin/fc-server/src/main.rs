//! FlowCatalyst Unified Production Server
//!
//! Single binary combining all subsystems, toggled via environment variables.
//! Background processors (router, scheduler, stream, outbox) can optionally
//! run in standby mode with Redis leader election — only the leader processes.
//!
//! ## Subcommands
//!
//! `fc-server backfill-secrets [--apply]` encrypts stored secrets written
//! before encrypt-on-write, then exits. Dry run (counts only) unless
//! `--apply` is given.
//!
//! ## Environment Variables
//!
//! Every variable is read with Go's name and semantics (flowcatalyst-go
//! `internal/server/envcfg.go`); the production task definitions
//! (`inhance/iac/compute/flowcatalyst.ts`) run unchanged. The full table,
//! Go vs Rust, is `docs/parity/platform-env-vs-go.md`.
//!
//! ### Core
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_API_PORT` / `PORT` | `8080` | HTTP API port |
//! | `FC_METRICS_PORT` | `9090` | Metrics/health port |
//! | `FC_DATABASE_URL` / `DATABASE_URL` | - | Full PostgreSQL URL (wins over the rest) |
//! | `DB_HOST` + `DB_SECRET_ARN` (+ `DB_SECRET_PROVIDER=aws`, `DB_NAME`, `DB_PORT`) | - | Credentials from AWS Secrets Manager, refreshed every `DB_SECRET_REFRESH_INTERVAL_MS` (5 min) on every pool |
//! | `DB_HOST` + `DB_USERNAME` + `DB_PASSWORD` (+ `DB_NAME`, `DB_PORT`) | - | Explicit credentials |
//! | `FC_STATIC_DIR` | `/app/frontend/dist` when present | The SPA served by the platform |
//!
//! ### Subsystem Toggles
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_PLATFORM_ENABLED` / `PLATFORM_ENABLED` | `true` | Run the platform API server |
//! | `FC_ROUTER_ENABLED` / `MESSAGE_ROUTER_ENABLED` | `false` | Run the SQS message router |
//! | `FC_SCHEDULER_ENABLED` / `DISPATCH_SCHEDULER_ENABLED` | `false` | Run the dispatch scheduler |
//! | `FC_SCHEDULED_JOB_ENABLED` / `SCHEDULED_JOB_SCHEDULER_ENABLED` | `false` | Run the scheduled-job cron engine |
//! | `FC_STREAM_PROCESSOR_ENABLED` / `STREAM_PROCESSOR_ENABLED` | `false` | Run the CQRS stream processor |
//! | `FC_OUTBOX_ENABLED` / `OUTBOX_PROCESSOR_ENABLED` | `false` | Run the outbox processor |
//!
//! ### Dispatch scheduler (Go's names; the scheduler refuses to start without a queue)
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_DISPATCH_QUEUE_TYPE` / `DISPATCH_QUEUE_TYPE` | - | `SQS` or `POSTGRES` |
//! | `FC_DISPATCH_QUEUE_URL` / `DISPATCH_QUEUE_URL` | - | Any SQS URL in the account; its account (and region) address the per-tenant queues |
//! | `FC_DISPATCH_QUEUE_REGION` / `DISPATCH_QUEUE_REGION` | from the URL | SQS region |
//! | `FC_DISPATCH_QUEUE_PREFIX` | - | Queue name prefix, required for SQS (`{prefix}-{tenant}-{priority}.fifo`) |
//! | `FC_DISPATCH_PROCESSING_ENDPOINT` / `DISPATCH_SCHEDULER_PROCESSING_ENDPOINT` | `http://localhost:{port}/api/dispatch/process` | The router's callback URL |
//! | `FLOWCATALYST_APP_KEY` | - | Signs the router's dispatch tokens (required) |
//!
//! ### Standby / HA
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_STANDBY_ENABLED` / `STANDBY_ENABLED` | `false` | Enable Redis leader election |
//! | `FC_STANDBY_REDIS_URL` / `REDIS_URL` | `redis://127.0.0.1:6379` | Redis URL (`rediss://` for TLS) |
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

use axum::{response::Json, routing::get, Router};
use tower_http::cors::{AllowOrigin, CorsLayer};
// SetResponseHeaderLayer moved to PlatformRoutes
use tower_http::trace::TraceLayer;
// CACHE_CONTROL moved to PlatformRoutes
// SPA serving is handled by PlatformRoutes::build()
use anyhow::Result;
use axum::http::{header as http_header, HeaderValue, Method};
use tokio::{net::TcpListener, sync::watch};
use tracing::{info, warn};

use fc_platform::api::middleware::{AppState, AuthLayer};
use fc_platform::repository::{CorsOriginRepository, Repositories};
use fc_platform::usecase::PgUnitOfWork;

use fc_common::config::{
    env_bool, env_first, env_first_bool_go, env_first_parse, env_or, env_or_parse,
};

/// Resolve database URL and (optionally) the live `SecretProvider` it came from.
///
/// Go's precedence (`fc_platform::shared::database::database_source`):
/// 1. `FC_DATABASE_URL` / `DATABASE_URL` — full connection string
/// 2. `DB_HOST` + `DB_NAME` + `DB_SECRET_ARN` — AWS Secrets Manager
///    (`DB_SECRET_PROVIDER=aws`, the default; anything else is refused)
/// 3. `DB_HOST` + `DB_NAME` + `DB_USERNAME` + `DB_PASSWORD` — explicit credentials
/// 4. nothing — the local default `postgresql://postgres@localhost:5432/flowcatalyst`
///
/// When mode 2 is used the `SecretProvider` is also returned so the caller
/// can register the credential-refresh task on every pool it opens.
async fn resolve_database_url() -> Result<(
    String,
    Option<Arc<dyn fc_platform::shared::database::SecretProvider>>,
)> {
    use fc_platform::shared::database::{
        database_source_from_env, AwsSecretProvider, DatabaseSource, SecretProvider,
    };
    match database_source_from_env()? {
        DatabaseSource::Url(url) => Ok((url, None)),
        DatabaseSource::AwsSecretsManager {
            secret_arn,
            host,
            db_name,
            fallback_port,
        } => {
            info!(secret_arn = %secret_arn, "Resolving database credentials from AWS Secrets Manager");
            let provider = Arc::new(AwsSecretProvider::new(
                secret_arn,
                host.clone(),
                db_name.clone(),
                fallback_port,
            ));
            let url = provider.get_db_url().await?;
            info!(
                "Database URL resolved from Secrets Manager (host: {}, db: {})",
                host, db_name
            );
            Ok((url, Some(provider as Arc<dyn SecretProvider>)))
        }
    }
}

/// The built SPA to serve, when the platform API runs: `FC_STATIC_DIR`, else
/// the image's `/app/frontend/dist` when it holds an `index.html`. Go embeds
/// the SPA in the binary and serves it whenever the platform is enabled; the
/// Rust image ships it beside the binary, and the production task definition
/// sets no directory, so the image path is the default.
fn static_dir() -> Option<String> {
    const IMAGE_SPA_DIR: &str = "/app/frontend/dist";
    std::env::var("FC_STATIC_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::path::Path::new(IMAGE_SPA_DIR)
                .join("index.html")
                .is_file()
                .then(|| IMAGE_SPA_DIR.to_string())
        })
}

// ── Maintenance ──────────────────────────────────────────────────────────────

/// `fc-server backfill-secrets [--apply]`: encrypt stored secrets that
/// predate encrypt-on-write (see `fc_platform::shared::secret_backfill`).
/// Dry run by default; prints counts per column, never values. Needs the
/// same `FLOWCATALYST_APP_KEY` as the server and connects with the server's
/// database settings. It does not run migrations.
async fn backfill_secrets(args: &[String]) -> Result<()> {
    use fc_platform::shared::encryption_service::EncryptionService;
    use fc_platform::shared::secret_backfill;

    let apply = match args {
        [] => false,
        [flag] if flag == "--apply" => true,
        _ => anyhow::bail!("usage: fc-server backfill-secrets [--apply]"),
    };
    let enc = EncryptionService::from_env().ok_or_else(|| {
        anyhow::anyhow!("FLOWCATALYST_APP_KEY is not set (or invalid); it is required to encrypt")
    })?;

    let (database_url, _) = resolve_database_url().await?;
    let pool = fc_platform::shared::database::create_pool(&database_url)
        .await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    let reports = secret_backfill::backfill_secrets(&pool, &enc, apply)
        .await
        .map_err(|e| anyhow::anyhow!("Secret backfill failed: {}", e))?;
    for line in secret_backfill::format_report(&reports, apply) {
        println!("{line}");
    }
    Ok(())
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // JSON logs by default, as Go's fc-server writes them (CloudWatch).
    fc_common::logging::init_production_logging("fc-server");

    // Both rustls crypto backends are compiled into this binary (the AWS SDK
    // brings aws-lc-rs, others ring), so a client that asks rustls for "the
    // default" provider — the `rediss://` Redis connection behind standby and
    // the rate-limit store — would panic without a process-level choice.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Maintenance subcommands run instead of the server.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("backfill-secrets") {
        return backfill_secrets(&args[1..]).await;
    }

    info!("Starting FlowCatalyst Unified Server");

    // ── Configuration ────────────────────────────────────────────────────────
    // Go's names first (internal/server/envcfg.go LoadEnv), then the aliases
    // the ECS task definitions set. Toggles use Go's envBool truth table.
    // API port: FC_API_PORT, then PORT, default 8080 — Go's. (The router
    // task definition's API_PORT is not read, exactly as Go ignores it; its
    // value is Go's default anyway.)
    let api_port: u16 = env_first_parse(&["FC_API_PORT", "PORT"], 8080);
    let metrics_port: u16 = env_or_parse("FC_METRICS_PORT", 9090);
    // JWT issuer should be the external base URL per OIDC spec
    let jwt_issuer = env_first(
        &["FC_JWT_ISSUER", "FC_EXTERNAL_BASE_URL", "EXTERNAL_BASE_URL"],
        &format!("http://localhost:{api_port}"),
    );

    // Subsystem toggles (TS names: PLATFORM_ENABLED, MESSAGE_ROUTER_ENABLED, etc.)
    let platform_enabled = env_first_bool_go(&["FC_PLATFORM_ENABLED", "PLATFORM_ENABLED"], true);
    let router_enabled = env_first_bool_go(&["FC_ROUTER_ENABLED", "MESSAGE_ROUTER_ENABLED"], false);
    let scheduler_enabled = env_first_bool_go(
        &["FC_SCHEDULER_ENABLED", "DISPATCH_SCHEDULER_ENABLED"],
        false,
    );
    // The scheduled-job cron engine has its own toggle, as in Go (the worker
    // task sets FC_SCHEDULED_JOB_ENABLED=true beside the dispatch scheduler).
    let scheduled_job_enabled = env_first_bool_go(
        &[
            "FC_SCHEDULED_JOB_ENABLED",
            "SCHEDULED_JOB_SCHEDULER_ENABLED",
        ],
        false,
    );
    let stream_enabled = env_first_bool_go(
        &["FC_STREAM_PROCESSOR_ENABLED", "STREAM_PROCESSOR_ENABLED"],
        false,
    );
    let outbox_enabled =
        env_first_bool_go(&["FC_OUTBOX_ENABLED", "OUTBOX_PROCESSOR_ENABLED"], false);

    // Standby / HA
    let standby_enabled = env_first_bool_go(&["FC_STANDBY_ENABLED", "STANDBY_ENABLED"], false);
    let standby_redis_url = env_first(
        &["FC_STANDBY_REDIS_URL", "REDIS_URL"],
        "redis://127.0.0.1:6379",
    );
    let standby_lock_key = env_or("FC_STANDBY_LOCK_KEY", "fc:server:leader");

    info!(
        platform = platform_enabled,
        router = router_enabled,
        scheduler = scheduler_enabled,
        scheduled_job = scheduled_job_enabled,
        stream = stream_enabled,
        outbox = outbox_enabled,
        standby = standby_enabled,
        api_port,
        metrics_port,
        "Subsystem configuration"
    );

    // The router's environment is validated before anything connects: a
    // half-configured platform credential refuses to start, as Go's
    // newRouterServer does.
    let router_env = if router_enabled {
        Some(fc_router::bootstrap::RouterEnv::from_env()?)
    } else {
        None
    };

    // A malformed FLOWCATALYST_APP_KEY is fatal at boot, as in Go (an unset
    // one is the documented "encryption disabled" state).
    fc_platform::shared::encryption_service::EncryptionService::from_env_checked()
        .map_err(|e| anyhow::anyhow!("FLOWCATALYST_APP_KEY: {e}"))?;

    // ── Database ─────────────────────────────────────────────────────────────
    // Only the subsystems that read or write Postgres need it (Go:
    // `needsDB`). A router-only instance (MESSAGE_ROUTER_ENABLED=true,
    // PLATFORM_ENABLED=false) reads its configuration from the platform API
    // and connects to no database at all.
    let needs_db = platform_enabled
        || stream_enabled
        || scheduler_enabled
        || scheduled_job_enabled
        || outbox_enabled;
    let db = if needs_db {
        Some(connect_database().await?)
    } else {
        info!(
            router = router_enabled,
            "no database-backed subsystem enabled; skipping postgres connect/migrate/seed"
        );
        None
    };

    // ── Leader Election ──────────────────────────────────────────────────────
    // Shared watch channel: true = active (process), false = standby (pause)
    let (active_tx, active_rx) = watch::channel(!standby_enabled); // if standby disabled, always active

    let leader_election: Option<Arc<fc_standby::LeaderElection>> = if standby_enabled {
        info!(redis_url = %standby_redis_url, lock_key = %standby_lock_key, "Initializing leader election");
        let config = fc_standby::LeaderElectionConfig::new(standby_redis_url)
            .with_lock_key(standby_lock_key);
        let election = Arc::new(
            fc_standby::LeaderElection::new(config)
                .await
                .map_err(|e| anyhow::anyhow!("Leader election init failed: {}", e))?,
        );
        election
            .clone()
            .start()
            .await
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

    let is_leader = move || leader_election.as_ref().is_none_or(|e| e.is_leader());

    // ── Platform ─────────────────────────────────────────────────────────────
    let (app, repos) = match db.as_ref() {
        Some(db) => {
            let (app, repos) =
                init_platform(db, platform_enabled, api_port, jwt_issuer, standby_enabled).await?;
            (app, Some(repos))
        }
        None => (minimal_app(), None),
    };

    // ── Router ───────────────────────────────────────────────────────────────
    // Go mounts the router's HTTP surface under FC_ROUTER_HTTP_PREFIX
    // (default /router) on the API listener; `/health` at the root stays the
    // plain liveness answer the load balancer probes.
    let (app, router_runtime) = match router_env {
        Some(env) => {
            info!("Starting message router subsystem...");
            let (runtime, router_app) = start_router(&env, active_rx.clone()).await?;
            let prefix = env
                .http_prefix
                .clone()
                .filter(|p| !p.trim().is_empty() && p.trim() != "/")
                .map(|p| format!("/{}", p.trim().trim_matches('/')))
                .unwrap_or_else(|| "/router".to_string());
            info!(prefix = %prefix, "router HTTP mounted");
            (app.nest(&prefix, router_app), Some(runtime))
        }
        None => (app, None),
    };

    // ── Background Processors ────────────────────────────────────────────────

    // Scheduler (dispatch job polling)
    if scheduler_enabled {
        let db = db.as_ref().expect("scheduler needs the database");
        info!("Starting scheduler subsystem...");
        spawn_scheduler(&db.pool, active_rx.clone(), api_port).await?;
    }

    // Scheduled-job cron engine (its own toggle, as Go)
    if scheduled_job_enabled {
        let repos = repos
            .as_ref()
            .expect("the scheduled-job scheduler needs the repositories");
        info!("Starting scheduled-job scheduler subsystem...");
        spawn_scheduled_job_scheduler(repos, active_rx.clone()).await?;
    }

    // Stream processor (CQRS projections)
    let _stream_handle = if stream_enabled {
        let db = db.as_ref().expect("stream processor needs the database");
        info!("Starting stream processor subsystem...");
        Some(
            spawn_stream_processor(
                &db.url,
                db.secret_provider.clone(),
                db.secret_refresh_interval,
                active_rx.clone(),
            )
            .await?,
        )
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
            let strategy = Arc::new(fc_router::AwsAlbTrafficStrategy::new(
                alb_config,
                &aws_config,
            ));
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
    // Stops both listeners at shutdown; see `drain_http`.
    let http_stop = tokio_util::sync::CancellationToken::new();
    // Keep-alive idle 75 s, 30 s to read a request (owner ruling 10).
    let api_task = {
        let stop = http_stop.clone();
        tokio::spawn(async move {
            fc_platform::router::serve_api(api_listener, app, stop.cancelled_owned()).await;
        })
    };

    let metrics_addr = format!("0.0.0.0:{}", metrics_port);
    info!(
        "Metrics server listening on http://{}/metrics",
        metrics_addr
    );

    let is_leader_for_health = is_leader.clone();
    let health_state = HealthState {
        platform_enabled,
        router_enabled,
        scheduler_enabled,
        scheduled_job_enabled,
        stream_enabled,
        outbox_enabled,
        is_leader: Arc::new(is_leader_for_health),
    };

    let metrics_app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route(
            "/health",
            get({
                let state = health_state.clone();
                move || combined_health_handler(state.clone())
            }),
        )
        .route(
            "/ready",
            get({
                let state = health_state.clone();
                move || ready_handler(state.clone())
            }),
        );

    let metrics_listener = TcpListener::bind(&metrics_addr).await?;
    let metrics_task = {
        let stop = http_stop.clone();
        tokio::spawn(async move {
            if let Err(e) = axum::serve(metrics_listener, metrics_app)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await
            {
                warn!(error = %e, "metrics server stopped with an error");
            }
        })
    };

    // ── Startup Summary ──────────────────────────────────────────────────────
    let state = |on: bool| if on { "ENABLED" } else { "DISABLED" };
    info!("=== FlowCatalyst Unified Server Started ===");
    info!("  Platform API: {}", state(platform_enabled));
    info!("  Router:       {}", state(router_enabled));
    info!("  Scheduler:    {}", state(scheduler_enabled));
    info!("  Scheduled jobs: {}", state(scheduled_job_enabled));
    info!("  Stream:       {}", state(stream_enabled));
    info!("  Outbox:       {}", state(outbox_enabled));
    info!(
        "  Database:     {}",
        if needs_db { "CONNECTED" } else { "NONE" }
    );
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

    // The router drains its pools before the listeners go (Go: Run cancels
    // the subsystems, then waits for them).
    if let Some(runtime) = router_runtime {
        runtime.shutdown().await;
    }

    // Then let the HTTP servers drain, as Go's `server.Run` does
    // (`apiSrv.Shutdown` with 30 s): stop accepting, finish the requests in
    // flight. A `/api/dispatch/process` call in flight has already sent its
    // webhook; aborting it lost the outcome and left the job PROCESSING
    // (delivery run 3, `platform-down`).
    drain_http(&http_stop, vec![api_task, metrics_task], HTTP_DRAIN_TIMEOUT).await;

    // Shutdown stream processor if running
    if let Some(handle) = _stream_handle {
        handle.stop().await;
    }

    info!("FlowCatalyst Unified Server shutdown complete");
    Ok(())
}

/// The platform database, connected, migrated and seeded.
struct Database {
    pool: sqlx::PgPool,
    url: String,
    secret_provider: Option<Arc<dyn fc_platform::shared::database::SecretProvider>>,
    secret_refresh_interval: Duration,
}

/// Connect, migrate and seed the platform database (Go: connect, migrate,
/// seed — only when a database-backed subsystem runs).
async fn connect_database() -> Result<Database> {
    let (database_url, secret_provider) = resolve_database_url().await?;
    info!("Connecting to PostgreSQL...");
    let pg_pool = fc_platform::shared::database::create_pool(&database_url)
        .await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    fc_platform::shared::database::run_migrations(
        &pg_pool,
        fc_platform::shared::database::MigrationProfile::Production,
    )
    .await
    .map_err(|e| anyhow::anyhow!("PostgreSQL migrations failed: {}", e))?;

    fc_platform::shared::database::seed_builtin_roles(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Built-in role seeding failed: {}", e))?;

    fc_platform::shared::database::seed_platform_application(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Platform application seeding failed: {}", e))?;

    // Go seeds the platform event-type catalogue on every start.
    fc_platform::shared::database::seed_platform_event_types(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Platform event type seeding failed: {}", e))?;

    fc_platform::shared::default_processes::seed_default_processes(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Default processes seeding failed: {}", e))?;

    // Create the initial platform admin if no anchor user exists yet. No-op
    // on subsequent boots; gated on FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL +
    // _PASSWORD env vars when first run.
    fc_platform::shared::bootstrap_admin::bootstrap_admin_user(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Bootstrap admin seeding failed: {}", e))?;

    // Referential-integrity scan — warns about orphaned junction rows.
    fc_platform::shared::integrity_scan::run(&pg_pool).await;

    // Bootstrap of users / clients / applications / service accounts is
    // owned by `fc-dev init`. fc-server is the production binary path —
    // it relies on bootstrap_admin (env-driven) above for the first
    // admin and operators take it from there via the platform UI / API.

    // ── DB credential refresh (AWS Secrets Manager rotation) ─────────────────
    // When credentials come from a secret provider, poll it on an interval and
    // update the pool's connect options when the password rotates. This avoids
    // the failure mode where AWS rotates the password and the pool keeps using
    // the now-stale credentials. Mirrors the TS implementation.
    // DB_SECRET_REFRESH_INTERVAL_MS (default 5 min; zero or negative: off).
    // Every pool opened from these credentials registers its own refresh.
    let secret_refresh_interval = fc_platform::shared::database::secret_refresh_interval_from_env();
    if let Some(provider) = secret_provider.clone() {
        fc_platform::shared::database::start_secret_refresh(
            provider,
            pg_pool.clone(),
            database_url.clone(),
            secret_refresh_interval,
        );
    }

    Ok(Database {
        pool: pg_pool,
        url: database_url,
        secret_provider,
        secret_refresh_interval,
    })
}

/// Platform wiring over the database: repositories, auth, housekeeping,
/// and the HTTP app (the platform API when enabled, else just `/health`).
async fn init_platform(
    db: &Database,
    platform_enabled: bool,
    api_port: u16,
    jwt_issuer: String,
    standby_enabled: bool,
) -> Result<(Router, Repositories)> {
    let pg_pool = &db.pool;
    // Repositories and auth are always initialized (needed by health checks and
    // potentially by background processors).

    let repos = Repositories::new(pg_pool);
    info!("Repositories initialized");

    // Event fan-out runs inside the stream processor (fc-stream). See
    // `spawn_stream_processor` below.

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
        let cors_repo_bg = CorsOriginRepository::new(pg_pool);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await;
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
                    Err(e) => warn!("Failed to refresh CORS origins: {}", e),
                }
            }
        });
    }

    // Sync code-defined roles
    {
        let role_sync = fc_platform::service::RoleSyncService::new(std::sync::Arc::new(
            fc_platform::repository::RoleRepository::new(pg_pool),
        ));
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            warn!("Role sync failed: {}", e);
        }
    }

    // Auth services
    let auth_init_config = fc_platform::shared::server_setup::AuthInitConfig {
        issuer: jwt_issuer,
        ..fc_platform::shared::server_setup::AuthInitConfig::from_env("http://localhost:8080")
    };
    // The session cookie lives as long as the session JWT (Go: both from
    // OIDC_SESSION_TTL).
    let session_ttl_secs = auth_init_config.session_token_expiry_secs;
    let auth_services =
        fc_platform::shared::server_setup::init_auth_services(&repos, auth_init_config)?;
    info!("Auth services initialized");

    let unit_of_work = Arc::new(PgUnitOfWork::new(pg_pool.clone()));

    let platform_application_id = repos
        .application_repo
        .find_by_code("platform")
        .await?
        .ok_or_else(|| anyhow::anyhow!("platform application row missing after seeding"))?
        .id;

    // Distributed rate-limit store (Redis when FC_REDIS_URL is reachable,
    // Postgres fallback). Constructed once here so the choice is logged at
    // startup, then handed to the platform router builder.
    let rate_limit_store =
        fc_platform::shared::rate_limit_store::build_rate_limit_store(pg_pool.clone()).await;
    let rate_limit_policies =
        Arc::new(fc_platform::shared::rate_limit_store::RateLimitPolicies::from_env());

    // The stranded-sibling reaper (Go's A-01 backstop): platform
    // housekeeping, run wherever the platform is, not leader-gated (each
    // sweep is a status-guarded UPDATE).
    if platform_enabled {
        tokio::spawn(fc_platform::dispatch_job::reaper::run_reaper(
            repos.dispatch_job_repo.clone(),
            fc_platform::dispatch_job::reaper::DEFAULT_REAPER_INTERVAL,
            fc_platform::dispatch_job::reaper::DEFAULT_PROCESSING_LIVE_AFTER,
            tokio_util::sync::CancellationToken::new(),
        ));
    }

    // Clear lapsed OAuth secret-rotation overlaps every minute (Go's auth
    // purger does the same).
    if platform_enabled {
        fc_platform::shared::server_setup::spawn_lapsed_previous_secret_purge(
            repos.oauth_client_repo.clone(),
        );
    }

    // Hourly prune of the Postgres rate-limit table (no-op for Redis — TTLs
    // age keys out automatically). Keeps row count bounded at peak-QPS ×
    // max-policy-window.
    if platform_enabled {
        let store = rate_limit_store.clone();
        let max_window = rate_limit_policies.max_window();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
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

    // ── Build HTTP app ───────────────────────────────────────────────────────
    let app = if platform_enabled {
        build_platform_app(
            api_port,
            &auth_services,
            &unit_of_work,
            &repos,
            &cors_origins_cache,
            standby_enabled,
            platform_application_id,
            rate_limit_store.clone(),
            rate_limit_policies.clone(),
            session_ttl_secs,
        )
    } else {
        minimal_app()
    };
    Ok((app, repos))
}

/// The API listener's app when the platform is off: Go's `/health`
/// (`{"status":"UP","version":…}`, always 200 — the load balancer's probe).
fn minimal_app() -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .layer(TraceLayer::new_for_http())
}

/// How long the HTTP servers get to finish their in-flight requests at
/// shutdown (Go `server.Run`: `context.WithTimeout(…, 30*time.Second)` around
/// `apiSrv.Shutdown`).
const HTTP_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Stop the HTTP servers gracefully: `stop` makes each stop accepting and
/// finish what it is serving; wait up to `timeout` for all of them, then
/// abort whatever is left.
async fn drain_http(
    stop: &tokio_util::sync::CancellationToken,
    mut servers: Vec<tokio::task::JoinHandle<()>>,
    timeout: std::time::Duration,
) -> bool {
    stop.cancel();
    let all = async {
        for s in servers.iter_mut() {
            let _ = s.await;
        }
    };
    if tokio::time::timeout(timeout, all).await.is_ok() {
        info!("HTTP servers drained");
        return true;
    }
    warn!(
        timeout_secs = timeout.as_secs(),
        "HTTP drain timed out; aborting the requests still in flight"
    );
    for s in &servers {
        s.abort();
    }
    false
}

// ── Platform App Builder ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn build_platform_app(
    api_port: u16,
    auth_services: &fc_platform::shared::server_setup::AuthServices,
    unit_of_work: &Arc<PgUnitOfWork>,
    repos: &Repositories,
    cors_origins_cache: &Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    _standby_enabled: bool,
    platform_application_id: String,
    rate_limit_store: Arc<dyn fc_platform::shared::rate_limit_store::RateLimitStore>,
    rate_limit_policies: Arc<fc_platform::shared::rate_limit_store::RateLimitPolicies>,
    session_ttl_secs: i64,
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
            rate_limit_store,
            rate_limit_policies,
            session_cookie_secure: true,
            session_cookie_same_site: std::env::var("FC_SESSION_COOKIE_SAME_SITE").unwrap_or_else(
                |_| {
                    fc_platform::shared::server_setup::PlatformRoutesConfig::DEFAULT_SAME_SITE
                        .to_string()
                },
            ),
            session_token_expiry_secs: session_ttl_secs,
            static_dir: static_dir(),
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
        platform_application_id,
    );
    let (app, _openapi) = routes.build();

    // Add middleware layers
    let app = app
        .layer(AuthLayer::new(app_state))
        .layer(TraceLayer::new_for_http())
        .layer({
            let cache = cors_origins_cache.clone();
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(
                    move |origin: &HeaderValue, _parts| {
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
                    },
                ))
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                    Method::HEAD,
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

/// Start the message router (Go `newRouterServer` + `Server.Run`): the
/// same runtime as the standalone `fc-router` binary, polling only while
/// this instance leads. Returns the runtime and its HTTP surface.
async fn start_router(
    env: &fc_router::bootstrap::RouterEnv,
    active_rx: watch::Receiver<bool>,
) -> Result<(fc_router::bootstrap::RouterRuntime, Router)> {
    use fc_router::bootstrap::{
        dev_router_config, sqs_client, RouterRuntime, RouterRuntimeOptions, SchemeConsumerFactory,
        SqsPublisher,
    };

    let metrics_handle = fc_router::init_prometheus_recorder();
    let sqs = sqs_client(env.dev_mode).await;
    let runtime = RouterRuntime::start(
        env,
        RouterRuntimeOptions {
            consumer_factory: Arc::new(SchemeConsumerFactory::new(sqs.clone())),
            standby: None,
            leadership: Some(active_rx),
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("router init: {e}"))?;
    let publisher = Arc::new(SqsPublisher::new(
        sqs,
        runtime.queue_manager.clone(),
        env.dev_mode
            .then(dev_router_config)
            .and_then(|c| c.queues.first().map(|q| q.uri.clone())),
    ));
    let app = runtime.api_router(publisher, Some(metrics_handle), None);
    Ok((runtime, app))
}

/// Spawn the dispatch scheduler, gated on leadership.
///
/// Refuses to start (an error at boot, not a silent no-op) when no dispatch
/// queue is configured or `FLOWCATALYST_APP_KEY` is missing: a scheduler
/// with nowhere to publish would claim jobs into the void, and one without
/// the key would publish tokens `/api/dispatch/process` rejects.
async fn spawn_scheduler(
    pg_pool: &sqlx::PgPool,
    active_rx: watch::Receiver<bool>,
    api_port: u16,
) -> Result<()> {
    use fc_platform::scheduler::{DispatchAuthService, DispatchQueueSettings, DispatchScheduler};

    let settings = DispatchQueueSettings::from_env()
        .map_err(|e| anyhow::anyhow!("dispatch scheduler refused to start: {e}"))?;
    let auth = DispatchAuthService::from_env().ok_or_else(|| {
        anyhow::anyhow!(
            "dispatch scheduler refused to start: FLOWCATALYST_APP_KEY is not set, so dispatch \
             tokens cannot be signed"
        )
    })?;
    let config = load_scheduler_config(api_port);
    let scheduler = DispatchScheduler::from_settings(config, pg_pool.clone(), &settings, auth)
        .await
        .map_err(|e| anyhow::anyhow!("dispatch scheduler refused to start: {e}"))?;

    // Only the leader claims: the per-group order needs one active scheduler.
    let is_leader: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || *active_rx.borrow());
    tokio::spawn(async move {
        scheduler
            .run(is_leader, tokio_util::sync::CancellationToken::new())
            .await;
    });
    Ok(())
}

/// Spawn the scheduled-job scheduler (cron poller + webhook dispatcher),
/// gated on leadership. Single-replica assumption inside the active region.
async fn spawn_scheduled_job_scheduler(
    repos: &fc_platform::repository::Repositories,
    mut active_rx: watch::Receiver<bool>,
) -> Result<()> {
    use fc_platform::scheduled_job::scheduler::{
        ScheduledJobSchedulerConfig, ScheduledJobSchedulerService,
    };

    // Firings are signed with each job's application's credentials (Java
    // JobDispatcher).
    let credentials = Arc::new(
        fc_platform::service_account::outbound_credentials::OutboundCredentialsResolver::new(
            repos.service_account_repo.clone(),
            fc_platform::shared::encryption_service::EncryptionService::from_env().map(Arc::new),
        ),
    );
    let svc = Arc::new(
        ScheduledJobSchedulerService::new(
            ScheduledJobSchedulerConfig::from_env(),
            repos.scheduled_job_repo.clone(),
            repos.scheduled_job_instance_repo.clone(),
        )
        .with_credentials(credentials),
    );

    tokio::spawn(async move {
        let mut handles: Option<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)>;
        loop {
            // Wait until active.
            if !*active_rx.borrow() {
                info!("Scheduled-job scheduler: waiting for leadership...");
                loop {
                    if active_rx.changed().await.is_err() {
                        return;
                    }
                    if *active_rx.borrow() {
                        break;
                    }
                }
                info!("Scheduled-job scheduler: acquired leadership, starting");
            }

            // Start. svc holds the shutdown channel internally; abort handles
            // on leadership loss to avoid blocking on in-flight HTTP.
            handles = Some(svc.start());

            // Wait for leadership loss.
            let mut lost_rx = active_rx.clone();
            loop {
                if lost_rx.changed().await.is_err() {
                    svc.shutdown();
                    if let Some((p, d)) = handles.take() {
                        p.abort();
                        d.abort();
                    }
                    return;
                }
                if !*lost_rx.borrow() {
                    info!("Scheduled-job scheduler: lost leadership, stopping");
                    svc.shutdown();
                    if let Some((p, d)) = handles.take() {
                        p.abort();
                        d.abort();
                    }
                    break;
                }
            }
        }
    });

    Ok(())
}

/// Dispatch scheduler tuning, read from the environment. The defaults are
/// Go's; the processing endpoint takes Go's names
/// (`FC_DISPATCH_PROCESSING_ENDPOINT`, then
/// `DISPATCH_SCHEDULER_PROCESSING_ENDPOINT`, which the ECS task definitions
/// set), then the older `FC_SCHEDULER_PROCESSING_ENDPOINT`, and defaults to
/// this server's own listener.
fn load_scheduler_config(api_port: u16) -> fc_platform::scheduler::SchedulerConfig {
    let defaults = fc_platform::scheduler::SchedulerConfig::default();
    let processing_endpoint = [
        "FC_DISPATCH_PROCESSING_ENDPOINT",
        "DISPATCH_SCHEDULER_PROCESSING_ENDPOINT",
        "FC_SCHEDULER_PROCESSING_ENDPOINT",
    ]
    .iter()
    .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
    .unwrap_or_else(|| format!("http://localhost:{api_port}/api/dispatch/process"));
    fc_platform::scheduler::SchedulerConfig {
        poll_interval: Duration::from_millis(env_or_parse(
            "FLOWCATALYST_SCHEDULER_POLL_INTERVAL_MS",
            defaults.poll_interval.as_millis() as u64,
        )),
        batch_size: env_or_parse("FLOWCATALYST_SCHEDULER_BATCH_SIZE", defaults.batch_size),
        processing_endpoint,
        ..defaults
    }
}

/// Spawn the CQRS stream processor, gated on leadership.
///
/// Builds a small dedicated pool (4 conns) so the projection loops don't
/// contend with the platform API. When credentials come from a secret
/// provider, the same refresh task is started against this pool — without
/// it, rotation (RDS-managed Secrets Manager) silently invalidates the
/// stream pool while the platform's pool keeps working, since each pool
/// caches connect options independently.
async fn spawn_stream_processor(
    database_url: &str,
    secret_provider: Option<Arc<dyn fc_platform::shared::database::SecretProvider>>,
    secret_refresh_interval: Duration,
    mut active_rx: watch::Receiver<bool>,
) -> Result<StreamProcessorShutdown> {
    use fc_stream::{start_stream_processor, StreamProcessorConfig};

    let config = StreamProcessorConfig {
        events_enabled: env_bool("FC_STREAM_EVENTS_ENABLED", true),
        events_batch_size: env_or_parse("FC_STREAM_EVENTS_BATCH_SIZE", 100),
        dispatch_jobs_enabled: env_bool("FC_STREAM_DISPATCH_JOBS_ENABLED", true),
        dispatch_jobs_batch_size: env_or_parse("FC_STREAM_DISPATCH_JOBS_BATCH_SIZE", 100),
        fan_out_enabled: env_bool("FC_STREAM_FAN_OUT_ENABLED", true),
        fan_out_batch_size: env_or_parse("FC_STREAM_FAN_OUT_BATCH_SIZE", 200),
        fan_out_subscription_refresh_secs: env_or_parse("FC_STREAM_FAN_OUT_SUBS_REFRESH_SECS", 5),
        partition_manager_enabled: env_bool("FC_STREAM_PARTITION_MANAGER_ENABLED", true),
    };

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .idle_timeout(Duration::from_secs(20))
        .acquire_timeout(Duration::from_secs(30))
        .connect(database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Stream processor PG pool failed: {}", e))?;

    if let Some(provider) = secret_provider {
        fc_platform::shared::database::start_secret_refresh(
            provider,
            pool.clone(),
            database_url.to_string(),
            secret_refresh_interval,
        );
    }

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
                fan_out_enabled: config.fan_out_enabled,
                fan_out_batch_size: config.fan_out_batch_size,
                fan_out_subscription_refresh_secs: config.fan_out_subscription_refresh_secs,
                partition_manager_enabled: config.partition_manager_enabled,
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

    Ok(StreamProcessorShutdown {
        _stop_tx: Some(stop_tx),
    })
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
async fn spawn_outbox_processor(mut active_rx: watch::Receiver<bool>) -> Result<()> {
    use fc_outbox::repository::{OutboxRepository, OutboxTableConfig};
    use fc_outbox::{EnhancedOutboxProcessor, EnhancedProcessorConfig, OutboxBackend};

    let backend: OutboxBackend = env_or("FC_OUTBOX_DB_TYPE", "postgres").parse()?;

    let table_config = OutboxTableConfig {
        events_table: env_or("FC_OUTBOX_EVENTS_TABLE", "outbox_messages"),
        dispatch_jobs_table: env_or("FC_OUTBOX_DISPATCH_JOBS_TABLE", "outbox_messages"),
        audit_logs_table: env_or("FC_OUTBOX_AUDIT_LOGS_TABLE", "outbox_messages"),
    };

    let outbox_repo: Arc<dyn OutboxRepository> = match backend {
        OutboxBackend::Sqlite => {
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
        OutboxBackend::Postgres => {
            let url = std::env::var("FC_OUTBOX_DB_URL")
                .map_err(|_| anyhow::anyhow!("FC_OUTBOX_DB_URL required for postgres outbox"))?;
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(10)
                .connect(&url)
                .await?;
            let repo =
                fc_outbox::postgres::PostgresOutboxRepository::with_config(pool, table_config);
            repo.init_schema().await?;
            Arc::new(repo)
        }
        OutboxBackend::Mongo => {
            return Err(anyhow::anyhow!(
                "The mongo outbox backend is not supported by fc-server; run fc-outbox-processor instead"
            ))
        }
    };

    // Go's variable names and defaults, with the earlier names as fallbacks.
    let config = EnhancedProcessorConfig::from_env();

    let processor = Arc::new(EnhancedOutboxProcessor::new(config, outbox_repo)?);

    tokio::spawn(async move {
        loop {
            // Wait until active
            if !*active_rx.borrow() {
                info!("Outbox: waiting for leadership...");
                loop {
                    if active_rx.changed().await.is_err() {
                        return;
                    }
                    if *active_rx.borrow() {
                        break;
                    }
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
    scheduled_job_enabled: bool,
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
            "scheduled_job": if state.scheduled_job_enabled { if leader { "UP" } else { "STANDBY" } } else { "DISABLED" },
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

/// Go's metrics-port `/ready`: the status plus which subsystems run.
async fn ready_handler(state: HealthState) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ready",
        "platform": state.platform_enabled,
        "router": state.router_enabled,
        "scheduler": state.scheduler_enabled,
        "scheduled_job": state.scheduled_job_enabled,
        "stream": state.stream_enabled,
        "outbox": state.outbox_enabled,
        "mcp": false,
    }))
}

#[cfg(test)]
mod drain_tests {
    use super::*;
    use std::time::Duration;

    async fn slow(delay: Duration) -> Router {
        Router::new().route(
            "/slow",
            axum::routing::post(move || async move {
                tokio::time::sleep(delay).await;
                "done"
            }),
        )
    }

    async fn serve(
        app: Router,
    ) -> (
        String,
        tokio_util::sync::CancellationToken,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = tokio_util::sync::CancellationToken::new();
        let task = {
            let stop = stop.clone();
            tokio::spawn(async move {
                fc_platform::router::serve_api(listener, app, stop.cancelled_owned()).await;
            })
        };
        (format!("http://{addr}/slow"), stop, task)
    }

    /// Delivery run 3, `platform-down`: a request in flight at shutdown (a
    /// `/api/dispatch/process` call whose webhook is already out) is
    /// finished and answered, not cut, as Go's `apiSrv.Shutdown` does.
    #[tokio::test]
    async fn shutdown_finishes_the_request_in_flight() {
        let (url, stop, task) = serve(slow(Duration::from_millis(300)).await).await;
        let call = tokio::spawn(async move { reqwest::Client::new().post(url).send().await });
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(drain_http(&stop, vec![task], Duration::from_secs(5)).await);
        let resp = call.await.unwrap().expect("answered, not reset");
        assert_eq!(resp.text().await.unwrap(), "done");
    }

    /// The drain is bounded: past the timeout what is left is aborted.
    #[tokio::test]
    async fn the_drain_gives_up_at_its_timeout() {
        let (url, stop, task) = serve(slow(Duration::from_secs(30)).await).await;
        tokio::spawn(async move { reqwest::Client::new().post(url).send().await });
        tokio::time::sleep(Duration::from_millis(100)).await;

        let started = std::time::Instant::now();
        assert!(!drain_http(&stop, vec![task], Duration::from_millis(200)).await);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
