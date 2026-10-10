//! An application-scoped anchor caller (an application's service account) is
//! refused a platform-config code no application has, so it cannot probe or
//! squat it (Go fcce86c). Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::json;

use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::roles;
use fc_platform::service_account::entity::RoleAssignment;
use support::{assert_status, read_json, TestApp};

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_application_scoped_anchor_is_refused_unregistered_config_codes() {
    let app = TestApp::setup().await;
    let own = Application::new("cfg-own", "CFG OWN");
    app.repos
        .application_repo
        .insert(&own)
        .await
        .expect("insert application");
    // The built-in application-service role (the harness runs no role sync).
    let role = roles::application_service();
    app.repos
        .role_repo
        .insert(&role)
        .await
        .expect("insert role");
    // An application's service account: anchor tier, bound to its own
    // application only.
    let mut sa = Principal::new_service("sa_cfg_own", "SA Cfg Own", UserScope::Anchor)
        .with_application_id(own.id.clone());
    sa.roles = vec![RoleAssignment::new(&role.name)];
    app.repos
        .principal_repo
        .insert(&sa)
        .await
        .expect("insert principal");
    sa.accessible_application_ids = vec![own.id.clone()];
    app.repos
        .principal_repo
        .update(&sa)
        .await
        .expect("grant application");
    let token = app.auth_service.generate_access_token(&sa).expect("token");
    let body = json!({ "value": "x" });

    assert_status(
        app.put("/api/config/cfg-own/general/colour", &token, body.clone())
            .await,
        StatusCode::OK,
    )
    .await;
    // A code no application has: refused (it could squat or probe it).
    let (status, resp) = read_json(
        app.put(
            "/api/config/cfg-unclaimed/general/colour",
            &token,
            body.clone(),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{resp}");
    let (status, resp) =
        read_json(app.get("/api/platform-config/cfg-unclaimed", &token).await).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{resp}");

    // A caller reaching every application is not confined.
    let admin = app.anchor_admin_token_all_applications().await;
    let (status, resp) = read_json(
        app.put("/api/config/cfg-unclaimed/general/colour", &admin, body)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
}
