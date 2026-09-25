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
    // A client with no pool or subscription still gets both its queues.
    app.repos
        .client_repo
        .insert(&fc_platform::client::entity::Client::new("Solo", "solo"))
        .await
        .unwrap();
    let body = assert_status(
        app.get("/api/dispatch/router-config", &admin).await,
        StatusCode::OK,
    )
    .await;
    let names: Vec<&str> = body["queues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["queueName"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "platform-DEFAULT",
            "platform-HIGH_PRIORITY",
            "acme-DEFAULT",
            "acme-HIGH_PRIORITY",
            "solo-DEFAULT",
            "solo-HIGH_PRIORITY"
        ]
    );
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

// ── Email-domain mappings ────────────────────────────────────────────────

async fn create_idp(app: &TestApp, admin: &str, code: &str) -> String {
    let body = assert_status(
        app.post(
            "/api/identity-providers",
            admin,
            json!({ "code": code, "name": code, "type": "INTERNAL", "oidcMultiTenant": false }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    body["id"].as_str().expect("idp id").to_string()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn email_domain_mappings_are_created_looked_up_and_moved_as_go() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let idp_y = create_idp(&app, &admin, "parity-idp-y").await;

    // Go does not require the provider to exist yet.
    let created = assert_status(
        app.post(
            "/api/email-domain-mappings",
            &admin,
            json!({ "emailDomain": "parity.example.test", "identityProviderId": "idp_parity_x",
                    "scopeType": "CLIENT", "primaryClientId": "clt_x" }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let edm_id = created["id"].as_str().unwrap().to_string();

    for (body, status, code) in [
        (
            json!({ "emailDomain": "parity.example.test", "identityProviderId": "idp_x", "scopeType": "ANCHOR" }),
            StatusCode::CONFLICT,
            "DOMAIN_ALREADY_MAPPED",
        ),
        (
            json!({ "emailDomain": "nodot", "identityProviderId": "idp_x", "scopeType": "ANCHOR" }),
            StatusCode::BAD_REQUEST,
            "INVALID_EMAIL_DOMAIN",
        ),
        (
            json!({ "emailDomain": "a.example.test", "identityProviderId": "idp_x", "scopeType": "GLOBAL" }),
            StatusCode::BAD_REQUEST,
            "INVALID_SCOPE_TYPE",
        ),
        (
            json!({ "emailDomain": "b.example.test", "identityProviderId": "idp_x", "scopeType": "PARTNER" }),
            StatusCode::BAD_REQUEST,
            "PRIMARY_CLIENT_REQUIRED",
        ),
    ] {
        let (s, b) = read_json(app.post("/api/email-domain-mappings", &admin, body).await).await;
        assert_eq!(s, status, "{b}");
        assert_eq!(b["code"], code);
    }

    // Lookup: no auth needed; {found:false} when absent; 400 without a domain.
    let found = assert_status(
        app.get_unauth("/api/email-domain-mappings/lookup?domain=parity.example.test")
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(found["id"], edm_id.as_str());
    let missing = assert_status(
        app.get_unauth("/api/email-domain-mappings/lookup?domain=nope.example.test")
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(missing, json!({ "found": false }));
    let (s, b) = read_json(app.get_unauth("/api/email-domain-mappings/lookup").await).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(b["code"], "DOMAIN_REQUIRED");

    // By domain: anchor + view permission.
    let got = assert_status(
        app.get(
            "/api/email-domain-mappings/by-domain/parity.example.test",
            &admin,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(got["id"], edm_id.as_str());
    let (s, _) = read_json(
        app.get(
            "/api/email-domain-mappings/by-domain/nope.example.test",
            &admin,
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = read_json(
        app.get(
            "/api/email-domain-mappings/by-domain/parity.example.test",
            &app.anchor_token(),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // A federated user of the domain, with an IdP-synced and an admin role.
    sqlx::query(
        "INSERT INTO iam_principals (id, type, scope, name, active, email, email_domain, idp_type, external_idp_id) \
         VALUES ('prn_fed', 'USER', 'CLIENT', 'Fed', true, 'fed@parity.example.test', 'parity.example.test', 'OIDC', 'ext-1')",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO iam_principal_roles (principal_id, role_name, assignment_source) \
         VALUES ('prn_fed', 'parity:synced', 'IDP_SYNC'), ('prn_fed', 'parity:kept', 'ADMIN_ASSIGNED')",
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let moved = assert_status(
        app.post(
            &format!("/api/email-domain-mappings/{edm_id}/move-provider"),
            &admin,
            json!({ "identityProviderId": idp_y }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        moved,
        json!({
            "mappingId": edm_id,
            "emailDomain": "parity.example.test",
            "fromIdentityProviderId": "idp_parity_x",
            "toIdentityProviderId": idp_y,
            "usersReset": 1
        })
    );
    let (idp_type, ext): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT idp_type, external_idp_id FROM iam_principals WHERE id = 'prn_fed'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(idp_type.as_deref(), Some("INTERNAL"));
    assert_eq!(ext, None);
    let roles: Vec<(String,)> =
        sqlx::query_as("SELECT role_name FROM iam_principal_roles WHERE principal_id = 'prn_fed'")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(roles, vec![("parity:kept".to_string(),)]);
    assert_eq!(
        app.event_count_by_type("platform:admin:email-domain-mapping:provider-changed")
            .await,
        1
    );

    for (id, body, status) in [
        (
            edm_id.as_str(),
            json!({ "identityProviderId": idp_y }),
            StatusCode::CONFLICT,
        ),
        (
            edm_id.as_str(),
            json!({ "identityProviderId": "idp_none" }),
            StatusCode::NOT_FOUND,
        ),
        (edm_id.as_str(), json!({}), StatusCode::BAD_REQUEST),
        (
            "edm_nope",
            json!({ "identityProviderId": idp_y }),
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (s, b) = read_json(
            app.post(
                &format!("/api/email-domain-mappings/{id}/move-provider"),
                &admin,
                body,
            )
            .await,
        )
        .await;
        assert_eq!(s, status, "{b}");
    }
    let (s, _) = read_json(
        app.post(
            &format!("/api/email-domain-mappings/{edm_id}/move-provider"),
            &nobody_token(&app),
            json!({ "identityProviderId": idp_y }),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

// ── Principals ───────────────────────────────────────────────────────────

async fn insert_client(app: &TestApp, identifier: &str) -> String {
    let c = fc_platform::client::entity::Client::new(identifier.to_uppercase(), identifier);
    app.repos.client_repo.insert(&c).await.unwrap();
    c.id
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn principals_are_bulk_imported_versioned_and_reassociated() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let acme = insert_client(&app, "acme").await;
    let other = insert_client(&app, "other").await;
    create_role(&app, &admin, "importer").await;
    // other.example.test belongs to another client.
    assert_status(
        app.post(
            "/api/email-domain-mappings",
            &admin,
            json!({ "emailDomain": "other.example.test", "identityProviderId": "idp_x",
                    "scopeType": "CLIENT", "primaryClientId": other }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;

    let body = assert_status(
        app.post(
            "/api/principals/bulk-import",
            &admin,
            json!({ "clientId": acme, "users": [
                { "name": "Ann", "email": " Ann@Acme.Test ", "roles": ["parity:importer"] },
                { "name": "Ann again", "email": "ann@acme.test" },
                { "name": "No at", "email": "nope" },
                { "name": "", "email": "blank@acme.test" },
                { "name": "Olly", "email": "olly@other.example.test" },
                { "name": "Bob", "email": "bob@acme.test", "roles": ["parity:missing"] }
            ]}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let statuses: Vec<&str> = body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["status"].as_str().unwrap())
        .collect();
    assert_eq!(
        statuses,
        ["created", "error", "error", "error", "dropped", "created"],
        "{body}"
    );
    assert_eq!(body["results"][0]["email"], "ann@acme.test");
    assert!(body["results"][0].get("message").is_none());
    assert_eq!(body["results"][1]["message"], "duplicate email in file");
    assert!(body["results"][5]["message"]
        .as_str()
        .unwrap()
        .starts_with("created, but roles not applied"));
    assert_eq!(body["created"], 2);
    assert_eq!(body["skipped"], 1);
    assert_eq!(body["failed"], 3);
    let ann = app
        .repos
        .principal_repo
        .find_by_email("ann@acme.test")
        .await
        .unwrap()
        .expect("ann");
    assert_eq!(ann.client_id.as_deref(), Some(acme.as_str()));
    assert!(ann.roles.iter().any(|r| r.role == "parity:importer"));

    // A second import skips the existing user.
    let body = assert_status(
        app.post(
            "/api/principals/bulk-import",
            &admin,
            json!({ "clientId": acme, "users": [{ "name": "Ann", "email": "ann@acme.test" }] }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["results"][0]["status"], "exists");

    for (body, code) in [
        (json!({ "clientId": " ", "users": [] }), "CLIENT_REQUIRED"),
        (json!({ "clientId": acme, "users": [] }), "NO_ROWS"),
    ] {
        let (s, b) = read_json(app.post("/api/principals/bulk-import", &admin, body).await).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(b["code"], code);
    }
    let (s, b) = read_json(
        app.post(
            "/api/principals/bulk-import",
            &nobody_token(&app),
            json!({ "clientId": acme, "users": [{ "name": "x", "email": "x@acme.test" }] }),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(b["code"], "SCOPE_FORBIDDEN");

    // Version: the admin reads anyone's; a stranger's is a 404.
    let v = assert_status(
        app.get(&format!("/api/principals/{}/version", ann.id), &admin)
            .await,
        StatusCode::OK,
    )
    .await;
    assert!(v["updatedAt"].as_str().unwrap().ends_with('Z'));
    let (s, _) = read_json(app.get("/api/principals/prn_nope/version", &admin).await).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = read_json(
        app.get(
            &format!("/api/principals/{}/version", ann.id),
            &nobody_token(&app),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // Anyone may read their own.
    let own = app.auth_service.generate_access_token(&ann).unwrap();
    assert_status(
        app.get(&format!("/api/principals/{}/version", ann.id), &own)
            .await,
        StatusCode::OK,
    )
    .await;

    // Client association: to partner keeps the old home as a grant.
    let p = assert_status(
        app.put(
            &format!("/api/principals/{}/client-association", ann.id),
            &admin,
            json!({ "clientId": other, "mode": "to_partner" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(p["scope"], "PARTNER");
    let ann = app
        .repos
        .principal_repo
        .find_by_id(&ann.id)
        .await
        .unwrap()
        .unwrap();
    let mut grants = ann.assigned_clients.clone();
    grants.sort();
    let mut want = vec![acme.clone(), other.clone()];
    want.sort();
    assert_eq!(grants, want);
    let p = assert_status(
        app.put(
            &format!("/api/principals/{}/client-association", ann.id),
            &admin,
            json!({ "clientId": acme, "mode": "CHANGE_CLIENT" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(p["scope"], "CLIENT");
    assert_eq!(p["clientId"], acme.as_str());
    let p = assert_status(
        app.put(
            &format!("/api/principals/{}/client-association", ann.id),
            &admin,
            json!({ "clientId": "*" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(p["scope"], "ANCHOR");

    for (body, status, code) in [
        (
            json!({ "clientId": acme }),
            StatusCode::BAD_REQUEST,
            "MODE_REQUIRED",
        ),
        (
            json!({ "clientId": "" }),
            StatusCode::BAD_REQUEST,
            "CLIENT_ID_REQUIRED",
        ),
        (
            json!({ "clientId": "clt_nope", "mode": "CHANGE_CLIENT" }),
            StatusCode::NOT_FOUND,
            "CLIENT_NOT_FOUND",
        ),
    ] {
        let (s, b) = read_json(
            app.put(
                &format!("/api/principals/{}/client-association", ann.id),
                &admin,
                body,
            )
            .await,
        )
        .await;
        assert_eq!(s, status, "{b}");
        assert_eq!(b["code"], code);
    }
    let (s, _) = read_json(
        app.put(
            &format!("/api/principals/{}/client-association", ann.id),
            &app.anchor_token(),
            json!({ "clientId": "*" }),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}
