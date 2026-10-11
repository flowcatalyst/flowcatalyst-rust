//! Go's F4 (`0ae0988`, docs/platform-bug-fixes.md in flowcatalyst-go): every
//! write to a service account checks the permission, anchor reach and, for an
//! application-scoped caller (no all-applications access, as an
//! application's own service account), that the account is bound to an
//! application it can access: never a platform-wide one or another
//! application's, even at anchor tier with the permissions. Create refuses
//! such a caller a platform-wide account or another application's. A caller
//! with all-applications access is not confined. Requires Docker (or
//! `FC_TEST_PG_BIN`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::application_scope_test::create_app;
use crate::event_type_app_scope_test::token_with;
use crate::support::{self, read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions::admin;

/// Provision `application`'s service account; returns its principal id.
async fn provision(app: &TestApp, admin: &str, application: &Application) -> String {
    let (status, body) = read_json(
        app.post(
            &format!(
                "/api/applications/{}/provision-service-account",
                application.id
            ),
            admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "provision: {body}");
    body["serviceAccount"]["principalId"]
        .as_str()
        .expect("principalId")
        .to_string()
}

/// Every write to the account `id`, in Go's test order.
async fn writes(app: &TestApp, token: &str, id: &str) -> Vec<(&'static str, StatusCode, Value)> {
    let base = format!("/api/service-accounts/{id}");
    let mut out = Vec::new();
    for (what, resp) in [
        (
            "deactivate",
            app.post(&format!("{base}/deactivate"), token, json!({}))
                .await,
        ),
        (
            "regenerate secret",
            app.post(
                &format!("{base}/regenerate-signing-secret"),
                token,
                json!({}),
            )
            .await,
        ),
        (
            "regenerate token",
            app.post(&format!("{base}/regenerate-auth-token"), token, json!({}))
                .await,
        ),
        (
            "mint",
            app.post(&format!("{base}/token"), token, json!({})).await,
        ),
        (
            "assign roles",
            app.put(&format!("{base}/roles"), token, json!({ "roles": [] }))
                .await,
        ),
        (
            "update",
            app.put(&base, token, json!({ "name": "renamed" })).await,
        ),
        ("delete", app.delete(&base, token).await),
    ] {
        let (status, body) = read_json(resp).await;
        out.push((what, status, body));
    }
    out
}

fn application_access_required(status: StatusCode, body: &Value, what: &str) {
    assert_eq!(status, StatusCode::FORBIDDEN, "{what}: {body}");
    assert_eq!(
        body["error"], "APPLICATION_ACCESS_REQUIRED",
        "{what}: {body}"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn application_scoped_caller_is_confined_to_its_applications_accounts() {
    support::set_app_key();
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    let own = create_app(&app, "sareach-own").await;
    let other = create_app(&app, "sareach-other").await;
    let own_sa = provision(&app, &admin, &own).await;
    let other_sa = provision(&app, &admin, &other).await;
    let (status, body) = read_json(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": "sa-reach-platform", "name": "Platform" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let platform_sa = body["principalId"]
        .as_str()
        .expect("principalId")
        .to_string();

    // Anchor tier, every service-account permission, bound to its own
    // application only.
    let confined = token_with(
        &app,
        "sareach-confined",
        &own,
        &[
            admin::SERVICE_ACCOUNT_CREATE,
            admin::SERVICE_ACCOUNT_UPDATE,
            admin::SERVICE_ACCOUNT_DELETE,
        ],
    )
    .await;

    for id in [&other_sa, &platform_sa] {
        for (what, status, body) in writes(&app, &confined, id).await {
            application_access_required(status, &body, what);
        }
    }

    // Creating a platform-wide account or another application's is refused.
    for body in [
        json!({ "code": "sa-reach-new", "name": "N" }),
        json!({ "code": "sa-reach-new", "name": "N", "applicationId": other.id.as_str() }),
    ] {
        let (status, resp) = read_json(
            app.post("/api/service-accounts", &confined, body.clone())
                .await,
        )
        .await;
        application_access_required(status, &resp, &format!("create {body}"));
    }

    // Its own application's account: managed.
    let base = format!("/api/service-accounts/{own_sa}");
    let (status, body) = read_json(
        app.post(
            &format!("{base}/regenerate-signing-secret"),
            &confined,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "own regenerate secret: {body}");
    let (status, body) = read_json(
        app.post(&format!("{base}/deactivate"), &confined, json!({}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "own deactivate: {body}");

    // All-applications access is not confined.
    for id in [&other_sa, &platform_sa] {
        let (status, body) = read_json(
            app.post(
                &format!("/api/service-accounts/{id}/deactivate"),
                &admin,
                json!({}),
            )
            .await,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "admin deactivate {id}: {body}"
        );
    }
}

/// Each write to an existing account asks `service-account:update` (delete
/// `:delete`), then anchor scope: assigning roles, the regenerations and the
/// mint asked anchor first (Go's order is the permission first).
#[tokio::test]
#[ignore = "requires Docker"]
async fn every_write_asks_the_permission_then_anchor() {
    support::set_app_key();
    let app = TestApp::setup().await;
    // A stored client-tier user with all-applications access and no
    // service-account permission (one other permission, so it is not a
    // profile-only caller).
    let user = Principal::new_user("sa-gate@flowcatalyst.test", UserScope::Client);
    app.repos
        .principal_repo
        .insert(&user)
        .await
        .expect("insert user");
    let none = app
        .auth_service
        .generate_access_token_with_scope(&user, &[admin::EVENT_TYPE_READ.to_string()], None)
        .expect("token");
    for (what, status, body) in writes(&app, &none, "prn_nope").await {
        assert_eq!(status, StatusCode::FORBIDDEN, "{what}: {body}");
        assert_eq!(body["error"], "PERMISSION_REQUIRED", "{what}: {body}");
    }
    // The permissions without anchor scope: ANCHOR_REQUIRED.
    let perms = [
        admin::EVENT_TYPE_READ.to_string(),
        admin::SERVICE_ACCOUNT_UPDATE.to_string(),
        admin::SERVICE_ACCOUNT_DELETE.to_string(),
    ];
    let non_anchor = app
        .auth_service
        .generate_access_token_with_scope(&user, &perms, None)
        .expect("token");
    for (what, status, body) in writes(&app, &non_anchor, "prn_nope").await {
        assert_eq!(status, StatusCode::FORBIDDEN, "{what}: {body}");
        assert_eq!(body["error"], "ANCHOR_REQUIRED", "{what}: {body}");
    }
}
