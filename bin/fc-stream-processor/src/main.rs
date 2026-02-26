//! FlowCatalyst Stream Processor
//!
//! PostgreSQL CQRS projection engine. Polls projection feed tables and
//! projects rows into read-model tables via SQL CTEs.
//!
//! ## Environment Variables
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_DATABASE_URL` | `postgresql://localhost:5432/flowcatalyst` | PostgreSQL connection URL |
//! | `FC_STREAM_EVENTS_ENABLED` | `true` | Enable event projection |
//! | `FC_STREAM_EVENTS_BATCH_SIZE` | `100` | Event projection batch size |
//! | `FC_STREAM_DISPATCH_JOBS_ENABLED` | `true` | Enable dispatch job projection |
//! | `FC_STREAM_DISPATCH_JOBS_BATCH_SIZE` | `100` | Dispatch job projection batch size |
//! | `FC_METRICS_PORT` | `9090` | Metrics/health port |

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tracing::info;
use tokio::signal;

use fc_stream::{StreamProcessorConfig, StreamHealthService, start_stream_processor};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| v == "true" || v == "1")
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> Result<()> {
    fc_common::logging::init_logging("fc-stream-processor");

    info!("Starting FlowCatalyst Stream Processor");

    // Configuration
    let database_url = env_or("FC_DATABASE_URL", "postgresql://localhost:5432/flowcatalyst");
    let metrics_port: u16 = env_u32("FC_METRICS_PORT", 9090) as u16;

    let config = StreamProcessorConfig {
        events_enabled: env_bool("FC_STREAM_EVENTS_ENABLED", true),
        events_batch_size: env_u32("FC_STREAM_EVENTS_BATCH_SIZE", 100),
        dispatch_jobs_enabled: env_bool("FC_STREAM_DISPATCH_JOBS_ENABLED", true),
        dispatch_jobs_batch_size: env_u32("FC_STREAM_DISPATCH_JOBS_BATCH_SIZE", 100),
    };

    // Connect to PostgreSQL
    info!("Connecting to PostgreSQL...");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .idle_timeout(Duration::from_secs(20))
        .connect(&database_url)
        .await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;
    info!("PostgreSQL connected");

    // Start projection loops
    let (handle, health_service) = start_stream_processor(pool.clone(), config);
    let health_service = Arc::new(health_service);

    // Start metrics / health server
    let metrics_addr = SocketAddr::from(([0, 0, 0, 0], metrics_port));
    info!("Metrics server listening on http://{}", metrics_addr);

    let health_svc = health_service.clone();
    let metrics_app = axum::Router::new()
        .route("/metrics", axum::routing::get(metrics_handler))
        .route(
            "/health",
            axum::routing::get({
                let svc = health_svc.clone();
                move || health_handler(svc.clone())
            }),
        )
        .route(
            "/ready",
            axum::routing::get({
                let svc = health_svc.clone();
                move || ready_handler(svc.clone())
            }),
        );

    let metrics_listener = tokio::net::TcpListener::bind(metrics_addr).await?;
    let metrics_handle = tokio::spawn(async move {
        axum::serve(metrics_listener, metrics_app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .ok();
    });

    info!("FlowCatalyst Stream Processor started");

    // Wait for shutdown
    shutdown_signal().await;
    info!("Shutdown signal received...");

    // Stop projections gracefully
    handle.stop().await;

    let _ = tokio::time::timeout(Duration::from_secs(5), metrics_handle).await;

    pool.close().await;

    info!("FlowCatalyst Stream Processor shutdown complete");
    Ok(())
}

async fn metrics_handler() -> String {
    "# HELP fc_stream_up Stream processor is up\n# TYPE fc_stream_up gauge\nfc_stream_up 1\n"
        .to_string()
}

async fn health_handler(svc: Arc<StreamHealthService>) -> axum::Json<serde_json::Value> {
    let health = svc.get_aggregated_health();
    let status = if health.is_live() && health.is_ready() {
        "UP"
    } else if health.is_live() {
        "DEGRADED"
    } else {
        "DOWN"
    };
    axum::Json(serde_json::json!({
        "status": status,
        "live": health.is_live(),
        "ready": health.is_ready(),
    }))
}

async fn ready_handler(svc: Arc<StreamHealthService>) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    let health = svc.get_aggregated_health();
    if health.is_ready() {
        (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({ "status": "READY" })),
        )
    } else {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({ "status": "NOT_READY" })),
        )
    }
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
