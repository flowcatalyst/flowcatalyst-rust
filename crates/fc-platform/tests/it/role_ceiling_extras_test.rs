//! The role ceiling where Go's F8 (`09b33e8`, `8b1cb0f`, `e08eee4`) goes
//! beyond what this platform already bounded: the admin token mint of a
//! service account, application service-account provisioning, and the
//! application-scoped principal sync. A caller may hand out (or mint a token
//! carrying) only roles whose platform permissions it holds: 403
//! `ROLE_ABOVE_CALLER`. Requires Docker (or `FC_TEST_PG_BIN`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::support::{self, read_json, TestApp};
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{roles, AuthRole};

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
