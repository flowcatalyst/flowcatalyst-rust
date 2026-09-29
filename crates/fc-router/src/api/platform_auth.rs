//! The router API's guard (owner ruling 2 of 2026-09-25, adopted for Rust as
//! decision #21; Java `docs/spec/router-api-auth.md`, `94b8c7bc`):
//! a platform-issued bearer token, verified against the platform's JWKS, with
//! `token_use = api`, and the permission the route needs.
//!
//! - `GET`/`HEAD`, and the in-flight check done over `POST`, need
//!   [`ROUTER_VIEW`]: the SDKs' stuck-message recovery calls these. The
//!   exception is the task dump (`GET /diagnostics/task-dump`), which
//!   needs [`ROUTER_OPERATE`]: it pauses the runtime.
//! - Every other method needs [`ROUTER_OPERATE`]: publish, breaker resets,
//!   in-flight ACK, group-flush clear, pool update, config reload, warning
//!   acknowledge and clear, broker-stats refresh.
//!
//! A missing or bad token is 401 with `WWW-Authenticate: Bearer` and
//! `X-Auth-Mode: BEARER` (which the dashboard reads to start its sign-in);
//! a valid token without the permission is 403 `PERMISSION_REQUIRED`. With
//! no platform to verify against, every protected route answers 401: it
//! fails closed, never open.
//!
//! Which guard a router gets is [`resolve`]'s decision, including the
//! transitional `AUTH_MODE=NONE` (decision #43).

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use fc_platform_jwks::clock;
use fc_platform_jwks::{BearerAuthenticator, JwksKeySource};
use reqwest::redirect::Policy;
use std::time::Duration;
use tracing::debug;

/// Monitoring reads.
pub const ROUTER_VIEW: &str = "platform:messaging:router:view";
/// Every operator action.
pub const ROUTER_OPERATE: &str = "platform:messaging:router:operate";

/// The realm the bearer challenge names.
pub const REALM: &str = "FlowCatalyst Router";

/// Reads done over `POST`: their body is too large for a query string.
const POST_READS: &[&str] = &["/monitoring/in-flight-messages/check-batch"];

/// `GET`s that need [`ROUTER_OPERATE`]: a task dump pauses the runtime's
/// workers while it walks every task.
const OPERATE_READS: &[&str] = &["/diagnostics/task-dump"];

/// What `AUTH_MODE=NONE` outside dev mode says, at startup and on the
/// health and monitoring output (decision #43).
pub const UNAUTHENTICATED_WARNING: &str =
    "router API unauthenticated; remove AUTH_MODE=NONE once SDKs send the platform bearer";

/// The permission a request needs (rule 2): reads need `view`, everything
/// else `operate`. `path` is relative to the router's mount.
pub fn required_permission(method: &Method, path: &str) -> &'static str {
    let read = ((method == Method::GET || method == Method::HEAD)
        && !OPERATE_READS.contains(&path))
        || (method == Method::POST && POST_READS.contains(&path));
    if read {
        ROUTER_VIEW
    } else {
        ROUTER_OPERATE
    }
}

/// Platform bearer-token verification for the router's API.
#[derive(Clone)]
pub struct PlatformAuth {
    /// `None` when there is no platform to verify against: every protected
    /// route answers 401.
    authenticator: Option<Arc<BearerAuthenticator>>,
}

impl PlatformAuth {
    /// Verifies against the platform at `platform_url`; `None` (or blank)
    /// refuses every token.
    pub fn new(platform_url: Option<&str>) -> Self {
        let authenticator = platform_url
            .map(|u| u.trim().trim_end_matches('/'))
            .filter(|u| !u.is_empty())
            .map(|url| {
                let http = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(5))
                    .redirect(Policy::none())
                    .build()
                    .unwrap_or_default();
                let keys = Arc::new(JwksKeySource::new(http, url.to_string(), clock::system()));
                Arc::new(BearerAuthenticator::new(keys, clock::system()))
            });
        Self { authenticator }
    }

    /// Whether there is a platform to verify against.
    pub fn is_verifying(&self) -> bool {
        self.authenticator.is_some()
    }

    /// The platform's discovery and keys (shared with the dashboard
    /// sign-in, whose authorize URL is in the same document).
    pub fn key_source(&self) -> Option<Arc<JwksKeySource>> {
        self.authenticator.as_ref().map(|a| a.key_source().clone())
    }
}

/// The guard itself; layered on the protected routes only, so the public
/// routes (health, metrics, the dashboard page and its sign-in helpers)
/// never reach it.
pub async fn platform_auth_middleware(
    State(auth): State<PlatformAuth>,
    request: Request,
    next: Next,
) -> Response {
    let Some(authenticator) = auth.authenticator.as_ref() else {
        return unauthorized(
            "router API authentication is not configured: no platform to verify tokens against",
        );
    };
    let authorization = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());
    let claims: fc_platform_jwks::TokenClaims =
        match authenticator.authenticate(authorization).await {
            Ok(claims) => claims,
            Err(reason) => {
                debug!(reason = %reason, "router API: bearer token refused");
                return unauthorized(&reason);
            }
        };
    // A session token (no `token_use`) or an identity token is not an API
    // credential here, the same rule as the platform's own API.
    if !claims.is_api_token() {
        return unauthorized("an API access token is required");
    }
    let required = required_permission(request.method(), request.uri().path());
    if !claims.grants(required) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "PERMISSION_REQUIRED",
                "message": format!("{required} required"),
            })),
        )
            .into_response();
    }
    next.run(request).await
}

fn unauthorized(reason: &str) -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "UNAUTHORIZED", "message": reason })),
    )
        .into_response();
    let headers = response.headers_mut();
    if let Ok(challenge) = HeaderValue::from_str(&format!("Bearer realm=\"{REALM}\"")) {
        headers.insert(header::WWW_AUTHENTICATE, challenge);
    }
    headers.insert(
        HeaderName::from_static("x-auth-mode"),
        HeaderValue::from_static("BEARER"),
    );
    response
}

// ── Which guard ─────────────────────────────────────────────────────────────

/// What the choice of guard depends on.
#[derive(Debug, Clone, Default)]
pub struct RouterAuthSettings {
    /// `FLOWCATALYST_DEV_MODE`.
    pub dev_mode: bool,
    /// Raw `AUTH_MODE`.
    pub auth_mode: Option<String>,
    /// `FC_ROUTER_AUTH_USER` (alias `AUTH_BASIC_USERNAME`).
    pub basic_user: Option<String>,
    /// `FC_ROUTER_AUTH_PASS` (alias `AUTH_BASIC_PASSWORD`).
    pub basic_password: Option<String>,
}

impl RouterAuthSettings {
    /// Reads `AUTH_MODE` and the Basic credentials through `get` (the first
    /// non-empty of each alias pair).
    pub fn from_lookup(dev_mode: bool, get: impl Fn(&str) -> Option<String>) -> Self {
        let first = |names: &[&str]| names.iter().filter_map(|n| get(n)).find(|v| !v.is_empty());
        Self {
            dev_mode,
            auth_mode: get("AUTH_MODE"),
            basic_user: first(&["FC_ROUTER_AUTH_USER", "AUTH_BASIC_USERNAME"]),
            basic_password: first(&["FC_ROUTER_AUTH_PASS", "AUTH_BASIC_PASSWORD"]),
        }
    }
}

/// The guard [`resolve`] picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterAuthChoice {
    /// No credential. Dev mode with `AUTH_MODE=NONE` (or nothing set); or,
    /// outside dev mode, the transitional `AUTH_MODE=NONE` of decision #43
    /// (`transitional`), which is warned about loudly.
    Open { transitional: bool },
    /// Dev mode only: §9.7's Basic auth, or the historical OIDC modes
    /// (`AuthConfig`, unchanged).
    Legacy,
    /// Platform bearer tokens.
    PlatformBearer,
}

/// The decision, with the settings it ignored (each WARNed at startup).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterAuthDecision {
    pub choice: RouterAuthChoice,
    pub ignored: Vec<&'static str>,
}

impl RouterAuthDecision {
    /// The warning the health and monitoring output carry, if any.
    pub fn warning(&self) -> Option<&'static str> {
        matches!(self.choice, RouterAuthChoice::Open { transitional: true })
            .then_some(UNAUTHENTICATED_WARNING)
    }
}

/// Owner ruling 2 with decision #43's transition:
///
/// - **dev mode** keeps §9.7 exactly (Basic when a user is set or
///   `AUTH_MODE=BASIC`, the OIDC modes as before, open for `NONE` or
///   nothing), and `AUTH_MODE=BEARER` opts into platform tokens;
/// - **elsewhere**, `AUTH_MODE` unset, `BEARER` or `OIDC` enforce the
///   platform bearer. `NONE` is honoured for now (decision #43: production's
///   task sets it and the apps' SDKs do not send router tokens yet) and
///   warned about. `BASIC`, `OIDC_FLOW` and any other value are ignored,
///   with a WARN, in favour of the platform bearer; so are
///   `FC_ROUTER_AUTH_USER` / `_PASS`.
pub fn resolve(s: &RouterAuthSettings) -> RouterAuthDecision {
    let set = |v: &Option<String>| v.as_deref().is_some_and(|v| !v.trim().is_empty());
    let mode = s
        .auth_mode
        .as_deref()
        .map(|m| m.trim().to_ascii_uppercase())
        .filter(|m| !m.is_empty());
    if s.dev_mode {
        let choice = match mode.as_deref() {
            Some("BEARER") => RouterAuthChoice::PlatformBearer,
            Some("NONE") => RouterAuthChoice::Open {
                transitional: false,
            },
            Some("BASIC" | "OIDC" | "OIDC_FLOW") => RouterAuthChoice::Legacy,
            // Unset or unrecognised: Basic when a user is configured (Go's
            // inference), else open.
            _ if set(&s.basic_user) => RouterAuthChoice::Legacy,
            _ => RouterAuthChoice::Open {
                transitional: false,
            },
        };
        return RouterAuthDecision {
            choice,
            ignored: Vec::new(),
        };
    }
    let mut ignored = Vec::new();
    let choice = match mode.as_deref() {
        Some("NONE") => RouterAuthChoice::Open { transitional: true },
        None | Some("BEARER" | "OIDC") => RouterAuthChoice::PlatformBearer,
        Some(_) => {
            ignored.push("AUTH_MODE");
            RouterAuthChoice::PlatformBearer
        }
    };
    if set(&s.basic_user) {
        ignored.push("FC_ROUTER_AUTH_USER");
    }
    if set(&s.basic_password) {
        ignored.push("FC_ROUTER_AUTH_PASS");
    }
    RouterAuthDecision { choice, ignored }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(dev_mode: bool, auth_mode: Option<&str>, user: Option<&str>) -> RouterAuthSettings {
        RouterAuthSettings {
            dev_mode,
            auth_mode: auth_mode.map(str::to_owned),
            basic_user: user.map(str::to_owned),
            basic_password: user.map(|_| "secret".to_owned()),
        }
    }

    #[test]
    fn reads_and_writes_need_view_and_operate() {
        assert_eq!(
            required_permission(&Method::GET, "/monitoring/health"),
            ROUTER_VIEW
        );
        assert_eq!(required_permission(&Method::HEAD, "/warnings"), ROUTER_VIEW);
        assert_eq!(
            required_permission(&Method::POST, "/monitoring/in-flight-messages/check-batch"),
            ROUTER_VIEW
        );
        for (method, path) in [
            (Method::POST, "/messages"),
            (Method::POST, "/monitoring/circuit-breakers/reset-all"),
            (Method::POST, "/monitoring/in-flight-messages/m1/ack"),
            (Method::POST, "/monitoring/group-flushes/p/g/clear"),
            (Method::PUT, "/monitoring/pools/p"),
            (Method::POST, "/config/reload"),
            (Method::POST, "/warnings/w1/acknowledge"),
            (Method::DELETE, "/warnings"),
            (Method::POST, "/monitoring/broker-stats/refresh"),
        ] {
            assert_eq!(
                required_permission(&method, path),
                ROUTER_OPERATE,
                "{method} {path}"
            );
        }
    }

    #[test]
    fn outside_dev_mode_the_platform_bearer_is_the_default() {
        for mode in [
            None,
            Some("BEARER"),
            Some("bearer"),
            Some(" OIDC "),
            Some(""),
        ] {
            let d = resolve(&settings(false, mode, None));
            assert_eq!(d.choice, RouterAuthChoice::PlatformBearer, "{mode:?}");
            assert!(d.ignored.is_empty(), "{mode:?}");
            assert_eq!(d.warning(), None);
        }
    }

    #[test]
    fn outside_dev_mode_none_is_honoured_and_warned_about() {
        let d = resolve(&settings(false, Some("none"), None));
        assert_eq!(d.choice, RouterAuthChoice::Open { transitional: true });
        assert_eq!(d.warning(), Some(UNAUTHENTICATED_WARNING));
    }

    #[test]
    fn outside_dev_mode_basic_is_ignored() {
        let d = resolve(&settings(false, Some("BASIC"), Some("admin")));
        assert_eq!(d.choice, RouterAuthChoice::PlatformBearer);
        assert_eq!(
            d.ignored,
            vec!["AUTH_MODE", "FC_ROUTER_AUTH_USER", "FC_ROUTER_AUTH_PASS"]
        );
        // A user alone (Go would have inferred Basic) no longer turns it on.
        let d = resolve(&settings(false, None, Some("admin")));
        assert_eq!(d.choice, RouterAuthChoice::PlatformBearer);
        assert_eq!(
            d.ignored,
            vec!["FC_ROUTER_AUTH_USER", "FC_ROUTER_AUTH_PASS"]
        );
        let d = resolve(&settings(false, Some("OIDC_FLOW"), None));
        assert_eq!(d.choice, RouterAuthChoice::PlatformBearer);
        assert_eq!(d.ignored, vec!["AUTH_MODE"]);
    }

    #[test]
    fn dev_mode_keeps_the_old_modes() {
        let open = RouterAuthChoice::Open {
            transitional: false,
        };
        assert_eq!(resolve(&settings(true, None, None)).choice, open);
        assert_eq!(
            resolve(&settings(true, Some("NONE"), Some("u"))).choice,
            open
        );
        assert_eq!(
            resolve(&settings(true, None, Some("u"))).choice,
            RouterAuthChoice::Legacy
        );
        assert_eq!(
            resolve(&settings(true, Some("BASIC"), Some("u"))).choice,
            RouterAuthChoice::Legacy
        );
        assert_eq!(
            resolve(&settings(true, Some("BEARER"), None)).choice,
            RouterAuthChoice::PlatformBearer
        );
        assert_eq!(resolve(&settings(true, None, None)).warning(), None);
    }
}
