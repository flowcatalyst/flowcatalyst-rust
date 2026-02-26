//! Platform Configuration API
//!
//! Returns platform feature flags and configuration.

use axum::{routing::get, Json, Router};
use serde::Serialize;
use utoipa::ToSchema;

/// Platform feature flags
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlatformFeatures {
    /// Whether messaging features are enabled
    pub messaging_enabled: bool,
}

/// Platform configuration response
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfig {
    /// Feature flags
    pub features: PlatformFeatures,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            features: PlatformFeatures {
                messaging_enabled: true,
            },
        }
    }
}

/// Get platform configuration
#[utoipa::path(
    get,
    path = "/platform",
    tag = "config",
    responses(
        (status = 200, description = "Platform configuration", body = PlatformConfig)
    )
)]
pub async fn get_platform_config() -> Json<PlatformConfig> {
    Json(PlatformConfig::default())
}

/// Create the platform config router
pub fn platform_config_router() -> Router {
    Router::new()
        .route("/platform", get(get_platform_config))
}
