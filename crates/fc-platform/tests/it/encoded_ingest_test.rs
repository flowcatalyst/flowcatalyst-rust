//! Batch ingest unwraps an SDK-encoded event `data` and audit
//! `operationData` (Go `8bf67d1`, F13; owner ruling 2026-10-12): a JSON
//! string holding an encoded object or array is stored as that object or
//! array, anything else as sent. Driven with the Rust SDK's golden wire
//! fixtures (`tests/data/rust-sdk-wire`, copied verbatim from
//! `flowcatalyst-go/clients/rust-sdk/tests/fixtures/wire`). Requires Docker
//! (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use support::{read_json, TestApp};

const EVENT_FULL: &str = include_str!("../data/rust-sdk-wire/event-full.json");
const EVENT_MINIMAL: &str = include_str!("../data/rust-sdk-wire/event-minimal.json");
const AUDIT_LOG_FULL: &str = include_str!("../data/rust-sdk-wire/audit-log-full.json");

/// An anchor token holding `perms`, stored with the all-applications flag
/// (the ingest confinement of application-scoped callers is not under test).
async fn anchor_token(app: &TestApp, perms: &[&str]) -> String {
    let principal = Principal::new_user("anchor@flowcatalyst.test", UserScope::Anchor);
    app.persist_all_applications(&principal).await;
    let granted: Vec<String> = perms.iter().map(|p| p.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&principal, &granted, None)
        .expect("token")
}

fn fixture(text: &str) -> Value {
    serde_json::from_str(text).expect("fixture")
}

/// One item through `POST /api/events/batch`: its result.
async fn ingest_event(app: &TestApp, token: &str, item: Value) -> Value {
    let (status, body) = read_json(
        app.post("/api/events/batch", token, json!({"items": [item]}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["results"][0].clone()
}

/// The stored event's `jsonb_typeof(data)` and `data`.
async fn stored_data(app: &TestApp, id: &str) -> (String, Value) {
    sqlx::query_as::<_, (String, Value)>(
        "SELECT jsonb_typeof(data), data FROM msg_events WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("stored event")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_events_batch_unwraps_encoded_data() {
    let app = TestApp::setup().await;
    let acme = Client::new("ACME", "acme");
    app.repos.client_repo.insert(&acme).await.expect("client");
    let token = anchor_token(&app, &[permissions::admin::BATCH_EVENTS_WRITE]).await;

    // The Rust SDK's full event (the Java/TS/Laravel/framework form): stored
    // as the object, under the client its clientCode names.
    let result = ingest_event(&app, &token, fixture(EVENT_FULL)).await;
    assert_eq!(result["status"], "SUCCESS", "{result}");
    let id = result["id"].as_str().unwrap();
    assert_eq!(
        stored_data(&app, id).await,
        (
            "object".to_string(),
            json!({"orderId": "ord_1", "total": 1500})
        )
    );
    let (client_id,): (Option<String>,) =
        sqlx::query_as("SELECT client_id FROM msg_events WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(client_id.as_deref(), Some(acme.id.as_str()));

    // The minimal fixture carries no source: refused in its slot.
    let result = ingest_event(&app, &token, fixture(EVENT_MINIMAL)).await;
    assert_eq!(result["status"], "BAD_REQUEST", "{result}");
    assert_eq!(result["error"], "source is required", "{result}");
    // With a source it is stored as `{}`.
    let mut minimal = fixture(EVENT_MINIMAL);
    minimal["source"] = json!("orders");
    let result = ingest_event(&app, &token, minimal).await;
    assert_eq!(result["status"], "SUCCESS", "{result}");
    assert_eq!(
        stored_data(&app, result["id"].as_str().unwrap()).await,
        ("object".to_string(), json!({}))
    );

    // Go's table: the value itself, encoded arrays, and strings kept.
    for (data, kind, want) in [
        (
            json!({"orderId": "ord_2"}),
            "object",
            json!({"orderId": "ord_2"}),
        ),
        (json!("[{\"line\":1}]"), "array", json!([{"line": 1}])),
        (json!("[]"), "array", json!([])),
        (json!("hello"), "string", json!("hello")),
        (json!("null"), "string", json!("null")),
        (json!("42"), "string", json!("42")),
        (json!("{not json"), "string", json!("{not json")),
        (
            json!("\"{\\\"a\\\":1}\""),
            "string",
            json!("\"{\\\"a\\\":1}\""),
        ),
        (json!(7), "number", json!(7)),
    ] {
        let item = json!({"type": "orders:sales:order:created", "source": "orders", "data": data});
        let result = ingest_event(&app, &token, item).await;
        assert_eq!(result["status"], "SUCCESS", "{data}: {result}");
        assert_eq!(
            stored_data(&app, result["id"].as_str().unwrap()).await,
            (kind.to_string(), want),
            "{data}"
        );
    }

    // The singular POST /api/events receives the value itself and is not
    // unwrapped: a string data stays a string.
    let (status, body) = read_json(
        app.post(
            "/api/events",
            &token,
            json!({"eventType": "orders:sales:order:created", "source": "orders",
                   "data": "{\"a\":1}"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        stored_data(&app, body["event"]["id"].as_str().unwrap()).await,
        ("string".to_string(), json!("{\"a\":1}"))
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_audit_batch_unwraps_encoded_operation_data_before_redaction() {
    let app = TestApp::setup().await;
    let acme = Client::new("ACME", "acme");
    app.repos.client_repo.insert(&acme).await.expect("client");
    let orders = fc_platform::Application::new("orders", "ORDERS");
    app.repos
        .application_repo
        .insert(&orders)
        .await
        .expect("application");
    let token = anchor_token(&app, &[permissions::admin::BATCH_AUDIT_LOGS_WRITE]).await;

    let item = |entity_id: &str, operation_data: Value| {
        json!({"entityType": "Order", "entityId": entity_id, "operation": "PlaceOrder",
               "principalId": "prn_sdk", "operationData": operation_data})
    };
    let mut full = fixture(AUDIT_LOG_FULL);
    full["entityId"] = json!("ord_full");
    let (status, body) = read_json(
        app.post(
            "/api/audit-logs/batch",
            &token,
            json!({"items": [
                full,
                item("ord_secret", json!("{\"password\":\"hunter2\",\"total\":1}")),
                item("ord_object", json!({"total": 2})),
                item("ord_text", json!("plain text")),
                item("ord_null", json!("null")),
            ]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let statuses: Vec<&str> = body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["SUCCESS"; 5], "{body}");

    for (entity_id, kind, want) in [
        (
            "ord_full",
            "object",
            json!({"apiKey": "***", "total": 1500}),
        ),
        (
            "ord_secret",
            "object",
            json!({"password": "***", "total": 1}),
        ),
        ("ord_object", "object", json!({"total": 2})),
        ("ord_text", "string", json!("plain text")),
        ("ord_null", "string", json!("null")),
    ] {
        let (typ, stored): (String, Value) = sqlx::query_as(
            "SELECT jsonb_typeof(operation_json), operation_json FROM aud_logs WHERE entity_id = $1",
        )
        .bind(entity_id)
        .fetch_one(&app.pool)
        .await
        .expect("stored audit row");
        assert_eq!((typ.as_str(), stored), (kind, want), "{entity_id}");
    }
    let (client_id, application_id): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT client_id, application_id FROM aud_logs WHERE entity_id = 'ord_full'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(client_id.as_deref(), Some(acme.id.as_str()));
    assert_eq!(application_id.as_deref(), Some(orders.id.as_str()));
}
