//! `POST /api/audit-logs/batch` against a real database: the Go platform's
//! behaviour (`internal/platform/shared/sdk/audit_batch.go`). Items for a
//! client the caller cannot access are skipped while the rest land,
//! `performedAt` is stored as sent (now when it is unparseable), and an item
//! without a principal is refused in its own slot.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use serde_json::json;

use fc_platform::client::entity::Client;
use support::{assert_status, read_json, TestApp};

async fn stored(app: &TestApp, entity_id: &str) -> Vec<(Option<String>, DateTime<Utc>)> {
    sqlx::query_as::<_, (Option<String>, DateTime<Utc>)>(
        "SELECT client_id, performed_at FROM aud_logs WHERE entity_id = $1",
    )
    .bind(entity_id)
    .fetch_all(&app.pool)
    .await
    .expect("read aud_logs")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn ingest_skips_inaccessible_clients_and_honours_performed_at() {
    let app = TestApp::setup().await;
    let mine = Client::new("Mine", "mine");
    let theirs = Client::new("Theirs", "theirs");
    app.repos.client_repo.insert(&mine).await.expect("client");
    app.repos.client_repo.insert(&theirs).await.expect("client");
    let token = app.service_account_token(&mine.id);

    let before = Utc::now();
    let resp = app
        .post(
            "/api/audit-logs/batch",
            &token,
            json!({ "items": [
                {
                    "entityType": "Order", "entityId": "ord_mine", "operation": "CREATE",
                    "principalId": "prn_sdk", "clientCode": "mine",
                    "performedAt": "2025-03-04T05:06:07Z",
                    "operationData": { "password": "x", "name": "kept" },
                },
                {
                    "entityType": "Order", "entityId": "ord_theirs", "operation": "CREATE",
                    "principalId": "prn_sdk", "clientCode": "theirs",
                },
                {
                    "entityType": "Order", "entityId": "ord_bad_time", "operation": "CREATE",
                    "principalId": "prn_sdk", "clientCode": "mine",
                    "performedAt": "not a time",
                },
                {
                    "entityType": "Order", "entityId": "ord_no_actor", "operation": "CREATE",
                },
                {
                    "entityType": "Order", "entityId": "ord_unknown", "operation": "CREATE",
                    "principalId": "prn_sdk", "clientCode": "nobody",
                },
            ]}),
        )
        .await;
    let body = assert_status(resp, StatusCode::OK).await;
    let statuses: Vec<&str> = body["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|r| r["status"].as_str().expect("status"))
        .collect();
    assert_eq!(
        statuses,
        ["SUCCESS", "SKIPPED", "SUCCESS", "BAD_REQUEST", "SKIPPED"],
        "{body}"
    );
    assert_eq!(body["results"][1]["id"], "", "{body}");
    assert!(body["results"][1].get("error").is_none(), "{body}");
    assert_eq!(body["results"][3]["error"], "principalId is required");

    let rows = stored(&app, "ord_mine").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0.as_deref(), Some(mine.id.as_str()));
    let sent: DateTime<Utc> = "2025-03-04T05:06:07Z".parse().unwrap();
    assert_eq!(rows[0].1, sent, "performedAt is stored as sent");

    let rows = stored(&app, "ord_bad_time").await;
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].1 >= before - Duration::seconds(1) && rows[0].1 <= Utc::now(),
        "an unparseable performedAt falls back to now: {:?}",
        rows[0].1
    );

    for skipped in ["ord_theirs", "ord_no_actor", "ord_unknown"] {
        assert!(stored(&app, skipped).await.is_empty(), "{skipped} stored");
    }

    // The redaction backstop still runs on the batched path.
    let (json,): (serde_json::Value,) =
        sqlx::query_as("SELECT operation_json FROM aud_logs WHERE entity_id = 'ord_mine'")
            .fetch_one(&app.pool)
            .await
            .expect("read operation_json");
    assert_eq!(json, json!({ "password": "***", "name": "kept" }));
}

/// A full batch lands in one insert, and the wire errors match Go: an
/// oversized batch is `400 BATCH_TOO_LARGE`, a malformed body
/// `400 INVALID_JSON`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn ingest_takes_a_full_batch_and_rejects_bad_bodies_like_go() {
    let app = TestApp::setup().await;
    let token = app.anchor_token();

    let items: Vec<_> = (0..100)
        .map(|i| {
            json!({
                "entityType": "Order", "entityId": format!("ord_{i}"),
                "operation": "CREATE", "principalId": "prn_sdk",
                "clientCode": format!("c{}", i % 3),
            })
        })
        .collect();
    for code in ["c0", "c1", "c2"] {
        app.repos
            .client_repo
            .insert(&Client::new(code, code))
            .await
            .expect("client");
    }
    let resp = app
        .post("/api/audit-logs/batch", &token, json!({ "items": items }))
        .await;
    let body = assert_status(resp, StatusCode::OK).await;
    assert!(body["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["status"] == "SUCCESS"));
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM aud_logs WHERE entity_id LIKE 'ord\\_%'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(count, 100);

    let too_many: Vec<_> = (0..101).map(|_| json!({})).collect();
    let (status, body) = read_json(
        app.post(
            "/api/audit-logs/batch",
            &token,
            json!({ "items": too_many }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "BATCH_TOO_LARGE", "{body}");
    assert_eq!(body["message"], "Maximum 100 items per batch", "{body}");

    let (status, body) = read_json(
        app.post("/api/audit-logs/batch", &token, json!({ "items": "nope" }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "INVALID_JSON", "{body}");
}
