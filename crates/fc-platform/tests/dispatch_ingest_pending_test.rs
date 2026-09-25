//! Dispatch jobs created through the API start PENDING, as Go inserts them
//! (pipeline review C2): the scheduler only claims PENDING, so a job
//! inserted QUEUED was never dispatched. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde_json::json;

use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use support::{read_json, TestApp};

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_batched_dispatch_job_is_inserted_pending() {
    let app = TestApp::setup().await;
    let caller = Principal::new_user("anchor@flowcatalyst.test", UserScope::Anchor);
    let token = app
        .auth_service
        .generate_access_token_with_scope(
            &caller,
            &[permissions::admin::BATCH_DISPATCH_JOBS_WRITE.to_string()],
            None,
        )
        .expect("token");
    let item = json!({
        "source": "integral",
        "code": "t:outbox:job:run",
        "targetUrl": "https://receiver.example.test/hook",
        "payload": "{\"k\":\"v\"}",
        "dataOnly": true,
        "messageGroup": "t:outbox:1"
    });
    let (status, body) = read_json(
        app.post("/api/dispatch-jobs/batch", &token, json!({"items": [item]}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, queued_at, scheduled_for): (String, Option<DateTime<Utc>>, Option<DateTime<Utc>>) =
        sqlx::query_as(
            "SELECT status, queued_at, scheduled_for FROM msg_dispatch_jobs WHERE code = 't:outbox:job:run'",
        )
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "PENDING");
    assert_eq!(queued_at, None);
    assert_eq!(scheduled_for, None, "due at once");
}
