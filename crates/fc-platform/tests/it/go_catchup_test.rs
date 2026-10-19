//! Go ledger items F29-F33 mirrored 2026-10-19 (docs/go-mirror/2026-10-19-identity-fixes-and-catch-up.md).
//! Requires a Postgres (see `support::db`).

use crate::support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fc_platform::application::entity::Application;
use fc_platform::client::entity::Client;
use serde_json::{json, Value};
use support::assert_status;
use support::TestApp;
use tower::ServiceExt;

async fn setup() -> TestApp {
    support::set_app_key();
    TestApp::setup().await
}

/// F29: the router-config document carries an ETag and answers 304 to a
/// matching `If-None-Match`; a changed document changes the ETag.
#[tokio::test]
#[ignore = "requires Docker"]
async fn router_config_is_conditional() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let fetch = |if_none_match: Option<String>| {
        let router = app.router.clone();
        let admin = admin.clone();
        async move {
            let mut req = Request::builder()
                .uri("/api/dispatch/router-config")
                .header("authorization", format!("Bearer {admin}"));
            if let Some(v) = if_none_match {
                req = req.header("if-none-match", v);
            }
            router
                .oneshot(req.body(Body::empty()).unwrap())
                .await
                .unwrap()
        }
    };

    let first = fetch(None).await;
    assert_eq!(first.status(), StatusCode::OK);
    let etag = first
        .headers()
        .get("etag")
        .expect("an ETag")
        .to_str()
        .unwrap()
        .to_string();
    assert!(etag.starts_with('"') && etag.ends_with('"'), "{etag}");

    for same in [
        etag.clone(),
        format!("W/{etag}"),
        format!("\"other\", {etag}"),
        "*".to_string(),
    ] {
        let resp = fetch(Some(same.clone())).await;
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED, "{same}");
        assert_eq!(resp.headers().get("etag").unwrap().to_str().unwrap(), etag);
    }
    let resp = fetch(Some("\"stale\"".to_string())).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // A new client adds its queues: the validator no longer matches.
    app.repos
        .client_repo
        .insert(&Client::new("Fresh", "fresh"))
        .await
        .unwrap();
    let resp = fetch(Some(etag.clone())).await;
    assert_eq!(resp.status(), StatusCode::OK, "the document changed");
    assert_ne!(resp.headers().get("etag").unwrap().to_str().unwrap(), etag);
}

// ── F30: unchanged definitions are not rewritten or counted ──────────────────

async fn sync_post(app: &TestApp, token: &str, path: &str, body: Value) -> Value {
    assert_status(app.post(path, token, body).await, StatusCode::OK).await
}

async fn event_count(app: &TestApp, like: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM msg_events WHERE type LIKE $1")
        .bind(like)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn updated_at(
    app: &TestApp,
    table: &str,
    col: &str,
    key: &str,
) -> chrono::DateTime<chrono::Utc> {
    sqlx::query_scalar(&format!("SELECT updated_at FROM {table} WHERE {col} = $1"))
        .bind(key)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_unchanged_sdk_role_sync_writes_and_counts_nothing() {
    let app = setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    app.repos
        .application_repo
        .insert(&Application::new("f30r", "F30"))
        .await
        .unwrap();
    let path = "/api/applications/f30r/roles/sync";
    let role = |display: &str, perms: Value| {
        json!({ "roles": [{ "name": "viewer", "displayName": display, "description": "d",
                            "permissions": perms }] })
    };
    let perms = json!(["f30r:orders:order:view", "f30r:orders:order:edit"]);

    let first = sync_post(&app, &admin, path, role("Viewer", perms.clone())).await;
    assert_eq!(first["created"], 1, "{first}");
    let stamp = updated_at(&app, "iam_roles", "name", "f30r:viewer").await;
    let events = event_count(&app, "%role:updated").await;

    // The same definition, the permissions reordered, and no permissions at
    // all (keep the stored ones): nothing is written or counted.
    for body in [
        role("Viewer", perms.clone()),
        role(
            "Viewer",
            json!(["f30r:orders:order:edit", "f30r:orders:order:view"]),
        ),
        role("Viewer", json!([])),
    ] {
        let again = sync_post(&app, &admin, path, body).await;
        assert_eq!(again["created"], 0, "{again}");
        assert_eq!(again["updated"], 0, "{again}");
    }
    assert_eq!(
        updated_at(&app, "iam_roles", "name", "f30r:viewer").await,
        stamp
    );
    assert_eq!(
        event_count(&app, "%role:updated").await,
        events,
        "no *Updated event"
    );

    // Each managed field still updates exactly this one row.
    for (what, body) in [
        ("display name", role("Viewer 2", perms.clone())),
        (
            "permissions",
            role("Viewer 2", json!(["f30r:orders:order:view"])),
        ),
    ] {
        let changed = sync_post(&app, &admin, path, body).await;
        assert_eq!(changed["updated"], 1, "{what}: {changed}");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_unchanged_platform_role_sync_counts_nothing() {
    let app = setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    // The boot state, then a sync: everything matches the catalogue.
    let _ = sync_post(&app, &admin, "/bff/roles/sync-platform", json!({})).await;
    let again = sync_post(&app, &admin, "/bff/roles/sync-platform", json!({})).await;
    assert_eq!(again["updated"], 0, "{again}");
    // One drifted role is the only one rewritten.
    sqlx::query("UPDATE iam_roles SET display_name = 'drifted' WHERE name = 'platform:router'")
        .execute(&app.pool)
        .await
        .unwrap();
    let drifted = sync_post(&app, &admin, "/bff/roles/sync-platform", json!({})).await;
    assert_eq!(drifted["updated"], 1, "{drifted}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn unchanged_connections_and_subscriptions_are_not_rewritten() {
    support::set_app_key();
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    let created = assert_status(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": "f30-bot", "name": "Bot" }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let mut application = Application::new("f30c", "F30");
    application.service_account_id = Some(created["principalId"].as_str().unwrap().to_string());
    app.repos
        .application_repo
        .insert(&application)
        .await
        .unwrap();

    // Connections.
    let conns = |name: &str| json!({ "connections": [{ "code": "f30-conn", "name": name }] });
    let path = "/api/applications/f30c/connections/sync";
    assert_eq!(
        sync_post(&app, &admin, path, conns("One")).await["created"],
        1
    );
    let stamp = updated_at(&app, "msg_connections", "code", "f30-conn").await;
    let again = sync_post(&app, &admin, path, conns("One")).await;
    assert_eq!(
        (again["created"].clone(), again["updated"].clone()),
        (json!(0), json!(0)),
        "{again}"
    );
    assert_eq!(
        updated_at(&app, "msg_connections", "code", "f30-conn").await,
        stamp
    );
    assert_eq!(
        sync_post(&app, &admin, path, conns("Two")).await["updated"],
        1
    );

    // Subscriptions.
    let subs = |name: &str, events: Value| {
        json!({ "subscriptions": [{
            "code": "f30-sub", "name": name,
            "target": "https://receiver.example.test/hook",
            "eventTypes": events
        }]})
    };
    let ab = json!([{ "eventTypeCode": "f30c:orders:order:created" }, { "eventTypeCode": "f30c:orders:order:shipped" }]);
    let ba = json!([{ "eventTypeCode": "f30c:orders:order:shipped" }, { "eventTypeCode": "f30c:orders:order:created" }]);
    let path = "/api/applications/f30c/subscriptions/sync";
    assert_eq!(
        sync_post(&app, &admin, path, subs("S", ab.clone())).await["created"],
        1
    );
    let stamp = updated_at(&app, "msg_subscriptions", "code", "f30-sub").await;
    let events = event_count(&app, "%subscription:updated").await;
    for body in [subs("S", ab.clone()), subs("S", ba.clone())] {
        let again = sync_post(&app, &admin, path, body).await;
        assert_eq!(again["updated"], 0, "{again}");
    }
    assert_eq!(
        updated_at(&app, "msg_subscriptions", "code", "f30-sub").await,
        stamp
    );
    assert_eq!(event_count(&app, "%subscription:updated").await, events);
    assert_eq!(
        sync_post(&app, &admin, path, subs("S renamed", ab)).await["updated"],
        1
    );
    assert_eq!(
        sync_post(
            &app,
            &admin,
            path,
            subs(
                "S renamed",
                json!([{ "eventTypeCode": "f30c:orders:order:created" }])
            )
        )
        .await["updated"],
        1,
        "a changed binding set is a change"
    );
}
