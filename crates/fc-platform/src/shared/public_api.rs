//! /api/public Routes — Unauthenticated public endpoints

use axum::{
    routing::get,
    Json, Router,
};
use serde::Serialize;
use utoipa::ToSchema;

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

#[derive(Debug, Serialize, ToSchema)]
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
async fn get_login_theme() -> Json<LoginThemeResponse> {
    Json(LoginThemeResponse {
        brand_name: None,
        brand_subtitle: None,
        logo_url: None,
        logo_svg: None,
        logo_height: None,
        primary_color: None,
        accent_color: None,
        background_color: None,
        background_gradient: None,
        footer_text: None,
        custom_css: None,
    })
}

pub fn public_router() -> Router {
    Router::new()
        .route("/platform", get(get_platform_info))
        .route("/login-theme", get(get_login_theme))
}
