//! The delivery policy (`fc_common::netguard`, Go's `internal/netguard`) on
//! the routes that write a URL the platform will POST to: a loopback,
//! link-local or private target is refused when it is written, and the
//! dispatch job ingest routes check the caller's permission before they read
//! the body, as Go's chi-style handlers do. Requires Docker:
//!   cargo test -p fc-platform --test it delivery_policy_test:: -- --ignored

use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use support::{read_json, TestApp};

/// The targets the default (strict) policy refuses, each with why.
const FORBIDDEN_TARGETS: [&str; 7] = [
    "http://127.0.0.1/hook",
    "http://localhost:8080/hook",
    "http://[::1]/hook",
    "http://169.254.169.254/latest/meta-data",
    "http://10.0.0.5/hook",
    "ftp://example.com/hook",
    "not a url",
];

fn token_with(app: &TestApp, permission: &str) -> String {
    let caller = Principal::new_user("anchor@flowcatalyst.test", UserScope::Anchor);
    app.auth_service
        .generate_access_token_with_scope(&caller, &[permission.to_string()], None)
        .expect("token")
}

fn ingest_token(app: &TestApp) -> String {
    token_with(app, permissions::admin::BATCH_DISPATCH_JOBS_WRITE)
}

fn job(code: &str, target: &str) -> Value {
    json!({
        "code": code,
        "targetUrl": target,
        "payload": "{\"k\":\"v\"}",
        "serviceAccountId": "sac_nobody",
    })
}

async fn job_count(app: &TestApp) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM msg_dispatch_jobs")
        .fetch_one(&app.pool)
        .await
        .expect("count")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_single_create_refuses_a_target_the_policy_forbids() {
    let app = TestApp::setup().await;
    let token = ingest_token(&app);

    for target in FORBIDDEN_TARGETS {
        let (status, body) = read_json(
            app.post(
                "/api/dispatch-jobs",
                &token,
                job("t:policy:job:one", target),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{target}: {body}");
        assert_eq!(body["error"], "INVALID_TARGET_URL", "{target}: {body}");
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|m| m.starts_with("targetUrl ")),
            "{target}: {body}"
        );
    }
    assert_eq!(job_count(&app).await, 0, "nothing was written");

    // A public host still creates.
    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs",
            &token,
            job("t:policy:job:ok", "https://receiver.example.test/hook"),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(job_count(&app).await, 1);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_single_create_requires_a_code_and_a_target() {
    let app = TestApp::setup().await;
    let token = ingest_token(&app);

    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs",
            &token,
            job("", "https://receiver.example.test/hook"),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "VALIDATION");
    assert_eq!(body["message"], "code is required");

    let (status, body) = read_json(
        app.post("/api/dispatch-jobs", &token, job("t:policy:job:x", ""))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "VALIDATION");
    assert_eq!(body["message"], "targetUrl is required");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn one_forbidden_target_refuses_the_whole_batch() {
    let app = TestApp::setup().await;
    let token = ingest_token(&app);

    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/batch",
            &token,
            json!({"items": [
                job("t:policy:job:a", "https://receiver.example.test/a"),
                job("t:policy:job:b", "http://169.254.169.254/latest/meta-data"),
            ]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_TARGET_URL");
    assert_eq!(job_count(&app).await, 0, "the good item was not written");

    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/batch",
            &token,
            json!({"items": [
                job("t:policy:job:a", "https://receiver.example.test/a"),
                job("t:policy:job:b", "https://receiver.example.test/b"),
            ]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(job_count(&app).await, 2);
}

/// Go checks the ingest permission, then decodes the body: a caller without
/// the permission is told 403 whatever it sent, and one with it gets a 400
/// `INVALID_JSON` for a body that is not the request.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_ingest_routes_check_the_permission_before_the_body() {
    let app = TestApp::setup().await;
    let without = token_with(&app, permissions::admin::CLIENT_READ);
    let with = ingest_token(&app);

    for path in ["/api/dispatch-jobs", "/api/dispatch-jobs/batch"] {
        let (status, body) =
            read_json(app.post(path, &without, json!("not a request")).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");

        let (status, body) = read_json(app.post(path, &with, json!("not a request")).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert_eq!(body["error"], "INVALID_JSON", "{path}: {body}");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_subscription_endpoint_the_policy_forbids_is_refused() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;

    for endpoint in FORBIDDEN_TARGETS {
        let (status, body) = read_json(
            app.post(
                "/api/subscriptions",
                &admin,
                json!({
                    "code": "policy-sub",
                    "name": "Policy",
                    "endpoint": endpoint,
                    "eventTypes": [{"eventTypeCode": "zzz:a:b:c"}],
                }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{endpoint}: {body}");
        assert_eq!(body["error"], "INVALID_ENDPOINT", "{endpoint}: {body}");
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|m| m.starts_with("endpoint ")),
            "{endpoint}: {body}"
        );
    }
}
