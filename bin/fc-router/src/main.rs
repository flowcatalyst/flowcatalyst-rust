//! FlowCatalyst Production Router
//!
//! Consumes messages from SQS and routes them through the processing pipeline.
//! Provides REST API for monitoring, health, and message publishing.
//!
//! ## Production Features
//!
//! - **Dynamic Configuration Sync**: Periodically fetches configuration from a central
//!   service and hot-reloads without restart.
//!
//! - **Active/Standby HA**: Uses Redis-based leader election for high availability.
//!   Only the leader processes messages. Enable with `FLOWCATALYST_STANDBY_ENABLED=true`.
//!
//! ## Development Mode
//!
//! Set `FLOWCATALYST_DEV_MODE=true` to enable development mode with:
//! - Built-in LocalStack SQS queue configuration
//! - Test endpoints for simulating various response scenarios
//! - Message seeding endpoints

use anyhow::Result;
use fc_common::{PoolConfig, QueueConfig, RouterConfig, WarningSeverity};
use fc_queue::sqs::SqsQueueConsumer;
use fc_queue::QueueScheme;
use fc_router::{
    api::{create_router_with_options, RouterDeps, RouterOptions},
    create_notification_service_with_scheduler, ConfigSyncConfig, ConfigSyncService,
    ConsumerFactory, HealthService, HealthServiceConfig, HttpMediatorConfig, LifecycleConfig,
    LifecycleManager, NotificationConfig, QueueManager, StandbyAwareProcessor, StandbyRouterConfig,
    WarningService, WarningServiceConfig,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::{net::TcpListener, signal};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present (for local development)
    let _ = dotenvy::dotenv();

    fc_common::logging::init_logging("fc-router");

    // Initialize Prometheus metrics recorder (must be before any metrics are recorded)
    let metrics_handle = fc_router::init_prometheus_recorder();

    info!("Starting FlowCatalyst Message Router (Production)");

    // 1. Setup AWS Config
    // In dev mode, configure to use LocalStack endpoint
    let dev_mode = std::env::var("FLOWCATALYST_DEV_MODE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let sqs_client = if dev_mode {
        let endpoint_url = std::env::var("LOCALSTACK_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:4566".to_string());
        info!(endpoint = %endpoint_url, "Configuring SQS client for LocalStack");

        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .endpoint_url(&endpoint_url)
            .load()
            .await;
        aws_sdk_sqs::Client::new(&config)
    } else {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        aws_sdk_sqs::Client::new(&config)
    };

    // 2. Initialize Notification Service (Teams webhooks), then the Warning
    //    Service that feeds it, and the Health Service.
    let notification_config = load_notification_config();
    let notification_scheduler = create_notification_service_with_scheduler(&notification_config);
    let warning_service = Arc::new(match notification_scheduler {
        Some(ref ns) => {
            info!(
                batch_interval = notification_config.batch_interval_seconds,
                "Notification service enabled (Teams webhook with batching)"
            );
            WarningService::with_notification(WarningServiceConfig::default(), ns.service.clone())
        }
        None => {
            info!("Notification service disabled - no channels configured");
            WarningService::new(WarningServiceConfig::default())
        }
    });
    let health_service = Arc::new(HealthService::new(
        HealthServiceConfig::default(),
        warning_service.clone(),
    ));

    // 3. R-13/R-16: FC_ROUTER_STRICT_ROUTING (default false). Operational
    // decision, not a code change — flip on once every producer is
    // confirmed to send poolCode/dispatchMode/messageGroupId on every
    // message.
    let strict_routing = std::env::var("FC_ROUTER_STRICT_ROUTING")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    info!(strict_routing, "Strict routing gate set");

    // 4. Create QueueManager. Mediator *config* is passed (not a singleton);
    //    each pool gets its own HttpMediator + connection pool.
    let queue_manager = Arc::new(
        QueueManager::builder(HttpMediatorConfig::production())
            .warning_service(warning_service.clone())
            .health_service(health_service.clone())
            .consumer_factory(Arc::new(SchemeConsumerFactory {
                sqs_client: sqs_client.clone(),
            }))
            .strict_routing(strict_routing)
            .build(),
    );

    // 5. Initialize Standby Processor (Active/Passive HA)
    let standby_config = load_standby_config();
    let standby = if standby_config.enabled {
        info!(
            redis_url = %standby_config.redis_url,
            lock_key = %standby_config.lock_key,
            "Initializing standby mode (Active/Passive HA)"
        );
        match StandbyAwareProcessor::new(standby_config).await {
            Ok(processor) => {
                if let Err(e) = processor.start().await {
                    error!(error = %e, "Failed to start standby processor");
                    return Err(anyhow::anyhow!("Standby processor failed to start: {}", e));
                }
                Some(Arc::new(processor))
            }
            Err(e) => {
                error!(error = %e, "Failed to create standby processor");
                return Err(anyhow::anyhow!("Standby processor creation failed: {}", e));
            }
        }
    } else {
        info!("Standby mode disabled - this instance will always be active");
        None
    };

    // 6. Wait for leadership if in standby mode
    if let Some(ref standby_proc) = standby {
        if !standby_proc.is_leader() {
            info!("Waiting to become leader before starting message processing...");
            standby_proc.wait_for_leadership().await;
            info!("Acquired leadership - starting message processing");
        }
    }

    // 7. Initialize Configuration
    // Dev mode uses built-in LocalStack config, production requires config URL
    let dev_mode = std::env::var("FLOWCATALYST_DEV_MODE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let (router_config, config_sync) = if dev_mode {
        info!("Development mode enabled - using built-in LocalStack configuration");
        let config = create_dev_config();
        info!(
            queues = config.queues.len(),
            pools = config.processing_pools.len(),
            "Loaded dev configuration"
        );
        (config, None)
    } else {
        // Production mode - fetch config from URL(s)
        // Supports comma-separated URLs for multi-platform environments
        let config_url = std::env::var("FLOWCATALYST_CONFIG_URL").map_err(|_| {
            anyhow::anyhow!(
                "FLOWCATALYST_CONFIG_URL is required (or set FLOWCATALYST_DEV_MODE=true)"
            )
        })?;

        if config_url.is_empty() {
            return Err(anyhow::anyhow!("FLOWCATALYST_CONFIG_URL cannot be empty"));
        }

        let config_sync_config = load_config_sync_config(&config_url);

        info!(
            urls = ?config_sync_config.config_urls,
            interval = ?config_sync_config.sync_interval,
            "Initializing configuration sync"
        );
        let sync_service = Arc::new(ConfigSyncService::new(
            config_sync_config,
            queue_manager.clone(),
            warning_service.clone(),
        ));

        // Perform initial sync - router cannot start without configuration
        let config = match sync_service.initial_sync().await {
            Ok(config) => config,
            Err(e) => {
                error!(error = %e, "Initial configuration sync failed - cannot start router");
                return Err(anyhow::anyhow!("Initial config sync failed: {}", e));
            }
        };

        (config, Some(sync_service))
    };

    // 8. Create consumers from config, dispatching on each queue URI's
    // scheme (item 1 — the blocker this fixes: every queue used to be
    // handed to SqsConsumerFactory regardless of its scheme).
    //
    // Item 1 (router bench rig, 2026-09-07): in production mode
    // (`config_sync.is_some()`, i.e. `FLOWCATALYST_CONFIG_URL` set),
    // `sync_service.initial_sync()` above already created AND spawned a
    // poll task for every one of these queues via `QueueManager`'s own
    // `reload_config` -> `sync_queue_consumers` (the manager was built
    // with a `consumer_factory` specifically so that path could do this —
    // see the `.consumer_factory(...)` call above). This loop used to
    // unconditionally create a SECOND, fully independent consumer per
    // queue here too (its own `PostgresQueue`/etc. instance, own DB
    // connection pool) and `add_consumer` it — `add_consumer` only
    // overwrites the manager's `consumers` map entry, it neither stops the
    // poll task `sync_queue_consumers` already spawned for that id nor
    // spawns one for this new instance (that only happened later, when
    // `QueueManager::start()` — step 11, below — spawned one for whatever
    // was currently in the map). Net effect: two independent pollers
    // racing each other against the same `queue_name`, reproduced as
    // spurious "ACK failed - message not found" warnings and a handful of
    // genuine duplicate deliveries within milliseconds of a message's
    // first claim (`QueueManager`'s new `polling_consumer_ids` guard is
    // the defence-in-depth backstop for this same class of bug; this is
    // the direct fix — never create the redundant instance in the first
    // place).
    //
    // Dev mode (`config_sync` is `None`) has no config-sync service at
    // all, so `initial_sync` never runs and `self.consumers` starts empty
    // — this loop is still the only thing that ever creates a consumer
    // there, so it still needs to run in full for that branch.
    //
    // In production the consumers already exist (and are already polling)
    // courtesy of initial_sync() above.
    if config_sync.is_none() {
        let scheme_factory = SchemeConsumerFactory {
            sqs_client: sqs_client.clone(),
        };
        for queue_config in &router_config.queues {
            let consumer = scheme_factory.create_consumer(queue_config).await?;
            queue_manager.add_consumer(consumer).await;
        }
    }
    // The first queue's URL is the publisher's target (still SQS-only — see
    // SqsPublisher's doc comment).
    let first_queue_url = router_config.queues.first().map(|q| q.uri.clone());

    if router_config.queues.is_empty() {
        error!("No queues configured - cannot start router");
        return Err(anyhow::anyhow!(
            "No queues configured in config sync response"
        ));
    }

    // 9. Start lifecycle manager with all features
    let mut lifecycle_config = LifecycleConfig::default();
    // R-59: FC_ROUTER_SYNTH_POOL_IDLE_SECS — idle TTL for synthesised
    // per-client fallback pools ({identifier}-DEFAULT-POOL). Mirrors Go's
    // FC_ROUTER_SYNTH_POOL_IDLE_SECS / ServerConfig.SynthPoolIdleAge: absent,
    // unparseable, or explicitly "0" all keep LifecycleConfig::default()'s
    // 1h TTL (Go's envInt can't tell "unset" from "explicit 0" either, and
    // both fall through to its own hour default). A negative value disables
    // the sweep — Duration has no negative representation, so that maps to
    // Duration::ZERO, which QueueManager::evict_idle_synth_pools treats as
    // a no-op, the same as Go's `ttl <= 0`.
    if let Ok(raw) = std::env::var("FC_ROUTER_SYNTH_POOL_IDLE_SECS") {
        if let Ok(secs) = raw.parse::<i64>() {
            lifecycle_config.synth_pool_idle_ttl = match secs {
                s if s < 0 => Duration::ZERO,
                0 => lifecycle_config.synth_pool_idle_ttl,
                s => Duration::from_secs(s as u64),
            };
        }
    }
    let cb_max_idle = lifecycle_config.circuit_breaker_max_idle;
    // POST /config/reload re-fetches from the same config source (Go:
    // Server.Reload). None in dev mode: there is no source to re-fetch.
    let config_reloader: Option<Arc<dyn fc_router::api::ConfigReloader>> = config_sync
        .clone()
        .map(|s| s as Arc<dyn fc_router::api::ConfigReloader>);
    let mut lifecycle = LifecycleManager::start_with_features(
        queue_manager.clone(),
        warning_service.clone(),
        health_service.clone(),
        lifecycle_config,
        config_sync,
        standby.clone(),
    );
    // Wire periodic idle-eviction against the manager's shared breaker registry.
    // Without this the eviction task never runs and shared breakers (PR1) grow
    // unbounded; the registry here is the same one the pools record into.
    lifecycle.spawn_circuit_breaker_eviction(
        queue_manager.circuit_breaker_registry().clone(),
        cb_max_idle,
    );

    // 10. Setup HTTP API server
    // FC_API_PORT is the canonical Go-dialect name (internal/server/envcfg.go
    // EnvCfg.APIPort); API_PORT is this binary's historical name and stays a
    // fallback so nothing already deployed against it breaks.
    let api_port: u16 = fc_common::config::env_first_parse(&["FC_API_PORT", "API_PORT"], 8080u16);

    // FC_METRICS_PORT: Go's unified fc-server can bind metrics on a separate
    // listener, but this binary always serves Prometheus metrics on the same
    // API port under /metrics (and /q/metrics) — there is no second listener
    // to bind. Accept-and-log the var as a no-op rather than failing a
    // drop-in deployment that sets it out of habit.
    if let Some(metrics_port) = fc_common::config::env_first_opt(&["FC_METRICS_PORT"]) {
        info!(
            fc_metrics_port = %metrics_port,
            api_port,
            "FC_METRICS_PORT is a no-op here — metrics are served on the API port at /metrics"
        );
    }

    // FC_ROUTER_HTTP_PREFIX: unset (default) keeps today's root-only route
    // tree; when set, the same route tree is additionally nested under the
    // prefix (see create_router_with_options doc comment) so a Go-dialect
    // deployment's /router/... probes and operator URLs answer too.
    let router_http_prefix = fc_common::config::env_first_opt(&["FC_ROUTER_HTTP_PREFIX"]);
    if let Some(ref prefix) = router_http_prefix {
        info!(prefix = %prefix, "FC_ROUTER_HTTP_PREFIX set — route tree also nested under prefix");
    }

    // Create a simple publisher that publishes to the first queue
    let publisher_queue_url = first_queue_url.expect("At least one queue must be configured");
    let publisher = Arc::new(SqsPublisher::new(sqs_client, publisher_queue_url));

    // Use the QueueManager's shared circuit breaker registry so the monitoring
    // API reads the *same* breakers the pools record into (and operator
    // reset/reset_all act on live state). Previously this was a separate
    // CircuitBreakerRegistry::default() that no pool ever wrote to.
    let circuit_breaker_registry = queue_manager.circuit_breaker_registry().clone();

    // Initialize authentication from environment variables
    let auth_config = fc_router::api::AuthConfig::from_env();
    let auth_state = if auth_config.mode != fc_router::api::AuthMode::None {
        info!(mode = ?auth_config.mode, "Authentication configured");
        Some(fc_router::api::create_auth_state(auth_config))
    } else {
        info!("Authentication disabled (AUTH_MODE=NONE or not set)");
        None
    };

    let app = create_router_with_options(
        RouterDeps {
            publisher,
            queue_manager: queue_manager.clone(),
            warning_service: warning_service.clone(),
            health_service: health_service.clone(),
            circuit_breaker_registry,
        },
        RouterOptions {
            standby_enabled: standby.is_some(),
            instance_id: standby
                .as_ref()
                .map(|s| s.instance_id().to_string())
                .unwrap_or_else(|| "default".to_string()),
            metrics_handle: Some(metrics_handle),
            auth_state,
            router_http_prefix,
            config_reloader,
            ..RouterOptions::default()
        },
    )
    .layer(TraceLayer::new_for_http())
    .layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    );

    let addr = format!("0.0.0.0:{}", api_port);
    info!(port = api_port, "Starting HTTP API server");

    let listener = TcpListener::bind(&addr).await?;
    let server_task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // 11. Start QueueManager in background (respecting standby status)
    // Create a shutdown channel for the manager loop
    let (manager_shutdown_tx, mut manager_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let manager_handle = {
        let manager = queue_manager.clone();
        let standby_for_loop = standby.clone();

        tokio::spawn(async move {
            // If we have standby, wait for leadership before processing
            if let Some(ref standby_proc) = standby_for_loop {
                loop {
                    tokio::select! {
                        _ = &mut manager_shutdown_rx => {
                            info!("Manager loop received shutdown signal");
                            break;
                        }
                        _ = async {
                            if standby_proc.should_process() {
                                info!("Leader status confirmed - starting message consumption");
                                if let Err(e) = manager.clone().start().await {
                                    error!("QueueManager error: {}", e);
                                }
                                // If start() returns, check if we lost leadership
                                if !standby_proc.should_process() {
                                    warn!("Lost leadership during processing - pausing");
                                    standby_proc.wait_for_leadership().await;
                                    info!("Re-acquired leadership - resuming");
                                }
                            } else {
                                // Not leader, wait
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                        } => {}
                    }
                }
            } else {
                // No standby mode - just run (start() already listens to shutdown_tx)
                if let Err(e) = manager.clone().start().await {
                    error!("QueueManager error: {}", e);
                }
            }
        })
    };

    // Log startup summary
    log_startup_summary(&lifecycle);

    info!("FlowCatalyst Router started. Press Ctrl+C to shutdown.");

    // Wait for shutdown signal
    shutdown_signal().await;
    info!("Shutdown signal received...");

    // Graceful shutdown
    // Signal the manager loop to exit
    let _ = manager_shutdown_tx.send(());

    // Go's order: stop polling, drain, tear the manager down; only then
    // stop the lifecycle tasks and release leadership.
    // FC_DRAIN_TIMEOUT_SECONDS makes the pool-drain budget operator/env
    // tunable instead of the crate's hardcoded 60s default
    // (QueueManager::DEFAULT_DRAIN_TIMEOUT).
    let drain_timeout_secs: u64 =
        fc_common::config::env_first_parse(&["FC_DRAIN_TIMEOUT_SECONDS"], 60u64);
    queue_manager
        .shutdown_with_timeout(Duration::from_secs(drain_timeout_secs))
        .await;
    lifecycle.shutdown().await;

    server_task.abort();

    // Wait for manager handle with timeout, then abort if still running
    match tokio::time::timeout(std::time::Duration::from_secs(30), manager_handle).await {
        Ok(_) => info!("Manager task completed gracefully"),
        Err(_) => {
            warn!("Manager task did not complete within 30s timeout");
            // The task will be cancelled when the runtime shuts down
        }
    }

    info!("FlowCatalyst Router shutdown complete");
    Ok(())
}

/// Load standby configuration from environment variables
fn load_standby_config() -> StandbyRouterConfig {
    // Safety-critical: standby/leader-election is how HA is enforced. A Go
    // task definition sets the canonical FC_STANDBY_* names (see
    // internal/server/envcfg.go EnvCfg.StandbyEnabled/StandbyRedisURL); this
    // binary previously read ONLY FLOWCATALYST_STANDBY_* / FLOWCATALYST_REDIS_URL,
    // so a Go-dialect env would silently resolve enabled=false and this
    // instance would run WITHOUT leader election — active/active against the
    // same queues. FC_STANDBY_* is read first, FLOWCATALYST_STANDBY_* stays
    // as this binary's own historical name, and Go's legacy (pre-FC_*)
    // STANDBY_ENABLED / REDIS_URL are honoured too so every previously-working
    // name keeps working.
    let enabled = fc_common::config::env_first_bool(
        &[
            "FC_STANDBY_ENABLED",
            "FLOWCATALYST_STANDBY_ENABLED",
            "STANDBY_ENABLED",
        ],
        false,
    );

    let redis_url = fc_common::config::env_first(
        &[
            "FC_STANDBY_REDIS_URL",
            "FLOWCATALYST_STANDBY_REDIS_URL",
            "FLOWCATALYST_REDIS_URL",
            "REDIS_URL",
        ],
        "redis://127.0.0.1:6379",
    );

    let lock_key = fc_common::config::env_first(
        &["FC_STANDBY_LOCK_KEY", "FLOWCATALYST_STANDBY_LOCK_KEY"],
        "fc:router:leader",
    );

    let lock_ttl = std::env::var("FLOWCATALYST_STANDBY_LOCK_TTL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    let heartbeat_interval = std::env::var("FLOWCATALYST_STANDBY_HEARTBEAT_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    let instance_id = std::env::var("FLOWCATALYST_INSTANCE_ID")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();

    StandbyRouterConfig {
        enabled,
        redis_url,
        lock_key,
        lock_ttl_seconds: lock_ttl,
        heartbeat_interval_seconds: heartbeat_interval,
        instance_id,
    }
}

/// Load notification configuration from environment variables
fn load_notification_config() -> NotificationConfig {
    // FC_NOTIFY_WEBHOOK_URL is the canonical Go-dialect name
    // (internal/server/envcfg.go EnvCfg.RouterNotifyWebhookURL); this
    // binary's own historical NOTIFICATION_TEAMS_WEBHOOK_URL stays as a
    // fallback. Go has no separate "enabled" flag — a non-empty webhook URL
    // alone means notify — so teams_enabled is derived the same way here;
    // the legacy NOTIFICATION_TEAMS_ENABLED flag is still honoured too (it
    // can only ever widen — not narrow — whether a configured URL fires).
    let teams_webhook_url = fc_common::config::env_first_opt(&[
        "FC_NOTIFY_WEBHOOK_URL",
        "NOTIFICATION_TEAMS_WEBHOOK_URL",
    ]);
    let teams_enabled = teams_webhook_url.as_deref().is_some_and(|u| !u.is_empty())
        || fc_common::config::env_first_bool(&["NOTIFICATION_TEAMS_ENABLED"], false);

    // X-04: the ruled name is FC_NOTIFY_MIN_SEVERITY. Read it first, falling
    // back to the legacy NOTIFICATION_MIN_SEVERITY (with a deprecation note)
    // so existing deployments don't silently lose their override.
    let min_severity_raw = std::env::var("FC_NOTIFY_MIN_SEVERITY").or_else(|_| {
        let legacy = std::env::var("NOTIFICATION_MIN_SEVERITY");
        if legacy.is_ok() {
            warn!("NOTIFICATION_MIN_SEVERITY is deprecated — set FC_NOTIFY_MIN_SEVERITY instead");
        }
        legacy
    });

    let min_severity = min_severity_raw
        .ok()
        .and_then(|s| fc_router::warning::parse_severity(&s))
        .unwrap_or(WarningSeverity::Warn);

    let batch_interval_seconds = std::env::var("NOTIFICATION_BATCH_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300); // 5 minutes default

    NotificationConfig {
        teams_enabled,
        teams_webhook_url,
        min_severity,
        batch_interval_seconds,
        #[cfg(feature = "email")]
        email_config: None,
    }
}

/// Load config sync configuration from environment variables.
/// `config_url` supports comma-separated URLs for multi-platform environments.
fn load_config_sync_config(config_url: &str) -> ConfigSyncConfig {
    let interval_secs = std::env::var("FLOWCATALYST_CONFIG_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300); // 5 minutes default

    let mut config = ConfigSyncConfig::new(config_url.to_string());
    config.sync_interval = Duration::from_secs(interval_secs);
    config
}

/// Create development configuration with LocalStack SQS queues
fn create_dev_config() -> RouterConfig {
    // LocalStack uses this URL format for SQS queues
    // Can be overridden via LOCALSTACK_SQS_HOST env var
    let sqs_host = std::env::var("LOCALSTACK_SQS_HOST")
        .unwrap_or_else(|_| "http://sqs.eu-west-1.localhost.localstack.cloud:4566".to_string());

    RouterConfig {
        processing_pools: vec![
            PoolConfig {
                code: "DEFAULT".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            },
            PoolConfig {
                code: "HIGH".to_string(),
                concurrency: 20,
                rate_limit_per_minute: None,
            },
            PoolConfig {
                code: "LOW".to_string(),
                concurrency: 5,
                rate_limit_per_minute: Some(60),
            },
        ],
        queues: vec![
            QueueConfig {
                name: "fc-high-priority.fifo".to_string(),
                uri: format!("{}/000000000000/fc-high-priority.fifo", sqs_host),
                connections: 2,
                visibility_timeout: 120,
            },
            QueueConfig {
                name: "fc-default.fifo".to_string(),
                uri: format!("{}/000000000000/fc-default.fifo", sqs_host),
                connections: 2,
                visibility_timeout: 120,
            },
            QueueConfig {
                name: "fc-low-priority.fifo".to_string(),
                uri: format!("{}/000000000000/fc-low-priority.fifo", sqs_host),
                connections: 1,
                visibility_timeout: 120,
            },
        ],
    }
}

/// Log startup summary
fn log_startup_summary(lifecycle: &LifecycleManager) {
    info!("=== FlowCatalyst Router Startup Summary ===");

    if lifecycle.is_leader() {
        info!("  Mode: ACTIVE (processing messages)");
    } else {
        info!("  Mode: STANDBY (waiting for leadership)");
    }

    if lifecycle.standby().is_some() {
        info!("  HA: Enabled (Active/Standby with Redis leader election)");
    } else {
        info!("  HA: Disabled (single instance mode)");
    }

    if lifecycle.config_sync().is_some() {
        info!("  Config Sync: Enabled (dynamic configuration updates)");
    } else {
        info!("  Config Sync: Disabled (static configuration)");
    }

    info!("==========================================");
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

// Consumer factory that dispatches on the queue URI's scheme (item 1) —
// used both for the initial consumer set (step 8, above) and for every
// consumer config-sync/reconfigure hot-adds afterwards. Mirrors Go/Java's
// scheme resolution (`docs/spec/router.md` §7.1): `nats://`, `postgres://`
// (the Postgres backend connects from the URI itself — see
// `build_postgres_consumer`), and `http(s)://sqs.<region>.amazonaws.com/…`
// all resolve to their real backend instead of every queue silently being
// handed to SQS regardless of its actual scheme.
struct SchemeConsumerFactory {
    sqs_client: aws_sdk_sqs::Client,
}

#[async_trait]
impl ConsumerFactory for SchemeConsumerFactory {
    async fn create_consumer(
        &self,
        config: &QueueConfig,
    ) -> std::result::Result<Arc<dyn fc_queue::QueueConsumer>, fc_router::RouterError> {
        let scheme = fc_queue::resolve_scheme(&config.uri).map_err(
            fc_router::RouterError::consumer(&config.name, "resolve queue scheme"),
        )?;

        match scheme {
            QueueScheme::Sqs => {
                info!(
                    queue_name = %config.name,
                    queue_uri = %config.uri,
                    visibility_timeout = config.visibility_timeout,
                    "Creating SQS consumer from config"
                );
                let consumer = SqsQueueConsumer::from_queue_url(
                    self.sqs_client.clone(),
                    config.uri.clone(),
                    config.visibility_timeout as i32,
                )
                .await;
                Ok(Arc::new(consumer))
            }
            QueueScheme::Nats => build_nats_consumer(config).await,
            QueueScheme::Postgres => build_postgres_consumer(config).await,
        }
    }
}

/// Build a [`fc_queue::nats::NatsQueueConsumer`] from a `nats://` queue URI
/// (`docs/spec/router.md` §7.4) — parses stream/consumer/subject/etc from
/// the URI's query string and provisions the stream + durable pull
/// consumer via `NatsQueueConsumer::new`.
async fn build_nats_consumer(
    config: &QueueConfig,
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

/// Build a [`fc_queue::postgres::PostgresQueue`] from a `postgres://` queue
/// URI — the URI carries its own connection info (`docs/spec/router.md`
/// §7.3: "the Java backend now connects from the queue URI like Go's
/// `pgxpool.New(ctx, cfg.URI)`"), so a dedicated pool is opened per queue
/// rather than sharing the platform's own database pool. `queue_name`
/// (the config's operator-chosen label) is the consumer's `identifier()`,
/// matching the other backends' "config name, not a broker-native id"
/// identity for Postgres.
async fn build_postgres_consumer(
    config: &QueueConfig,
) -> std::result::Result<Arc<dyn fc_queue::QueueConsumer>, fc_router::RouterError> {
    // Item 1 (router bench rig, 2026-09-07): a hardcoded max_connections(4)
    // starved this queue's acks under load (see
    // `fc_queue::postgres::default_max_connections`'s doc comment for the
    // full mechanism — undersized pool -> acks queue up behind claim
    // traffic -> visibility timeout lapses before the ack lands -> the row
    // gets reclaimed out from under the still-in-flight delivery -> the
    // eventual ack fails "message not found", and the retry path only
    // resolves on the row's next natural reclaim). Sizing now mirrors Go's
    // own `pgxpool.New` default of `max(4, NumCPU)`.
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

// Simple SQS publisher implementation
use async_trait::async_trait;
use fc_common::Message;
use fc_queue::{QueueError, QueuePublisher};

struct SqsPublisher {
    client: aws_sdk_sqs::Client,
    queue_url: String,
}

impl SqsPublisher {
    fn new(client: aws_sdk_sqs::Client, queue_url: String) -> Self {
        Self { client, queue_url }
    }
}

#[async_trait]
impl QueuePublisher for SqsPublisher {
    fn identifier(&self) -> &str {
        &self.queue_url
    }

    async fn publish(&self, message: Message) -> fc_queue::Result<String> {
        let message_id = message.id.clone();
        let body = serde_json::to_string(&message)?;

        let mut request = self
            .client
            .send_message()
            .queue_url(&self.queue_url)
            .message_body(body);

        // FIFO queues require message_group_id and message_deduplication_id
        if self.queue_url.ends_with(".fifo") {
            let group_id = message
                .message_group_id
                .clone()
                .unwrap_or_else(|| "default".to_string());
            request = request
                .message_group_id(group_id)
                .message_deduplication_id(&message_id);
        }

        request.send().await.map_err(QueueError::sqs)?;

        Ok(message_id)
    }

    async fn publish_batch(&self, messages: Vec<Message>) -> fc_queue::Result<Vec<String>> {
        let mut ids = Vec::with_capacity(messages.len());
        for message in messages {
            let id = self.publish(message).await?;
            ids.push(id);
        }
        Ok(ids)
    }
}
