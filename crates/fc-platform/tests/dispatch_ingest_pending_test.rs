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

/// A directly created job carries its own queue priority, as Go's
/// `jobFromItem` (`dispatchqueue.Parse`): matched ignoring case and stored
/// canonical, blank left unset (the subscription's then applies), anything
/// else 400 `INVALID_QUEUE` with nothing written.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_created_job_carries_its_own_queue_priority() {
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
    let item = |code: &str, queue: Option<&str>| {
        let mut item = json!({
            "source": "integral",
            "code": code,
            "targetUrl": "https://receiver.example.test/hook",
            "payload": "{}",
            "serviceAccountId": "sac_nobody",
        });
        if let Some(q) = queue {
            item["queue"] = json!(q);
        }
        item
    };

    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs",
            &token,
            item("t:q:job:single", Some(" high_priority ")),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/batch",
            &token,
            json!({"items": [
                item("t:q:job:default", Some("Default")),
                item("t:q:job:blank", Some("  ")),
                item("t:q:job:absent", None),
            ]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    for (path, body) in [
        ("/api/dispatch-jobs", item("t:q:job:bad", Some("URGENT"))),
        (
            "/api/dispatch-jobs/batch",
            json!({"items": [item("t:q:job:ok", None), item("t:q:job:bad", Some("URGENT"))]}),
        ),
    ] {
        let (status, body) = read_json(app.post(path, &token, body).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert_eq!(body["error"], "INVALID_QUEUE", "{path}: {body}");
    }

    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT code, queue FROM msg_dispatch_jobs ORDER BY code")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            ("t:q:job:absent".to_string(), None),
            ("t:q:job:blank".to_string(), None),
            ("t:q:job:default".to_string(), Some("DEFAULT".to_string())),
            (
                "t:q:job:single".to_string(),
                Some("HIGH_PRIORITY".to_string())
            ),
        ]
    );
}
