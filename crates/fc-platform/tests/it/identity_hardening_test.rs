//! Identity-review fixes (docs/go-mirror/2026-10-19-identity-fixes-and-catch-up.md):
//! one regression test per finding that needs a running platform. Requires a
//! Postgres (see `support::db`).

use crate::support;

use axum::body::Body;
use axum::http::{Request, Response};
use fc_platform::auth::password_service::PasswordService;
use fc_platform::domain::{Principal, UserScope};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use support::TestApp;
use tower::ServiceExt;

async fn post_json(app: &TestApp, path: &str, body: Value) -> Response<Body> {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.router.clone().oneshot(req).await.unwrap()
}

/// R2: a refused login for an email that is not registered spends the
/// Argon2id work a wrong password for a real account spends, so response time
/// does not enumerate emails. Before the fix the unknown email answered in
/// microseconds. The bound is loose (half) to stay stable on a busy machine.
#[tokio::test]
#[ignore = "requires Docker"]
async fn unknown_email_login_costs_what_a_wrong_password_costs() {
    let app = TestApp::setup().await;
    let mut user = Principal::new_user("timing@example.test", UserScope::Anchor);
    if let Some(identity) = user.user_identity.as_mut() {
        identity.password_hash = Some(
            PasswordService::default()
                .hash_password("Pw-1234567!x")
                .unwrap(),
        );
    }
    app.repos.principal_repo.insert(&user).await.unwrap();

    // Warm the dummy hash and the connection pool.
    post_json(
        &app,
        "/auth/login",
        json!({"email": "warm@example.test", "password": "x"}),
    )
    .await;

    // One attempt per distinct email: the per-(email, IP) backoff gives three
    // free failures, and each distinct email keeps this clear of it.
    async fn time(app: &TestApp, email: &str) -> Duration {
        let t = Instant::now();
        let resp = post_json(
            app,
            "/auth/login",
            json!({"email": email, "password": "Wrong-1234567!x"}),
        )
        .await;
        assert_eq!(resp.status(), 401);
        t.elapsed()
    }
    let known = time(&app, "timing@example.test").await;
    let mut unknown = Vec::new();
    for i in 0..3 {
        unknown.push(time(&app, &format!("nobody{i}@example.test")).await);
    }
    unknown.sort();
    let unknown = unknown[1];
    assert!(
        unknown * 2 > known,
        "unknown email answered in {unknown:?}, a real account's wrong password in {known:?}"
    );
}
