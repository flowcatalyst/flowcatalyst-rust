//! FlowCatalyst Dispatch Scheduler Server

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{routing::get, Json, Router};
use fc_config::AppConfig;
use fc_scheduler::{DispatchScheduler, QueueMessage, QueuePublisher, SchedulerConfig, SchedulerError};
use mongodb::Client as MongoClient;
use serde::Serialize;
use tracing::info;

struct DevQueuePublisher;

#[async_trait::async_trait]
impl QueuePublisher for DevQueuePublisher {
    async fn publish(&self, message: QueueMessage) -> Result<(), SchedulerError> {
        info!(id = %message.id, "DEV: Message published to queue");
        Ok(())
    }

    fn is_healthy(&self) -> bool { true }
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    scheduler_running: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fc_common::logging::init_logging("fc-scheduler-server");

    info!("Starting FlowCatalyst Dispatch Scheduler");

    let config = AppConfig::load()?;
    info!(enabled = config.scheduler.enabled, poll_interval_ms = config.scheduler.poll_interval_ms, "Scheduler configuration loaded");

    let mongo_client = MongoClient::with_uri_str(&config.mongodb.uri).await?;
    let db = mongo_client.database(&config.mongodb.database);
    info!(database = %config.mongodb.database, "Connected to MongoDB");

    let scheduler_config = SchedulerConfig {
        enabled: config.scheduler.enabled,
        poll_interval: std::time::Duration::from_millis(config.scheduler.poll_interval_ms),
        batch_size: config.scheduler.batch_size,
        stale_threshold: std::time::Duration::from_secs(config.scheduler.stale_threshold_minutes * 60),
        default_dispatch_mode: config.scheduler.default_dispatch_mode.as_str().into(),
        default_pool_code: "default".to_string(),
        processing_endpoint: format!("http://localhost:{}/api/router/process", config.http.port),
        app_key: if config.scheduler.app_key.is_empty() { None } else { Some(config.scheduler.app_key.clone()) },
    };

    let queue_publisher: Arc<dyn QueuePublisher> = Arc::new(DevQueuePublisher);
    let scheduler = Arc::new(DispatchScheduler::new(scheduler_config, db, queue_publisher));
    scheduler.start().await;

    let scheduler_clone = scheduler.clone();
    let app = Router::new()
        .route("/q/health", get(move || {
            let s = scheduler_clone.clone();
            async move {
                let running = s.is_running().await;
                Json(HealthResponse { status: if running { "UP".to_string() } else { "DOWN".to_string() }, scheduler_running: running })
            }
        }))
        .route("/q/health/live", get(|| async { Json(serde_json::json!({"status": "UP"})) }))
        .route("/q/health/ready", get(|| async { Json(serde_json::json!({"status": "UP"})) }));

    let addr = SocketAddr::from(([0, 0, 0, 0], config.http.port));
    info!(?addr, "HTTP server starting");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(scheduler))
        .await?;

    info!("Scheduler server stopped");
    Ok(())
}

async fn shutdown_signal(scheduler: Arc<DispatchScheduler>) {
    tokio::signal::ctrl_c().await.expect("Failed to install CTRL+C handler");
    info!("Shutdown signal received");
    scheduler.stop().await;
}
