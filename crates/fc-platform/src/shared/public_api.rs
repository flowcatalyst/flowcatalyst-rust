//! /api/public Routes — Unauthenticated public endpoints

use axum::{extract::State, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::platform_config::repository::PlatformConfigRepository;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeaturesResponse {
    pub messaging_enabled: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfoResponse {
    pub features: FeaturesResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoginThemeResponse {
    pub brand_name: Option<String>,
    pub brand_subtitle: Option<String>,
    pub logo_url: Option<String>,
    pub logo_svg: Option<String>,
    pub logo_height: Option<u32>,
    pub primary_color: Option<String>,
    pub accent_color: Option<String>,
    pub background_color: Option<String>,
    pub background_gradient: Option<String>,
    pub footer_text: Option<String>,
    pub custom_css: Option<String>,
}

#[derive(Clone)]
pub struct PublicApiState {
    pub config_repo: Arc<PlatformConfigRepository>,
}

/// Get platform feature flags
#[utoipa::path(
    get,
    path = "/platform",
    tag = "public",
    operation_id = "getApiPublicPlatform",
    responses(
        (status = 200, description = "Platform feature flags", body = PlatformInfoResponse)
    )
)]
async fn get_platform_info() -> Json<PlatformInfoResponse> {
    Json(PlatformInfoResponse {
        features: FeaturesResponse {
            messaging_enabled: true,
        },
    })
}

/// Get login theme configuration
#[utoipa::path(
    get,
    path = "/login-theme",
    tag = "public",
    operation_id = "getApiPublicLoginTheme",
    responses(
        (status = 200, description = "Login theme configuration", body = LoginThemeResponse)
    )
)]
async fn get_login_theme(State(state): State<PublicApiState>) -> Json<LoginThemeResponse> {
    Json(load_login_theme(&state.config_repo).await)
}

/// The global login theme, shared by `GET /api/public/login-theme` and the
/// server-rendered `fc-web` login page. Any failure (missing row, bad JSON,
/// query error) falls back to the default theme.
pub async fn load_login_theme(config_repo: &PlatformConfigRepository) -> LoginThemeResponse {
    // Read from app_platform_configs: app_code="platform", section="login", property="theme", scope="GLOBAL"
    match config_repo
        .find_by_key("platform", "login", "theme", "GLOBAL", None)
        .await
    {
        Ok(Some(config)) => {
            tracing::debug!(value_len = config.value.len(), "Found login theme config");
            match serde_json::from_str::<LoginThemeResponse>(&config.value) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, value = %config.value, "Failed to parse login theme JSON");
                    LoginThemeResponse::default()
                }
            }
        }
        Ok(None) => {
            tracing::debug!("No login theme config found in DB");
            LoginThemeResponse::default()
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to query login theme config");
            LoginThemeResponse::default()
        }
    }
}

pub fn public_router(state: PublicApiState) -> Router {
    Router::new()
        .route("/platform", get(get_platform_info))
        .route("/login-theme", get(get_login_theme))
        .with_state(state)
}
