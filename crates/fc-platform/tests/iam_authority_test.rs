//! IAM authority: anchor scope is reach, never authority (Java 6068fe6b,
//! owner decisions #19 and #25), and nobody hands out authority they do not
//! hold (owner rulings 13-15 of 2026-09-25). Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions;
use support::{read_json, TestApp};

/// Service-account creation encrypts the generated credentials; any 32-byte
/// key will do.
async fn setup() -> TestApp {
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );
    TestApp::setup().await
}

/// A token whose `scope` grants exactly `perms`, for a user principal of the
/// given tier that is not stored (so it reaches no application).
fn caller(app: &TestApp, scope: UserScope, client: Option<&str>, perms: &[&str]) -> String {
    let mut p = Principal::new_user(format!("caller-{}@iam.test", perms.len()), scope);
    if let Some(c) = client {
        p = p.with_client_id(c);
    }
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .expect("token")
}

/// As [`caller`], for a principal that is stored first (a new user reaches
/// every application).
async fn stored_caller(app: &TestApp, email: &str, scope: UserScope, perms: &[&str]) -> String {
    let p = Principal::new_user(email, scope);
    app.repos
        .principal_repo
        .insert(&p)
        .await
        .expect("insert caller");
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .expect("token")
}

async fn create_client(app: &TestApp, identifier: &str) -> String {
    let client = fc_platform::client::entity::Client::new(identifier.to_uppercase(), identifier);
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");
    client.id
}

fn code(body: &Value) -> &str {
    body["error"].as_str().unwrap_or_default()
}

async fn create_sa(app: &TestApp, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(app.post("/api/service-accounts", token, body).await).await
}

// ── Service accounts (fix S3) ────────────────────────────────────────────

/// Role assignment and credential regeneration need anchor scope and
/// `service-account:update`; neither alone will do.
#[tokio::test]
#[ignore = "requires Docker"]
async fn service_account_credentials_and_roles_need_anchor_and_update() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let (status, body) =
        create_sa(&app, &admin, json!({ "code": "sa-gate", "name": "Gate" })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["serviceAccount"]["id"].as_str().unwrap().to_string();
    let clt = create_client(&app, "sa-gate-clt").await;

    let anchor_without = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::admin::SERVICE_ACCOUNT_CREATE],
    );
    let client_with = caller(
        &app,
        UserScope::Client,
        Some(&clt),
        &[permissions::admin::SERVICE_ACCOUNT_UPDATE],
    );
    let anchor_with = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::admin::SERVICE_ACCOUNT_UPDATE],
    );

    for (method, path) in [
        ("PUT", format!("/api/service-accounts/{id}/roles")),
        ("PUT", format!("/api/service-accounts/{id}/auth-token")),
        (
            "POST",
            format!("/api/service-accounts/{id}/regenerate-auth-token"),
        ),
        (
            "POST",
            format!("/api/service-accounts/{id}/regenerate-signing-secret"),
        ),
    ] {
        let send = |token: String| {
            let path = path.clone();
            let app = &app;
            async move {
                let resp = if method == "PUT" {
                    app.put(&path, &token, json!({ "roles": [] })).await
                } else {
                    app.post(&path, &token, json!({})).await
                };
                read_json(resp).await
            }
        };
        let (status, body) = send(anchor_without.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert_eq!(code(&body), "PERMISSION_REQUIRED", "{path}");
        let (status, body) = send(client_with.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert_eq!(code(&body), "ANCHOR_REQUIRED", "{path}");
        let (status, body) = send(anchor_with.clone()).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
    }
}

/// Go's `allApplications` opt-in: off by default; on only for a caller that
/// itself reaches every application; never alongside `applicationId`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn service_account_all_applications_opt_in() {
    let app = setup().await;
    let writer = [permissions::admin::SERVICE_ACCOUNT_CREATE];

    // Default: no application access.
    let all_apps = stored_caller(&app, "all-apps@iam.test", UserScope::Anchor, &writer).await;
    let (status, body) = create_sa(
        &app,
        &all_apps,
        json!({ "code": "sa-none", "name": "None" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let p = app
        .repos
        .principal_repo
        .find_by_id(body["principalId"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(!p.all_applications);

    // Asked for by an all-applications caller: granted.
    let (status, body) = create_sa(
        &app,
        &all_apps,
        json!({ "code": "sa-all", "name": "All", "allApplications": true }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let p = app
        .repos
        .principal_repo
        .find_by_id(body["principalId"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(p.all_applications);

    // With applicationId: 400.
    let (status, body) = create_sa(
        &app,
        &all_apps,
        json!({ "code": "sa-both", "name": "Both", "allApplications": true, "applicationId": "app_x" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(code(&body), "ALL_APPLICATIONS_WITH_APPLICATION_ID");

    // A caller that reaches no application: 403, nothing created.
    let confined = caller(&app, UserScope::Anchor, None, &writer);
    let (status, body) = create_sa(
        &app,
        &confined,
        json!({ "code": "sa-escalate", "name": "Escalate", "allApplications": true }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(app
        .repos
        .service_account_repo
        .find_by_code("sa-escalate")
        .await
        .unwrap()
        .is_none());
}

// ── Principals (fix S4) ──────────────────────────────────────────────────

async fn stored_user(app: &TestApp, email: &str, scope: UserScope, client: Option<&str>) -> String {
    let mut p = Principal::new_user(email, scope);
    if let Some(c) = client {
        p = p.with_client_id(c);
    }
    app.repos
        .principal_repo
        .insert(&p)
        .await
        .expect("insert user");
    p.id
}

/// Every principal write needs a user permission on top of its tier check:
/// an anchor with none is refused, whatever the route.
#[tokio::test]
#[ignore = "requires Docker"]
async fn principal_writes_need_a_user_permission_at_every_tier() {
    let app = setup().await;
    let target = stored_user(&app, "target@iam.test", UserScope::Client, None).await;
    let bare_anchor = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::iam::USER_READ],
    );

    let posts = [
        (
            "/api/principals/users".to_string(),
            json!({ "email": "new@iam.test", "name": "New" }),
        ),
        (format!("/api/principals/{target}/activate"), json!({})),
        (format!("/api/principals/{target}/deactivate"), json!({})),
        (
            format!("/api/principals/{target}/reset-password"),
            json!({ "newPassword": "An0ther-Passw0rd!" }),
        ),
        (
            format!("/api/principals/{target}/send-password-reset"),
            json!({}),
        ),
        (
            "/api/principals/sync".to_string(),
            json!({ "principals": [{ "email": "synced@iam.test", "name": "S" }] }),
        ),
    ];
    for (path, body) in posts {
        let (status, resp) = read_json(app.post(&path, &bare_anchor, body).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {resp}");
        // The sync's own check answers with its older FORBIDDEN body.
        if !path.ends_with("/sync") {
            assert_eq!(code(&resp), "PERMISSION_REQUIRED", "{path}");
        }
    }
    for (path, body) in [
        (
            format!("/api/principals/{target}"),
            json!({ "name": "Renamed" }),
        ),
        (
            format!("/api/principals/{target}/application-access"),
            json!({ "applicationIds": [] }),
        ),
    ] {
        let (status, resp) = read_json(app.put(&path, &bare_anchor, body).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {resp}");
        assert_eq!(code(&resp), "PERMISSION_REQUIRED", "{path}");
    }
    let (status, resp) = read_json(
        app.delete(&format!("/api/principals/{target}"), &bare_anchor)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_REQUIRED");

    // Nothing was touched.
    let p = app
        .repos
        .principal_repo
        .find_by_id(&target)
        .await
        .unwrap()
        .unwrap();
    assert!(p.active);
    assert_ne!(p.name, "Renamed");
    assert!(app
        .repos
        .principal_repo
        .find_by_email("synced@iam.test")
        .await
        .unwrap()
        .is_none());

    // Update and delete lands with their permissions; delete needs delete.
    let updater = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::iam::USER_UPDATE],
    );
    let (status, resp) = read_json(
        app.put(
            &format!("/api/principals/{target}"),
            &updater,
            json!({ "name": "Renamed" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    let (status, resp) = read_json(
        app.delete(&format!("/api/principals/{target}"), &updater)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_REQUIRED");
    let deleter = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::iam::USER_DELETE],
    );
    let resp = app
        .delete(&format!("/api/principals/{target}"), &deleter)
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

/// The platform-level user sync is anchor-only, as every other principal
/// write here.
#[tokio::test]
#[ignore = "requires Docker"]
async fn platform_user_sync_needs_anchor() {
    let app = setup().await;
    let clt = create_client(&app, "sync-tier").await;
    let client_admin = caller(
        &app,
        UserScope::Client,
        Some(&clt),
        &[permissions::iam::USER_CREATE],
    );
    let (status, resp) = read_json(
        app.post(
            "/api/principals/sync",
            &client_admin,
            json!({ "principals": [{ "email": "x@iam.test", "name": "X" }] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert!(app
        .repos
        .principal_repo
        .find_by_email("x@iam.test")
        .await
        .unwrap()
        .is_none());
}

/// A client administrator may update its client's CLIENT-tier users only,
/// never a partner homed at that client (Go's `blockNonClientTarget`).
#[tokio::test]
#[ignore = "requires Docker"]
async fn client_admin_updates_client_users_only() {
    let app = setup().await;
    let clt = create_client(&app, "upd-tier").await;
    let member = stored_user(&app, "member@iam.test", UserScope::Client, Some(&clt)).await;
    let partner = stored_user(&app, "partner@iam.test", UserScope::Partner, Some(&clt)).await;
    let client_admin = caller(
        &app,
        UserScope::Client,
        Some(&clt),
        &[permissions::iam::USER_UPDATE],
    );

    let (status, resp) = read_json(
        app.put(
            &format!("/api/principals/{member}"),
            &client_admin,
            json!({ "name": "Member Renamed" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    let (status, resp) = read_json(
        app.put(
            &format!("/api/principals/{partner}"),
            &client_admin,
            json!({ "name": "Partner Renamed" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");

    // And without the permission, not even its own client's users.
    let reader = caller(
        &app,
        UserScope::Client,
        Some(&clt),
        &[permissions::iam::USER_READ],
    );
    let (status, resp) = read_json(
        app.put(
            &format!("/api/principals/{member}"),
            &reader,
            json!({ "name": "Nope" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_REQUIRED");
}

// ── Platform-owner routes: Go's anchorWith(perm) (fix S4) ────────────────

/// Client, identity-provider, email-domain, anchor-domain, auth-config and
/// CORS routes need anchor reach and the family's permission.
#[tokio::test]
#[ignore = "requires Docker"]
async fn platform_owner_routes_need_anchor_and_the_permission() {
    let app = setup().await;
    let clt = create_client(&app, "owner-clt").await;
    let bare_anchor = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::iam::USER_READ],
    );

    let posts = [
        ("/api/clients", json!({ "identifier": "c2", "name": "C2" })),
        (
            "/api/identity-providers",
            json!({ "code": "idp-x", "name": "IdP", "type": "INTERNAL" }),
        ),
        (
            "/api/email-domain-mappings",
            json!({ "emailDomain": "x.test", "identityProviderId": "idp_x", "scopeType": "ANCHOR" }),
        ),
        ("/api/anchor-domains", json!({ "domain": "anchor.test" })),
        ("/api/platform/cors", json!({ "origin": "https://x.test" })),
    ];
    for (path, body) in &posts {
        let (status, resp) = read_json(app.post(path, &bare_anchor, body.clone()).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {resp}");
        assert_eq!(code(&resp), "PERMISSION_REQUIRED", "{path}");
    }
    for path in [
        "/api/anchor-domains".to_string(),
        "/api/auth-configs".to_string(),
        "/api/idp-role-mappings".to_string(),
    ] {
        let (status, resp) = read_json(app.get(&path, &bare_anchor).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {resp}");
        assert_eq!(code(&resp), "PERMISSION_REQUIRED", "{path}");
    }
    for path in [
        format!("/api/clients/{clt}/suspend"),
        format!("/api/clients/{clt}/deactivate"),
        format!("/api/clients/{clt}/activate"),
    ] {
        let (status, resp) =
            read_json(app.post(&path, &bare_anchor, json!({"reason": "x"})).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {resp}");
        assert_eq!(code(&resp), "PERMISSION_REQUIRED", "{path}");
    }

    // The permission without anchor reach: refused on the tier.
    let client_creator = caller(
        &app,
        UserScope::Client,
        Some(&clt),
        &[permissions::admin::CLIENT_CREATE],
    );
    let (status, resp) = read_json(
        app.post(
            "/api/clients",
            &client_creator,
            json!({ "identifier": "c3", "name": "C3" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "ANCHOR_REQUIRED");

    // Both: the write lands.
    let creator = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::admin::CLIENT_CREATE],
    );
    let (status, resp) = read_json(
        app.post(
            "/api/clients",
            &creator,
            json!({ "identifier": "c4", "name": "C4" }),
        )
        .await,
    )
    .await;
    assert!(status.is_success(), "{status}: {resp}");
    let domains = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::admin::ANCHOR_DOMAIN_READ],
    );
    let (status, resp) = read_json(app.get("/api/anchor-domains", &domains).await).await;
    assert_eq!(status, StatusCode::OK, "{resp}");
}
