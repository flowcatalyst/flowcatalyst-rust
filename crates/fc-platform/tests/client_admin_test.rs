//! What the `platform:client-admin` role lets its holder do, as Go enforces it
//! (principal/api/api.go, principal/operations/authz.go, shared/auth
//! `RequireUserAdmin`): manage CLIENT-tier users of the clients it reaches,
//! assign them the application roles and applications of apps their client
//! is entitled to, and nothing beyond. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::application::entity::Application;
use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::AuthRole;
use support::{read_json, TestApp};

struct Fixture {
    app: TestApp,
    admin: String,
    mine: String,
    theirs: String,
    hr: Application,
    other_app: Application,
}

async fn setup() -> Fixture {
    let app = TestApp::setup().await;
    let mine = Client::new("Mine", "ca-mine");
    let theirs = Client::new("Theirs", "ca-theirs");
    for c in [&mine, &theirs] {
        app.repos
            .client_repo
            .insert(c)
            .await
            .expect("insert client");
    }
    let hr = Application::new("cahr", "CA HR");
    let other_app = Application::new("caother", "CA Other");
    for a in [&hr, &other_app] {
        app.repos
            .application_repo
            .insert(a)
            .await
            .expect("insert application");
    }
    // Only hr is enabled for my client.
    app.repos
        .application_client_config_repo
        .enable_for_client(&hr.id, &mine.id)
        .await
        .expect("enable hr");
    for (application, name) in [(&hr, "clerk"), (&other_app, "clerk")] {
        let mut role = AuthRole::new(application.code.clone(), name, "Clerk");
        role.application_id = Some(application.id.clone());
        app.repos
            .role_repo
            .insert(&role)
            .await
            .expect("insert role");
    }

    // The client administrator: CLIENT tier in my client, holding exactly
    // the platform:client-admin role's permissions.
    let role = fc_platform::role::entity::roles::client_admin();
    let caller = Principal::new_user("ca@ca.test", UserScope::Client).with_client_id(&mine.id);
    let granted: Vec<String> = role.permissions.iter().cloned().collect();
    let admin = app
        .auth_service
        .generate_access_token_with_scope(&caller, &granted, None)
        .expect("token");

    Fixture {
        admin,
        mine: mine.id,
        theirs: theirs.id,
        hr,
        other_app,
        app,
    }
}

async fn stored(app: &TestApp, email: &str, scope: UserScope, client: Option<&str>) -> Principal {
    let mut p = Principal::new_user(email, scope);
    if let Some(c) = client {
        p = p.with_client_id(c);
    }
    app.repos
        .principal_repo
        .insert(&p)
        .await
        .expect("insert user");
    p
}

fn code(body: &Value) -> &str {
    body["error"].as_str().unwrap_or_default()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_client_admin_creates_client_users_in_its_own_client_only() {
    let f = setup().await;
    let (status, body) = read_json(
        f.app
            .post(
                "/api/principals/users",
                &f.admin,
                json!({ "email": "new@ca.test", "name": "New", "scope": "CLIENT", "clientId": f.mine }),
            )
            .await,
    )
    .await;
    assert!(status.is_success(), "{status} {body}");

    let (status, body) = read_json(
        f.app
            .post(
                "/api/principals/users",
                &f.admin,
                json!({ "email": "x@ca.test", "name": "X", "scope": "CLIENT", "clientId": f.theirs }),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(code(&body), "SCOPE_FORBIDDEN");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_client_admin_manages_its_clients_client_users_only() {
    let f = setup().await;
    let member = stored(&f.app, "m@ca.test", UserScope::Client, Some(&f.mine)).await;
    let partner = stored(&f.app, "pt@ca.test", UserScope::Partner, Some(&f.mine)).await;
    let stranger = stored(&f.app, "s@ca.test", UserScope::Client, Some(&f.theirs)).await;

    for action in ["deactivate", "activate"] {
        let path = format!("/api/principals/{}/{action}", member.id);
        let (status, body) = read_json(f.app.post(&path, &f.admin, json!({})).await).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");

        // Another client's user answers as a missing one.
        let path = format!("/api/principals/{}/{action}", stranger.id);
        let (status, body) = read_json(f.app.post(&path, &f.admin, json!({})).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");

        // A partner homed at my client is the wrong kind of user.
        let path = format!("/api/principals/{}/{action}", partner.id);
        let (status, body) = read_json(f.app.post(&path, &f.admin, json!({})).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
    }

    let (status, body) = read_json(
        f.app
            .delete(&format!("/api/principals/{}", stranger.id), &f.admin)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let resp = f
        .app
        .delete(&format!("/api/principals/{}", member.id), &f.admin)
        .await;
    assert!(resp.status().is_success(), "{}", resp.status());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_client_admin_assigns_only_its_clients_application_roles() {
    let f = setup().await;
    let viewer =
        AuthRole::new("platform", "viewer", "Viewer").with_permission("platform:iam:user:view");
    let auditor = AuthRole::new("platform", "auditor", "Auditor");
    for role in [&viewer, &auditor] {
        f.app
            .repos
            .role_repo
            .insert(role)
            .await
            .expect("insert role");
    }
    let mut member = Principal::new_user("r@ca.test", UserScope::Client).with_client_id(&f.mine);
    // A platform role the client administrator can neither grant nor strip.
    member.assign_role("platform:viewer");
    f.app
        .repos
        .principal_repo
        .insert(&member)
        .await
        .expect("insert");
    let path = format!("/api/principals/{}/roles", member.id);

    // An application role of an app my client is entitled to: granted, and
    // the SET keeps the platform role it could not have removed.
    let (status, body) = read_json(
        f.app
            .put(&path, &f.admin, json!({ "roles": ["cahr:clerk"] }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let stored = f
        .app
        .repos
        .principal_repo
        .find_by_id(&member.id)
        .await
        .unwrap()
        .unwrap();
    let mut roles: Vec<String> = stored.roles.iter().map(|r| r.role.clone()).collect();
    roles.sort();
    assert_eq!(roles, vec!["cahr:clerk", "platform:viewer"]);

    // A platform role: refused.
    let (status, body) = read_json(
        f.app
            .put(
                &path,
                &f.admin,
                json!({ "roles": ["cahr:clerk", "platform:viewer", "platform:auditor"] }),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(code(&body), "PLATFORM_ROLE_FORBIDDEN");

    // A role of an application my client is not entitled to: refused.
    let (status, body) = read_json(
        f.app
            .post(&path, &f.admin, json!({ "role": "caother:clerk" }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(code(&body), "ROLE_APP_FORBIDDEN");

    // Nor may it remove the platform role.
    let (status, body) = read_json(
        f.app
            .delete(
                &format!("/api/principals/{}/roles/platform:viewer", member.id),
                &f.admin,
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_client_admin_grants_only_its_clients_applications() {
    let f = setup().await;
    let member = stored(&f.app, "a@ca.test", UserScope::Client, Some(&f.mine)).await;
    let path = format!("/api/principals/{}/application-access", member.id);

    let (status, body) = read_json(
        f.app
            .put(&path, &f.admin, json!({ "applicationIds": [f.hr.id] }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = read_json(
        f.app
            .put(
                &path,
                &f.admin,
                json!({ "applicationIds": [f.other_app.id] }),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(code(&body), "APP_FORBIDDEN");
}
