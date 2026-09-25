//! Config reload, local config snapshot, and standby/traffic status.

use super::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde::Serialize;
use tracing::{error, info, warn};
use utoipa::ToSchema;

/// Triggers an immediate re-fetch of the router's configuration from its
/// config source and applies it (Go: `api.ConfigReloader` /
/// `Server.Reload`). Implemented by [`crate::ConfigSyncService`].
#[async_trait::async_trait]
pub trait ConfigReloader: Send + Sync {
    /// Fetch and apply. `Ok` when what is running now matches the source
    /// (whether or not anything changed).
    async fn reload(&self) -> Result<(), String>;
}

#[async_trait::async_trait]
impl ConfigReloader for crate::ConfigSyncService {
    async fn reload(&self) -> Result<(), String> {
        self.apply_latest()
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Response after a config reload (Go: `ConfigReloadResponse`).
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfigReloadResponse {
    /// Whether the configuration now running matches the config source
    pub success: bool,
}

/// Error body, shaped as Go's huma problem responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfigReloadError {
    pub title: String,
    pub status: u16,
    pub detail: String,
}

fn reload_error(status: StatusCode, detail: String) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/problem+json")],
        Json(ConfigReloadError {
            title: status.canonical_reason().unwrap_or("Error").to_string(),
            status: status.as_u16(),
            detail,
        }),
    )
        .into_response()
}

/// Reload configuration: re-fetch it from the config source now and apply
/// it, ahead of the watcher's next tick (Go: `POST /config/reload` →
/// `Server.Reload`). Any request body is ignored. The previous handler
/// built a config from the request body with NO queues, so the reconcile
/// treated every queue as removed and stopped every consumer — and the
/// watcher never restored them, because the source's hash had not changed.
#[utoipa::path(
    post,
    path = "/config/reload",
    tag = "monitoring",
    responses(
        (status = 200, description = "Configuration matches the config source", body = ConfigReloadResponse),
        (status = 409, description = "Not the leader — reload refused", body = ConfigReloadError),
        (status = 500, description = "Fetch or apply failed, or no config source", body = ConfigReloadError)
    )
)]
pub(crate) async fn reload_config(State(state): State<AppState>) -> Response {
    // R-33: gate on leadership, before any fetch/reconfigure. A follower
    // must never start/reconfigure consumers or pools. When standby is
    // disabled this is unchanged — every instance is its own leader.
    if state.standby_enabled && !state.queue_manager.is_leader() {
        warn!("Configuration reload refused — this instance is not the leader");
        return reload_error(
            StatusCode::CONFLICT,
            "not leader; config reload is only served by the current leader".to_string(),
        );
    }

    let Some(reloader) = state.config_reloader.clone() else {
        return reload_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "reload: router has no config source configured".to_string(),
        );
    };

    match reloader.reload().await {
        Ok(()) => {
            info!("Configuration reloaded via API");
            (StatusCode::OK, Json(ConfigReloadResponse { success: true })).into_response()
        }
        Err(e) => {
            error!(error = %e, "Failed to reload configuration");
            reload_error(StatusCode::INTERNAL_SERVER_ERROR, format!("reload: {e}"))
        }
    }
}

/// Get local configuration
///
/// In dev mode (FLOWCATALYST_DEV_MODE=true), returns LocalStack queue URLs.
/// Otherwise returns current pool configuration.
#[utoipa::path(
    get,
    path = "/api/config",
    tag = "monitoring",
    responses(
        (status = 200, description = "Local configuration")
    )
)]
pub(crate) async fn get_local_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let pool_stats = state.queue_manager.get_pool_stats();
    let dev_mode = std::env::var("FLOWCATALYST_DEV_MODE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let pools: Vec<serde_json::Value> = if dev_mode && pool_stats.is_empty() {
        // Return default dev pools
        vec![
            serde_json::json!({
                "code": "DEFAULT",
                "concurrency": 10,
                "rateLimitPerMinute": null,
            }),
            serde_json::json!({
                "code": "HIGH",
                "concurrency": 20,
                "rateLimitPerMinute": null,
            }),
            serde_json::json!({
                "code": "LOW",
                "concurrency": 5,
                "rateLimitPerMinute": 60,
            }),
        ]
    } else {
        pool_stats
            .iter()
            .map(|p| {
                serde_json::json!({
                    "code": p.pool_code,
                    "concurrency": p.concurrency,
                    "rateLimitPerMinute": p.rate_limit_per_minute,
                })
            })
            .collect()
    };

    let queues: Vec<serde_json::Value> = if dev_mode {
        // Return LocalStack queue URLs for development
        // LocalStack uses this URL format for SQS queues
        let sqs_host = std::env::var("LOCALSTACK_SQS_HOST")
            .unwrap_or_else(|_| "http://sqs.eu-west-1.localhost.localstack.cloud:4566".to_string());

        vec![
            serde_json::json!({
                "queueName": "fc-high-priority.fifo",
                "queueUri": format!("{}/000000000000/fc-high-priority.fifo", sqs_host),
                "connections": 2,
                "visibilityTimeout": 120,
            }),
            serde_json::json!({
                "queueName": "fc-default.fifo",
                "queueUri": format!("{}/000000000000/fc-default.fifo", sqs_host),
                "connections": 2,
                "visibilityTimeout": 120,
            }),
            serde_json::json!({
                "queueName": "fc-low-priority.fifo",
                "queueUri": format!("{}/000000000000/fc-low-priority.fifo", sqs_host),
                "connections": 1,
                "visibilityTimeout": 120,
            }),
        ]
    } else {
        vec![]
    };

    Json(serde_json::json!({
        "queues": queues,
        "connections": 1,
        "processingPools": pools,
    }))
}

/// Standby status response
#[derive(Serialize, ToSchema)]
pub(crate) struct StandbyStatusResponse {
    #[serde(rename = "standbyEnabled")]
    standby_enabled: bool,
    #[serde(rename = "instanceId")]
    instance_id: String,
    role: String,
    #[serde(rename = "redisAvailable")]
    redis_available: bool,
    #[serde(rename = "currentLockHolder")]
    current_lock_holder: Option<String>,
    #[serde(rename = "lastSuccessfulRefresh")]
    last_successful_refresh: Option<String>,
    #[serde(rename = "hasWarning")]
    has_warning: bool,
}

/// Get standby status
#[utoipa::path(
    get,
    path = "/monitoring/standby-status",
    tag = "standby",
    responses(
        (status = 200, description = "Standby status", body = StandbyStatusResponse)
    )
)]
pub(crate) async fn get_standby_status(
    State(state): State<AppState>,
) -> Json<StandbyStatusResponse> {
    Json(StandbyStatusResponse {
        standby_enabled: state.standby_enabled,
        instance_id: state.instance_id.clone(),
        role: "PRIMARY".to_string(), // Always primary when standby not enabled
        redis_available: false,
        current_lock_holder: Some(state.instance_id.clone()),
        last_successful_refresh: Some(Utc::now().to_rfc3339()),
        has_warning: false,
    })
}

/// Traffic status response
#[derive(Serialize, ToSchema)]
pub(crate) struct TrafficStatusResponse {
    enabled: bool,
    #[serde(rename = "strategyType")]
    strategy_type: String,
    registered: bool,
    #[serde(rename = "targetInfo")]
    target_info: Option<String>,
    #[serde(rename = "lastOperation")]
    last_operation: Option<String>,
    #[serde(rename = "lastError")]
    last_error: String,
}

/// Get traffic status
#[utoipa::path(
    get,
    path = "/monitoring/traffic-status",
    tag = "standby",
    responses(
        (status = 200, description = "Traffic status", body = TrafficStatusResponse)
    )
)]
pub(crate) async fn get_traffic_status(
    State(state): State<AppState>,
) -> Json<TrafficStatusResponse> {
    match &state.traffic_strategy {
        Some(strategy) => Json(TrafficStatusResponse {
            enabled: true,
            strategy_type: strategy.strategy_type().to_string(),
            registered: strategy.is_registered(),
            target_info: None,
            last_operation: Some(Utc::now().to_rfc3339()),
            last_error: "none".to_string(),
        }),
        None => Json(TrafficStatusResponse {
            enabled: false,
            strategy_type: "NONE".to_string(),
            registered: true,
            target_info: None,
            last_operation: Some(Utc::now().to_rfc3339()),
            last_error: "none".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;
    use crate::{CircuitBreakerRegistry, HealthService, QueueManager, WarningService};
    use std::sync::Arc;

    /// Publisher that never actually sends anywhere — `reload_config` never
    /// touches the publisher, but `AppState` requires one.
    struct NoopPublisher;

    #[async_trait::async_trait]
    impl fc_queue::QueuePublisher for NoopPublisher {
        fn identifier(&self) -> &str {
            "noop"
        }
        async fn publish(&self, message: fc_common::Message) -> fc_queue::Result<String> {
            Ok(message.id)
        }
        async fn publish_batch(
            &self,
            messages: Vec<fc_common::Message>,
        ) -> fc_queue::Result<Vec<String>> {
            Ok(messages.into_iter().map(|m| m.id).collect())
        }
    }

    /// Minimal `AppState` for handler-level tests — everything but
    /// `queue_manager`/`standby_enabled` (the two the R-33 gate reads) is a
    /// bare default/noop instance.
    fn test_app_state(queue_manager: Arc<QueueManager>, standby_enabled: bool) -> AppState {
        let warning_service = Arc::new(WarningService::noop());
        let health_service = Arc::new(HealthService::new(
            crate::health::HealthServiceConfig::default(),
            warning_service.clone(),
        ));
        AppState {
            publisher: Arc::new(NoopPublisher) as Arc<dyn fc_queue::QueuePublisher>,
            queue_manager: queue_manager.clone(),
            warning_service,
            health_service,
            circuit_breaker_registry: Arc::new(CircuitBreakerRegistry::default()),
            standby_enabled,
            instance_id: "test-instance".to_string(),
            stream_health_service: None,
            traffic_strategy: None,
            metrics_handle: None,
            cached_broker_stats: Arc::new(super::super::CachedBrokerStats::new(queue_manager)),
            config_reloader: None,
        }
    }

    struct StubReloader {
        calls: std::sync::atomic::AtomicU32,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl ConfigReloader for StubReloader {
        async fn reload(&self) -> Result<(), String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                Err("config: all 1 source(s) failed".to_string())
            } else {
                Ok(())
            }
        }
    }

    fn with_reloader(mut state: AppState, fail: bool) -> (AppState, Arc<StubReloader>) {
        let r = Arc::new(StubReloader {
            calls: std::sync::atomic::AtomicU32::new(0),
            fail,
        });
        state.config_reloader = Some(r.clone());
        (state, r)
    }

    /// R-33: standby enabled + not leader ⇒ reload refused (409) before any
    /// fetch.
    #[tokio::test]
    async fn reload_config_refused_when_standby_enabled_and_not_leader() {
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));
        manager.set_leader(false);
        let (state, r) = with_reloader(test_app_state(manager.clone(), true), false);

        let response = reload_config(State(state)).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(r.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// C5 (Go `Server.Reload`): the reload re-fetches from the config
    /// source; it never builds a config of its own.
    #[tokio::test]
    async fn reload_config_refetches_from_the_config_source() {
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));
        manager.set_leader(true);
        let (state, r) = with_reloader(test_app_state(manager.clone(), true), false);

        let response = reload_config(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(r.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], br#"{"success":true}"#);
    }

    /// A failed fetch/apply is a 500 naming the reason; standby off never
    /// consults leadership.
    #[tokio::test]
    async fn reload_config_failure_is_a_500() {
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));
        let (state, _r) = with_reloader(test_app_state(manager.clone(), false), true);
        let response = reload_config(State(state)).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// No config source (dev/default broker) — Go: 500 "router has no
    /// config source configured".
    #[tokio::test]
    async fn reload_config_without_a_config_source_is_a_500() {
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));
        let state = test_app_state(manager.clone(), false);
        let response = reload_config(State(state)).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// C5 end to end: a running consumer survives an API reload. The old
    /// handler applied a config with no queues, which stopped every
    /// consumer; the watcher never put them back because the source had
    /// not changed.
    #[tokio::test]
    async fn api_reload_keeps_consumers_running() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        struct NullFactory;
        #[async_trait::async_trait]
        impl crate::ConsumerFactory for NullFactory {
            async fn create_consumer(
                &self,
                config: &fc_common::QueueConfig,
            ) -> crate::Result<Arc<dyn fc_queue::QueueConsumer>> {
                Ok(Arc::new(IdleConsumer(config.name.clone())))
            }
        }
        struct IdleConsumer(String);
        #[async_trait::async_trait]
        impl fc_queue::QueueConsumer for IdleConsumer {
            fn identifier(&self) -> &str {
                &self.0
            }
            async fn poll(&self, _: u32) -> fc_queue::Result<Vec<fc_common::QueuedMessage>> {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                Ok(vec![])
            }
            async fn ack(&self, _: &str) -> fc_queue::Result<()> {
                Ok(())
            }
            async fn nack(&self, _: &str, _: Option<u32>) -> fc_queue::Result<()> {
                Ok(())
            }
            async fn extend_visibility(&self, _: &str, _: u32) -> fc_queue::Result<()> {
                Ok(())
            }
            fn is_healthy(&self) -> bool {
                true
            }
            async fn stop(&self) {}
        }

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "processingPools": [{"code": "P", "concurrency": 2}],
                "queues": [{"queueName": "q1", "queueUri": "test://q1"}],
            })))
            .mount(&server)
            .await;

        let manager = Arc::new(
            QueueManager::builder(HttpMediatorConfig::dev())
                .consumer_factory(Arc::new(NullFactory))
                .build(),
        );
        let mut cfg = crate::ConfigSyncConfig::new(server.uri());
        cfg.max_retry_attempts = 1;
        let sync = Arc::new(crate::ConfigSyncService::new(
            cfg,
            manager.clone(),
            Arc::new(WarningService::noop()),
        ));
        sync.apply_latest().await.unwrap();
        assert_eq!(manager.consumer_ids().await, vec!["q1".to_string()]);

        let mut state = test_app_state(manager.clone(), false);
        state.config_reloader = Some(sync.clone());
        let response = reload_config(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            manager.consumer_ids().await,
            vec!["q1".to_string()],
            "an API reload must never tear down the configured queues"
        );
        assert_eq!(manager.detaching_consumer_count(), 0);
        manager.shutdown().await;
    }
}
