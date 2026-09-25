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
