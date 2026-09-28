//! FlowCatalyst Outbox Processor
//!
//! Reads messages from application database outbox tables and dispatches them
//! to the FlowCatalyst HTTP API with message group ordering, as Go's outbox
//! processor does (see `fc_outbox::enhanced_processor`).
//!
//! Supports multiple database backends: SQLite, PostgreSQL, MySQL, MongoDB. It is
//! the third of FlowCatalyst's three binaries (`fc-server`, `fc-dev`,
//! `fc-outbox-processor`): the application-side sidecar. `fc-server`'s
//! outbox role (`FC_OUTBOX_ENABLED`) runs the same processor through the
//! same start-up (`fc_outbox::setup`), without the MongoDB backend.
//!
//! ## Environment Variables
//!
//! Go's names are read first; the earlier names still work.
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_OUTBOX_BACKEND` / `FC_OUTBOX_DB_TYPE` | `postgres` | Database type: `sqlite`, `postgres`, `mysql`, `mongo` |
//! | `FC_OUTBOX_DB_URL` (mongo also `FC_OUTBOX_MONGO_URI`) | - | Database connection URL (required) |
//! | `FC_OUTBOX_MONGO_DB` | `flowcatalyst` | MongoDB database name |
//! | `FC_OUTBOX_EVENTS_TABLE` | `outbox_messages` | Table name for EVENT items |
//! | `FC_OUTBOX_DISPATCH_JOBS_TABLE` | `outbox_messages` | Table name for DISPATCH_JOB items |
//! | `FC_OUTBOX_AUDIT_LOGS_TABLE` | `outbox_messages` | Table name for AUDIT_LOG items |
//! | `FC_OUTBOX_PLATFORM_URL` / `FC_OUTBOX_API_URL` / `FC_API_BASE_URL` | `http://localhost:8080` | FlowCatalyst API URL |
//! | `FC_OUTBOX_PLATFORM_AUTH_TOKEN` / `FC_OUTBOX_TOKEN` / `FC_API_TOKEN` | - | API Bearer token |
//! | `FC_OUTBOX_POLL_INTERVAL_MS` | `1000` | Poll interval in milliseconds |
//! | `FC_OUTBOX_BATCH_SIZE` | `100` | Rows claimed per poll |
//! | `FC_API_BATCH_SIZE` | `100` | Most items per API call |
//! | `FC_OUTBOX_MAX_IN_FLIGHT` / `FC_MAX_IN_FLIGHT` | `1000` | No poll while this many items are in flight |
//! | `FC_OUTBOX_MAX_CONCURRENT_GROUPS` / `FC_MAX_CONCURRENT_GROUPS` | `10` | Max concurrent message groups |
//! | `FC_OUTBOX_MAX_RETRIES` | `3` | Attempts before a retryable failure is final |
//! | `FC_OUTBOX_BLOCK_ON_ERROR` | `true` | A failed item stops its message group |
//! | `FC_OUTBOX_ADMIN_PORT` | - | Serve the group admin API on 127.0.0.1 (Go's) |
//! | `FC_METRICS_PORT` | `9090` | Metrics/health port |
//! | `RUST_LOG` | `info` | Log level |
//!
//! ## Group admin API (`FC_OUTBOX_ADMIN_PORT`)
//!
//! | Route | Effect |
//! |---|---|
//! | `GET /outbox/groups` | Paused and Blocked groups |
//! | `GET /outbox/groups/blocked` | Blocked groups only |
//! | `POST /outbox/groups/{group}/pause` | Stop sending the group |
//! | `POST /outbox/groups/{group}/resume` | Resume a Paused group |
//! | `POST /outbox/groups/{group}/unblock` | Re-queue the blocking item and run the group again (404 if not Blocked) |
//! | `POST /outbox/groups/{group}/skip` | Leave the blocking item failed and advance (404 if not Blocked) |

use anyhow::Result;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::broadcast;
use tracing::info;

use fc_outbox::setup;
use fc_outbox::{EnhancedOutboxProcessor, EnhancedProcessorConfig};

use fc_common::config::env_or_parse;
use fc_common::diagnostics;
use fc_common::diagnostics::Exposition;
use fc_common::logging;
use tokio::net::TcpListener;
use tokio::time;

#[tokio::main]
async fn main() -> Result<()> {
    logging::init_logging("fc-outbox-processor");

    info!("Starting FlowCatalyst Outbox Processor");

    // Configuration
    let backend = setup::backend_from_env()?;
    let metrics_port: u16 = env_or_parse("FC_METRICS_PORT", 9090);
    let admin_port = setup::admin_port_from_env();

    let table_config = setup::table_config_from_env();
    info!("Table config: {:?}", table_config.unique_tables());

    // Setup shutdown signal
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // Initialize outbox repository
    let url = setup::database_url_from_env(backend)
        .ok_or_else(|| anyhow::anyhow!("FC_OUTBOX_DB_URL environment variable is required"))?;
    let outbox_repo = setup::connect(backend, &url, table_config).await?;

    let config = EnhancedProcessorConfig::from_env();
    info!(
        api_base_url = %config.http_config.api_base_url,
        poll_batch_size = config.poll_batch_size,
        max_in_flight = config.max_in_flight,
        max_concurrent_groups = config.max_concurrent_groups,
        block_on_error = config.block_on_error,
        "Sending to the platform with message group ordering"
    );

    let processor = Arc::new(EnhancedOutboxProcessor::new(config, outbox_repo)?);

    let mut shutdown_rx = shutdown_tx.subscribe();
    let processor_clone = Arc::clone(&processor);
    let processor_handle = tokio::spawn(async move {
        tokio::select! {
            _ = processor_clone.start() => {}
            _ = shutdown_rx.recv() => {
                info!("Outbox processor shutting down");
                // Stop, and hand what the groups still hold back to PENDING.
                processor_clone.shutdown().await;
            }
        }
    });

    // Start metrics server
    let metrics_addr = SocketAddr::from(([0, 0, 0, 0], metrics_port));
    info!(
        "Metrics server listening on http://{}/metrics",
        metrics_addr
    );

    let metrics_app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .with_state(Arc::clone(&processor));

    let metrics_listener = TcpListener::bind(metrics_addr).await?;
    let metrics_handle = {
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            axum::serve(metrics_listener, metrics_app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.recv().await;
                })
                .await
                .ok();
        })
    };

    // Group admin API, localhost only (Go `FC_OUTBOX_ADMIN_PORT`).
    let admin_handle = if admin_port > 0 {
        let mut shutdown_rx = shutdown_tx.subscribe();
        Some(
            setup::serve_admin(admin_port, Arc::clone(&processor), async move {
                let _ = shutdown_rx.recv().await;
            })
            .await?,
        )
    } else {
        None
    };

    info!("FlowCatalyst Outbox Processor started");
    info!("Press Ctrl+C to shutdown");

    // Wait for shutdown
    shutdown_signal().await;
    info!("Shutdown signal received...");

    let _ = shutdown_tx.send(());

    let _ = time::timeout(Duration::from_secs(30), async {
        let _ = processor_handle.await;
        let _ = metrics_handle.await;
        if let Some(handle) = admin_handle {
            let _ = handle.await;
        }
    })
    .await;

    info!("FlowCatalyst Outbox Processor shutdown complete");
    logging::shutdown();
    Ok(())
}

type Processor = Arc<EnhancedOutboxProcessor>;

async fn metrics_handler(State(p): State<Processor>) -> String {
    let m = p.metrics().await;
    let mut out = format!(
        "# HELP fc_outbox_up Outbox processor is up\n# TYPE fc_outbox_up gauge\nfc_outbox_up 1\n\
         # TYPE fc_outbox_items_polled_total counter\nfc_outbox_items_polled_total {}\n\
         # TYPE fc_outbox_items_succeeded_total counter\nfc_outbox_items_succeeded_total {}\n\
         # TYPE fc_outbox_items_failed_total counter\nfc_outbox_items_failed_total {}\n\
         # TYPE fc_outbox_items_released_total counter\nfc_outbox_items_released_total {}\n\
         # TYPE fc_outbox_items_recovered_total counter\nfc_outbox_items_recovered_total {}\n\
         # TYPE fc_outbox_in_flight gauge\nfc_outbox_in_flight {}\n\
         # TYPE fc_outbox_blocked_groups gauge\nfc_outbox_blocked_groups {}\n",
        m.items_polled,
        m.items_succeeded,
        m.items_failed,
        m.items_released,
        m.items_recovered,
        m.current_in_flight,
        m.blocked_groups,
    );
    // The tokio runtime and the process (CPU, RSS, fds, threads, panics).
    diagnostics::render_prometheus(&mut out, None, Exposition::Prometheus);
    out
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "UP",
        "version": fc_common::BUILD_VERSION
    }))
}

async fn ready_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "READY"
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
