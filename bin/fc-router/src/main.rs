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
use fc_router::bootstrap::{
    sqs_client, RouterEnv, RouterRuntime, RouterRuntimeOptions, SchemeConsumerFactory, SqsPublisher,
};
use fc_router::StandbyAwareProcessor;
use std::sync::Arc;
use tokio::{net::TcpListener, signal};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present (for local development)
    let _ = dotenvy::dotenv();

    fc_common::logging::init_logging("fc-router");

    // Initialize Prometheus metrics recorder (must be before any metrics are recorded)
    let metrics_handle = fc_router::init_prometheus_recorder();

    info!("Starting FlowCatalyst Message Router (Production)");

    // 1. The environment, with Go's names and semantics (the deployed
    //    task definition is `inhance/iac/compute/fc-router.ts`; see
    //    docs/parity/router-env-vs-go.md). A half-configured platform
    //    credential is refused here, as Go refuses it.
    let env = RouterEnv::from_env()?;
    let sqs = sqs_client(env.dev_mode).await;

    // 2. Standby (Active/Passive HA). A Redis that cannot be reached at boot
    //    is fatal, as in Go (election.Start pings Redis and Run returns the
    //    error).
    let standby = if env.standby.enabled {
        info!(
            redis_url = %env.standby.redis_url,
            lock_key = %env.standby.lock_key,
            "Initializing standby mode (Active/Passive HA)"
        );
        let processor = StandbyAwareProcessor::new(env.standby.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Standby processor creation failed: {}", e))?;
        processor
            .start()
            .await
            .map_err(|e| anyhow::anyhow!("Standby processor failed to start: {}", e))?;
        Some(Arc::new(processor))
    } else {
        info!("Standby mode disabled - this instance will always be active");
        None
    };

    // 3. The router: queue manager, config watcher (retrying at boot until
    //    a configuration lands), settled reporter, lifecycle tasks.
    let runtime = RouterRuntime::start(
        &env,
        RouterRuntimeOptions {
            consumer_factory: Arc::new(SchemeConsumerFactory::new(sqs.clone())),
            standby,
            leadership: None,
        },
    )
    .await?;

    // 4. HTTP API. FC_API_PORT is Go's name; API_PORT (the deployed task
    //    definition's) and PORT are accepted too. Metrics are served on the
    //    API port at /metrics; FC_METRICS_PORT is accepted and ignored.
    let api_port: u16 =
        fc_common::config::env_first_parse(&["FC_API_PORT", "API_PORT", "PORT"], 8080u16);
    if let Some(metrics_port) = fc_common::config::env_first_opt(&["FC_METRICS_PORT"]) {
        info!(
            fc_metrics_port = %metrics_port,
            api_port,
            "FC_METRICS_PORT is a no-op here — metrics are served on the API port at /metrics"
        );
    }
    // The publisher resolves its target queue when it publishes (Go:
    // Manager.Publisher), since the queues arrive with the config.
    let publisher = Arc::new(SqsPublisher::new(
        sqs,
        runtime.queue_manager.clone(),
        env.dev_mode
            .then(fc_router::bootstrap::dev_router_config)
            .and_then(|c| c.queues.first().map(|q| q.uri.clone())),
    ));
    // FC_ROUTER_HTTP_PREFIX: unset keeps the root-only route tree; set, the
    // same tree is also nested under the prefix (Go's fc-server mounts it
    // at /router).
    let app = runtime
        .api_router(publisher, Some(metrics_handle), env.http_prefix.clone())
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
        if let Err(e) = axum::serve(listener, app).await {
            error!(error = %e, "HTTP API server failed");
        }
    });

    log_startup_summary(&runtime);
    info!("FlowCatalyst Router started. Press Ctrl+C to shutdown.");

    shutdown_signal().await;
    info!("Shutdown signal received...");

    runtime.shutdown().await;
    server_task.abort();

    info!("FlowCatalyst Router shutdown complete");
    Ok(())
}

/// Log startup summary
fn log_startup_summary(runtime: &RouterRuntime) {
    info!("=== FlowCatalyst Router Startup Summary ===");

    if runtime.is_leader() {
        info!("  Mode: ACTIVE (processing messages)");
    } else {
        info!("  Mode: STANDBY (waiting for leadership)");
    }

    if runtime.standby.is_some() {
        info!("  HA: Enabled (Active/Standby with Redis leader election)");
    } else {
        info!("  HA: Disabled (single instance mode)");
    }

    if runtime.config_sync.is_some() {
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
