//! An event without a `source` is not refused: the platform stores the
//! type's `{application}:{subdomain}` (its first two segments), the source
//! every platform and SDK domain event carries (Go `4c3654a`, F14; Java
//! `5fc01261`; owner ruling 2026-10-12). The Java, TypeScript and Laravel
//! SDKs omit an unset source; before, the batch answered the item
//! `BAD_REQUEST` "source is required" (terminal for the outbox row) and the
//! singular create refused the body. Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use support::{read_json, TestApp};

async fn anchor_token(app: &TestApp) -> String {
    let principal = Principal::new_user("anchor@flowcatalyst.test", UserScope::Anchor);
    app.persist_all_applications(&principal).await;
    app.auth_service
        .generate_access_token_with_scope(
            &principal,
            &[permissions::admin::BATCH_EVENTS_WRITE.to_string()],
            None,
        )
        .expect("token")
}

async fn stored_source(app: &TestApp, id: &str) -> String {
    let (source,): (String,) = sqlx::query_as("SELECT source FROM msg_events WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("stored event");
    source
}

async fn batch(app: &TestApp, token: &str, items: Vec<Value>) -> Value {
    let (status, body) = read_json(
        app.post("/api/events/batch", token, json!({ "items": items }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_missing_event_source_defaults_to_the_types_application_and_subdomain() {
    let app = TestApp::setup().await;
    let token = anchor_token(&app).await;

    // Go's table, through POST /api/events/batch.
    for (item, want) in [
        (
            json!({"type": "orders:sales:order:created", "data": {}}),
            "orders:sales",
        ),
        (
            json!({"type": "orders:sales:order:created", "source": null, "data": {}}),
            "orders:sales",
        ),
        (
            json!({"type": "orders:sales:order:created", "source": "  ", "data": {}}),
            "orders:sales",
        ),
        (
            json!({"event_type": "orders:sales:order:created", "data": {}}),
            "orders:sales",
        ),
        (
            json!({"type": "orders:sales:order:created", "source": "orders", "data": {}}),
            "orders",
        ),
        (
            json!({"type": " orders:sales:order:created ", "data": {}}),
            "orders:sales",
        ),
        (json!({"type": "orders:sales", "data": {}}), "orders:sales"),
        (json!({"type": "orders", "data": {}}), "orders"),
    ] {
        let body = batch(&app, &token, vec![item.clone()]).await;
        let result = &body["results"][0];
        assert_eq!(result["status"], "SUCCESS", "{item}: {body}");
        assert_eq!(
            stored_source(&app, result["id"].as_str().unwrap()).await,
            want,
            "{item}"
        );
    }

    // A type with no application segment still needs a source, refused in
    // its slot; the item beside it lands.
    let body = batch(
        &app,
        &token,
        vec![
            json!({"type": ":sales:order:created", "data": {}}),
            json!({"type": "orders:sales:order:shipped", "data": {}}),
        ],
    )
    .await;
    assert_eq!(body["results"][0]["status"], "BAD_REQUEST", "{body}");
    assert_eq!(body["results"][0]["error"], "source is required", "{body}");
    assert_eq!(body["results"][1]["status"], "SUCCESS", "{body}");

    // The singular POST /api/events: source optional, defaulted the same
    // way; a blank application is 400 VALIDATION and nothing is written.
    for (path_body, want) in [
        (
            json!({"eventType": "orders:sales:order:created", "data": {"a": 1}}),
            "orders:sales",
        ),
        (
            json!({"eventType": "orders:sales:order:created", "source": " ", "data": {}}),
            "orders:sales",
        ),
        (
            json!({"eventType": "orders:sales:order:created", "source": "orders", "data": {}}),
            "orders",
        ),
    ] {
        let (status, body) =
            read_json(app.post("/api/events", &token, path_body.clone()).await).await;
        assert_eq!(status, StatusCode::CREATED, "{path_body}: {body}");
        assert_eq!(body["event"]["source"], want, "{body}");
        assert_eq!(
            stored_source(&app, body["event"]["id"].as_str().unwrap()).await,
            want
        );
    }
    let (status, body) = read_json(
        app.post(
            "/api/events",
            &token,
            json!({"eventType": ":sales:order:created", "data": {}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "VALIDATION", "{body}");
    assert_eq!(body["message"], "source is required", "{body}");
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM msg_events WHERE type LIKE ':%'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);

    // The BFF batch (the singular create's shape) defaults it too, and a
    // blank application refuses the batch.
    let (status, body) = read_json(
        app.post(
            "/bff/events/batch",
            &token,
            json!({"events": [{"eventType": "orders:billing:invoice:sent", "data": {}}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["events"][0]["source"], "orders:billing", "{body}");
    let (status, body) = read_json(
        app.post(
            "/bff/events/batch",
            &token,
            json!({"events": [{"eventType": ":billing:invoice:sent", "data": {}}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["message"], "source is required", "{body}");
}
