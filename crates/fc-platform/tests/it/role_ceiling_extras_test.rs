//! The role ceiling where Go's F8 (`09b33e8`, `8b1cb0f`, `e08eee4`) goes
//! beyond what this platform already bounded: the admin token mint of a
//! service account, application service-account provisioning, and the
//! application-scoped principal sync. A caller may hand out (or mint a token
//! carrying) only roles whose platform permissions it holds: 403
//! `ROLE_ABOVE_CALLER`. Requires Docker (or `FC_TEST_PG_BIN`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::support::{self, read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{permissions, roles, AuthRole};
use fc_platform::service_account::entity::{AssignmentSource, RoleAssignment};
use fc_platform::shared::id::PrincipalId;

async fn setup() -> TestApp {
    support::set_app_key();
    TestApp::setup().await
}

/// Insert the roles a test hands out (the harness seeds none).
async fn seed_roles(app: &TestApp, roles: Vec<AuthRole>) {
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

/// A stored anchor user whose token grants exactly `perms` (stored, as the
/// service-account writes confine a caller that reaches no application).
async fn stored_caller(app: &TestApp, email: &str, perms: &[&str]) -> String {
    let p = Principal::new_user(email, UserScope::Anchor);
    app.repos.principal_repo.insert(&p).await.unwrap();
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .unwrap()
}

fn code(body: &Value) -> &str {
    body["error"].as_str().unwrap_or_default()
}

fn message(body: &Value) -> &str {
    body["message"].as_str().unwrap_or_default()
}

/// Go `09b33e8`: the minted bearer carries the account's whole authority, so
/// a holder of `service-account:update` may not mint a token for an account
/// holding a role above it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_token_mint_is_bounded_by_the_callers_permissions() {
    let app = setup().await;
    seed_roles(&app, vec![roles::super_admin(), roles::iam_readonly()]).await;
    let admin = app.anchor_admin_token_all_applications().await;
    let (status, body) = read_json(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": "mint-ceiling", "name": "Mint" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["serviceAccount"]["id"].as_str().unwrap().to_string();
    let set_roles = |roles: Value| {
        let (app, admin, id) = (&app, admin.clone(), id.clone());
        async move {
            let (status, body) = read_json(
                app.put(
                    &format!("/api/service-accounts/{id}/roles"),
                    &admin,
                    json!({ "roles": roles }),
                )
                .await,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
    };
    set_roles(json!(["platform:super-admin"])).await;

    // An IAM administrator (holds service-account:update, not super-admin).
    let iam_admin_role = roles::iam_admin();
    let perms: Vec<&str> = iam_admin_role
        .permissions
        .iter()
        .map(String::as_str)
        .collect();
    let sa_admin = stored_caller(&app, "mint-ceiling@iam.test", &perms).await;
    let mint = format!("/api/service-accounts/{id}/token");

    let (status, body) = read_json(app.post(&mint, &sa_admin, json!({})).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(code(&body), "ROLE_ABOVE_CALLER");
    assert_eq!(
        message(&body),
        "You may only mint a token for a service account whose roles' permissions you hold yourself: platform:super-admin"
    );
    assert_eq!(
        app.event_count_by_type("platform:iam:serviceaccount:token-minted")
            .await,
        0
    );

    // A super-admin may.
    let (status, body) = read_json(app.post(&mint, &admin, json!({})).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // An account within the caller's permissions: minted.
    set_roles(json!(["platform:iam-readonly"])).await;
    let (status, body) = read_json(app.post(&mint, &sa_admin, json!({})).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["accessToken"].is_string());
}

/// Go `8b1cb0f`: provisioning gives the account `platform:application-service`,
/// so the caller must hold that role's platform permissions.
#[tokio::test]
#[ignore = "requires Docker"]
async fn provisioning_is_bounded_by_the_application_service_role() {
    let app = setup().await;
    seed_roles(&app, vec![roles::application_service()]).await;
    let mut ids = Vec::new();
    for code in ["prov-a", "prov-b"] {
        let a = Application::new(code, code);
        app.repos.application_repo.insert(&a).await.unwrap();
        ids.push(a.id.to_string());
    }
    let path = |id: &str| format!("/api/applications/{id}/provision-service-account");

    // The gate alone (anchor, service-account:create, application:update).
    let gated = stored_caller(
        &app,
        "prov-gate@iam.test",
        &[
            permissions::admin::SERVICE_ACCOUNT_CREATE,
            permissions::admin::APPLICATION_UPDATE,
        ],
    )
    .await;
    let (status, body) = read_json(app.post(&path(&ids[0]), &gated, json!({})).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(code(&body), "ROLE_ABOVE_CALLER");
    assert!(
        message(&body).ends_with(": platform:application-service"),
        "{body}"
    );
    let app_row = app
        .repos
        .application_repo
        .find_by_code("prov-a")
        .await
        .unwrap()
        .unwrap();
    assert!(app_row.service_account_id.is_none(), "nothing provisioned");

    // The gate plus every permission of the role: provisioned.
    let role = roles::application_service();
    let mut perms: Vec<&str> = role.permissions.iter().map(String::as_str).collect();
    perms.extend([
        permissions::admin::SERVICE_ACCOUNT_CREATE,
        permissions::admin::APPLICATION_UPDATE,
    ]);
    let holder = stored_caller(&app, "prov-holder@iam.test", &perms).await;
    let (status, body) = read_json(app.post(&path(&ids[0]), &holder, json!({})).await).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // A super-admin may.
    let super_admin = stored_caller(&app, "prov-super@iam.test", &[permissions::ADMIN_ALL]).await;
    let (status, body) = read_json(app.post(&path(&ids[1]), &super_admin, json!({})).await).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn roles_of(app: &TestApp, id: &str) -> Vec<String> {
    let mut roles: Vec<String> = app
        .repos
        .principal_repo
        .find_by_id(&PrincipalId::from_wire(id))
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

async fn sdk_user(app: &TestApp, email: &str, roles: &[&str]) -> String {
    let mut p = Principal::new_user(email, UserScope::Client);
    for r in roles {
        p.roles
            .push(RoleAssignment::with_source(*r, AssignmentSource::SdkSync));
    }
    app.repos.principal_repo.insert(&p).await.unwrap();
    p.id.to_string()
}

/// Go `e08eee4`: the application-scoped principal sync applies the ceiling
/// to every role it adds or removes, the sweep's included. An application
/// role holding only the application's permissions passes for any caller; one
/// carrying a platform permission (a super-admin's cross-application edit, or
/// a role from before the confinement rule) does not, though the name rule
/// admits it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_application_principal_sync_is_bounded_by_the_callers_permissions() {
    let app = setup().await;
    let hr = Application::new("hr", "hr");
    app.repos.application_repo.insert(&hr).await.unwrap();
    seed_roles(
        &app,
        vec![
            AuthRole::new("hr", "boss", "Boss").with_permission(permissions::iam::ROLE_CREATE),
            AuthRole::new("hr", "clerk", "Clerk").with_permission("hr:leave:view"),
        ],
    )
    .await;
    let syncer = stored_caller(&app, "hr-syncer@iam.test", &[permissions::iam::USER_CREATE]).await;
    let sync = |remove_unlisted: bool, principals: Value| {
        let (app, syncer) = (&app, syncer.clone());
        async move {
            let q = if remove_unlisted {
                "?removeUnlisted=true"
            } else {
                ""
            };
            read_json(
                app.post(
                    &format!("/api/applications/hr/principals/sync{q}"),
                    &syncer,
                    json!({ "principals": principals }),
                )
                .await,
            )
            .await
        }
    };
    let refused = |status: StatusCode, body: &Value| {
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(code(body), "ROLE_ABOVE_CALLER");
        assert_eq!(
            message(body),
            "You may only assign or remove roles whose permissions you hold yourself: hr:boss"
        );
    };
    let exists = |email: &'static str| {
        let app = &app;
        async move {
            app.repos
                .principal_repo
                .find_by_email(email)
                .await
                .unwrap()
                .is_some()
        }
    };

    // Adding the role above: refused, nothing written.
    let (status, body) = sync(
        false,
        json!([{ "email": "new@iam.test", "name": "N", "roles": ["hr:boss"] }]),
    )
    .await;
    refused(status, &body);
    assert!(!exists("new@iam.test").await);

    // Removing it from a listed user: refused.
    let holder = sdk_user(&app, "holder@iam.test", &["hr:boss"]).await;
    let (status, body) = sync(
        false,
        json!([{ "email": "holder@iam.test", "name": "H", "roles": [] }]),
    )
    .await;
    refused(status, &body);
    assert_eq!(roles_of(&app, &holder).await, vec!["hr:boss"]);

    // Sweeping it from an unlisted user: refused, the listed user not created.
    let (status, body) = sync(
        true,
        json!([{ "email": "clerk@iam.test", "name": "C", "roles": ["hr:clerk"] }]),
    )
    .await;
    refused(status, &body);
    assert!(!exists("clerk@iam.test").await);
    assert_eq!(roles_of(&app, &holder).await, vec!["hr:boss"]);

    // The application's own role (application permissions only) passes, and
    // a role the target keeps is not checked.
    let (status, body) = sync(
        false,
        json!([
            { "email": "clerk@iam.test", "name": "C", "roles": ["hr:clerk"] },
            { "email": "holder@iam.test", "name": "H", "roles": ["hr:boss", "hr:clerk"] }
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(exists("clerk@iam.test").await);
    assert_eq!(roles_of(&app, &holder).await, vec!["hr:boss", "hr:clerk"]);

    // A super-admin may.
    let super_admin = stored_caller(&app, "hr-super@iam.test", &[permissions::ADMIN_ALL]).await;
    let (status, body) = read_json(
        app.post(
            "/api/applications/hr/principals/sync",
            &super_admin,
            json!({ "principals": [{ "email": "holder@iam.test", "name": "H", "roles": [] }] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(roles_of(&app, &holder).await.is_empty());
}
