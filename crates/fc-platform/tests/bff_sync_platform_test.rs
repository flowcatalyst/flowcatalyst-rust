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

async fn insert_event_type(app: &TestApp, code: &str, name: &str, source: &str) {
    let parts: Vec<&str> = code.split(':').collect();
    sqlx::query(
        "INSERT INTO msg_event_types
           (id, code, name, status, source, client_scoped, application, subdomain, aggregate,
            created_at, updated_at)
         VALUES ($1, $2, $3, 'CURRENT', $4, false, $5, $6, $7, NOW(), NOW())",
    )
    .bind(fc_common::tsid::generate_untyped())
    .bind(code)
    .bind(name)
    .bind(source)
    .bind(parts[0])
    .bind(parts[1])
    .bind(parts[2])
    .execute(&app.pool)
    .await
    .expect("insert event type");
}

async fn event_type_name(app: &TestApp, code: &str) -> Option<String> {
    sqlx::query_as::<_, (String,)>("SELECT name FROM msg_event_types WHERE code = $1")
        .bind(code)
        .fetch_optional(&app.pool)
        .await
        .expect("read event type")
        .map(|(n,)| n)
}

/// Go parity (`eventtype/operations/sync.go`): a listed code is updated
/// whatever its source, and `removeUnlisted` removes only API-sourced rows —
/// never UI- or CODE-managed ones (the platform's own catalogue).
#[tokio::test]
#[ignore = "requires Docker"]
async fn application_event_type_sync_follows_go() {
    let app = TestApp::setup().await;
    // Seeds the platform:test-admin role; the caller is then stored with
    // every application in reach (the sync answers 404 out of scope).
    app.anchor_admin_token().await;
    let mut caller = fc_platform::Principal::new_user(
        "ets-admin@flowcatalyst.test",
        fc_platform::UserScope::Anchor,
    );
    caller.all_applications = true;
    caller.roles = vec![fc_platform::service_account::entity::RoleAssignment::new(
        "platform:test-admin",
    )];
    app.repos
        .principal_repo
        .insert(&caller)
        .await
        .expect("insert caller");
    let token = app
        .auth_service
        .generate_access_token(&caller)
        .expect("caller token");
    app.repos
        .application_repo
        .insert(&fc_platform::application::entity::Application::new(
            "ets", "ets",
        ))
        .await
        .expect("insert application");
    insert_event_type(&app, "ets:orders:order:created", "old ui name", "UI").await;
    insert_event_type(&app, "ets:orders:order:code-kept", "code row", "CODE").await;
    insert_event_type(&app, "ets:orders:order:ui-kept", "ui row", "UI").await;
    insert_event_type(&app, "ets:orders:order:api-gone", "api row", "API").await;

    let resp = app
        .post(
            "/api/applications/ets/event-types/sync?removeUnlisted=true",
            &token,
            json!({ "eventTypes": [
                { "code": "ets:orders:order:created", "name": "new name" }
            ]}),
        )
        .await;
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        status.is_success(),
        "{status}: {}",
        String::from_utf8_lossy(&body)
    );

    assert_eq!(
        event_type_name(&app, "ets:orders:order:created")
            .await
            .as_deref(),
        Some("new name"),
        "a listed UI-sourced type is updated"
    );
    assert!(event_type_name(&app, "ets:orders:order:code-kept")
        .await
        .is_some());
    assert!(event_type_name(&app, "ets:orders:order:ui-kept")
        .await
        .is_some());
    assert!(
        event_type_name(&app, "ets:orders:order:api-gone")
            .await
            .is_none(),
        "an unlisted API-sourced type is removed"
    );
}
