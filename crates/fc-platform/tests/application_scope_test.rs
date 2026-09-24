//! Application scope on the `/api/applications/{appCode}/…` SDK routes.
//!
//! A service account bound to an application may act on that application
//! and on any application it holds an explicit access grant for. An unbound
//! principal may act on every application only when its `all_applications`
//! flag is set (users, by default); a new service account has it off and
//! reaches only what it is granted. Everything else answers the same 404 as
//! an application that doesn't exist. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::roles;
use fc_platform::service_account::entity::RoleAssignment;
use support::{read_json, TestApp};

async fn create_app(app: &TestApp, code: &str) -> Application {
    let application = Application::new(code, code.to_uppercase());
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");
    application
}

/// Seed the built-in application-service role (the harness doesn't run the
/// startup role sync) and return its code.
async fn application_service_role(app: &TestApp) -> String {
    let role = roles::application_service();
    if app
        .repos
        .role_repo
        .find_by_name(&role.name)
        .await
        .expect("find role")
        .is_none()
    {
        app.repos
            .role_repo
            .insert(&role)
            .await
            .expect("insert role");
    }
    role.name
}

/// Persist `principal` (with the application-service role and the given
/// grants) and mint a token for it.
async fn token_for(app: &TestApp, mut principal: Principal, grants: &[&Application]) -> String {
    principal.roles = vec![RoleAssignment::new(application_service_role(app).await)];
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
    app.auth_service
        .generate_access_token(&principal)
        .expect("token")
}

async fn list_roles(app: &TestApp, token: &str, code: &str) -> (StatusCode, Value) {
    read_json(
        app.get(&format!("/api/applications/{}/roles", code), token)
            .await,
    )
    .await
}

async fn sync_roles(app: &TestApp, token: &str, code: &str) -> (StatusCode, Value) {
    read_json(
        app.post(
            &format!("/api/applications/{}/roles/sync", code),
            token,
            json!({ "roles": [{ "name": "viewer" }] }),
        )
        .await,
    )
    .await
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn service_account_is_confined_to_its_applications() {
    let app = TestApp::setup().await;
    let app_a = create_app(&app, "scope-a").await;
    let app_b = create_app(&app, "scope-b").await;
    let app_c = create_app(&app, "scope-c").await;

    // An application's own service account (as provisioning creates it).
    let sa_a = token_for(
        &app,
        Principal::new_service("sa_a", "SA A", UserScope::Anchor).with_application_id(&app_a.id),
        &[],
    )
    .await;

    assert_eq!(list_roles(&app, &sa_a, "scope-a").await.0, StatusCode::OK);
    assert_eq!(sync_roles(&app, &sa_a, "scope-a").await.0, StatusCode::OK);

    // Another application: the same 404 as one that doesn't exist.
    let (status, out_of_scope) = sync_roles(&app, &sa_a, "scope-b").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, missing) = sync_roles(&app, &sa_a, "scope-zz").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        out_of_scope.to_string(),
        missing.to_string().replace("scope-zz", "scope-b"),
        "out-of-scope must be indistinguishable from not-found"
    );
    assert_eq!(
        list_roles(&app, &sa_a, "scope-b").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete("/api/applications/scope-b/roles/viewer", &sa_a)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    // Nothing was written to B.
    assert!(app
        .repos
        .role_repo
        .find_by_name("scope-b:viewer")
        .await
        .expect("find role")
        .is_none());

    // A service account bound to A with a grant for C reaches both, not B.
    let sa_many = token_for(
        &app,
        Principal::new_service("sa_many", "SA Many", UserScope::Anchor)
            .with_application_id(&app_a.id),
        &[&app_c],
    )
    .await;
    assert_eq!(
        sync_roles(&app, &sa_many, "scope-a").await.0,
        StatusCode::OK
    );
    assert_eq!(
        sync_roles(&app, &sa_many, "scope-c").await.0,
        StatusCode::OK
    );
    assert_eq!(
        sync_roles(&app, &sa_many, "scope-b").await.0,
        StatusCode::NOT_FOUND
    );

    // An unbound service account starts with no application access: the
    // same 404 on every application, anchor tier or not.
    let sa_none = token_for(
        &app,
        Principal::new_service("sa_unbound", "SA Unbound", UserScope::Anchor),
        &[],
    )
    .await;
    for code in ["scope-a", "scope-b", "scope-c", "scope-zz"] {
        assert_eq!(
            sync_roles(&app, &sa_none, code).await.0,
            StatusCode::NOT_FOUND,
            "{code}"
        );
    }

    // With an explicit grant it reaches only the granted application.
    let sa_granted = token_for(
        &app,
        Principal::new_service("sa_granted", "SA Granted", UserScope::Anchor),
        &[&app_c],
    )
    .await;
    assert_eq!(
        sync_roles(&app, &sa_granted, "scope-c").await.0,
        StatusCode::OK
    );
    for code in ["scope-a", "scope-b"] {
        assert_eq!(
            sync_roles(&app, &sa_granted, code).await.0,
            StatusCode::NOT_FOUND,
            "{code}"
        );
    }

    // Only the stored all-applications flag reaches every application.
    let mut all = Principal::new_service("sa_all", "SA All", UserScope::Anchor);
    all.all_applications = true;
    let sa_all = token_for(&app, all, &[]).await;
    for code in ["scope-a", "scope-b", "scope-c"] {
        assert_eq!(sync_roles(&app, &sa_all, code).await.0, StatusCode::OK);
    }
    // ...but an application that doesn't exist is still a 404.
    assert_eq!(
        sync_roles(&app, &sa_all, "scope-zz").await.0,
        StatusCode::NOT_FOUND
    );

    // The flag doesn't widen an account bound to an application.
    let mut bound_all = Principal::new_service("sa_bound_all", "SA Bound All", UserScope::Anchor)
        .with_application_id(&app_a.id);
    bound_all.all_applications = true;
    let sa_bound_all = token_for(&app, bound_all, &[]).await;
    assert_eq!(
        sync_roles(&app, &sa_bound_all, "scope-a").await.0,
        StatusCode::OK
    );
    assert_eq!(
        sync_roles(&app, &sa_bound_all, "scope-b").await.0,
        StatusCode::NOT_FOUND
    );

    // An anchor user (all applications by default) reaches every one.
    let admin = token_for(
        &app,
        Principal::new_user("scope-admin@flowcatalyst.test", UserScope::Anchor),
        &[],
    )
    .await;
    assert_eq!(list_roles(&app, &admin, "scope-b").await.0, StatusCode::OK);
    assert_eq!(sync_roles(&app, &admin, "scope-b").await.0, StatusCode::OK);

    // B's service account reaches B.
    let sa_b = token_for(
        &app,
        Principal::new_service("sa_b", "SA B", UserScope::Anchor).with_application_id(&app_b.id),
        &[],
    )
    .await;
    assert_eq!(sync_roles(&app, &sa_b, "scope-b").await.0, StatusCode::OK);
    assert_eq!(
        sync_roles(&app, &sa_b, "scope-a").await.0,
        StatusCode::NOT_FOUND
    );
}

/// The permission gate runs before any lookup: a caller without the
/// permission gets 403 whether or not the application exists.
#[tokio::test]
#[ignore = "requires Docker"]
async fn permission_is_checked_before_the_application() {
    let app = TestApp::setup().await;
    create_app(&app, "scope-p").await;
    let principal = Principal::new_service("sa_none", "SA None", UserScope::Anchor);
    app.repos
        .principal_repo
        .insert(&principal)
        .await
        .expect("insert principal");
    let token = app
        .auth_service
        .generate_access_token(&principal)
        .expect("token");

    assert_eq!(
        sync_roles(&app, &token, "scope-p").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        sync_roles(&app, &token, "scope-zz").await.0,
        StatusCode::FORBIDDEN
    );
}

/// A service account made by the real provisioning endpoint reaches its own
/// application and nothing else. Provisioning doesn't assign roles, so the
/// test grants the application-service role the way an admin would.
#[tokio::test]
#[ignore = "requires Docker"]
async fn provisioned_service_account_reaches_only_its_application() {
    // Provisioning encrypts the OAuth client secret; any 32-byte key will do.
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );
    let app = TestApp::setup().await;
    let app_a = create_app(&app, "prov-a").await;
    create_app(&app, "prov-b").await;

    let (status, body) = read_json(
        app.post(
            &format!("/api/applications/{}/provision-service-account", app_a.id),
            &app.anchor_token(),
            json!({}),
        )
        .await,
    )
    .await;
    assert!(status.is_success(), "provision failed: {} {}", status, body);
    let principal_id = body["serviceAccount"]["principalId"]
        .as_str()
        .expect("principalId")
        .to_string();

    let mut principal = app
        .repos
        .principal_repo
        .find_by_id(&principal_id)
        .await
        .expect("find principal")
        .expect("provisioned principal");
    // Stored as Go provisions it: no all-applications, one access row.
    assert!(!principal.all_applications);
    assert_eq!(principal.accessible_application_ids, vec![app_a.id.clone()]);

    principal.roles = vec![RoleAssignment::new(application_service_role(&app).await)];
    let token = app
        .auth_service
        .generate_access_token(&principal)
        .expect("token");

    assert_eq!(sync_roles(&app, &token, "prov-a").await.0, StatusCode::OK);
    assert_eq!(
        sync_roles(&app, &token, "prov-b").await.0,
        StatusCode::NOT_FOUND
    );
}

/// Platform config is addressed by `{appCode}` too, and every service
/// account is anchor scope, so `require_anchor` alone let one application's
/// service account rewrite another's config.
#[tokio::test]
#[ignore = "requires Docker"]
async fn platform_config_is_confined_to_the_callers_applications() {
    let app = TestApp::setup().await;
    let app_a = create_app(&app, "cfg-a").await;
    create_app(&app, "cfg-b").await;
    let sa_a = token_for(
        &app,
        Principal::new_service("sa_cfg_a", "SA Cfg A", UserScope::Anchor)
            .with_application_id(&app_a.id),
        &[],
    )
    .await;
    let body = json!({ "value": "x" });

    let own = app
        .put("/api/config/cfg-a/general/colour", &sa_a, body.clone())
        .await;
    assert_eq!(own.status(), StatusCode::CREATED);
    let other = app
        .put("/api/config/cfg-b/general/colour", &sa_a, body.clone())
        .await;
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        app.get("/api/config/cfg-b", &sa_a).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get("/api/config-access/cfg-b", &sa_a).await.status(),
        StatusCode::NOT_FOUND
    );

    let admin = token_for(
        &app,
        Principal::new_user("cfg-admin@flowcatalyst.test", UserScope::Anchor),
        &[],
    )
    .await;
    let as_admin = app
        .put("/api/config/cfg-b/general/colour", &admin, body)
        .await;
    assert_eq!(as_admin.status(), StatusCode::CREATED);
}

/// Give the stored principal `role` and mint a token for it, returning the
/// token and the principal's stored application access.
async fn mint_with_role(app: &TestApp, id: &str, role: &str) -> (String, bool, Vec<String>) {
    let mut p = app
        .repos
        .principal_repo
        .find_by_id(id)
        .await
        .unwrap()
        .unwrap();
    p.roles = vec![RoleAssignment::new(role)];
    app.repos.principal_repo.update(&p).await.unwrap();
    (
        app.auth_service.generate_access_token(&p).unwrap(),
        p.all_applications,
        p.accessible_application_ids,
    )
}

/// A service account made through `POST /api/service-accounts` starts with
/// no application access at all, and reaches exactly what it is granted
/// afterwards.
#[tokio::test]
#[ignore = "requires Docker"]
async fn created_service_account_reaches_only_granted_applications() {
    // Creation encrypts the generated credentials; any 32-byte key will do.
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );
    let app = TestApp::setup().await;
    let app_x = create_app(&app, "new-x").await;
    create_app(&app, "new-y").await;
    let admin = app.anchor_token();

    let (status, body) = read_json(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": "fresh-bot", "name": "Fresh bot", "scope": "ANCHOR" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["serviceAccount"]["id"].as_str().unwrap().to_string();

    // Binding an application on create isn't accepted.
    let (status, _) = read_json(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": "bound-bot", "name": "x", "applicationId": app_x.id }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let role = application_service_role(&app).await;
    let (sa, all_apps, granted) = mint_with_role(&app, &id, &role).await;
    assert!(!all_apps);
    assert!(granted.is_empty());
    for code in ["new-x", "new-y"] {
        assert_eq!(
            sync_roles(&app, &sa, code).await.0,
            StatusCode::NOT_FOUND,
            "{code}"
        );
    }

    // Grant X through the application-access endpoint.
    let resp = app
        .put(
            &format!("/api/principals/{id}/application-access"),
            &admin,
            json!({ "applicationIds": [app_x.id] }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    // The endpoint drops the cached scope, so the grant applies at once.
    let (sa, _, granted) = mint_with_role(&app, &id, &role).await;
    assert_eq!(granted, vec![app_x.id.clone()]);
    assert_eq!(sync_roles(&app, &sa, "new-x").await.0, StatusCode::OK);
    assert_eq!(
        sync_roles(&app, &sa, "new-y").await.0,
        StatusCode::NOT_FOUND
    );
}
