//! Go's F2 (`0a8df19`, docs/platform-bug-fixes.md in flowcatalyst-go): event
//! types have no client (`msg_event_types` has no client column). A
//! `clientId` in the create body (the SDKs expose it) is accepted and
//! ignored: the type is platform-wide, its created event names no client,
//! and creating it needs anchor reach whatever `clientId` says. It used to
//! pass the scope check for the caller's own client and then be dropped, so
//! a client user created a platform-wide event type. Requires Docker (or
//! `FC_TEST_PG_BIN`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::support::{read_json, TestApp};
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions::admin;
use fc_platform::shared::id::ClientId;

const CLIENT: &str = "clt_etignore01";

async fn exists(app: &TestApp, code: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM msg_event_types WHERE code = $1)")
        .bind(code)
        .fetch_one(&app.pool)
        .await
        .expect("query event type")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn create_ignores_client_id_and_needs_anchor_reach() {
    let app = TestApp::setup().await;
    let body = |code: &str| json!({ "code": code, "name": "N", "clientId": CLIENT });

    // A client user (event-type create, its own client named) is refused on
    // both tiers: the type would be platform-wide.
    let client_user = Principal::new_user("et-ignore@flowcatalyst.test", UserScope::Client)
        .with_client_id(ClientId::parse(CLIENT).unwrap());
    let client_user = app
        .auth_service
        .generate_access_token_with_scope(
            &client_user,
            &[admin::EVENT_TYPE_CREATE.to_string()],
            None,
        )
        .expect("token");
    for (path, code) in [
        ("/api/event-types", "etignore:orders:order:shipped"),
        ("/bff/event-types", "etignore:orders:order:packed"),
    ] {
        let (status, resp) = read_json(app.post(path, &client_user, body(code)).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {resp}");
        assert_eq!(resp["error"], "SCOPE_FORBIDDEN", "{path}: {resp}");
        assert!(!exists(&app, code).await, "{path}: created {code}");
    }

    // An anchor's create succeeds; the created event names no client.
    let admin = app.anchor_admin_token().await;
    let (status, resp) = read_json(
        app.post(
            "/api/event-types",
            &admin,
            body("etignore:orders:order:created"),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{resp}");
    let id = resp["id"].as_str().expect("id").to_string();
    let data: Value = sqlx::query_scalar(
        "SELECT data FROM msg_events
          WHERE type = 'platform:admin:eventtype:created' AND data->>'eventTypeId' = $1",
    )
    .bind(&id)
    .fetch_one(&app.pool)
    .await
    .expect("created event");
    assert!(
        data.get("clientId").is_none(),
        "the created event names a client the type does not have: {data}"
    );
}
