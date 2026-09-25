//! Routes Go serves that Rust lacked (parity run 1, root cause 4), each
//! against its Go behaviour: happy path, permission and validation errors.
//! Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

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

// ── Service accounts ─────────────────────────────────────────────────────

async fn create_sa(app: &TestApp, admin: &str, code: &str) -> Value {
    assert_status(
        app.post(
            "/api/service-accounts",
            admin,
            json!({ "code": code, "name": "Parity bot", "scope": "ANCHOR" }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_service_account_token_is_minted_and_deactivation_stops_it() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let created = create_sa(&app, &admin, "mint-bot").await;
    let id = created["serviceAccount"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let body = assert_status(
        app.post(
            &format!("/api/service-accounts/{id}/token"),
            &admin,
            json!({}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["tokenType"], "Bearer");
    assert_eq!(body["expiresIn"], 3600);
    let token = body["accessToken"].as_str().unwrap();
    let claims = app.auth_service.validate_token(token).expect("valid token");
    assert_eq!(claims.sub, id);
    assert_eq!(
        app.event_count_by_type("platform:iam:serviceaccount:token-minted")
            .await,
        1
    );

    // Permission and not-found errors.
    let (status, _) = read_json(
        app.post(
            &format!("/api/service-accounts/{id}/token"),
            &nobody_token(&app),
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = read_json(
        app.post("/api/service-accounts/sac_nope/token", &admin, json!({}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Deactivate (twice: idempotent), then the mint refuses.
    for _ in 0..2 {
        let (status, _) = read_json(
            app.post(
                &format!("/api/service-accounts/{id}/deactivate"),
                &admin,
                json!({}),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let (status, body) = read_json(
        app.post(
            &format!("/api/service-accounts/{id}/token"),
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "SERVICE_ACCOUNT_INACTIVE");
    assert_eq!(
        app.event_count_by_type("platform:iam:serviceaccount:deactivated")
            .await,
        2
    );

    let (status, _) = read_json(
        app.post(
            &format!("/api/service-accounts/{id}/deactivate"),
            &nobody_token(&app),
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = read_json(
        app.post(
            "/api/service-accounts/sac_nope/deactivate",
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn go_regenerate_spellings_are_aliases() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let created = create_sa(&app, &admin, "regen-bot").await;
    let id = created["serviceAccount"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let body = assert_status(
        app.post(
            &format!("/api/service-accounts/{id}/regenerate-token"),
            &admin,
            json!({}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["id"], id.as_str());
    assert!(body["authToken"].as_str().is_some());
    let body = assert_status(
        app.post(
            &format!("/api/service-accounts/{id}/regenerate-secret"),
            &admin,
            json!({}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["id"], id.as_str());
    assert!(body["signingSecret"].as_str().is_some());
    let (status, _) = read_json(
        app.post(
            &format!("/api/service-accounts/{id}/regenerate-secret"),
            &nobody_token(&app),
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ── Clients ──────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn clients_are_searched_by_body() {
    use fc_platform::client::entity::Client;
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    for (name, identifier) in [
        ("Acme Ltd", "acme"),
        ("Beta", "beta-acme"),
        ("Gamma", "gamma"),
    ] {
        app.repos
            .client_repo
            .insert(&Client::new(name, identifier))
            .await
            .unwrap();
    }
    let body = assert_status(
        app.post("/api/clients/search", &admin, json!({ "term": "acme" }))
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["total"], 2);
    assert_eq!(body["clients"][0]["identifier"], "acme");
    assert_eq!(body["clients"][1]["identifier"], "beta-acme");

    // An empty term matches everything.
    let body = assert_status(
        app.post("/api/clients/search", &admin, json!({ "term": "" }))
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["total"], 3);

    // Anchor alone is not enough; a non-anchor is refused.
    let (status, _) = read_json(
        app.post(
            "/api/clients/search",
            &app.anchor_token(),
            json!({ "term": "a" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = read_json(
        app.post(
            "/api/clients/search",
            &nobody_token(&app),
            json!({ "term": "a" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ── Platform config ──────────────────────────────────────────────────────

/// A client user holding `role` (and nothing else).
fn role_holder_token(app: &TestApp, role: &str) -> String {
    use fc_platform::service_account::entity::RoleAssignment;
    let mut p = Principal::new_user("reader@flowcatalyst.test", UserScope::Client)
        .with_client_id("clt_reader");
    p.roles = vec![RoleAssignment::new(role)];
    app.auth_service.generate_access_token(&p).expect("token")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn config_properties_are_set_read_and_deleted_as_go() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let base = "/api/config/parity-unregistered/section-a";

    let set = assert_status(
        app.put(
            &format!("{base}/prop-one"),
            &admin,
            json!({ "value": "hello", "description": "d" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let id = set["id"].as_str().unwrap().to_string();
    assert_eq!(set["scope"], "GLOBAL");
    assert!(set.get("clientId").is_none());

    // Update in place.
    let again = assert_status(
        app.put(
            &format!("{base}/prop-one"),
            &admin,
            json!({ "value": "hello again" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(again["id"], id.as_str());
    let got = assert_status(
        app.get(&format!("{base}/prop-one"), &admin).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(got["value"], "hello again");

    // A client-scoped write leaves the global one alone.
    let scoped = assert_status(
        app.put(
            &format!("{base}/prop-one?clientId=clt_x"),
            &admin,
            json!({ "value": "client value" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(scoped["scope"], "CLIENT");
    assert_ne!(scoped["id"], id.as_str());
    let got = assert_status(
        app.get(&format!("{base}/prop-one"), &admin).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(got["value"], "hello again");

    // A secret reads back unmasked to an anchor.
    assert_status(
        app.put(
            &format!("{base}/secret-one"),
            &admin,
            json!({ "value": "s3kret", "valueType": "SECRET" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let got = assert_status(
        app.get(&format!("{base}/secret-one"), &admin).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(got["value"], "s3kret");

    let (status, body) = read_json(
        app.put(
            &format!("{base}/bad"),
            &admin,
            json!({ "value": "v", "valueType": "NOPE" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "INVALID_VALUE_TYPE");

    let list = assert_status(
        app.get("/api/platform-config/parity-unregistered", &admin)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 3);

    // Delete twice (idempotent), then 404.
    for _ in 0..2 {
        let (status, _) = read_json(app.delete(&format!("{base}/prop-one"), &admin).await).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let (status, body) = read_json(app.get(&format!("{base}/prop-one"), &admin).await).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // No grant: refused. A read grant: reads, secrets masked, no writes.
    let reader = role_holder_token(&app, "parity:config-reader");
    let (status, _) = read_json(app.get(&format!("{base}/secret-one"), &reader).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let grant = assert_status(
        app.post(
            "/api/platform-config/parity-unregistered/access",
            &admin,
            json!({ "roleCode": "parity:config-reader", "canWrite": false }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let grant_id = grant["id"].as_str().unwrap().to_string();
    let got = assert_status(
        app.get(&format!("{base}/secret-one"), &reader).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(got["value"], "***");
    let (status, _) = read_json(
        app.put(
            &format!("{base}/secret-one"),
            &reader,
            json!({ "value": "x" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = read_json(app.delete(&format!("{base}/secret-one"), &reader).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Access grants: list, regrant in place, revoke by id.
    let list = assert_status(
        app.get("/api/platform-config/parity-unregistered/access", &admin)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(list["items"][0]["roleCode"], "parity:config-reader");
    let regrant = assert_status(
        app.post(
            "/api/platform-config/parity-unregistered/access",
            &admin,
            json!({ "roleCode": "parity:config-reader", "canWrite": true }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(regrant["id"], grant_id.as_str());
    let (status, _) = read_json(
        app.post(
            "/api/platform-config/parity-unregistered/access",
            &app.anchor_token(),
            json!({ "roleCode": "x", "canWrite": true }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = read_json(
        app.delete(&format!("/api/platform-config/access/{grant_id}"), &admin)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = read_json(
        app.delete(&format!("/api/platform-config/access/{grant_id}"), &admin)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── Router config ────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_router_config_lists_pools_and_tenant_queues() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    sqlx::query(
        "INSERT INTO msg_dispatch_pools (id, code, name, rate_limit, concurrency, client_identifier, status) \
         VALUES ('dpl_a', 'fast', 'Fast', 60, 5, 'acme', 'ARCHIVED'), \
                ('dpl_b', 'DEFAULT-POOL', 'Default', NULL, 10, NULL, 'ACTIVE')",
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let body = assert_status(
        app.get("/api/dispatch/router-config", &admin).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        body["processingPools"],
        json!([
            { "code": "platform-DEFAULT-POOL", "concurrency": 10 },
            { "code": "acme-fast", "concurrency": 5, "rateLimitPerMinute": 60 }
        ])
    );
    let names: Vec<&str> = body["queues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["queueName"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["platform-DEFAULT", "acme-DEFAULT"]);
    assert_eq!(body["queues"][0]["connections"], 0);
    assert!(body["queues"][0]["queueUri"]
        .as_str()
        .unwrap()
        .starts_with("postgres"));

    // Anchor alone, or the permission alone, is not enough.
    let (status, _) = read_json(
        app.get("/api/dispatch/router-config", &app.anchor_token())
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = read_json(
        app.get("/api/dispatch/router-config", &nobody_token(&app))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
