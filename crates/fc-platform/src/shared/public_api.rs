//! /api/public Routes — Unauthenticated public endpoints

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
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
    /// The configured brand name (`platform` / `branding` /
    /// `platform-name`), `FlowCatalyst` when unset (Go
    /// `branding.PlatformName`).
    pub platform_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoginThemeResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand_subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_svg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_gradient: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_css: Option<String>,
}

/// `?clientId=<id>` (the admin preview) or `?client=<identifier>` (a
/// relying party's slug): whose CLIENT-scoped theme to layer over the
/// platform's.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginThemeQuery {
    pub client_id: Option<String>,
    pub client: Option<String>,
}

#[derive(Clone)]
pub struct PublicApiState {
    pub config_repo: Arc<PlatformConfigRepository>,
    /// Resolves `?client=<identifier>` on the login theme.
    pub client_repo: Arc<crate::ClientRepository>,
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
async fn get_platform_info(State(state): State<PublicApiState>) -> Json<PlatformInfoResponse> {
    let platform_name = crate::mfa::notify::PlatformName {
        configs: Some(state.config_repo.clone()),
    }
    .resolve()
    .await;
    Json(PlatformInfoResponse {
        features: FeaturesResponse {
            messaging_enabled: true,
        },
        platform_name,
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
async fn get_login_theme(
    State(state): State<PublicApiState>,
    Query(query): Query<LoginThemeQuery>,
) -> Json<LoginThemeResponse> {
    // Go handleLoginTheme: `clientId` first, else `client` resolved by
    // identifier; both cosmetic, so an unknown one yields the platform's.
    let mut client_id = query
        .client_id
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());
    if client_id.is_none() {
        if let Some(ident) = query
            .client
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            match state.client_repo.find_by_identifier(ident).await {
                Ok(found) => client_id = found.map(|c| c.id),
                Err(e) => {
                    tracing::warn!(identifier = %ident, error = %e, "login theme: client lookup failed")
                }
            }
        }
    }
    Json(load_client_login_theme(&state.config_repo, client_id.as_deref()).await)
}

/// The stored theme document at one scope, `None` on a miss, a failed
/// read, or a value that is not a JSON object.
async fn theme_document(
    config_repo: &PlatformConfigRepository,
    scope: &str,
    client_id: Option<&str>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    match config_repo
        .find_by_key("platform", "login", "theme", scope, client_id)
        .await
    {
        Ok(Some(config)) if !config.value.is_empty() => {
            match serde_json::from_str::<serde_json::Value>(&config.value) {
                Ok(serde_json::Value::Object(map)) => Some(map),
                _ => {
                    tracing::warn!(scope, "login theme: stored value is not a JSON object");
                    None
                }
            }
        }
        Ok(_) => None,
        Err(e) => {
            tracing::warn!(scope, error = %e, "login theme: lookup failed");
            None
        }
    }
}

/// The login theme: the GLOBAL row, with the CLIENT row for `client_id`
/// layered over it field by field (Go `loadLoginTheme`): a key the client
/// row omits keeps the platform value, an explicit `null` clears it.
pub async fn load_client_login_theme(
    config_repo: &PlatformConfigRepository,
    client_id: Option<&str>,
) -> LoginThemeResponse {
    let mut merged = theme_document(config_repo, "GLOBAL", None)
        .await
        .unwrap_or_default();
    if let Some(cid) = client_id {
        if let Some(overlay) = theme_document(config_repo, "CLIENT", Some(cid)).await {
            merged.extend(overlay);
        }
    }
    serde_json::from_value(serde_json::Value::Object(merged)).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "login theme: stored value does not fit the theme");
        LoginThemeResponse::default()
    })
}

/// The global login theme, shared by `GET /api/public/login-theme` and the
/// server-rendered `fc-web` login page. Any failure (missing row, bad JSON,
/// query error) falls back to the default theme.
pub async fn load_login_theme(config_repo: &PlatformConfigRepository) -> LoginThemeResponse {
    load_client_login_theme(config_repo, None).await
}

/// `/platform` alone, for Go's SPA-bootstrap alias `/api/config/platform`.
pub fn platform_info_router(state: PublicApiState) -> Router {
    Router::new()
        .route("/platform", get(get_platform_info))
        .with_state(state)
}

pub fn public_router(state: PublicApiState) -> Router {
    Router::new()
        .route("/platform", get(get_platform_info))
        .route("/login-theme", get(get_login_theme))
        .with_state(state)
}
