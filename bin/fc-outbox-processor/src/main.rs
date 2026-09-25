//! FlowCatalyst Outbox Processor
//!
//! Reads messages from application database outbox tables and dispatches them
//! to the FlowCatalyst HTTP API with message group ordering.
//!
//! Supports multiple database backends: SQLite, PostgreSQL, MongoDB.
//!
//! ## Environment Variables
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_OUTBOX_DB_TYPE` | `postgres` | Database type: `sqlite`, `postgres`, `mongo` |
//! | `FC_OUTBOX_DB_URL` | - | Database connection URL (required) |
//! | `FC_OUTBOX_MONGO_DB` | `flowcatalyst` | MongoDB database name |
//! | `FC_OUTBOX_EVENTS_TABLE` | `outbox_messages` | Table name for EVENT items |
//! | `FC_OUTBOX_DISPATCH_JOBS_TABLE` | `outbox_messages` | Table name for DISPATCH_JOB items |
//! | `FC_OUTBOX_AUDIT_LOGS_TABLE` | `outbox_messages` | Table name for AUDIT_LOG items |
//! | `FC_OUTBOX_POLL_INTERVAL_MS` | `1000` | Poll interval in milliseconds |
//! | `FC_OUTBOX_BATCH_SIZE` | `500` | Max items fetched per poll |
//! | `FC_API_BASE_URL` | `http://localhost:8080` | FlowCatalyst API URL |
//! | `FC_API_TOKEN` | - | API Bearer token (optional) |
//! | `FC_API_BATCH_SIZE` | `100` | Items per API call |
//! | `FC_MAX_IN_FLIGHT` | `5000` | Max concurrent items |
//! | `FC_GLOBAL_BUFFER_SIZE` | `1000` | Buffer capacity |
//! | `FC_MAX_CONCURRENT_GROUPS` | `10` | Max concurrent message groups |
//! | `FC_METRICS_PORT` | `9090` | Metrics/health port |
//! | `RUST_LOG` | `info` | Log level |

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::broadcast;
use tracing::info;

use fc_outbox::repository::OutboxRepository;
use fc_outbox::repository::OutboxTableConfig;
use fc_outbox::{EnhancedOutboxProcessor, EnhancedProcessorConfig, OutboxBackend};

use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::SqlitePoolOptions;

use fc_common::config::{env_or, env_or_parse, env_required};

/// Build table config from environment variables
fn build_table_config() -> OutboxTableConfig {
    OutboxTableConfig {
        events_table: env_or("FC_OUTBOX_EVENTS_TABLE", "outbox_messages"),
        dispatch_jobs_table: env_or("FC_OUTBOX_DISPATCH_JOBS_TABLE", "outbox_messages"),
        audit_logs_table: env_or("FC_OUTBOX_AUDIT_LOGS_TABLE", "outbox_messages"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    fc_common::logging::init_logging("fc-outbox-processor");

    info!("Starting FlowCatalyst Outbox Processor");

    // Configuration
    let backend: OutboxBackend = env_or("FC_OUTBOX_DB_TYPE", "postgres").parse()?;
    let metrics_port: u16 = env_or_parse("FC_METRICS_PORT", 9090);

    let table_config = build_table_config();
    info!("Table config: {:?}", table_config.unique_tables());

    // Setup shutdown signal
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // Initialize outbox repository
    let outbox_repo = create_outbox_repository(backend, table_config).await?;
    info!("Outbox repository initialized ({})", backend);

    // Go's variable names and defaults, with the earlier names as fallbacks.
    let config = EnhancedProcessorConfig::from_env();
    info!(
        "Sending to {} with message group ordering",
        config.http_config.api_base_url
    );

    let processor = Arc::new(EnhancedOutboxProcessor::new(config, outbox_repo)?);

    let mut shutdown_rx = shutdown_tx.subscribe();
    let processor_clone = Arc::clone(&processor);
    let processor_handle = tokio::spawn(async move {
        tokio::select! {
            _ = processor_clone.start() => {}
            _ = shutdown_rx.recv() => {
                processor_clone.stop();
                info!("Enhanced outbox processor shutting down");
            }
        }
    });

    // Start metrics server
    let metrics_addr = SocketAddr::from(([0, 0, 0, 0], metrics_port));
    info!(
        "Metrics server listening on http://{}/metrics",
        metrics_addr
    );

    let metrics_app = axum::Router::new()
        .route("/metrics", axum::routing::get(metrics_handler))
        .route("/health", axum::routing::get(health_handler))
        .route("/ready", axum::routing::get(ready_handler));

    let metrics_listener = tokio::net::TcpListener::bind(metrics_addr).await?;
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

    info!("FlowCatalyst Outbox Processor started");
    info!("Press Ctrl+C to shutdown");

    // Wait for shutdown
    shutdown_signal().await;
    info!("Shutdown signal received...");

    let _ = shutdown_tx.send(());

    let _ = tokio::time::timeout(Duration::from_secs(30), async {
        let _ = processor_handle.await;
        let _ = metrics_handle.await;
    })
    .await;

    info!("FlowCatalyst Outbox Processor shutdown complete");
    Ok(())
}

async fn create_outbox_repository(
    backend: OutboxBackend,
    table_config: OutboxTableConfig,
) -> Result<Arc<dyn OutboxRepository>> {
    match backend {
        OutboxBackend::Sqlite => {
            let url = env_required("FC_OUTBOX_DB_URL")?;
            let pool = SqlitePoolOptions::new()
                .max_connections(5)
                .connect(&url)
                .await?;
            let repo = fc_outbox::sqlite::SqliteOutboxRepository::with_config(pool, table_config);
            repo.init_schema().await?;
            info!("Using SQLite outbox: {}", url);
            Ok(Arc::new(repo))
        }
        OutboxBackend::Postgres => {
            let url = env_required("FC_OUTBOX_DB_URL")?;
            let pool = PgPoolOptions::new()
                .max_connections(10)
                .connect(&url)
                .await?;
            let repo =
                fc_outbox::postgres::PostgresOutboxRepository::with_config(pool, table_config);
            repo.init_schema().await?;
            info!("Using PostgreSQL outbox");
            Ok(Arc::new(repo))
        }
        OutboxBackend::Mongo => {
            let url = env_required("FC_OUTBOX_DB_URL")?;
            let db_name = env_or("FC_OUTBOX_MONGO_DB", "flowcatalyst");
            let client = mongodb::Client::with_uri_str(&url).await?;
            let repo = fc_outbox::mongo::MongoOutboxRepository::with_config(
                client,
                &db_name,
                table_config,
            );
            repo.init_schema().await?;
            info!("Using MongoDB outbox: {}", db_name);
            Ok(Arc::new(repo))
        }
    }
}

async fn metrics_handler() -> String {
    "# HELP fc_outbox_up Outbox processor is up\n# TYPE fc_outbox_up gauge\nfc_outbox_up 1\n"
        .to_string()
}

async fn health_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "UP",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn ready_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
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
