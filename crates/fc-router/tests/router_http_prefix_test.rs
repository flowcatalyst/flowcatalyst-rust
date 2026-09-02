//! HTTP-level tests for `FC_ROUTER_HTTP_PREFIX` route nesting
//! (`create_router_with_options`'s `router_http_prefix` parameter).
//!
//! Drop-in-compat requirement (see `docs/router-specification.md` §10):
//! when a prefix is configured, the *entire* route tree — public and
//! protected — must answer BOTH at root (today's default) and nested
//! under the prefix, with the public/protected split unaffected by
//! nesting: public routes (health, metrics) stay auth-free under the
//! prefix too, and protected routes (monitoring) stay guarded under the
//! prefix too.
//!
//! No queue/mediator machinery is needed here — every case only touches
//! health/monitoring endpoints — so the harness below is deliberately
//! bare (a manager with zero pools/queues is enough).

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use fc_common::{MediationOutcome, Message};
use fc_queue::QueuePublisher;
use fc_router::{
    api::{create_router_with_options, AuthConfig, AuthState},
    HealthService, HealthServiceConfig, Mediator, QueueManager, WarningService,
    WarningServiceConfig,
};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

struct NoOpPublisher;

#[async_trait]
impl QueuePublisher for NoOpPublisher {
    fn identifier(&self) -> &str {
        "noop"
    }
    async fn publish(&self, _message: Message) -> fc_queue::Result<String> {
        Ok("noop".to_string())
    }
    async fn publish_batch(&self, messages: Vec<Message>) -> fc_queue::Result<Vec<String>> {
        Ok(messages.iter().map(|_| "noop".to_string()).collect())
    }
}

struct NoOpMediator;

#[async_trait]
impl Mediator for NoOpMediator {
    async fn mediate(&self, _message: &Message) -> MediationOutcome {
        MediationOutcome::success(200)
    }
}

/// Build a router with (optionally) a prefix and (optionally) BasicAuth,
/// no queues/pools required for the health/monitoring surface under test.
async fn build_app(router_http_prefix: Option<&str>, auth_state: Option<AuthState>) -> axum::Router {
    let publisher: Arc<dyn QueuePublisher> = Arc::new(NoOpPublisher);
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        Arc::new(NoOpMediator) as Arc<dyn Mediator>,
    ));
    let warnings = Arc::new(WarningService::new(WarningServiceConfig::default()));
    let health = Arc::new(HealthService::new(HealthServiceConfig::default(), warnings.clone()));
    let breakers = manager.circuit_breaker_registry().clone();

    create_router_with_options(
        publisher,
        manager,
        warnings,
        health,
        breakers,
        false,
        "default".to_string(),
        None,
        None,
        None,
        auth_state,
        router_http_prefix.map(str::to_string),
    )
}

async fn get(app: &axum::Router, path: &str) -> StatusCode {
    get_with_auth(app, path, None).await.0
}

async fn get_with_auth(
    app: &axum::Router,
    path: &str,
    basic: Option<(&str, &str)>,
) -> (StatusCode, String) {
    let mut req = Request::builder().uri(path);
    if let Some((user, pass)) = basic {
        let creds = BASE64.encode(format!("{user}:{pass}"));
        req = req.header("authorization", format!("Basic {creds}"));
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

// ---------------------------------------------------------------------
// No prefix configured: today's root-only behaviour is unchanged, and
// the prefix path must NOT exist.
// ---------------------------------------------------------------------

#[tokio::test]
async fn no_prefix_serves_root_only() {
    let app = build_app(None, None).await;

    assert_eq!(get(&app, "/health/live").await, StatusCode::OK);
    assert_eq!(get(&app, "/monitoring").await, StatusCode::OK);
    assert_eq!(
        get(&app, "/router/health/live").await,
        StatusCode::NOT_FOUND,
        "no nesting happens unless FC_ROUTER_HTTP_PREFIX is set"
    );
}

// ---------------------------------------------------------------------
// Prefix configured, no auth: both root AND nested paths answer.
// ---------------------------------------------------------------------

#[tokio::test]
async fn prefix_serves_health_and_monitoring_at_both_root_and_nested() {
    let app = build_app(Some("/router"), None).await;

    for path in ["/health/live", "/health/ready", "/metrics", "/monitoring"] {
        assert_eq!(get(&app, path).await, StatusCode::OK, "root path {path}");
        let nested = format!("/router{path}");
        assert_eq!(get(&app, &nested).await, StatusCode::OK, "nested path {nested}");
    }
}

#[tokio::test]
async fn prefix_without_leading_slash_is_normalized() {
    let app = build_app(Some("router"), None).await;
    assert_eq!(get(&app, "/router/health/live").await, StatusCode::OK);
}

#[tokio::test]
async fn prefix_with_trailing_slash_is_normalized() {
    let app = build_app(Some("/router/"), None).await;
    assert_eq!(get(&app, "/router/health/live").await, StatusCode::OK);
    // No double-slash route.
    assert_eq!(get(&app, "/router//health/live").await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn blank_or_root_prefix_disables_nesting() {
    for prefix in ["", "   ", "/"] {
        let app = build_app(Some(prefix), None).await;
        assert_eq!(get(&app, "/health/live").await, StatusCode::OK);
        // Nothing nested under any prefix-shaped path.
        assert_eq!(get(&app, "/router/health/live").await, StatusCode::NOT_FOUND);
    }
}

// ---------------------------------------------------------------------
// Prefix + BasicAuth: the public/protected split must hold at BOTH
// mount points. This is the sharpest edge in Go's own history (R-43:
// mountRelativePath) — pin it here for the Rust nesting design too.
// ---------------------------------------------------------------------

#[tokio::test]
async fn prefix_with_auth_keeps_public_routes_open_at_both_mounts() {
    let auth = fc_router::api::create_auth_state(AuthConfig::basic("u", "p"));
    let app = build_app(Some("/router"), Some(auth)).await;

    for path in ["/health/live", "/health/ready", "/metrics"] {
        let (status, _) = get_with_auth(&app, path, None).await;
        assert_eq!(status, StatusCode::OK, "root public path {path} must be open");

        let nested = format!("/router{path}");
        let (status, _) = get_with_auth(&app, &nested, None).await;
        assert_eq!(status, StatusCode::OK, "nested public path {nested} must be open");
    }
}

#[tokio::test]
async fn prefix_with_auth_guards_protected_routes_at_both_mounts() {
    let auth = fc_router::api::create_auth_state(AuthConfig::basic("u", "p"));
    let app = build_app(Some("/router"), Some(auth)).await;

    // No credentials: 401 at both root and nested.
    let (status, _) = get_with_auth(&app, "/monitoring", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "root protected path with no creds");
    let (status, _) = get_with_auth(&app, "/router/monitoring", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "nested protected path with no creds");

    // Wrong credentials: still 401 at both.
    let (status, _) = get_with_auth(&app, "/monitoring", Some(("u", "wrong"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "root protected path with wrong creds");
    let (status, _) = get_with_auth(&app, "/router/monitoring", Some(("u", "wrong"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "nested protected path with wrong creds");

    // Valid credentials: 200 at both.
    let (status, _) = get_with_auth(&app, "/monitoring", Some(("u", "p"))).await;
    assert_eq!(status, StatusCode::OK, "root protected path with valid creds");
    let (status, _) = get_with_auth(&app, "/router/monitoring", Some(("u", "p"))).await;
    assert_eq!(status, StatusCode::OK, "nested protected path with valid creds");
}
