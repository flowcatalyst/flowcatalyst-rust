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

use axum::{response::Json, routing::get, Router};
use tower_http::cors::{AllowOrigin, CorsLayer};
// SetResponseHeaderLayer moved to PlatformRoutes
use tower_http::trace::TraceLayer;
// CACHE_CONTROL moved to PlatformRoutes
// SPA serving is handled by PlatformRoutes::build()
use anyhow::Result;
use axum::http::{header as http_header, HeaderValue, Method};
use tokio::{net::TcpListener, sync::watch};
use tracing::{error, info, warn};

use fc_platform::api::middleware::{AppState, AuthLayer};
use fc_platform::repository::{CorsOriginRepository, Repositories};
use fc_platform::usecase::PgUnitOfWork;

use fc_common::config::{
    env_bool, env_bool_alias, env_or, env_or_alias, env_or_alias_parse, env_or_parse,
};

/// Resolve database URL and (optionally) the live `SecretProvider` it came from.
///
/// Supports three modes:
/// 1. `FC_DATABASE_URL` / `DATABASE_URL` — full connection string (preferred for Rust)
/// 2. `DB_HOST` + `DB_NAME` + `DB_SECRET_ARN` — AWS Secrets Manager (TS compatibility)
/// 3. `DB_HOST` + `DB_NAME` + `DB_USERNAME` + `DB_PASSWORD` — explicit credentials
///
/// When mode 2 is used the returned `SecretProvider` is also returned so the
/// caller can spawn the background credential-refresh task.
async fn resolve_database_url() -> Result<(
    String,
    Option<Arc<dyn fc_platform::shared::database::SecretProvider>>,
)> {
    // Mode 1: Full connection string
    if let Ok(url) = std::env::var("FC_DATABASE_URL").or_else(|_| std::env::var("DATABASE_URL")) {
        return Ok((url, None));
    }

    // Mode 2/3: Build from components
    let host = std::env::var("DB_HOST").map_err(|_| {
        anyhow::anyhow!("No database config found. Set FC_DATABASE_URL or DB_HOST+DB_NAME")
    })?;
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
            let url = fc_platform::shared::database::SecretProvider::get_db_url(provider.as_ref())
                .await?;
            info!(
                "Database URL resolved from Secrets Manager (host: {}, db: {})",
                host, name
            );
            return Ok((
                url,
                Some(provider as Arc<dyn fc_platform::shared::database::SecretProvider>),
            ));
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
        format!(
            "postgresql://{}:{}@{}/{}",
            username, password_encoded, host_port, name
        )
    };
    Ok((url, None))
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
    fc_common::logging::init_logging("fc-server");

    // Maintenance subcommands run instead of the server.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("backfill-secrets") {
        return backfill_secrets(&args[1..]).await;
    }

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
    let scheduler_enabled =
        env_bool_alias("FC_SCHEDULER_ENABLED", "DISPATCH_SCHEDULER_ENABLED", false);
    let stream_enabled = env_bool_alias(
        "FC_STREAM_PROCESSOR_ENABLED",
        "STREAM_PROCESSOR_ENABLED",
        false,
    );
    let outbox_enabled = env_bool_alias("FC_OUTBOX_ENABLED", "OUTBOX_PROCESSOR_ENABLED", false);

    // Standby / HA
    let standby_enabled = env_bool_alias("FC_STANDBY_ENABLED", "STANDBY_ENABLED", false);
    let standby_redis_url = env_or_alias(
        "FC_STANDBY_REDIS_URL",
        "REDIS_URL",
        "redis://127.0.0.1:6379",
    );
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
    let secret_refresh_interval = std::time::Duration::from_millis(env_or_parse::<u64>(
        "DB_SECRET_REFRESH_INTERVAL_MS",
        300_000,
    ));
    if let Some(provider) = secret_provider.clone() {
        fc_platform::shared::database::start_secret_refresh(
            provider,
            pg_pool.clone(),
            database_url.clone(),
            secret_refresh_interval,
        );
    }

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

    // ── Platform API ─────────────────────────────────────────────────────────
    // Repositories and auth are always initialized (needed by health checks and
    // potentially by background processors).

    let repos = Repositories::new(&pg_pool);
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
            fc_platform::repository::RoleRepository::new(&pg_pool),
        ));
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            warn!("Role sync failed: {}", e);
        }
    }

    // Auth services
    let auth_init_config = fc_platform::shared::server_setup::AuthInitConfig {
        issuer: jwt_issuer,
        ..fc_platform::shared::server_setup::AuthInitConfig::from_env("http://localhost:3000")
    };
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
    fc_platform::shared::server_setup::spawn_lapsed_previous_secret_purge(
        repos.oauth_client_repo.clone(),
    );

    // Hourly prune of the Postgres rate-limit table (no-op for Redis — TTLs
    // age keys out automatically). Keeps row count bounded at peak-QPS ×
    // max-policy-window.
    {
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
        spawn_scheduler(&pg_pool, active_rx.clone(), api_port).await?;
        spawn_scheduled_job_scheduler(&repos, active_rx.clone()).await?;
    }

    // Stream processor (CQRS projections)
    let _stream_handle = if stream_enabled {
        info!("Starting stream processor subsystem...");
        Some(
            spawn_stream_processor(
                &database_url,
                secret_provider.clone(),
                secret_refresh_interval,
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
    // Keep-alive idle 75 s, 30 s to read a request (owner ruling 10).
    let api_task = tokio::spawn(async move {
        fc_platform::router::serve_api(api_listener, app, std::future::pending()).await;
    });

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
        .route("/ready", get(ready_handler));

    let metrics_listener = TcpListener::bind(&metrics_addr).await?;
    let metrics_task = tokio::spawn(async move {
        axum::serve(metrics_listener, metrics_app).await.unwrap();
    });

    // ── Startup Summary ──────────────────────────────────────────────────────
    info!("=== FlowCatalyst Unified Server Started ===");
    info!(
        "  Platform API: {}",
        if platform_enabled {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "  Router:       {}",
        if router_enabled {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "  Scheduler:    {}",
        if scheduler_enabled {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "  Stream:       {}",
        if stream_enabled {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "  Outbox:       {}",
        if outbox_enabled {
            "ENABLED"
        } else {
            "DISABLED"
        }
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
    auth_services: &fc_platform::shared::server_setup::AuthServices,
    unit_of_work: &Arc<PgUnitOfWork>,
    repos: &Repositories,
    cors_origins_cache: &Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    _standby_enabled: bool,
    platform_application_id: String,
    rate_limit_store: Arc<dyn fc_platform::shared::rate_limit_store::RateLimitStore>,
    rate_limit_policies: Arc<fc_platform::shared::rate_limit_store::RateLimitPolicies>,
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
            session_cookie_same_site: std::env::var("FC_SESSION_COOKIE_SAME_SITE")
                .unwrap_or_else(|_| fc_platform::shared::server_setup::PlatformRoutesConfig::DEFAULT_SAME_SITE.to_string()),
            session_token_expiry_secs: std::env::var("FC_SESSION_TOKEN_EXPIRY_SECS")
                .ok().and_then(|v| v.parse().ok())
                .unwrap_or(fc_platform::shared::server_setup::PlatformRoutesConfig::DEFAULT_SESSION_EXPIRY_SECS),
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

/// Spawn the message router, gated on leadership.
async fn spawn_router(mut active_rx: watch::Receiver<bool>) -> Option<tokio::task::JoinHandle<()>> {
    use fc_router::{
        ConsumerFactory, HealthService, HealthServiceConfig, HttpMediatorConfig, QueueManager,
        WarningService, WarningServiceConfig,
    };

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
    // Pass mediator *config* (not a singleton instance) — QueueManager builds
    // a fresh HttpMediator per pool so each pool has its own HTTP connection
    // pool, sidestepping AWS's 128-stream cap per H/2 connection.
    let queue_manager = Arc::new(
        QueueManager::builder(HttpMediatorConfig::production())
            .warning_service(warning_service.clone())
            .health_service(health_service.clone())
            .consumer_factory(Arc::new(SchemeConsumerFactory {
                sqs_client: sqs_client.clone(),
            }))
            .build(),
    );

    // Load configuration
    let router_config = if dev_mode {
        use fc_common::{PoolConfig, QueueConfig, RouterConfig};
        let sqs_host = env_or(
            "LOCALSTACK_SQS_HOST",
            "http://sqs.eu-west-1.localhost.localstack.cloud:4566",
        );
        RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            }],
            queues: vec![QueueConfig {
                name: "fc-default.fifo".to_string(),
                uri: format!("{}/000000000000/fc-default.fifo", sqs_host),
                connections: 2,
                visibility_timeout: 120,
            }],
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

    // Add consumers, dispatching on each queue URI's scheme (item 1 — every
    // queue used to be handed to the SQS consumer regardless of scheme).
    //
    // Item 1 (router bench rig, 2026-09-07; identical fix to
    // `bin/fc-router/src/main.rs` — see that file's doc comment for the
    // full mechanism): when `!dev_mode`, `sync_service.initial_sync()`
    // above already created AND spawned a poll task for every one of these
    // queues via `QueueManager::reload_config` -> `sync_queue_consumers`
    // (the manager was built with a `consumer_factory` specifically so
    // that path could do this). Running this loop unconditionally used to
    // create a SECOND, independent consumer per queue here too and
    // `add_consumer` it — orphaning the first poll task and racing it with
    // a second one spawned later by `QueueManager::start()`. Dev mode
    // synthesises `router_config` inline above with no config-sync
    // service involved at all, so this loop is still the only thing that
    // ever creates a consumer there.
    if dev_mode {
        let scheme_factory = SchemeConsumerFactory {
            sqs_client: sqs_client.clone(),
        };
        for queue_config in &router_config.queues {
            match scheme_factory.create_consumer(queue_config).await {
                Ok(consumer) => queue_manager.add_consumer(consumer).await,
                Err(e) => {
                    error!(queue = %queue_config.name, error = %e, "Failed to create queue consumer");
                    return None;
                }
            }
        }
    }

    // Losing leadership only pauses polling (owner ruling; as `fc-router`):
    // the manager starts once and follows the leadership watch, so in-flight
    // work finishes and a regained lead resumes the same consumers. Shutting
    // the manager down on a lost lead and calling `start()` again never
    // recovered a shut-down manager.
    queue_manager.set_leader(*active_rx.borrow());
    let follower = queue_manager.clone();
    tokio::spawn(async move {
        while active_rx.changed().await.is_ok() {
            let leader = *active_rx.borrow();
            follower.set_leader(leader);
        }
    });
    let manager = queue_manager.clone();
    let handle = tokio::spawn(async move {
        if let Err(e) = manager.start().await {
            error!("QueueManager error: {}", e);
        }
    });

    Some(handle)
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

// ── Consumer factory: dispatch by queue URI scheme (item 1) ─────────────────
//
// Mirrors the identical factory in `bin/fc-router/src/main.rs` — kept as a
// separate copy per binary (same pattern the pre-existing SQS-only wiring
// used) rather than shared, since `fc-server` is a single monolithic
// `main.rs` with no internal module split today.

struct SchemeConsumerFactory {
    sqs_client: aws_sdk_sqs::Client,
}

#[async_trait::async_trait]
impl fc_router::ConsumerFactory for SchemeConsumerFactory {
    async fn create_consumer(
        &self,
        config: &fc_common::QueueConfig,
    ) -> std::result::Result<Arc<dyn fc_queue::QueueConsumer>, fc_router::RouterError> {
        let scheme = fc_queue::resolve_scheme(&config.uri).map_err(
            fc_router::RouterError::consumer(&config.name, "resolve queue scheme"),
        )?;

        match scheme {
            fc_queue::QueueScheme::Sqs => {
                info!(
                    queue_name = %config.name,
                    queue_uri = %config.uri,
                    visibility_timeout = config.visibility_timeout,
                    "Creating SQS consumer from config"
                );
                let consumer = fc_queue::sqs::SqsQueueConsumer::from_queue_url(
                    self.sqs_client.clone(),
                    config.uri.clone(),
                    config.visibility_timeout as i32,
                )
                .await;
                Ok(Arc::new(consumer))
            }
            fc_queue::QueueScheme::Nats => build_nats_consumer(config).await,
            fc_queue::QueueScheme::Postgres => build_postgres_consumer(config).await,
        }
    }
}

/// Build a NATS JetStream consumer from a `nats://` queue URI — see the
/// identical helper's doc comment in `bin/fc-router/src/main.rs`.
async fn build_nats_consumer(
    config: &fc_common::QueueConfig,
) -> std::result::Result<Arc<dyn fc_queue::QueueConsumer>, fc_router::RouterError> {
    let nats_config = fc_queue::nats::NatsConfig::from_uri(&config.uri).map_err(
        fc_router::RouterError::consumer(&config.name, "invalid NATS URI"),
    )?;
    info!(
        queue_name = %config.name,
        stream = %nats_config.stream_name,
        consumer = %nats_config.consumer_name,
        subject = %nats_config.subject,
        "Creating NATS JetStream consumer from config"
    );
    let consumer = fc_queue::nats::NatsQueueConsumer::new(nats_config)
        .await
        .map_err(fc_router::RouterError::consumer(
            &config.name,
            "NATS consumer setup failed",
        ))?;
    Ok(Arc::new(consumer))
}

/// Build a Postgres queue consumer from a `postgres://` queue URI — connects
/// from the URI itself (a dedicated pool per queue), matching the other
/// routers per `docs/spec/router.md` §7.3. See the identical helper's doc
/// comment in `bin/fc-router/src/main.rs`.
async fn build_postgres_consumer(
    config: &fc_common::QueueConfig,
) -> std::result::Result<Arc<dyn fc_queue::QueueConsumer>, fc_router::RouterError> {
    // Item 1 (router bench rig, 2026-09-07) — see the identical fix's doc
    // comment on `fc_queue::postgres::default_max_connections` and on the
    // sibling helper in `bin/fc-router/src/main.rs`: a hardcoded
    // max_connections(4) starved this queue's acks under load.
    let max_connections = fc_queue::postgres::default_max_connections();
    info!(
        queue_name = %config.name,
        max_connections,
        "Creating Postgres queue consumer from config"
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&config.uri)
        .await
        .map_err(fc_router::RouterError::consumer(
            &config.name,
            "Postgres pool connect failed",
        ))?;

    let visibility = if config.visibility_timeout == 0 {
        30
    } else {
        config.visibility_timeout
    };
    let consumer = fc_queue::postgres::PostgresQueue::new(pool, config.name.clone(), visibility);

    use fc_queue::EmbeddedQueue;
    consumer
        .init_schema()
        .await
        .map_err(fc_router::RouterError::consumer(
            &config.name,
            "Postgres schema init failed",
        ))?;

    Ok(Arc::new(consumer))
}
