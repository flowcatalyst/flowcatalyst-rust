//! `/bff/developer` reach (owner decision #37). Requires Docker.
//!
//! - An anchor caller holding `…:application-openapi:view` sees every
//!   application (Go's `CanReadDeveloperPortal`).
//! - A non-anchor caller holding the view permission (an application-scoped
//!   developer) sees the applications it can access plus `platform`; any
//!   other application answers 404.
//! - A non-anchor caller holding neither view nor manage is refused (403).

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::Value;

use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions;
use support::{read_json, TestApp};

/// Inserts `principal` with application grants `grants`, and mints a token
/// whose scope grants exactly `perms`.
async fn caller(
    app: &TestApp,
    mut principal: Principal,
    grants: &[&Application],
    perms: &[&str],
) -> String {
    app.repos
        .principal_repo
        .insert(&principal)
        .await
        .expect("insert principal");
    if !grants.is_empty() {
        principal.accessible_application_ids = grants.iter().map(|a| a.id.clone()).collect();
        app.repos
            .principal_repo
            .update(&principal)
            .await
            .expect("grant application access");
    }
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&principal, &granted, None)
        .expect("token")
}

fn codes(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|a| a["code"].as_str().map(str::to_string))
        .collect()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn developer_portal_admits_application_scoped_developers_for_their_applications() {
    let app = TestApp::setup().await;
    let granted = Application::new("dev-granted", "Dev Granted");
    let other = Application::new("dev-other", "Dev Other");
    for a in [&granted, &other] {
        app.repos
            .application_repo
            .insert(a)
            .await
            .expect("insert application");
    }
    let view = permissions::developer::APPLICATION_OPENAPI_VIEW;

    // An application-scoped developer: its grant and the platform document.
    let mut dev = Principal::new_user("dev@portal-scope.test", UserScope::Client);
    dev.all_applications = false;
    let dev = caller(&app, dev, &[&granted], &[view]).await;
    let (status, body) = read_json(app.get("/bff/developer/applications", &dev).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let listed = codes(&body);
    assert!(listed.contains(&"dev-granted".to_string()), "{body}");
    assert!(listed.contains(&"platform".to_string()), "{body}");
    assert!(!listed.contains(&"dev-other".to_string()), "{body}");

    let (status, body) = read_json(
        app.get(&format!("/bff/developer/applications/{}", granted.id), &dev)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for path in [
        format!("/bff/developer/applications/{}", other.id),
        format!("/bff/developer/applications/{}/event-types", other.id),
        format!("/bff/developer/applications/{}/openapi/versions", other.id),
    ] {
        let (status, body) = read_json(app.get(&path, &dev).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
    }

    // An anchor caller with the permission sees every application.
    let anchor = caller(
        &app,
        Principal::new_user("anchor-dev@portal-scope.test", UserScope::Anchor),
        &[],
        &[view],
    )
    .await;
    let (status, body) = read_json(app.get("/bff/developer/applications", &anchor).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let listed = codes(&body);
    for code in ["dev-granted", "dev-other", "platform"] {
        assert!(listed.contains(&code.to_string()), "{code}: {body}");
    }

    // Neither view nor manage: refused.
    let outsider = caller(
        &app,
        Principal::new_user("outsider@portal-scope.test", UserScope::Client),
        &[&granted],
        &[permissions::developer::APPLICATION_OPENAPI_SYNC],
    )
    .await;
    let (status, _) = read_json(app.get("/bff/developer/applications", &outsider).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
