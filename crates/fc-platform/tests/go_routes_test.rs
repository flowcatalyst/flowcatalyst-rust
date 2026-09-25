//! Routes Go serves that Rust lacked (parity run 1, root cause 4), each
//! against its Go behaviour: happy path, permission and validation errors.
//! Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::json;

use fc_platform::domain::{Principal, UserScope};
use support::{assert_status, read_json, TestApp};

async fn setup() -> TestApp {
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );
    TestApp::setup().await
}

/// A non-anchor (client-scoped) user holding nothing.
fn nobody_token(app: &TestApp) -> String {
    let p = Principal::new_user("nobody@flowcatalyst.test", UserScope::Client)
        .with_client_id("clt_nobody");
    app.auth_service.generate_access_token(&p).expect("token")
}

async fn create_role(app: &TestApp, token: &str, name: &str) -> String {
    let body = assert_status(
        app.post(
            "/api/roles",
            token,
            json!({
                "applicationCode": "parity",
                "roleName": name,
                "displayName": "Parity role",
                "permissions": ["parity:admin:widget:view"]
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    body["id"].as_str().expect("role id").to_string()
}

// ── Roles ────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn role_permissions_are_granted_and_revoked_by_path() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let role_id = create_role(&app, &admin, "widgets").await;
    let role = "parity:widgets";

    let body = assert_status(
        app.get(&format!("/api/roles/{role}/permissions"), &admin)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body, json!({ "permissions": ["parity:admin:widget:view"] }));

    // Grant by path, twice (idempotent), then by body.
    for _ in 0..2 {
        let body = assert_status(
            app.post(
                &format!("/api/roles/{role}/permissions/parity:admin:widget:manage"),
                &admin,
                json!({}),
            )
            .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(body["id"], role_id.as_str());
    }
    let body = assert_status(
        app.post(
            &format!("/api/roles/{role}/permissions"),
            &admin,
            json!({ "permission": "parity:admin:widget:delete" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let mut perms: Vec<String> = serde_json::from_value(body["permissions"].clone()).unwrap();
    perms.sort();
    assert_eq!(
        perms,
        [
            "parity:admin:widget:delete",
            "parity:admin:widget:manage",
            "parity:admin:widget:view"
        ]
    );

    // Revoke, and revoke of an absent permission is a 200 no-op.
    for _ in 0..2 {
        assert_status(
            app.delete(
                &format!("/api/roles/{role}/permissions/parity:admin:widget:manage"),
                &admin,
            )
            .await,
            StatusCode::OK,
        )
        .await;
    }
    let body = assert_status(
        app.get(&format!("/api/roles/{role}/permissions"), &admin)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        body["permissions"],
        json!(["parity:admin:widget:delete", "parity:admin:widget:view"])
    );

    // Go's event types, one per grant/revoke including the repeats.
    assert_eq!(
        app.event_count_by_type("platform:admin:role:permission-granted")
            .await,
        3
    );
    assert_eq!(
        app.event_count_by_type("platform:admin:role:permission-revoked")
            .await,
        2
    );
    assert!(app.audit_count_for(&role_id).await >= 5);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn role_permission_routes_refuse_unknown_roles_and_non_admins() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    create_role(&app, &admin, "gadgets").await;

    let (status, _) = read_json(
        app.post(
            "/api/roles/parity:nope/permissions/parity:x:y:z",
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = read_json(app.get("/api/roles/parity:nope/permissions", &admin).await).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let nobody = nobody_token(&app);
    let (status, _) = read_json(
        app.post(
            "/api/roles/parity:gadgets/permissions/parity:x:y:z",
            &nobody,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = read_json(
        app.delete(
            "/api/roles/parity:gadgets/permissions/parity:admin:widget:view",
            &nobody,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = read_json(
        app.get("/api/roles/parity:gadgets/permissions", &nobody)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A blank permission in the body is a validation error.
    let (status, body) = read_json(
        app.post(
            "/api/roles/parity:gadgets/permissions",
            &admin,
            json!({ "permission": " " }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_permission_catalogue_is_defined_and_deleted() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;

    let body = assert_status(
        app.post(
            "/bff/roles/permissions",
            &admin,
            json!({
                "application": "shop",
                "context": "orders",
                "aggregate": "order",
                "action": "ship",
                "description": " Ship an order "
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(
        body,
        json!({
            "permission": "shop:orders:order:ship",
            "application": "shop",
            "context": "orders",
            "aggregate": "order",
            "action": "ship",
            "description": "Ship an order"
        })
    );
    // Idempotent by code.
    assert_status(
        app.post(
            "/bff/roles/permissions",
            &admin,
            json!({ "application": "shop", "context": "orders", "aggregate": "order", "action": "ship" }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let rows: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM iam_permissions WHERE code = 'shop:orders:order:ship'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(rows.0, 1);

    let (status, body) = read_json(
        app.post(
            "/bff/roles/permissions",
            &admin,
            json!({ "application": "Shop", "context": "orders", "aggregate": "order", "action": "ship" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "INVALID_PERMISSION");

    let (status, _) = read_json(
        app.post(
            "/bff/roles/permissions",
            &nobody_token(&app),
            json!({ "application": "a", "context": "b", "aggregate": "c", "action": "d" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Delete, then delete again: 204 both times.
    for _ in 0..2 {
        let (status, _) = read_json(
            app.delete("/api/roles/permissions/shop:orders:order:ship", &admin)
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let rows: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM iam_permissions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(rows.0, 0);
    assert_eq!(
        app.event_count_by_type("platform:admin:permission:deleted")
            .await,
        1
    );
    let (status, _) = read_json(
        app.delete(
            "/api/roles/permissions/shop:orders:order:ship",
            &nobody_token(&app),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
