//! A dispatch job's descriptor and the read projection's metadata (Go's
//! migration 057): the fan-out stores the raising subscription's name as the
//! job's descriptor and copies the raising event's context data onto its
//! metadata; a directly created job carries the descriptor and metadata it
//! is sent; the projector copies both to `msg_dispatch_jobs_read`; and every
//! dispatch-job read answers them in Go's shape. Requires Docker:
//!   cargo test -p fc-platform --test it dispatch_descriptor_test:: -- --ignored

use crate::support;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use fc_stream::health::StreamHealth;
use fc_stream::EventFanOutConfig;
use support::{read_json, TestApp};

/// Run the event fan-out, then the dispatch-job projector, each for a moment.
async fn fan_out_and_project(pool: &PgPool) {
    let cancel = CancellationToken::new();
    let fan_out = tokio::spawn(fc_stream::event_fan_out::run(
        pool.clone(),
        EventFanOutConfig {
            batch_size: 200,
            subscription_refresh: Duration::from_millis(100),
        },
        Arc::new(StreamHealth::new("event-fan-out".into())),
        cancel.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(10), fan_out)
        .await
        .expect("fan-out stops")
        .unwrap();
    project(pool).await;
}

async fn project(pool: &PgPool) {
    let cancel = CancellationToken::new();
    let projector = tokio::spawn(fc_stream::dispatch_job_projection::run(
        pool.clone(),
        200,
        Arc::new(StreamHealth::new("dispatch-job-projection".into())),
        cancel.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(10), projector)
        .await
        .expect("projector stops")
        .unwrap();
}

fn ingest_token(app: &TestApp) -> String {
    let caller = Principal::new_user("anchor@flowcatalyst.test", UserScope::Anchor);
    app.auth_service
        .generate_access_token_with_scope(
            &caller,
            &[permissions::admin::BATCH_DISPATCH_JOBS_WRITE.to_string()],
            None,
        )
        .expect("token")
}

/// The one element of `rows` whose `key` is `value`.
fn find<'a>(rows: &'a Value, key: &str, value: &str) -> &'a Value {
    rows.as_array()
        .expect("an array")
        .iter()
        .find(|r| r[key] == value)
        .unwrap_or_else(|| panic!("no row with {key} = {value}: {rows}"))
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_fanned_out_job_carries_its_subscription_name_and_its_events_context() {
    let app = TestApp::setup().await;
    let pool = &app.pool;

    // Two subscriptions on the same event type: one named (with space around
    // the name, which is trimmed), one whose name is blank (no descriptor).
    sqlx::query(
        "INSERT INTO msg_subscriptions (id, code, name, target, status, mode) VALUES \
         ('sub_notify', 'notify', '  Notify Value of user logins ', 'http://subscriber.test/a', 'ACTIVE', 'IMMEDIATE'), \
         ('sub_blank', 'blank', '   ', 'http://subscriber.test/b', 'ACTIVE', 'IMMEDIATE')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO msg_subscription_event_types (subscription_id, event_type_code) VALUES \
         ('sub_notify', 'value:iam:user:logged-in'), ('sub_blank', 'value:iam:user:logged-in')",
    )
    .execute(pool)
    .await
    .unwrap();
    // One event with context data, one without.
    sqlx::query(
        "INSERT INTO msg_events (id, type, source, time, data, context_data) VALUES \
         ('evtctx0000001', 'value:iam:user:logged-in', 'value', NOW(), '{\"u\":1}', \
          '[{\"key\":\"userId\",\"value\":\"u1\"},{\"key\":\"tenant\",\"value\":\"acme\"}]'), \
         ('evtbare000001', 'value:iam:user:logged-in', 'value', NOW(), '{\"u\":2}', NULL)",
    )
    .execute(pool)
    .await
    .unwrap();

    fan_out_and_project(pool).await;

    // The write rows.
    let rows: Vec<(String, String, Option<String>, Value)> = sqlx::query_as(
        "SELECT event_id, subscription_id, descriptor, metadata FROM msg_dispatch_jobs \
         ORDER BY event_id, subscription_id",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let context = json!([{"key": "userId", "value": "u1"}, {"key": "tenant", "value": "acme"}]);
    assert_eq!(
        rows,
        vec![
            ("evtbare000001".into(), "sub_blank".into(), None, json!([])),
            (
                "evtbare000001".into(),
                "sub_notify".into(),
                Some("Notify Value of user logins".into()),
                json!([])
            ),
            (
                "evtctx0000001".into(),
                "sub_blank".into(),
                None,
                context.clone()
            ),
            (
                "evtctx0000001".into(),
                "sub_notify".into(),
                Some("Notify Value of user logins".into()),
                context.clone()
            ),
        ]
    );

    // The projector copied both to the read rows.
    let read: Vec<(String, String, Option<String>, Value)> = sqlx::query_as(
        "SELECT event_id, subscription_id, descriptor, metadata FROM msg_dispatch_jobs_read \
         ORDER BY event_id, subscription_id",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(read, rows);

    let token = app.anchor_admin_token().await;
    let (notify_id,): (String,) = sqlx::query_as(
        "SELECT id FROM msg_dispatch_jobs WHERE event_id = 'evtctx0000001' AND subscription_id = 'sub_notify'",
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // The list rows (list, list-raw, raw alias, by-event), in Go's
    // `DispatchJobRead` shape: `descriptor` absent when none, `metadata`
    // absent when empty.
    for path in [
        "/api/dispatch-jobs",
        "/api/dispatch-jobs/list-raw",
        "/api/dispatch-jobs/raw",
        "/api/dispatch-jobs/by-event/evtctx0000001",
        "/bff/dispatch-jobs",
    ] {
        let (status, body) = read_json(app.get(path, &token).await).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        let row = find(&body, "id", &notify_id);
        assert_eq!(row["descriptor"], "Notify Value of user logins", "{path}");
        assert_eq!(row["metadata"], context, "{path}");
        for other in body.as_array().unwrap() {
            if other["subscriptionId"] == "sub_blank" {
                assert!(other.get("descriptor").is_none(), "{path}: {other}");
            }
            if other["eventId"] == "evtbare000001" {
                assert!(other.get("metadata").is_none(), "{path}: {other}");
            }
        }
    }

    // The detail and raw reads (Go's `DispatchJobResponse`).
    for path in [
        format!("/api/dispatch-jobs/{notify_id}"),
        format!("/api/dispatch-jobs/{notify_id}/raw"),
        format!("/bff/dispatch-jobs/{notify_id}"),
    ] {
        let (status, body) = read_json(app.get(&path, &token).await).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["descriptor"], "Notify Value of user logins", "{path}");
        assert_eq!(body["metadata"], context, "{path}");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_directly_created_job_carries_the_descriptor_and_metadata_it_is_sent() {
    let app = TestApp::setup().await;
    let pool = &app.pool;
    let token = ingest_token(&app);
    let item = |code: &str| {
        json!({
            "source": "integral",
            "code": code,
            "targetUrl": "https://receiver.example.test/hook",
            "payload": "{\"k\":\"v\"}",
            "serviceAccountId": "sac_nobody",
        })
    };

    // The single create: Go's string map, stored key-sorted.
    let mut single = item("t:direct:job:single");
    single["descriptor"] = json!("Rebuild the ledger");
    single["metadata"] = json!({"orderId": "42", "batch": "7"});
    let (status, body) = read_json(app.post("/api/dispatch-jobs", &token, single).await).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // The SDK batch: Go's `[{key, value}]` array (kept in order), and the
    // SDKs' string map.
    let mut listed = item("t:direct:job:listed");
    listed["descriptor"] = json!("Ship order 42");
    listed["metadata"] = json!([{"key": "z", "value": "1"}, {"key": "a", "value": "2"}]);
    let mut mapped = item("t:direct:job:mapped");
    mapped["metadata"] = json!({"y": "1", "x": "2"});
    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/batch",
            &token,
            json!({"items": [listed, mapped]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // The SPA-facing batch.
    let mut bff = item("t:direct:job:bff");
    bff["descriptor"] = json!("From the console");
    bff["metadata"] = json!([{"key": "k", "value": "v"}]);
    let (status, body) = read_json(
        app.post("/bff/dispatch-jobs/batch", &token, json!({"jobs": [bff]}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["jobs"][0]["descriptor"], "From the console");

    // A descriptor longer than the column is refused, and nothing is written.
    let mut long = item("t:direct:job:long");
    long["descriptor"] = json!("x".repeat(256));
    for (path, body) in [
        ("/api/dispatch-jobs", long.clone()),
        ("/api/dispatch-jobs/batch", json!({"items": [long.clone()]})),
    ] {
        let (status, body) = read_json(app.post(path, &token, body).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert_eq!(body["error"], "VALIDATION", "{path}: {body}");
    }
    let mut exact = item("t:direct:job:exact");
    exact["descriptor"] = json!("é".repeat(255));
    let (status, body) = read_json(app.post("/api/dispatch-jobs", &token, exact).await).await;
    assert_eq!(status, StatusCode::CREATED, "255 characters fit: {body}");

    let rows: Vec<(String, Option<String>, Value)> =
        sqlx::query_as("SELECT code, descriptor, metadata FROM msg_dispatch_jobs ORDER BY code")
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                "t:direct:job:bff".into(),
                Some("From the console".into()),
                json!([{"key": "k", "value": "v"}])
            ),
            (
                "t:direct:job:exact".into(),
                Some("é".repeat(255)),
                json!([])
            ),
            (
                "t:direct:job:listed".into(),
                Some("Ship order 42".into()),
                json!([{"key": "z", "value": "1"}, {"key": "a", "value": "2"}])
            ),
            (
                "t:direct:job:mapped".into(),
                None,
                json!([{"key": "x", "value": "2"}, {"key": "y", "value": "1"}])
            ),
            (
                "t:direct:job:single".into(),
                Some("Rebuild the ledger".into()),
                json!([{"key": "batch", "value": "7"}, {"key": "orderId", "value": "42"}])
            ),
        ]
    );

    // Projected and listed.
    project(pool).await;
    let reader = app.anchor_admin_token().await;
    let (status, body) = read_json(app.get("/api/dispatch-jobs", &reader).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let single = find(&body, "code", "t:direct:job:single");
    assert_eq!(single["descriptor"], "Rebuild the ledger");
    assert_eq!(
        single["metadata"],
        json!([{"key": "batch", "value": "7"}, {"key": "orderId", "value": "42"}])
    );
    let mapped = find(&body, "code", "t:direct:job:mapped");
    assert!(mapped.get("descriptor").is_none(), "{mapped}");

    // A status change re-projects the row; the read flags follow Go's:
    // is_completed only for COMPLETED, is_terminal for any end state. The
    // descriptor and metadata stay.
    sqlx::query(
        "UPDATE msg_dispatch_jobs SET updated_at = NOW() + INTERVAL '1 second', \
         status = CASE code WHEN 't:direct:job:single' THEN 'COMPLETED' \
                            WHEN 't:direct:job:listed' THEN 'FAILED' ELSE status END",
    )
    .execute(pool)
    .await
    .unwrap();
    project(pool).await;
    let flags: Vec<(String, String, bool, bool, Option<String>)> = sqlx::query_as(
        "SELECT code, status, is_completed, is_terminal, descriptor FROM msg_dispatch_jobs_read \
         WHERE code IN ('t:direct:job:single', 't:direct:job:listed', 't:direct:job:mapped') \
         ORDER BY code",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(
        flags,
        vec![
            (
                "t:direct:job:listed".into(),
                "FAILED".into(),
                false,
                true,
                Some("Ship order 42".into())
            ),
            (
                "t:direct:job:mapped".into(),
                "PENDING".into(),
                false,
                false,
                None
            ),
            (
                "t:direct:job:single".into(),
                "COMPLETED".into(),
                true,
                true,
                Some("Rebuild the ledger".into())
            ),
        ]
    );
}
