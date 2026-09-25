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

// ── Decision #25: roles, client access, applications ─────────────────────

/// `/api/roles`, `/bff/roles` writes and client-access grants need anchor
/// and the permission; application writes and provisioning need their
/// application, service-account or OAuth-client permission too.
#[tokio::test]
#[ignore = "requires Docker"]
async fn roles_client_access_and_applications_need_anchor_and_the_permission() {
    let app = setup().await;
    let clt = create_client(&app, "d25-clt").await;
    let user = stored_user(&app, "d25@iam.test", UserScope::Partner, Some(&clt)).await;
    let application = fc_platform::application::entity::Application::new("d25", "D25");
    app.repos
        .application_repo
        .insert(&application)
        .await
        .unwrap();
    let bare_anchor = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::iam::USER_READ],
    );

    let role_body = json!({
        "applicationCode": "d25", "roleName": "clerk", "displayName": "Clerk", "permissions": []
    });
    let posts = [
        ("/api/roles".to_string(), role_body.clone()),
        ("/bff/roles".to_string(), role_body.clone()),
        ("/bff/roles/sync-platform".to_string(), json!({})),
        (
            format!("/api/principals/{user}/client-access"),
            json!({ "clientId": clt }),
        ),
        (
            "/api/applications".to_string(),
            json!({ "code": "d25b", "name": "D25b" }),
        ),
        (
            format!(
                "/api/applications/{}/provision-service-account",
                application.id
            ),
            json!({}),
        ),
        (
            format!(
                "/api/applications/{}/provision-login-client",
                application.id
            ),
            json!({ "redirectUris": ["https://d25.test/cb"] }),
        ),
        (
            format!("/api/applications/{}/clients/{clt}/enable", application.id),
            json!({}),
        ),
    ];
    for (path, body) in &posts {
        let (status, resp) = read_json(app.post(path, &bare_anchor, body.clone()).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {resp}");
        assert_eq!(code(&resp), "PERMISSION_REQUIRED", "{path}");
    }
    let (status, resp) = read_json(
        app.delete(
            &format!("/api/principals/{user}/client-access/{clt}"),
            &bare_anchor,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_REQUIRED");

    // The role permission without anchor reach is refused on the tier.
    let client_role_writer = caller(
        &app,
        UserScope::Client,
        Some(&clt),
        &[permissions::iam::ROLE_CREATE],
    );
    let (status, resp) = read_json(
        app.post("/api/roles", &client_role_writer, role_body.clone())
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "ANCHOR_REQUIRED");

    // Both: the writes land.
    let role_writer = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::iam::ROLE_CREATE],
    );
    let (status, resp) = read_json(app.post("/api/roles", &role_writer, role_body).await).await;
    assert_eq!(status, StatusCode::CREATED, "{resp}");
    let granter = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::iam::CLIENT_ACCESS_GRANT],
    );
    let other = create_client(&app, "d25-other").await;
    let (status, resp) = read_json(
        app.post(
            &format!("/api/principals/{user}/client-access"),
            &granter,
            json!({ "clientId": other }),
        )
        .await,
    )
    .await;
    assert!(status.is_success(), "{status}: {resp}");
}

// ── Rulings 13 + 14: the role ceiling ────────────────────────────────────

/// Seed the built-in roles a test hands out (the harness runs no start-up
/// role seeding).
async fn seed_roles(app: &TestApp, roles: Vec<fc_platform::role::entity::AuthRole>) {
    for role in roles {
        if app
            .repos
            .role_repo
            .find_by_name(&role.name)
            .await
            .unwrap()
            .is_none()
        {
            app.repos.role_repo.insert(&role).await.unwrap();
        }
    }
}

/// An anchor caller holding the built-in `platform:iam-admin` permissions
/// plus `extra`.
fn iam_admin(app: &TestApp, extra: &[&str]) -> String {
    let role = fc_platform::role::entity::roles::iam_admin();
    let mut perms: Vec<&str> = role.permissions.iter().map(String::as_str).collect();
    perms.extend_from_slice(extra);
    caller(app, UserScope::Anchor, None, &perms)
}

async fn roles_of(app: &TestApp, id: &str) -> Vec<String> {
    let mut roles: Vec<String> = app
        .repos
        .principal_repo
        .find_by_id(id)
        .await
        .unwrap()
        .unwrap()
        .roles
        .iter()
        .map(|r| r.role.clone())
        .collect();
    roles.sort();
    roles
}

/// Setting a user's roles needs `user:assign-roles`; the caller may add or
/// remove only roles within its own permissions; kept roles are not checked.
#[tokio::test]
#[ignore = "requires Docker"]
async fn user_roles_are_bounded_by_the_callers_own_permissions() {
    use fc_platform::role::entity::roles;
    let app = setup().await;
    seed_roles(
        &app,
        vec![roles::super_admin(), roles::iam_readonly(), roles::viewer()],
    )
    .await;
    let mut target = Principal::new_user("ceiling@iam.test", UserScope::Anchor);
    target.assign_role("platform:super-admin");
    app.repos.principal_repo.insert(&target).await.unwrap();
    let id = target.id.clone();
    let path = format!("/api/principals/{id}/roles");

    // user:update alone no longer changes roles.
    let updater = caller(
        &app,
        UserScope::Anchor,
        None,
        &[permissions::iam::USER_UPDATE],
    );
    let (status, resp) = read_json(
        app.put(
            &path,
            &updater,
            json!({ "roles": ["platform:super-admin"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_REQUIRED");

    let admin = iam_admin(&app, &[]);
    // A role within: added, the super-admin role kept untouched.
    let (status, resp) = read_json(
        app.put(
            &path,
            &admin,
            json!({ "roles": ["platform:super-admin", "platform:iam-readonly"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(
        roles_of(&app, &id).await,
        vec!["platform:iam-readonly", "platform:super-admin"]
    );

    // Removing the super-admin role counts: refused, named.
    let (status, resp) = read_json(
        app.put(&path, &admin, json!({ "roles": ["platform:iam-readonly"] }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "ROLE_ABOVE_CALLER");
    assert!(resp["message"]
        .as_str()
        .unwrap()
        .contains("platform:super-admin"));

    // Adding a role above (viewer holds admin permissions iam-admin lacks),
    // through each route.
    let (status, resp) = read_json(
        app.post(&path, &admin, json!({ "role": "platform:viewer" }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "ROLE_ABOVE_CALLER");
    let (status, resp) = read_json(
        app.delete(&format!("{path}/platform:super-admin"), &admin)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "ROLE_ABOVE_CALLER");
    assert_eq!(
        roles_of(&app, &id).await,
        vec!["platform:iam-readonly", "platform:super-admin"]
    );

    // A super-admin may.
    let super_admin = caller(&app, UserScope::Anchor, None, &[permissions::ADMIN_ALL]);
    let (status, resp) = read_json(
        app.put(&path, &super_admin, json!({ "roles": ["platform:viewer"] }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(roles_of(&app, &id).await, vec!["platform:viewer"]);
}

/// A service account's roles: service-account:update, and the same ceiling.
#[tokio::test]
#[ignore = "requires Docker"]
async fn service_account_roles_are_bounded_by_the_callers_own_permissions() {
    use fc_platform::role::entity::roles;
    let app = setup().await;
    seed_roles(&app, vec![roles::super_admin(), roles::iam_readonly()]).await;
    let admin = app.anchor_admin_token().await;
    let (_, body) = create_sa(&app, &admin, json!({ "code": "sa-ceiling", "name": "C" })).await;
    let id = body["serviceAccount"]["id"].as_str().unwrap().to_string();
    let path = format!("/api/service-accounts/{id}/roles");

    let sa_admin = iam_admin(&app, &[]);
    let (status, resp) = read_json(
        app.put(
            &path,
            &sa_admin,
            json!({ "roles": ["platform:super-admin"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "ROLE_ABOVE_CALLER");
    let (status, resp) = read_json(
        app.put(
            &path,
            &sa_admin,
            json!({ "roles": ["platform:iam-readonly"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
}

/// The platform-level user sync, IdP role mappings, email-domain allowed
/// roles and role permission edits are bounded the same way.
#[tokio::test]
#[ignore = "requires Docker"]
async fn sync_mappings_and_role_edits_are_bounded() {
    use fc_platform::role::entity::roles;
    let app = setup().await;
    seed_roles(&app, vec![roles::super_admin(), roles::iam_readonly()]).await;
    let admin = iam_admin(&app, &[]);

    // Platform sync: a role above refuses the whole sync; nothing is written.
    let (status, resp) = read_json(
        app.post(
            "/api/principals/sync",
            &admin,
            json!({ "principals": [
                { "email": "ok@iam.test", "name": "Ok", "roles": ["platform:iam-readonly"] },
                { "email": "up@iam.test", "name": "Up", "roles": ["platform:super-admin"] }
            ] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "ROLE_ABOVE_CALLER");
    assert!(app
        .repos
        .principal_repo
        .find_by_email("ok@iam.test")
        .await
        .unwrap()
        .is_none());
    // Within, and application roles (unknown here): accepted as today.
    let (status, resp) = read_json(
        app.post(
            "/api/principals/sync",
            &admin,
            json!({ "principals": [
                { "email": "ok@iam.test", "name": "Ok", "roles": ["platform:iam-readonly", "hr:manager"] }
            ] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");

    // IdP role mapping to a role above.
    let (status, resp) = read_json(
        app.post(
            "/api/idp-role-mappings",
            &admin,
            json!({ "idpType": "OIDC", "idpRoleName": "Admins", "platformRoleName": "platform:super-admin" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "ROLE_ABOVE_CALLER");

    // Email-domain allowedRoleIds naming a role above, by id.
    let super_admin_id = app
        .repos
        .role_repo
        .find_by_name("platform:super-admin")
        .await
        .unwrap()
        .unwrap()
        .id;
    let (status, resp) = read_json(
        app.post(
            "/api/email-domain-mappings",
            &admin,
            json!({
                "emailDomain": "ceiling.test", "identityProviderId": "idp_x",
                "scopeType": "ANCHOR", "allowedRoleIds": [super_admin_id]
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "ROLE_ABOVE_CALLER");

    // Role permission edits: only permissions the caller holds.
    let (status, resp) = read_json(
        app.post(
            "/api/roles",
            &admin,
            json!({
                "applicationCode": "platform", "roleName": "sneaky", "displayName": "Sneaky",
                "permissions": ["platform:*:*:*"]
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_ABOVE_CALLER");
    let (status, resp) = read_json(
        app.post(
            "/api/roles",
            &admin,
            json!({
                "applicationCode": "platform", "roleName": "helper", "displayName": "Helper",
                "permissions": ["platform:iam:user:view"]
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{resp}");
    let (status, resp) = read_json(
        app.post(
            "/api/roles/platform:helper/permissions",
            &admin,
            json!({ "permission": "platform:*:*:*" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_ABOVE_CALLER");
    let (status, resp) = read_json(
        app.put(
            "/bff/roles/platform:helper",
            &admin,
            json!({ "permissions": ["platform:iam:user:view", "platform:admin:client:view"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_ABOVE_CALLER");
}

// ── Ruling 15: a role holds its own application's permissions ────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn roles_hold_only_their_own_applications_permissions() {
    let app = setup().await;
    let application = fc_platform::application::entity::Application::new("d15", "D15");
    app.repos
        .application_repo
        .insert(&application)
        .await
        .unwrap();
    // A role writer holding every permission it names, but no super-admin.
    let writer = caller(
        &app,
        UserScope::Anchor,
        None,
        &[
            permissions::iam::ROLE_CREATE,
            permissions::iam::ROLE_UPDATE,
            permissions::iam::USER_READ,
        ],
    );
    let body = json!({
        "applicationCode": "d15", "roleName": "reader", "displayName": "Reader",
        "permissions": ["d15:thing:view", "platform:iam:user:view"]
    });
    for path in ["/api/roles", "/bff/roles"] {
        let (status, resp) = read_json(app.post(path, &writer, body.clone()).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {resp}");
        assert_eq!(code(&resp), "PERMISSION_OUTSIDE_APPLICATION", "{path}");
    }
    assert!(app
        .repos
        .role_repo
        .find_by_name("d15:reader")
        .await
        .unwrap()
        .is_none());

    // Its own application's permissions: fine; adding another's on update
    // is refused.
    let (status, resp) = read_json(
        app.post(
            "/api/roles",
            &writer,
            json!({
                "applicationCode": "d15", "roleName": "reader", "displayName": "Reader",
                "permissions": ["d15:thing:view"]
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{resp}");
    let (status, resp) = read_json(
        app.put(
            "/bff/roles/d15:reader",
            &writer,
            json!({ "permissions": ["d15:thing:view", "platform:iam:user:view"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_OUTSIDE_APPLICATION");

    // A super-admin, through the admin API, may.
    let super_admin = stored_caller(
        &app,
        "d15-super@iam.test",
        UserScope::Anchor,
        &[permissions::ADMIN_ALL],
    )
    .await;
    let resp = app
        .put(
            "/bff/roles/d15:reader",
            &super_admin,
            json!({ "permissions": ["d15:thing:view", "platform:iam:user:view"] }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let role = app
        .repos
        .role_repo
        .find_by_name("d15:reader")
        .await
        .unwrap()
        .unwrap();
    assert!(role.permissions.contains("platform:iam:user:view"));

    // The SDK sync never, not even for a super-admin.
    let (status, resp) = read_json(
        app.post(
            "/api/applications/d15/roles/sync",
            &super_admin,
            json!({ "roles": [{ "name": "synced", "permissions": ["platform:*:*:*"] }] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(code(&resp), "PERMISSION_OUTSIDE_APPLICATION");
    assert!(app
        .repos
        .role_repo
        .find_by_name("d15:synced")
        .await
        .unwrap()
        .is_none());
}

// ── Decision #23 + S1.3: the application-scoped principal sync ───────────

async fn sdk_user(app: &TestApp, email: &str, client: Option<&str>, roles: &[&str]) -> String {
    use fc_platform::service_account::entity::{AssignmentSource, RoleAssignment};
    let mut p = Principal::new_user(email, UserScope::Client);
    if let Some(c) = client {
        p = p.with_client_id(c);
    }
    for r in roles {
        p.roles
            .push(RoleAssignment::with_source(*r, AssignmentSource::SdkSync));
    }
    app.repos.principal_repo.insert(&p).await.unwrap();
    p.id
}

async fn application(app: &TestApp, code: &str) {
    let a = fc_platform::application::entity::Application::new(code, code);
    app.repos.application_repo.insert(&a).await.unwrap();
}

/// The sync replaces and sweeps only its own application's SDK roles, never
/// takes another application's or a platform role name, and an anchor's
/// sync reaches every user.
#[tokio::test]
#[ignore = "requires Docker"]
async fn application_sync_keeps_to_its_own_roles() {
    let app = setup().await;
    application(&app, "hr").await;
    application(&app, "rfp").await;
    let syncer = stored_caller(
        &app,
        "hr-syncer@iam.test",
        UserScope::Anchor,
        &[permissions::iam::USER_CREATE],
    )
    .await;
    let named = sdk_user(&app, "named@iam.test", None, &["hr:employee", "rfp:buyer"]).await;
    let mut kept = app
        .repos
        .principal_repo
        .find_by_id(&named)
        .await
        .unwrap()
        .unwrap();
    kept.assign_role("platform:viewer");
    app.repos.principal_repo.update(&kept).await.unwrap();
    let swept = sdk_user(&app, "swept@iam.test", None, &["hr:employee", "rfp:buyer"]).await;

    // Refused names: a platform role, another application's role.
    for role in ["platform:super-admin", "RFP:Buyer"] {
        let (status, resp) = read_json(
            app.post(
                "/api/applications/hr/principals/sync",
                &syncer,
                json!({ "principals": [{ "email": "new@iam.test", "name": "N", "roles": [role] }] }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{role}: {resp}");
        assert_eq!(code(&resp), "ROLE_APP_FORBIDDEN", "{role}");
    }
    assert!(app
        .repos
        .principal_repo
        .find_by_email("new@iam.test")
        .await
        .unwrap()
        .is_none());

    // Accepted: its own, an unprefixed name, an unknown prefix.
    let (status, resp) = read_json(
        app.post(
            "/api/applications/hr/principals/sync?removeUnlisted=true",
            &syncer,
            json!({ "principals": [{
                "email": "Named@iam.test", "name": "Named",
                "roles": ["hr:manager", "employee", "legacy:thing"]
            }] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["updated"], 1);
    assert_eq!(resp["deleted"], 1, "{resp}");
    assert_eq!(
        roles_of(&app, &named).await,
        vec![
            "employee",
            "hr:manager",
            "legacy:thing",
            "platform:viewer",
            "rfp:buyer"
        ]
    );
    // The sweep stripped only hr's SDK role.
    assert_eq!(roles_of(&app, &swept).await, vec!["rfp:buyer"]);
    assert!(app.event_count_by_type("platform:iam:user:updated").await >= 2);
}

/// A non-anchor caller touches only its own client's users: a listed user
/// outside its reach refuses the whole sync; the sweep skips such users.
#[tokio::test]
#[ignore = "requires Docker"]
async fn application_sync_stays_within_the_callers_reach() {
    let app = setup().await;
    application(&app, "hr").await;
    let mine = create_client(&app, "reach-mine").await;
    let theirs = create_client(&app, "reach-theirs").await;
    // A client-tier syncer that reaches every application.
    let p = Principal::new_user("reach-syncer@iam.test", UserScope::Client).with_client_id(&mine);
    app.repos.principal_repo.insert(&p).await.unwrap();
    let syncer = app
        .auth_service
        .generate_access_token_with_scope(&p, &[permissions::iam::USER_CREATE.to_string()], None)
        .unwrap();
    let own = sdk_user(&app, "own@iam.test", Some(&mine), &["hr:employee"]).await;
    let foreign = sdk_user(&app, "foreign@iam.test", Some(&theirs), &["hr:employee"]).await;

    let (status, resp) = read_json(
        app.post(
            "/api/applications/hr/principals/sync",
            &syncer,
            json!({ "principals": [
                { "email": "brand-new@iam.test", "name": "New" },
                { "email": "foreign@iam.test", "name": "Hijacked", "roles": ["hr:manager"] }
            ] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{resp}");
    assert_eq!(code(&resp), "SYNC_TARGET_FORBIDDEN");
    assert!(app
        .repos
        .principal_repo
        .find_by_email("brand-new@iam.test")
        .await
        .unwrap()
        .is_none());
    assert_eq!(roles_of(&app, &foreign).await, vec!["hr:employee"]);

    // The sweep: its own client's user loses hr's role; the other client's
    // user is out of reach and keeps it.
    let other_own = sdk_user(&app, "other-own@iam.test", Some(&mine), &["hr:employee"]).await;
    let (status, resp) = read_json(
        app.post(
            "/api/applications/hr/principals/sync?removeUnlisted=true",
            &syncer,
            json!({ "principals": [{ "email": "own@iam.test", "name": "Own", "roles": ["hr:manager"] }] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(roles_of(&app, &own).await, vec!["hr:manager"]);
    assert!(roles_of(&app, &other_own).await.is_empty());
    assert_eq!(roles_of(&app, &foreign).await, vec!["hr:employee"]);
}

/// Go parity (`role/operations/sync.go`): a role name already carrying the
/// application's prefix is not prefixed twice, and a sync that names a role
/// without permissions keeps the permissions curated in the UI.
#[tokio::test]
#[ignore = "requires Docker"]
async fn role_sync_names_and_permissions_follow_go() {
    let app = setup().await;
    application(&app, "rs1").await;
    let super_admin = stored_caller(
        &app,
        "rs1-super@iam.test",
        UserScope::Anchor,
        &[permissions::ADMIN_ALL],
    )
    .await;
    let sync = |roles: serde_json::Value| {
        let app = &app;
        let super_admin = &super_admin;
        async move {
            let (status, resp) = read_json(
                app.post(
                    "/api/applications/rs1/roles/sync",
                    super_admin,
                    json!({ "roles": roles }),
                )
                .await,
            )
            .await;
            assert!(status.is_success(), "{status}: {resp}");
        }
    };
    let permissions_of = |name: &'static str| {
        let app = &app;
        async move {
            let role = app
                .repos
                .role_repo
                .find_by_name(name)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("role {name} missing"));
            let mut p: Vec<String> = role.permissions.into_iter().collect();
            p.sort();
            p
        }
    };

    sync(json!([{ "name": "rs1:admin", "permissions": ["rs1:thing:view"] }])).await;
    assert!(app
        .repos
        .role_repo
        .find_by_name("rs1:rs1:admin")
        .await
        .unwrap()
        .is_none());
    assert_eq!(permissions_of("rs1:admin").await, vec!["rs1:thing:view"]);

    // Named again with no permissions: the stored ones stay.
    sync(json!([{ "name": "admin" }])).await;
    assert_eq!(permissions_of("rs1:admin").await, vec!["rs1:thing:view"]);

    // A non-empty list replaces them.
    sync(json!([{ "name": "admin", "permissions": ["rs1:thing:edit"] }])).await;
    assert_eq!(permissions_of("rs1:admin").await, vec!["rs1:thing:edit"]);
}
