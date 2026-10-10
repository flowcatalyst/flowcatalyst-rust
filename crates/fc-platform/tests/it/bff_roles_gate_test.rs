//! The `/bff/roles` routes check the role permission for the operation (Go
//! 5b75de5): reads need role:view, defining a permission role:create, the
//! platform role sync create, update and delete. Requires Docker (or
//! `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::json;

use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions::{admin, iam};
use support::{assert_status, read_json, TestApp};

/// An anchor-tier token whose `scope` grants exactly `perms`.
fn anchor_holding(app: &TestApp, perms: &[&str]) -> String {
    let p = Principal::new_user("roles-gate@flowcatalyst.test", UserScope::Anchor);
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .expect("token")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn bff_roles_check_the_role_permission_for_the_operation() {
    let app = TestApp::setup().await;
    // An unrelated permission: a principal with none is profile-only and
    // refused before any route (NO_PLATFORM_ROLE).
    let nothing = anchor_holding(&app, &[admin::EVENT_READ]);

    // Reads need role:view (they checked nothing).
    for path in [
        "/bff/roles",
        "/bff/roles/filters/applications",
        "/bff/roles/permissions",
        "/bff/roles/permissions/platform:iam:role:view",
        "/bff/roles/platform:nope",
    ] {
        let (status, body) = read_json(app.get(path, &nothing).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert_eq!(body["error"], "PERMISSION_REQUIRED", "{path}: {body}");
    }
    let viewer = anchor_holding(&app, &[iam::ROLE_READ]);
    assert_status(app.get("/bff/roles", &viewer).await, StatusCode::OK).await;

    // Defining a permission is a role create (it checked anchor scope only).
    let body = json!({
        "application": "gate", "context": "orders", "aggregate": "order", "action": "view"
    });
    let (status, resp) = read_json(
        app.post("/bff/roles/permissions", &nothing, body.clone())
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(
        resp["message"],
        format!("permission required: {}", iam::ROLE_CREATE)
    );
    let creator = anchor_holding(&app, &[iam::ROLE_CREATE]);
    let (status, resp) = read_json(app.post("/bff/roles/permissions", &creator, body).await).await;
    assert!(status != StatusCode::FORBIDDEN, "{status}: {resp}");

    // The platform role sync needs create, update and delete, all three.
    for perms in [
        vec![iam::ROLE_CREATE],
        vec![iam::ROLE_CREATE, iam::ROLE_UPDATE],
        vec![iam::ROLE_UPDATE, iam::ROLE_DELETE],
    ] {
        let token = anchor_holding(&app, &perms);
        let (status, resp) = read_json(
            app.post("/bff/roles/sync-platform", &token, json!({}))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{perms:?}: {resp}");
        assert_eq!(resp["error"], "PERMISSION_REQUIRED", "{perms:?}: {resp}");
    }
}
