//! Application scope on the `/api/applications/{appCode}/…` SDK routes.
//!
//! As in Go's `CanAccessApplication`: a principal may act on every
//! application when its `all_applications` flag is set (users, by default),
//! otherwise only on the applications it holds an explicit access grant for.
//! A provisioned service account has the flag off and one grant for its
//! application; a new one has neither. Everything else answers the same 404
//! as an application that doesn't exist. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::roles;
use fc_platform::service_account::entity::{AssignmentSource, RoleAssignment};
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

    // An application's own service account, as provisioning creates it: no
    // all-applications flag and one access grant for its application.
    let sa_a = token_for(
        &app,
        Principal::new_service("sa_a", "SA A", UserScope::Anchor).with_application_id(&app_a.id),
        &[&app_a],
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

    // A service account granted A and C reaches both, not B.
    let sa_many = token_for(
        &app,
        Principal::new_service("sa_many", "SA Many", UserScope::Anchor)
            .with_application_id(&app_a.id),
        &[&app_a, &app_c],
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

    // As in Go, the flag reaches every application even for an account made
    // for one application.
    let mut bound_all = Principal::new_service("sa_bound_all", "SA Bound All", UserScope::Anchor)
        .with_application_id(&app_a.id);
    bound_all.all_applications = true;
    let sa_bound_all = token_for(&app, bound_all, &[]).await;
    for code in ["scope-a", "scope-b", "scope-c"] {
        assert_eq!(
            sync_roles(&app, &sa_bound_all, code).await.0,
            StatusCode::OK
        );
    }

    // Being the application's attached service account grants nothing by
    // itself (Go has no such pass).
    let attached = Principal::new_service("sa_attached", "SA Attached", UserScope::Anchor);
    let attached_id = attached.id.clone();
    let sa_attached = token_for(&app, attached, &[]).await;
    let mut app_d = Application::new("scope-d", "SCOPE-D");
    app_d.service_account_id = Some(attached_id);
    app.repos
        .application_repo
        .insert(&app_d)
        .await
        .expect("insert application");
    assert_eq!(
        sync_roles(&app, &sa_attached, "scope-d").await.0,
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
        &[&app_b],
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

/// A service account made by the real provisioning endpoint is granted the
/// application-service role (PROVISIONED, as Go does) and reaches its own
/// application and nothing else.
#[tokio::test]
#[ignore = "requires Docker"]
async fn provisioned_service_account_reaches_only_its_application() {
    // Provisioning encrypts the OAuth client secret; any 32-byte key will do.
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );
    let app = TestApp::setup().await;
    let role_name = application_service_role(&app).await;
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
    assert_eq!(status, StatusCode::CREATED, "provision failed: {}", body);
    let principal_id = body["serviceAccount"]["principalId"]
        .as_str()
        .expect("principalId")
        .to_string();

    let principal = app
        .repos
        .principal_repo
        .find_by_id(&principal_id)
        .await
        .expect("find principal")
        .expect("provisioned principal");
    // Stored as Go provisions it: no all-applications, one access row, and
    // the application-service role marked PROVISIONED.
    assert!(!principal.all_applications);
    assert_eq!(principal.accessible_application_ids, vec![app_a.id.clone()]);
    assert_eq!(principal.roles.len(), 1);
    assert_eq!(principal.roles[0].role, role_name);
    assert_eq!(
        principal.roles[0].assignment_source,
        Some(AssignmentSource::Provisioned)
    );

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

/// Platform config is addressed by `{appCode}`, and a caller is confined to
/// its own applications (A15, owner request). On top of that Java's rule
/// applies (PlatformConfigApi.java:66-97, config-permissions.md §A.2):
/// `platform:admin:config:view` reads, `platform:admin:config:manage` (or the
/// older `:update` stored roles carry) writes. Anchor scope alone passes
/// nothing, and a config-access grant no longer opens the property routes.
#[tokio::test]
#[ignore = "requires Docker"]
async fn platform_config_is_confined_to_the_callers_applications() {
    use fc_platform::role::entity::{permissions, AuthRole};

    let app = TestApp::setup().await;
    let app_a = create_app(&app, "cfg-a").await;
    create_app(&app, "cfg-b").await;
    let body = json!({ "value": "x" });

    let mut role_names = Vec::new();
    for (code, perms) in [
        (
            "cfg-manage",
            vec![
                permissions::admin::CONFIG_READ,
                permissions::admin::CONFIG_MANAGE,
            ],
        ),
        ("cfg-view", vec![permissions::admin::CONFIG_READ]),
        ("cfg-update", vec![permissions::admin::CONFIG_UPDATE]),
    ] {
        let role = AuthRole::new("test", code, code).with_permissions(perms);
        app.repos
            .role_repo
            .insert(&role)
            .await
            .expect("insert role");
        role_names.push(role.name);
    }
    let stored = |email: &str, scope: UserScope, role: Option<&String>| {
        let mut p = Principal::new_user(email, scope);
        p.roles = role.into_iter().map(RoleAssignment::new).collect();
        p
    };
    let insert = |p: Principal| {
        let app = &app;
        async move {
            app.repos
                .principal_repo
                .insert(&p)
                .await
                .expect("insert principal");
            token(app, &p)
        }
    };

    // A service account holding manage, confined to application A.
    let mut sa = Principal::new_service("sa_cfg_a", "SA Cfg A", UserScope::Anchor)
        .with_application_id(&app_a.id);
    sa.roles = vec![RoleAssignment::new(role_names[0].clone())];
    app.repos
        .principal_repo
        .insert(&sa)
        .await
        .expect("insert service account");
    sa.accessible_application_ids = vec![app_a.id.clone()];
    app.repos
        .principal_repo
        .update(&sa)
        .await
        .expect("grant application access");
    let sa_a = token(&app, &sa);
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

    // An anchor with no config permission reads and writes nothing.
    let bare = insert(stored(
        "cfg-bare@flowcatalyst.test",
        UserScope::Anchor,
        None,
    ))
    .await;
    assert_eq!(
        app.get("/api/config/cfg-b", &bare).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.put("/api/config/cfg-b/general/colour", &bare, body.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // view reads but doesn't write, whatever the tier.
    let viewer = insert(stored(
        "cfg-viewer@flowcatalyst.test",
        UserScope::Client,
        Some(&role_names[1]),
    ))
    .await;
    assert_eq!(
        app.get("/api/config/cfg-b", &viewer).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        app.put("/api/config/cfg-b/general/colour", &viewer, body.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // The older update code still writes.
    let updater = insert(stored(
        "cfg-updater@flowcatalyst.test",
        UserScope::Anchor,
        Some(&role_names[2]),
    ))
    .await;
    assert_eq!(
        app.put("/api/config/cfg-b/general/colour", &updater, body)
            .await
            .status(),
        StatusCode::CREATED
    );

    // Access grants keep their own gate (anchor plus the config permission),
    // and a grant no longer opens the property routes.
    let full_admin = {
        app.anchor_admin_token().await; // seeds the platform:test-admin role
        let full = stored(
            "cfg-full@flowcatalyst.test",
            UserScope::Anchor,
            Some(&"platform:test-admin".to_string()),
        );
        insert(full).await
    };
    let (status, grant) = read_json(
        app.post(
            "/api/config-access/cfg-b",
            &full_admin,
            json!({ "roleCode": "cfg-b:reader", "canRead": true, "canWrite": true }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{grant}");
    let mut granted = stored("cfg-granted@flowcatalyst.test", UserScope::Client, None);
    granted.roles = vec![RoleAssignment::new("cfg-b:reader")];
    let granted = insert(granted).await;
    assert_eq!(
        app.get("/api/config/cfg-b", &granted).await.status(),
        StatusCode::FORBIDDEN
    );
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
    assert_eq!(status, StatusCode::CREATED, "{body}");
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

/// The all-applications toggle on the application-access endpoint: only an
/// all-applications caller may turn it on, never on an account bound to an
/// application, and it applies at once.
#[tokio::test]
#[ignore = "requires Docker"]
async fn all_applications_toggle() {
    let app = TestApp::setup().await;
    let app_a = create_app(&app, "tog-a").await;
    create_app(&app, "tog-b").await;

    // A persisted anchor admin (users default to all applications) and one
    // confined to its grants.
    let admin_principal = Principal::new_user("tog-admin@flowcatalyst.test", UserScope::Anchor);
    app.repos
        .principal_repo
        .insert(&admin_principal)
        .await
        .unwrap();
    let admin = token(&app, &admin_principal);
    let mut narrow_principal =
        Principal::new_user("tog-narrow@flowcatalyst.test", UserScope::Anchor);
    narrow_principal.all_applications = false;
    app.repos
        .principal_repo
        .insert(&narrow_principal)
        .await
        .unwrap();
    let narrow = token(&app, &narrow_principal);

    let sa = Principal::new_service("sa_toggle", "SA Toggle", UserScope::Anchor);
    let sa_id = sa.id.clone();
    let sa_token = token_for(&app, sa, &[]).await;
    let path = format!("/api/principals/{sa_id}/application-access");

    // Off: nothing, and the read says so.
    assert_eq!(
        sync_roles(&app, &sa_token, "tog-b").await.0,
        StatusCode::NOT_FOUND
    );
    let (_, read) = read_json(app.get(&path, &admin).await).await;
    assert_eq!(read["allApplications"], false);

    // Only an all-applications caller may turn it on.
    let resp = app
        .put(
            &path,
            &narrow,
            json!({ "applicationIds": [], "allApplications": true }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let (status, body) = read_json(
        app.put(
            &path,
            &admin,
            json!({ "applicationIds": [], "allApplications": true }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allApplications"], true);
    for code in ["tog-a", "tog-b"] {
        assert_eq!(
            sync_roles(&app, &sa_token, code).await.0,
            StatusCode::OK,
            "{code}"
        );
    }

    // Editing only the list leaves the flag alone.
    let (_, body) = read_json(
        app.put(&path, &admin, json!({ "applicationIds": [app_a.id] }))
            .await,
    )
    .await;
    assert_eq!(body["allApplications"], true);

    // Off again: back to the grants only.
    let (_, body) = read_json(
        app.put(
            &path,
            &admin,
            json!({ "applicationIds": [app_a.id], "allApplications": false }),
        )
        .await,
    )
    .await;
    assert_eq!(body["allApplications"], false);
    assert_eq!(sync_roles(&app, &sa_token, "tog-a").await.0, StatusCode::OK);
    assert_eq!(
        sync_roles(&app, &sa_token, "tog-b").await.0,
        StatusCode::NOT_FOUND
    );

    // An account made for one application may be given every application
    // too, as in Go: the flag is the whole rule.
    let bound = Principal::new_service("sa_tog_bound", "SA Bound", UserScope::Anchor)
        .with_application_id(&app_a.id);
    let bound_id = bound.id.clone();
    token_for(&app, bound, &[]).await;
    let resp = app
        .put(
            &format!("/api/principals/{bound_id}/application-access"),
            &admin,
            json!({ "applicationIds": [], "allApplications": true }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

fn token(app: &TestApp, principal: &Principal) -> String {
    app.auth_service
        .generate_access_token(principal)
        .expect("token")
}

/// A removeUnlisted sweep needs only the sync permission and the application,
/// as in Java (SyncDispatchPools.java:52): no anchor requirement on top.
#[tokio::test]
#[ignore = "requires Docker"]
async fn dispatch_pool_sweep_needs_only_the_sync_permission() {
    use fc_platform::role::entity::{permissions, AuthRole};

    let app = TestApp::setup().await;
    create_app(&app, "sweep-a").await;
    let role = AuthRole::new("test", "pool-sync", "Pool Sync")
        .with_permission(permissions::admin::DISPATCH_POOL_SYNC);
    app.repos
        .role_repo
        .insert(&role)
        .await
        .expect("insert role");

    let mut syncer = Principal::new_user("sweeper@flowcatalyst.test", UserScope::Client);
    syncer.all_applications = true;
    syncer.roles = vec![RoleAssignment::new(role.name.clone())];
    app.repos
        .principal_repo
        .insert(&syncer)
        .await
        .expect("insert principal");
    let syncer_token = token(&app, &syncer);

    let body = json!({ "pools": [{ "code": "sweep-pool", "name": "Sweep", "concurrency": 1 }] });
    let sync = |token: String, sweep: bool| {
        let body = body.clone();
        let app = &app;
        async move {
            let path = if sweep {
                "/api/applications/sweep-a/dispatch-pools/sync?removeUnlisted=true"
            } else {
                "/api/applications/sweep-a/dispatch-pools/sync"
            };
            app.post(path, &token, body).await.status()
        }
    };

    assert_eq!(sync(syncer_token.clone(), false).await, StatusCode::OK);
    assert_eq!(sync(syncer_token, true).await, StatusCode::OK);

    // Without the sync permission the sweep is still refused.
    let mut other = Principal::new_user("sweep-none@flowcatalyst.test", UserScope::Anchor);
    other.roles = vec![];
    app.repos
        .principal_repo
        .insert(&other)
        .await
        .expect("insert principal");
    assert_eq!(sync(token(&app, &other), true).await, StatusCode::FORBIDDEN);
}
