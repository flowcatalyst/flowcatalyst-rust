//! Config reload, local config snapshot, and standby/traffic status.

use super::AppState;
use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use chrono::Utc;
use fc_common::PoolConfig;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use utoipa::ToSchema;

/// Request to reload router configuration
#[derive(Debug, Deserialize, ToSchema)]
pub struct ConfigReloadRequest {
    /// List of pool configurations
    pub processing_pools: Vec<PoolConfigRequest>,
}

/// Pool configuration in reload request
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PoolConfigRequest {
    /// Pool code/identifier
    pub code: String,
    /// Worker concurrency
    pub concurrency: u32,
    /// Optional rate limit (messages per minute)
    pub rate_limit_per_minute: Option<u32>,
}

/// Response after config reload
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfigReloadResponse {
    /// Whether the reload was successful
    pub success: bool,
    /// Number of pools updated
    pub pools_updated: usize,
    /// Number of new pools created
    pub pools_created: usize,
    /// Number of pools removed (draining)
    pub pools_removed: usize,
    /// Total active pools after reload
    pub total_active_pools: usize,
    /// Total pools currently draining
    pub total_draining_pools: usize,
}

/// Reload configuration (hot reload)
#[utoipa::path(
    post,
    path = "/config/reload",
    tag = "monitoring",
    request_body = ConfigReloadRequest,
    responses(
        (status = 200, description = "Configuration reloaded", body = ConfigReloadResponse),
        (status = 409, description = "Not the leader — reload refused", body = ConfigReloadResponse),
        (status = 503, description = "Service unavailable", body = ConfigReloadResponse),
        (status = 500, description = "Internal error", body = ConfigReloadResponse)
    )
)]
pub(crate) async fn reload_config(
    State(state): State<AppState>,
    Json(req): Json<ConfigReloadRequest>,
) -> Response {
    use fc_common::RouterConfig;

    // R-33: gate on leadership, before any fetch/reconfigure. A follower
    // must never start/reconfigure consumers or pools. When standby is
    // disabled (state.standby_enabled == false) this is unchanged —
    // QueueManager::is_leader defaults `true` and nothing ever flips it, so
    // every instance is its own leader in that mode.
    if state.standby_enabled && !state.queue_manager.is_leader() {
        warn!("Configuration reload refused — this instance is not the leader");
        return (
            StatusCode::CONFLICT,
            Json(ConfigReloadResponse {
                success: false,
                pools_updated: 0,
                pools_created: 0,
                pools_removed: 0,
                total_active_pools: 0,
                total_draining_pools: 0,
            }),
        )
            .into_response();
    }

    let router_config = RouterConfig {
        processing_pools: req
            .processing_pools
            .into_iter()
            .map(|p| PoolConfig {
                code: p.code,
                concurrency: p.concurrency,
                rate_limit_per_minute: p.rate_limit_per_minute,
            })
            .collect(),
        queues: vec![],
    };

    let pools_before = state.queue_manager.pool_codes().len();

    match state.queue_manager.reload_config(router_config).await {
        Ok(true) => {
            let pools_after = state.queue_manager.pool_codes().len();
            let pool_stats = state.queue_manager.get_pool_stats();
            let pools_created = pools_after.saturating_sub(pools_before);
            let pools_removed = pools_before.saturating_sub(pools_after);

            info!(
                pools_before = pools_before,
                pools_after = pools_after,
                pools_created = pools_created,
                pools_removed = pools_removed,
                "Configuration reloaded via API"
            );

            (
                StatusCode::OK,
                Json(ConfigReloadResponse {
                    success: true,
                    pools_updated: 0,
                    pools_created,
                    pools_removed,
                    total_active_pools: pool_stats.len(),
                    total_draining_pools: 0,
                }),
            )
                .into_response()
        }
        Ok(false) => {
            warn!("Configuration reload was skipped (shutdown in progress)");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ConfigReloadResponse {
                    success: false,
                    pools_updated: 0,
                    pools_created: 0,
                    pools_removed: 0,
                    total_active_pools: 0,
                    total_draining_pools: 0,
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!(error = %e, "Failed to reload configuration");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ConfigReloadResponse {
                    success: false,
                    pools_updated: 0,
                    pools_created: 0,
                    pools_removed: 0,
                    total_active_pools: 0,
                    total_draining_pools: 0,
                }),
            )
                .into_response()
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
        }
    }

    fn reload_request() -> ConfigReloadRequest {
        ConfigReloadRequest {
            processing_pools: vec![PoolConfigRequest {
                code: "DEFAULT".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            }],
        }
    }

    /// R-33: standby enabled + not leader ⇒ reload refused (409), and —
    /// critically — before any fetch/reconfigure: pool_codes() must be
    /// unchanged.
    #[tokio::test]
    async fn reload_config_refused_when_standby_enabled_and_not_leader() {
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));
        manager.set_leader(false);
        let state = test_app_state(manager.clone(), true);

        let response = reload_config(State(state), Json(reload_request())).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(
            manager.pool_codes().is_empty(),
            "no pool should have been created — the gate must run before any reconfigure"
        );
    }

    /// R-33: standby enabled + leader ⇒ reload proceeds normally (200).
    #[tokio::test]
    async fn reload_config_allowed_when_standby_enabled_and_leader() {
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));
        manager.set_leader(true);
        let state = test_app_state(manager.clone(), true);

        let response = reload_config(State(state), Json(reload_request())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(manager.pool_codes(), vec!["DEFAULT".to_string()]);
    }

    /// R-33: standby disabled ⇒ unchanged, even though `QueueManager` was
    /// never told it's the leader — `is_leader()` defaults `true` but the
    /// gate must not even consult it when standby is off.
    #[tokio::test]
    async fn reload_config_unaffected_by_leadership_when_standby_disabled() {
        let manager = Arc::new(QueueManager::new(HttpMediatorConfig::dev()));
        let state = test_app_state(manager.clone(), false);

        let response = reload_config(State(state), Json(reload_request())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(manager.pool_codes(), vec!["DEFAULT".to_string()]);
    }
}
