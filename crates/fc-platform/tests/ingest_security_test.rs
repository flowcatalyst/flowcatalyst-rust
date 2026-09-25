//! Ingest security: who may write events and dispatch jobs, under which
//! client, and signed by whom (docs/parity/java-2026-09-25-triage.md S1, S5,
//! S6; owner decision #24). Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use support::{read_json, TestApp};

const EVENTS_WRITE: &str = permissions::admin::BATCH_EVENTS_WRITE;
const JOBS_WRITE: &str = permissions::admin::BATCH_DISPATCH_JOBS_WRITE;

/// A token for `principal` granting exactly `perms` on its `scope` claim
/// (none: the principal's roles, of which it has none).
fn token_for(app: &TestApp, principal: &Principal, perms: &[&str]) -> String {
    let granted: Vec<String> = perms.iter().map(|p| p.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(principal, &granted, None)
        .expect("token")
}

fn anchor_user() -> Principal {
    Principal::new_user("anchor@flowcatalyst.test", UserScope::Anchor)
}

fn event_item(event_type: &str) -> Value {
    json!({"type": event_type, "source": "test", "data": {"k": "v"}})
}

fn job_item(code: &str) -> Value {
    json!({
        "code": code,
        "targetUrl": "https://receiver.example.test/hook",
        "payload": "{\"k\":\"v\"}",
        "serviceAccountId": "sac_unused"
    })
}

async fn post(app: &TestApp, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(app.post(path, token, body).await).await
}

async fn count(app: &TestApp, sql: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(sql)
        .fetch_one(&app.pool)
        .await
        .expect("count");
    n
}

// ── S1: the ingest permissions ──────────────────────────────────────────────

/// Every ingest route asks for Go's permission and answers Go's body without
/// it; the application-service `event:create` grant is not it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn ingest_routes_require_the_batch_write_permissions() {
    let app = TestApp::setup().await;
    let none = token_for(&app, &anchor_user(), &[]);
    let app_event_create = token_for(
        &app,
        &anchor_user(),
        &[permissions::application_service::EVENT_CREATE],
    );

    let event = json!({"eventType": "x:y:z:created", "source": "t", "data": {}});
    for (path, body, needs) in [
        (
            "/api/events/batch",
            json!({"items": [event_item("x:y:z:created")]}),
            EVENTS_WRITE,
        ),
        ("/api/events", event.clone(), EVENTS_WRITE),
        (
            "/api/dispatch-jobs/batch",
            json!({"items": [job_item("x:y:z:job")]}),
            JOBS_WRITE,
        ),
        ("/api/dispatch-jobs", job_item("x:y:z:job"), JOBS_WRITE),
    ] {
        for token in [&none, &app_event_create] {
            let (status, body) = post(&app, path, token, body.clone()).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
            assert_eq!(body["error"], "PERMISSION_REQUIRED", "{path}: {body}");
            assert_eq!(
                body["message"],
                format!("permission required: {needs}"),
                "{path}"
            );
        }
    }
    assert_eq!(count(&app, "SELECT COUNT(*) FROM msg_events").await, 0);
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM msg_dispatch_jobs").await,
        0
    );

    // With the permissions, the same requests are written.
    let ingest = token_for(&app, &anchor_user(), &[EVENTS_WRITE, JOBS_WRITE]);
    let (status, body) = post(
        &app,
        "/api/events/batch",
        &ingest,
        json!({"items": [event_item("x:y:z:created")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = post(
        &app,
        "/api/dispatch-jobs/batch",
        &ingest,
        json!({"items": [job_item("x:y:z:job")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
