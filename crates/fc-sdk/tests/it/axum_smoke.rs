//! End-to-end smoke test for the Axum auth integration.
//!
//! Spins up a Tokio test server, registers the auth router + middleware,
//! and exercises the routes that don't need a live OIDC issuer:
//!
//!   - GET  /auth/login    — 302 to issuer's authorize URL + state cookie
//!   - GET  /auth/callback — 400 when state cookie is missing/invalid
//!   - POST /auth/logout   — clears the session cookie
//!
//! Full token-exchange flow (the equivalent of the TS `plugin.test.ts`
//! with an in-process mock issuer) is a deliberate follow-up — the
//! constituent pieces (`TokenValidator`, `OAuthClient`, the RBAC catalogue,
//! crypto, cookie store, principal extractor) are each covered by unit
//! tests in the library.

#![cfg(feature = "axum")]

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use fc_sdk::auth::axum::{
    AuthMechanism, AuthRoutes, FlowcatalystAuthBuilder, RbacBuilder, fc_auth_middleware,
};
use fc_sdk::auth::oauth::{OAuthClient, OAuthConfig};
use fc_sdk::auth::{TokenValidator, TokenValidatorConfig};
use tower::ServiceExt;

fn build_app() -> Router {
    let validator = Arc::new(TokenValidator::new(TokenValidatorConfig {
        issuer_url: "http://127.0.0.1:1/never-resolved".into(),
        audience: "flowcatalyst".into(),
        ..Default::default()
    }));
    let oauth = Arc::new(OAuthClient::new(OAuthConfig {
        issuer_url: "http://127.0.0.1:1/never-resolved".into(),
        client_id: "clt_test".into(),
        client_secret: Some("secret".into()),
        redirect_uri: "http://localhost:4000/auth/callback".into(),
        ..Default::default()
    }));
    let rbac = RbacBuilder::new()
        .role("admin")
        .grants(["billing:*"])
        .build();

    let (state, auth_router) = FlowcatalystAuthBuilder::new(validator, oauth)
        .cookie_secret([fc_sdk::auth::axum::generate_session_secret()])
        .rbac(rbac)
        .routes(AuthRoutes::default())
        .build()
        .expect("auth builder");

    Router::new()
        .merge(auth_router)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            fc_auth_middleware,
        ))
        .layer(axum::Extension(state))
}

#[tokio::test]
async fn login_redirects_to_issuer_with_state_cookie() {
    let app = build_app();
    let res = app
        .oneshot(
            Request::builder()
                .uri("/auth/login?returnTo=%2Fdashboard")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let location = res
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        location.contains("/oauth/authorize"),
        "expected authorize url, got {location}"
    );
    assert!(location.contains("state="));
    assert!(location.contains("code_challenge="));

    let set_cookie = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        set_cookie.starts_with("fc_oauth_state="),
        "expected state cookie, got {set_cookie}"
    );
}

#[tokio::test]
async fn callback_rejects_when_state_cookie_missing() {
    let app = build_app();
    let res = app
        .oneshot(
            Request::builder()
                .uri("/auth/callback?code=abc&state=xyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn logout_returns_no_content_and_clears_cookie() {
    let app = build_app();
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let set_cookie = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        set_cookie.starts_with("fc_session=") && set_cookie.contains("Max-Age=0"),
        "expected cleared session cookie, got {set_cookie}"
    );
}

#[test]
fn mechanism_serializes_as_expected() {
    let s = serde_json::to_string(&AuthMechanism::Bearer).unwrap();
    assert_eq!(s, "\"bearer\"");
    let s2 = serde_json::to_string(&AuthMechanism::Session).unwrap();
    assert_eq!(s2, "\"session\"");
}
