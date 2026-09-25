//! Smoke test for the shared test harness. Verifies TestApp::setup()
//! produces a working router and that token generation + a trivial GET
//! both succeed. All tests here are `#[ignore]` since they require Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use support::{assert_status, TestApp};

#[tokio::test]
#[ignore = "requires Docker"]
async fn harness_boots_and_serves_health() {
    let app = TestApp::setup().await;
    // `/q/healthz` is the public health endpoint exposed by the platform router.
    let resp = app.get_unauth("/health").await;
    let _ = assert_status(resp, StatusCode::OK).await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn anchor_token_authorizes_list_clients() {
    let app = TestApp::setup().await;
    // Anchor reach and the client read permission (Go's anchorWith).
    let token = app.anchor_admin_token().await;
    let resp = app.get("/api/clients", &token).await;
    // 200 OK with an empty list is fine; we just want to confirm auth
    // passes end-to-end.
    assert!(
        resp.status() == StatusCode::OK,
        "expected 200 OK for authed /api/clients, got {}",
        resp.status()
    );
}
