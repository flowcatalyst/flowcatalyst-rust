//! Health Check Endpoints
//!
//! Standard health check endpoints for Kubernetes probes and monitoring.
//! - /health - Combined health status
//! - /health/live - Liveness probe
//! - /health/ready - Readiness probe
//! - /health/startup - Startup probe

use axum::{
    routing::get,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json, Router,
};
use utoipa::ToSchema;
use serde::Serialize;
use std::sync::Arc;
use chrono::{DateTime, Utc};

/// Health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum HealthStatus {
    /// Service is healthy
    Up,
    /// Service is unhealthy
    Down,
    /// Service is degraded but functional
    Degraded,
}

/// Individual health check result
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    /// Name of the check
    pub name: String,

    /// Status of the check
    pub status: HealthStatus,

    /// Optional details/message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Time taken for the check in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Full health response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    /// Overall status
    pub status: HealthStatus,

    /// Current server time
    pub timestamp: DateTime<Utc>,

    /// Service version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Individual health checks
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<HealthCheck>,
}

/// Simple health status response
#[derive(Debug, Serialize, ToSchema)]
pub struct SimpleHealthResponse {
    pub status: HealthStatus,
}

/// Health check dependencies
pub trait HealthChecker: Send + Sync {
    /// Perform the health check
    fn check(&self) -> impl std::future::Future<Output = HealthCheck> + Send;
}

/// MongoDB health checker
pub struct MongoHealthChecker {
    pub db: mongodb::Database,
}

impl HealthChecker for MongoHealthChecker {
    async fn check(&self) -> HealthCheck {
        let start = std::time::Instant::now();

        match self.db.run_command(mongodb::bson::doc! { "ping": 1 }).await {
            Ok(_) => HealthCheck {
                name: "mongodb".to_string(),
                status: HealthStatus::Up,
                message: None,
                duration_ms: Some(start.elapsed().as_millis() as u64),
            },
            Err(e) => HealthCheck {
                name: "mongodb".to_string(),
                status: HealthStatus::Down,
                message: Some(format!("Connection failed: {}", e)),
                duration_ms: Some(start.elapsed().as_millis() as u64),
            },
        }
    }
}

/// Health service state
#[derive(Clone)]
pub struct HealthState {
    /// Database for connectivity check
    pub db: Option<mongodb::Database>,

    /// Service version
    pub version: Option<String>,

    /// Startup time
    pub started_at: DateTime<Utc>,

    /// Ready flag (set after initialization complete)
    pub ready: Arc<std::sync::atomic::AtomicBool>,
}

impl HealthState {
    pub fn new(db: Option<mongodb::Database>, version: Option<String>) -> Self {
        Self {
            db,
            version,
            started_at: Utc::now(),
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Mark the service as ready
    pub fn set_ready(&self) {
        self.ready.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check if the service is ready
    pub fn is_ready(&self) -> bool {
        self.ready.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Combined health check
///
/// Returns the overall health status including all checks.
/// Use this for monitoring dashboards.
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse),
        (status = 503, description = "Service is unhealthy", body = HealthResponse)
    )
)]
pub async fn get_health(State(state): State<HealthState>) -> Response {
    let mut checks = Vec::new();
    let mut overall_status = HealthStatus::Up;

    // MongoDB check
    if let Some(db) = &state.db {
        let checker = MongoHealthChecker { db: db.clone() };
        let check = checker.check().await;

        if check.status == HealthStatus::Down {
            overall_status = HealthStatus::Down;
        } else if check.status == HealthStatus::Degraded && overall_status == HealthStatus::Up {
            overall_status = HealthStatus::Degraded;
        }

        checks.push(check);
    }

    // Readiness check
    if !state.is_ready() && overall_status == HealthStatus::Up {
        overall_status = HealthStatus::Degraded;
    }

    let response = HealthResponse {
        status: overall_status,
        timestamp: Utc::now(),
        version: state.version.clone(),
        checks,
    };

    let status_code = if overall_status == HealthStatus::Down {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (status_code, Json(response)).into_response()
}

/// Liveness probe
///
/// Simple check that the service is alive and responding.
/// Should always return 200 unless the service needs restart.
/// Used by Kubernetes liveness probe.
#[utoipa::path(
    get,
    path = "/health/live",
    tag = "health",
    responses(
        (status = 200, description = "Service is alive", body = SimpleHealthResponse)
    )
)]
pub async fn get_liveness() -> Json<SimpleHealthResponse> {
    Json(SimpleHealthResponse {
        status: HealthStatus::Up,
    })
}

/// Readiness probe
///
/// Checks if the service is ready to accept traffic.
/// Returns 503 if dependencies are not ready.
/// Used by Kubernetes readiness probe.
#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "health",
    responses(
        (status = 200, description = "Service is ready", body = SimpleHealthResponse),
        (status = 503, description = "Service is not ready", body = SimpleHealthResponse)
    )
)]
pub async fn get_readiness(State(state): State<HealthState>) -> Response {
    let status = if state.is_ready() {
        // Also check MongoDB if available
        if let Some(db) = &state.db {
            let checker = MongoHealthChecker { db: db.clone() };
            let check = checker.check().await;
            if check.status == HealthStatus::Up {
                HealthStatus::Up
            } else {
                HealthStatus::Down
            }
        } else {
            HealthStatus::Up
        }
    } else {
        HealthStatus::Down
    };

    let status_code = if status == HealthStatus::Down {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (status_code, Json(SimpleHealthResponse { status })).into_response()
}

/// Startup probe
///
/// Checks if the service has completed initialization.
/// Returns 503 until initialization is complete.
/// Used by Kubernetes startup probe.
#[utoipa::path(
    get,
    path = "/health/startup",
    tag = "health",
    responses(
        (status = 200, description = "Service has started", body = SimpleHealthResponse),
        (status = 503, description = "Service is starting", body = SimpleHealthResponse)
    )
)]
pub async fn get_startup(State(state): State<HealthState>) -> Response {
    let status = if state.is_ready() {
        HealthStatus::Up
    } else {
        HealthStatus::Down
    };

    let status_code = if status == HealthStatus::Down {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (status_code, Json(SimpleHealthResponse { status })).into_response()
}

/// Create the health router
pub fn health_router(state: HealthState) -> Router {
    Router::new()
        .route("/", get(get_health))
        .route("/live", get(get_liveness))
        .route("/ready", get(get_readiness))
        .route("/startup", get(get_startup))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_serialization() {
        let up = serde_json::to_string(&HealthStatus::Up).unwrap();
        assert_eq!(up, "\"UP\"");

        let down = serde_json::to_string(&HealthStatus::Down).unwrap();
        assert_eq!(down, "\"DOWN\"");
    }

    #[test]
    fn test_health_response_serialization() {
        let response = HealthResponse {
            status: HealthStatus::Up,
            timestamp: Utc::now(),
            version: Some("1.0.0".to_string()),
            checks: vec![HealthCheck {
                name: "mongodb".to_string(),
                status: HealthStatus::Up,
                message: None,
                duration_ms: Some(5),
            }],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"UP\""));
        assert!(json.contains("\"version\":\"1.0.0\""));
    }

    #[test]
    fn test_health_state() {
        let state = HealthState::new(None, Some("1.0.0".to_string()));
        assert!(!state.is_ready());

        state.set_ready();
        assert!(state.is_ready());
    }
}
