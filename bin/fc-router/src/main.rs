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

use std::sync::Arc;
use std::time::Duration;
use fc_router::{
    QueueManager, HttpMediatorConfig, LifecycleManager, LifecycleConfig,
    WarningService, WarningServiceConfig,
    HealthService, HealthServiceConfig,
    CircuitBreakerRegistry, ConsumerFactory,
    ConfigSyncService, ConfigSyncConfig,
    StandbyProcessor, StandbyRouterConfig,
    NotificationConfig, create_notification_service_with_scheduler,
    api::create_router_with_options,
};
use fc_common::{RouterConfig, PoolConfig, QueueConfig, WarningSeverity};
use fc_queue::sqs::SqsQueueConsumer;
use anyhow::Result;
use tracing::{info, warn, error};
use tokio::{signal, net::TcpListener};
use tower_http::cors::{CorsLayer, Any};
use tower_http::trace::TraceLayer;

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

    // 2. Initialize Warning and Health Services
    let warning_service = Arc::new(WarningService::new(WarningServiceConfig::default()));
    let health_service = Arc::new(HealthService::new(
        HealthServiceConfig::default(),
        warning_service.clone(),
    ));

    // 2b. Initialize Notification Service (Teams webhooks)
    let notification_config = load_notification_config();
    let notification_scheduler = create_notification_service_with_scheduler(&notification_config);
    if let Some(ref ns) = notification_scheduler {
        info!(
            batch_interval = notification_config.batch_interval_seconds,
            "Notification service enabled (Teams webhook with batching)"
        );
        // Wire up notification service to warning service
        warning_service.set_notification_service(ns.service.clone());
    } else {
        info!("Notification service disabled - no channels configured");
    }

    // 3. Create QueueManager. Mediator *config* is passed (not a singleton);
    //    each pool gets its own HttpMediator + connection pool.
    let mut queue_manager_inner = QueueManager::new(HttpMediatorConfig::production());
    queue_manager_inner.set_warning_service(warning_service.clone());
    queue_manager_inner.set_health_service(health_service.clone());
    queue_manager_inner.set_consumer_factory(Arc::new(SqsConsumerFactory {
        sqs_client: sqs_client.clone(),
    }));
    let queue_manager = Arc::new(queue_manager_inner);

    // 5. Initialize Standby Processor (Active/Passive HA)
    let standby_config = load_standby_config();
    let standby = if standby_config.enabled {
        info!(
            redis_url = %standby_config.redis_url,
            lock_key = %standby_config.lock_key,
            "Initializing standby mode (Active/Passive HA)"
        );
        match StandbyProcessor::new(standby_config).await {
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
        let config_url = std::env::var("FLOWCATALYST_CONFIG_URL")
            .map_err(|_| anyhow::anyhow!("FLOWCATALYST_CONFIG_URL is required (or set FLOWCATALYST_DEV_MODE=true)"))?;

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

    // 8. Create SQS consumers from config
    let mut first_queue_url: Option<String> = None;
    for queue_config in &router_config.queues {
        info!(
            queue_name = %queue_config.name,
            queue_uri = %queue_config.uri,
            connections = queue_config.connections,
            visibility_timeout = queue_config.visibility_timeout,
            "Creating SQS consumer from config"
        );

        let consumer = Arc::new(SqsQueueConsumer::from_queue_url(
            sqs_client.clone(),
            queue_config.uri.clone(),
            queue_config.visibility_timeout as i32,
        ).await);
        queue_manager.add_consumer(consumer).await;

        // Track first queue URL for publisher
        if first_queue_url.is_none() {
            first_queue_url = Some(queue_config.uri.clone());
        }
    }

    if router_config.queues.is_empty() {
        error!("No queues configured - cannot start router");
        return Err(anyhow::anyhow!("No queues configured in config sync response"));
    }

    // 9. Start lifecycle manager with all features
    let lifecycle = LifecycleManager::start_with_features(
        queue_manager.clone(),
        warning_service.clone(),
        health_service.clone(),
        LifecycleConfig::default(),
        config_sync,
        standby.clone(),
    );

    // 10. Setup HTTP API server
    let api_port: u16 = std::env::var("API_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);

    // Create a simple publisher that publishes to the first queue
    let publisher_queue_url = first_queue_url.expect("At least one queue must be configured");
    let publisher = Arc::new(SqsPublisher::new(sqs_client, publisher_queue_url));

    // Create circuit breaker registry for endpoint tracking
    let circuit_breaker_registry = Arc::new(CircuitBreakerRegistry::default());

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
        publisher,
        queue_manager.clone(),
        warning_service.clone(),
        health_service.clone(),
        circuit_breaker_registry,
        standby.is_some(),
        standby.as_ref().map(|s| s.instance_id().to_string()).unwrap_or_else(|| "default".to_string()),
        None, // stream_health_service
        None, // traffic_strategy
        Some(metrics_handle),
        auth_state,
    )
    .layer(TraceLayer::new_for_http())
    .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any));

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

    lifecycle.shutdown().await;
    queue_manager.shutdown().await;

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
    let enabled = std::env::var("FLOWCATALYST_STANDBY_ENABLED")
        .map(|v| v.parse().unwrap_or(false))
        .unwrap_or(false);

    let redis_url = std::env::var("FLOWCATALYST_STANDBY_REDIS_URL")
        .or_else(|_| std::env::var("FLOWCATALYST_REDIS_URL"))
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let lock_key = std::env::var("FLOWCATALYST_STANDBY_LOCK_KEY")
        .unwrap_or_else(|_| "fc:router:leader".to_string());

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
    let teams_enabled = std::env::var("NOTIFICATION_TEAMS_ENABLED")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let teams_webhook_url = std::env::var("NOTIFICATION_TEAMS_WEBHOOK_URL").ok();

    let min_severity = std::env::var("NOTIFICATION_MIN_SEVERITY")
        .map(|s| match s.to_uppercase().as_str() {
            "INFO" => WarningSeverity::Info,
            "WARN" | "WARNING" => WarningSeverity::Warn,
            "ERROR" => WarningSeverity::Error,
            "CRITICAL" => WarningSeverity::Critical,
            _ => WarningSeverity::Warn,
        })
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

// SQS consumer factory for hot-reloading new queues from config sync
struct SqsConsumerFactory {
    sqs_client: aws_sdk_sqs::Client,
}

#[async_trait]
impl ConsumerFactory for SqsConsumerFactory {
    async fn create_consumer(
        &self,
        config: &QueueConfig,
    ) -> std::result::Result<Arc<dyn fc_queue::QueueConsumer + Send + Sync>, fc_router::RouterError> {
        info!(
            queue_name = %config.name,
            queue_uri = %config.uri,
            visibility_timeout = config.visibility_timeout,
            "Creating SQS consumer from config sync"
        );
        let consumer = SqsQueueConsumer::from_queue_url(
            self.sqs_client.clone(),
            config.uri.clone(),
            config.visibility_timeout as i32,
        ).await;
        Ok(Arc::new(consumer))
    }
}

// Simple SQS publisher implementation
use async_trait::async_trait;
use fc_queue::{QueuePublisher, QueueError};
use fc_common::Message;

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

        let mut request = self.client.send_message()
            .queue_url(&self.queue_url)
            .message_body(body);

        // FIFO queues require message_group_id and message_deduplication_id
        if self.queue_url.ends_with(".fifo") {
            let group_id = message.message_group_id.clone()
                .unwrap_or_else(|| "default".to_string());
            request = request
                .message_group_id(group_id)
                .message_deduplication_id(&message_id);
        }

        request.send()
            .await
            .map_err(|e| QueueError::Sqs(e.to_string()))?;

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
