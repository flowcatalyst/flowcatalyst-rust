//! FlowCatalyst Outbox Processor
//!
//! Reads messages from application database outbox tables and dispatches them
//! to the FlowCatalyst HTTP API with message group ordering, as Go's outbox
//! processor does (see `fc_outbox::enhanced_processor`).
//!
//! Supports multiple database backends: SQLite, PostgreSQL, MongoDB.
//!
//! ## Environment Variables
//!
//! Go's names are read first; the earlier names still work.
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_OUTBOX_BACKEND` / `FC_OUTBOX_DB_TYPE` | `postgres` | Database type: `sqlite`, `postgres`, `mongo` |
//! | `FC_OUTBOX_DB_URL` | - | Database connection URL (required) |
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
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
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

use fc_common::config::{env_first, env_or, env_or_parse, env_required};

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
    let backend: OutboxBackend =
        env_first(&["FC_OUTBOX_BACKEND", "FC_OUTBOX_DB_TYPE"], "postgres").parse()?;
    let metrics_port: u16 = env_or_parse("FC_METRICS_PORT", 9090);
    let admin_port: u16 = env_or_parse("FC_OUTBOX_ADMIN_PORT", 0);

    let table_config = build_table_config();
    info!("Table config: {:?}", table_config.unique_tables());

    // Setup shutdown signal
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // Initialize outbox repository
    let outbox_repo = create_outbox_repository(backend, table_config).await?;
    info!("Outbox repository initialized ({})", backend);

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
                processor_clone.stop();
                info!("Outbox processor shutting down");
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

    // Group admin API, localhost only (Go `FC_OUTBOX_ADMIN_PORT`).
    let admin_handle = if admin_port > 0 {
        let addr = SocketAddr::from(([127, 0, 0, 1], admin_port));
        let listener = tokio::net::TcpListener::bind(addr).await?;
        info!("Outbox admin API listening on http://{}", addr);
        let app = admin_router(Arc::clone(&processor));
        let mut shutdown_rx = shutdown_tx.subscribe();
        Some(tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.recv().await;
                })
                .await
                .ok();
        }))
    } else {
        None
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
        if let Some(handle) = admin_handle {
            let _ = handle.await;
        }
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

type Processor = Arc<EnhancedOutboxProcessor>;

/// Go's `AdminHandler` routes and answers.
fn admin_router(processor: Processor) -> Router {
    Router::new()
        .route("/outbox/groups", get(admin_groups))
        .route("/outbox/groups/blocked", get(admin_blocked))
        .route("/outbox/groups/{group}/pause", post(admin_pause))
        .route("/outbox/groups/{group}/resume", post(admin_resume))
        .route("/outbox/groups/{group}/unblock", post(admin_unblock))
        .route("/outbox/groups/{group}/skip", post(admin_skip))
        .with_state(processor)
}

async fn admin_groups(State(p): State<Processor>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "groups": p.group_states() }))
}

async fn admin_blocked(State(p): State<Processor>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "blocked": p.blocked_groups() }))
}

async fn admin_pause(
    State(p): State<Processor>,
    Path(group): Path<String>,
) -> Json<serde_json::Value> {
    p.pause_group(&group);
    Json(serde_json::json!({ "status": "PAUSED" }))
}

async fn admin_resume(
    State(p): State<Processor>,
    Path(group): Path<String>,
) -> Json<serde_json::Value> {
    p.resume_group(&group);
    Json(serde_json::json!({ "status": "RUNNING" }))
}

async fn admin_unblock(
    State(p): State<Processor>,
    Path(group): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if p.unblock_group(&group).await {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "UNBLOCKED" })),
        )
    } else {
        not_blocked()
    }
}

async fn admin_skip(
    State(p): State<Processor>,
    Path(group): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if p.skip_group(&group) {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "SKIPPED" })),
        )
    } else {
        not_blocked()
    }
}

fn not_blocked() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "group not blocked" })),
    )
}

async fn metrics_handler(State(p): State<Processor>) -> String {
    let m = p.metrics().await;
    format!(
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
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
    use fc_outbox::repository::ClaimedBatch;
    use fc_outbox::{DispatchOutcome, OutboxDispatcher};
    use std::sync::Mutex;

    /// A repository with one PENDING item that fails for good.
    #[derive(Default)]
    struct OneItem {
        requeued: Mutex<Vec<String>>,
        claimed: Mutex<bool>,
        config: OutboxTableConfig,
    }

    #[async_trait]
    impl OutboxRepository for OneItem {
        async fn claim_pending(&self, _limit: u32) -> Result<ClaimedBatch> {
            let mut claimed = self.claimed.lock().unwrap();
            let mut batch = ClaimedBatch::default();
            if !*claimed {
                *claimed = true;
                let now = chrono::Utc::now();
                batch.push_row(
                    "i1".into(),
                    OutboxItemType::Event,
                    Some("g".into()),
                    "{}",
                    0,
                    None,
                    now,
                    now,
                );
            }
            Ok(batch)
        }
        async fn mark_success(&self, _: OutboxItemType, _: &[String]) -> Result<()> {
            Ok(())
        }
        async fn mark_failed(
            &self,
            _: OutboxItemType,
            _: &[String],
            _: OutboxStatus,
            _: &str,
            _: bool,
        ) -> Result<()> {
            Ok(())
        }
        async fn release(&self, _: OutboxItemType, _: &[String]) -> Result<()> {
            Ok(())
        }
        async fn requeue(&self, _: OutboxItemType, ids: &[String]) -> Result<()> {
            self.requeued.lock().unwrap().extend(ids.iter().cloned());
            Ok(())
        }
        async fn recover_stuck(&self, _: Duration) -> Result<u64> {
            Ok(0)
        }
        async fn init_schema(&self) -> Result<()> {
            Ok(())
        }
        fn table_config(&self) -> &OutboxTableConfig {
            &self.config
        }
    }

    struct Refuse;

    #[async_trait]
    impl OutboxDispatcher for Refuse {
        async fn send_batch(&self, items: &[OutboxItem]) -> Vec<DispatchOutcome> {
            items
                .iter()
                .map(|_| DispatchOutcome::failed(OutboxStatus::Forbidden, "403"))
                .collect()
        }
    }

    async fn call(app: &Router, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn the_admin_api_answers_as_gos() {
        let repo = Arc::new(OneItem::default());
        let processor = Arc::new(EnhancedOutboxProcessor::with_dispatcher(
            EnhancedProcessorConfig::default(),
            repo.clone(),
            Arc::new(Refuse),
        ));
        let app = admin_router(processor.clone());

        assert_eq!(
            call(&app, "POST", "/outbox/groups/g/unblock").await,
            (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "group not blocked"})
            )
        );

        processor.poll_once().await.unwrap();
        for _ in 0..200 {
            if !processor.blocked_groups().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(
            call(&app, "GET", "/outbox/groups/blocked").await,
            (
                StatusCode::OK,
                serde_json::json!({"blocked": [
                    {"group": "g", "status": "BLOCKED", "blockedItemId": "i1", "error": "403"}
                ]})
            )
        );
        assert_eq!(
            call(&app, "POST", "/outbox/groups/g/unblock").await,
            (StatusCode::OK, serde_json::json!({"status": "UNBLOCKED"}))
        );
        assert_eq!(*repo.requeued.lock().unwrap(), vec!["i1"]);

        assert_eq!(
            call(&app, "POST", "/outbox/groups/p/pause").await,
            (StatusCode::OK, serde_json::json!({"status": "PAUSED"}))
        );
        assert_eq!(
            call(&app, "GET", "/outbox/groups").await,
            (
                StatusCode::OK,
                serde_json::json!({"groups": [{"group": "p", "status": "PAUSED"}]})
            )
        );
        assert_eq!(
            call(&app, "POST", "/outbox/groups/p/resume").await,
            (StatusCode::OK, serde_json::json!({"status": "RUNNING"}))
        );
        assert_eq!(
            call(&app, "POST", "/outbox/groups/p/skip").await.0,
            StatusCode::NOT_FOUND
        );
    }
}
