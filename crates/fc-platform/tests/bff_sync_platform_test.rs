//! Owner ruling 16 (Java 635c2e3e): `POST /bff/event-types/sync-platform`
//! syncs the platform's own event types into `platform` only. Requires
//! Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::json;

use support::{assert_status, TestApp};

async fn event_type_count(app: &TestApp, application: &str) -> i64 {
    let (n,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM msg_event_types WHERE application = $1")
            .bind(application)
            .fetch_one(&app.pool)
            .await
            .expect("count event types");
    n
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn sync_platform_targets_the_platform_only() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let path = "/bff/event-types/sync-platform";

    let aimed = assert_status(
        app.post(path, &token, json!({"applicationCode": "orders"}))
            .await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(aimed["error"], "PLATFORM_SYNC_ONLY", "{aimed}");
    assert_eq!(
        event_type_count(&app, "orders").await,
        0,
        "nothing synced into orders"
    );
    assert_eq!(event_type_count(&app, "platform").await, 0);

    let named = assert_status(
        app.post(path, &token, json!({"applicationCode": "platform"}))
            .await,
        StatusCode::OK,
    )
    .await;
    assert!(named["total"].as_u64().unwrap() > 0, "{named}");
    assert_status(app.post(path, &token, json!({})).await, StatusCode::OK).await;
    assert!(event_type_count(&app, "platform").await > 0);
}
