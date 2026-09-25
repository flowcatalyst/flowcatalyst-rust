//! S2.7: `POST /auth/password-reset/request` is budgeted per address as
//! well as per IP, through the real distributed store, and the answer over
//! budget is exactly the handler's own, so the limit can't tell a known
//! address from an unknown one.

#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use fc_platform::domain::{Principal, UserScope};
use fc_platform::shared::rate_limit_store::PostgresRateLimitStore;
use support::TestApp;

async fn request_reset(app: &TestApp, email: &str) -> (StatusCode, String) {
    let req = Request::post("/auth/password-reset/request")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "email": email }).to_string()))
        .unwrap();
    let res = app.router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let body = axum::body::to_bytes(res.into_body(), 64 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn password_reset_email_budget_is_silent() {
    // The default budget: 5 requests per address per hour.
    std::env::remove_var("FC_RL_PASSWORD_RESET_EMAIL_PER_HOUR");
    let app = TestApp::setup_with_rate_limit_store(|pool| {
        Arc::new(PostgresRateLimitStore::new(pool.clone()))
    })
    .await;
    let known = "reset-me@flowcatalyst.test";
    app.repos
        .principal_repo
        .insert(&Principal::new_user(known, UserScope::Anchor))
        .await
        .expect("insert principal");

    let unknown = request_reset(&app, "nobody@flowcatalyst.test").await;
    assert_eq!(unknown.0, StatusCode::OK);
    let mut answers = Vec::new();
    for _ in 0..7 {
        answers.push(request_reset(&app, known).await);
    }
    for answer in &answers {
        assert_eq!(answer, &unknown, "every answer is the handler's own");
    }

    let issued = app
        .event_count_by_type("platform:iam:user:password-reset-requested")
        .await;
    assert_eq!(issued, 5, "past the budget nothing is issued");
}
