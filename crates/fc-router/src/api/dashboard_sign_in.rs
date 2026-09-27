//! The dashboard's sign-in helpers (owner ruling 2; Java `DashboardSignIn`,
//! `7da64502`): authorization code with PKCE against the platform, through a
//! public OAuth client.
//!
//! - `GET {prefix}/dashboard/auth-config` tells the page where to send the
//!   browser. The authorize URL comes from the platform's discovery
//!   document, so it is the platform's external address, which the browser
//!   can reach even when the router reaches the platform over an internal
//!   alias.
//! - `POST {prefix}/dashboard/token` exchanges the code at the platform's
//!   token endpoint and answers only the access token and its lifetime. The
//!   router proxies because the page is on the router's origin, and a direct
//!   browser call would need CORS on the platform's token endpoint. The
//!   refresh and ID tokens are dropped; the page keeps the access token in
//!   memory only.
//!
//! Both routes are public: a signed-out browser needs them to sign in, and
//! neither carries router data. Sign-in is off (`enabled: false`, and the
//! exchange 404s) without `FC_ROUTER_DASHBOARD_CLIENT_ID`, without a
//! platform, or when the router is not guarded by platform tokens.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use fc_platform_jwks::JwksKeySource;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// What the dashboard asks for, and no more: a token held in a browser page
/// carries only what the router needs.
pub const SCOPE: &str = "platform:messaging:router:view platform:messaging:router:operate";

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    pub enabled: bool,
    pub authorization_endpoint: Option<String>,
    pub client_id: Option<String>,
    pub scope: Option<String>,
}

impl AuthConfig {
    fn off() -> Self {
        Self {
            enabled: false,
            authorization_endpoint: None,
            client_id: None,
            scope: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenRequest {
    code: Option<String>,
    code_verifier: Option<String>,
    redirect_uri: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

/// The sign-in helpers' configuration.
pub struct DashboardSignIn {
    /// The platform's discovery (shared with the token guard); `None` when
    /// the router has no platform, or is not guarded by platform tokens.
    discovery: Option<Arc<JwksKeySource>>,
    /// `FC_ROUTER_DASHBOARD_CLIENT_ID`; blank leaves sign-in off.
    client_id: String,
    http: reqwest::Client,
}

impl DashboardSignIn {
    pub fn new(discovery: Option<Arc<JwksKeySource>>, client_id: Option<&str>) -> Self {
        Self {
            discovery,
            client_id: client_id.unwrap_or_default().trim().to_string(),
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
        }
    }

    /// Sign-in off: the page falls back to Basic (dev mode) or no sign-in.
    pub fn off() -> Self {
        Self::new(None, None)
    }

    pub async fn config(&self) -> AuthConfig {
        let Some(discovery) = self.discovery.as_ref() else {
            return AuthConfig::off();
        };
        if self.client_id.is_empty() {
            return AuthConfig::off();
        }
        match discovery.authorization_endpoint().await {
            Some(authorize) => AuthConfig {
                enabled: true,
                authorization_endpoint: Some(authorize),
                client_id: Some(self.client_id.clone()),
                scope: Some(SCOPE.to_string()),
            },
            None => AuthConfig::off(),
        }
    }

    /// `GET /dashboard/auth-config` and `POST /dashboard/token`.
    pub fn routes(self: Arc<Self>) -> Router {
        Router::new()
            .route("/dashboard/auth-config", get(auth_config_handler))
            .route("/dashboard/token", post(token_handler))
            .with_state(self)
    }
}

async fn auth_config_handler(State(s): State<Arc<DashboardSignIn>>) -> Json<AuthConfig> {
    Json(s.config().await)
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

async fn token_handler(State(s): State<Arc<DashboardSignIn>>, body: axum::body::Bytes) -> Response {
    if !s.config().await.enabled {
        return error(StatusCode::NOT_FOUND, "dashboard sign-in is not configured");
    }
    let Some(discovery) = s.discovery.as_ref() else {
        return error(StatusCode::NOT_FOUND, "dashboard sign-in is not configured");
    };
    let Ok(req) = serde_json::from_slice::<TokenRequest>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "body must be {code, codeVerifier, redirectUri}",
        );
    };
    let present = |v: &Option<String>| v.as_deref().is_some_and(|v| !v.trim().is_empty());
    if !present(&req.code) || !present(&req.code_verifier) || !present(&req.redirect_uri) {
        return error(
            StatusCode::BAD_REQUEST,
            "code, codeVerifier and redirectUri are required",
        );
    }
    // The public client: no secret.
    let form = [
        ("grant_type", "authorization_code"),
        ("code", req.code.as_deref().unwrap_or_default()),
        (
            "code_verifier",
            req.code_verifier.as_deref().unwrap_or_default(),
        ),
        (
            "redirect_uri",
            req.redirect_uri.as_deref().unwrap_or_default(),
        ),
        ("client_id", s.client_id.as_str()),
    ];
    let url = format!("{}/oauth/token", discovery.platform_url());
    let response = match s.http.post(&url).form(&form).send().await {
        Ok(response) => response,
        Err(e) => {
            warn!(platform_url = %discovery.platform_url(), error = %e,
                "router dashboard: the platform's token endpoint could not be reached");
            return error(
                StatusCode::BAD_GATEWAY,
                "the platform's token endpoint could not be reached",
            );
        }
    };
    let status = response.status();
    let body: Option<serde_json::Value> = response
        .text()
        .await
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let access_token = body
        .as_ref()
        .and_then(|b| b.get("access_token"))
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty());
    match (status == reqwest::StatusCode::OK, access_token) {
        (true, Some(token)) => Json(TokenResponse {
            access_token: token.to_string(),
            expires_in: body
                .as_ref()
                .and_then(|b| b.get("expires_in"))
                .and_then(|e| e.as_i64())
                .unwrap_or(0),
        })
        .into_response(),
        _ => {
            // The platform's OAuth error code (invalid_grant, ...) is safe to
            // pass on; its description is not needed by the page.
            let code = body
                .as_ref()
                .and_then(|b| b.get("error"))
                .and_then(|e| e.as_str())
                .unwrap_or("token exchange failed");
            let status = if status.is_client_error() {
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
            } else {
                StatusCode::BAD_GATEWAY
            };
            error(status, code)
        }
    }
}
