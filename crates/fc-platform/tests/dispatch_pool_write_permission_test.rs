//! Dispatch-pool writes need Go's `CanWriteDispatchPools` (one of the pool
//! create/update/delete permissions) before client reach is considered.
//! Before, client reach alone let any user of a client create or change that
//! client's pools. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::json;

use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions;
use support::{read_json, TestApp};

/// A CLIENT-tier token for `client` whose `scope` grants exactly `perms`.
fn client_caller(app: &TestApp, client: &str, perms: &[&str]) -> String {
    let p = Principal::new_user("pools@pool-perms.test", UserScope::Client).with_client_id(client);
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .expect("token")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn pool_writes_need_a_pool_permission_not_just_client_reach() {
    let app = TestApp::setup().await;
    let client = "clt_0POOLPERMS01";
    let body = json!({ "code": "perm-pool", "name": "Perm Pool", "clientId": client });

    let unrelated = client_caller(&app, client, &[permissions::admin::EVENT_TYPE_READ]);
    let (status, resp) = read_json(
        app.post("/api/dispatch-pools", &unrelated, body.clone())
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(resp["error"], "PERMISSION_REQUIRED", "{resp}");

    for path in [
        "/api/dispatch-pools/dpl_nope/archive",
        "/api/dispatch-pools/dpl_nope/suspend",
        "/api/dispatch-pools/dpl_nope/activate",
    ] {
        let (status, resp) = read_json(app.post(path, &unrelated, json!({})).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {resp}");
    }
    let (status, resp) = read_json(
        app.put(
            "/api/dispatch-pools/dpl_nope",
            &unrelated,
            json!({ "name": "x" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "update: {resp}");

    // With the permission, the gate lets the call through (to client reach
    // and the use case).
    let writer = client_caller(&app, client, &[permissions::admin::DISPATCH_POOL_CREATE]);
    let (status, resp) = read_json(app.post("/api/dispatch-pools", &writer, body).await).await;
    assert!(
        status != StatusCode::FORBIDDEN && status != StatusCode::UNAUTHORIZED,
        "{status}: {resp}"
    );
}
