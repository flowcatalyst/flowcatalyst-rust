//! Go's F18 (`52c51e68`, docs/platform-bug-fixes.md in flowcatalyst-go): a
//! service account's token carried the role string `example: dispatcher` (a
//! space after the colon) because the service-account role assignment stored
//! every string it was sent. Role names and permissions are now shape-checked
//! at every write (400 `INVALID_ROLE_NAME` / `INVALID_PERMISSION`, rejected,
//! never trimmed); the service-account path also requires an existing role
//! (400 `ROLE_NOT_FOUND`); a role is written with its application's id (admin
//! create refuses an unknown application, 400 `APPLICATION_NOT_FOUND`), and
//! the platform catalogue carries the platform application's id.
//! Requires Docker (or `FC_TEST_DATABASE_URL`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::support::{self, assert_status, read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{roles, AuthRole, RoleSource};
use fc_platform::service::RoleSyncService;
use fc_platform::shared::database::seed_builtin_roles;
use fc_platform::usecase::Caller;

fn error_code(body: &Value) -> &str {
    body["error"].as_str().unwrap_or_default()
}

async fn app_with(code: &str) -> (TestApp, String, Application) {
    support::set_app_key();
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    let application = Application::new(code, code);
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");
    (app, admin, application)
}

async fn create_role(
    app: &TestApp,
    admin: &str,
    application: &str,
    name: &str,
) -> (StatusCode, Value) {
    read_json(
        app.post(
            "/api/roles",
            admin,
            json!({ "applicationCode": application, "roleName": name, "displayName": name }),
        )
        .await,
    )
    .await
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_service_account_assignment_refuses_malformed_and_unknown_roles() {
    let (app, admin, _) = app_with("examplef18").await;
    let (status, body) = create_role(&app, &admin, "examplef18", "dispatcher").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let created = assert_status(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": "saroles-f18", "name": "Roles F18" }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let sa = created["serviceAccount"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let path = format!("/api/service-accounts/{sa}/roles");
    let stored = || {
        let (app, sa) = (&app, sa.clone());
        async move {
            let mut roles: Vec<String> = sqlx::query_scalar(
                "SELECT pr.role_name FROM iam_principal_roles pr \
                 JOIN iam_principals p ON p.id = pr.principal_id \
                 WHERE p.service_account_id = $1 ORDER BY 1",
            )
            .bind(&sa)
            .fetch_all(&app.pool)
            .await
            .expect("roles");
            roles.sort();
            roles
        }
    };

    let (status, body) = read_json(
        app.put(&path, &admin, json!({ "roles": ["examplef18:dispatcher"] }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(stored().await, vec!["examplef18:dispatcher"]);

    for bad in [
        "examplef18: dispatcher",
        "examplef18 :dispatcher",
        " examplef18:dispatcher",
        "examplef18:dispatcher ",
        "examplef18:dis patcher",
        "dispatcher",
        "",
    ] {
        let (status, body) = read_json(
            app.put(
                &path,
                &admin,
                json!({ "roles": ["examplef18:dispatcher", bad] }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}: {body}");
        assert_eq!(error_code(&body), "INVALID_ROLE_NAME", "{bad:?}: {body}");
    }

    let (status, body) = read_json(
        app.put(&path, &admin, json!({ "roles": ["examplef18:nosuchrole"] }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "ROLE_NOT_FOUND", "{body}");

    assert_eq!(
        stored().await,
        vec!["examplef18:dispatcher"],
        "a refused assignment leaves the stored roles alone"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn role_writes_refuse_malformed_names_and_permissions() {
    let (app, admin, application) = app_with("f18create").await;

    for (code, name) in [
        ("f18create", " dispatcher"),
        ("f18create", "dis patcher"),
        ("f18create ", "dispatcher"),
        ("f18create", "dispatcher\n"),
        ("f18create", "a:"),
    ] {
        let (status, body) = create_role(&app, &admin, code, name).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{code:?}/{name:?}: {body}");
        assert_eq!(
            error_code(&body),
            "INVALID_ROLE_NAME",
            "{code:?}/{name:?}: {body}"
        );
    }
    let (status, body) = read_json(
        app.post(
            "/api/roles",
            &admin,
            json!({ "applicationCode": "f18create", "roleName": "ok", "displayName": "d",
                    "permissions": ["f18create: thing:view"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "INVALID_PERMISSION", "{body}");
    assert!(
        app.repos
            .role_repo
            .find_by_name("f18create: dispatcher")
            .await
            .unwrap()
            .is_none(),
        "nothing was stored under a refused spelling"
    );

    // The role is written with the id of the application that owns it.
    let (status, body) = create_role(&app, &admin, "f18create", "dispatcher").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let stored = app
        .repos
        .role_repo
        .find_by_name("f18create:dispatcher")
        .await
        .unwrap()
        .expect("role");
    assert_eq!(stored.application_id, Some(application.id.clone()));

    // A role for an application that does not exist is refused.
    let (status, body) = create_role(&app, &admin, "f18nosuchapp", "x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "APPLICATION_NOT_FOUND", "{body}");

    // Update (permissions) and grant (by body).
    let (status, body) = read_json(
        app.put(
            "/api/roles/f18create:dispatcher",
            &admin,
            json!({ "permissions": ["f18create:thing: view"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "INVALID_PERMISSION", "{body}");
    let (status, body) = read_json(
        app.post(
            "/api/roles/f18create:dispatcher/permissions",
            &admin,
            json!({ "permission": "f18create:thing: view" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "INVALID_PERMISSION", "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_sdk_role_sync_refuses_malformed_entries_and_adopts_an_idless_role() {
    let (app, admin, application) = app_with("f18sync").await;
    let sync = |roles: Value| {
        let (app, admin) = (&app, admin.clone());
        async move {
            read_json(
                app.post(
                    "/api/applications/f18sync/roles/sync",
                    &admin,
                    json!({ "roles": roles }),
                )
                .await,
            )
            .await
        }
    };

    let (status, body) = sync(json!([{ "name": "good" }, { "name": "bad name" }])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "INVALID_ROLE_NAME", "{body}");
    assert!(
        app.repos
            .role_repo
            .find_by_name("f18sync:good")
            .await
            .unwrap()
            .is_none(),
        "a malformed entry refuses the whole sync"
    );

    let (status, body) = sync(json!([{ "name": "good", "permissions": ["f18sync: x"] }])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "INVALID_PERMISSION", "{body}");

    // A legacy row of the application with a null application_id (the admin
    // create used to leave it null) is adopted, not collided with.
    let mut legacy = AuthRole::new("f18sync", "legacy", "Legacy");
    legacy.source = RoleSource::Sdk;
    assert!(legacy.application_id.is_none());
    app.repos.role_repo.insert(&legacy).await.expect("legacy");
    let (status, body) = sync(json!([{ "name": "legacy" }])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let adopted = app
        .repos
        .role_repo
        .find_by_name("f18sync:legacy")
        .await
        .unwrap()
        .expect("role");
    assert_eq!(
        adopted.application_id,
        Some(application.id.clone()),
        "the sync gives the adopted role its application id"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn principal_and_idp_mapping_writes_check_the_role_name() {
    let (app, admin, _) = app_with("f18prn").await;
    let (status, body) = create_role(&app, &admin, "f18prn", "clerk").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // A user to assign to.
    let user = Principal::new_user("f18@f18prn.test", UserScope::Anchor);
    app.repos
        .principal_repo
        .insert(&user)
        .await
        .expect("insert user");
    let user = user.id.to_string();

    let (status, body) = read_json(
        app.put(
            &format!("/api/principals/{user}/roles"),
            &admin,
            json!({ "roles": ["f18prn: clerk"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "INVALID_ROLE_NAME", "{body}");
    let (status, body) = read_json(
        app.put(
            &format!("/api/principals/{user}/roles"),
            &admin,
            json!({ "roles": ["f18prn:ghost"] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "ROLE_NOT_FOUND", "{body}");

    // IdP role mapping: a malformed platform role is refused.
    let (status, body) = read_json(
        app.post(
            "/api/idp-role-mappings",
            &admin,
            json!({ "idpType": "OIDC", "idpRoleName": "staff", "platformRoleName": "f18prn: clerk" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "INVALID_ROLE_NAME", "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_platform_principal_sync_skips_names_that_are_not_roles() {
    let (app, admin, _) = app_with("f18mal").await;
    let (status, body) = create_role(&app, &admin, "f18mal", "dispatcher").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = read_json(
        app.post(
            "/api/principals/sync",
            &admin,
            json!({ "principals": [{
                "email": "a@f18malformed.test", "name": "A",
                "roles": ["f18mal: dispatcher", "f18mal:dispatcher", "f18mal:ghost"]
            }] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT pr.role_name FROM iam_principal_roles pr \
         JOIN iam_principals p ON p.id = pr.principal_id \
         WHERE p.email = 'a@f18malformed.test' ORDER BY 1",
    )
    .fetch_all(&app.pool)
    .await
    .expect("roles");
    assert_eq!(
        stored,
        vec!["f18mal:dispatcher"],
        "a malformed name and an unknown one are never stored"
    );

    // The application's route skips a malformed name and reports it.
    let (status, body) = read_json(
        app.post(
            "/api/applications/f18mal/principals/sync",
            &admin,
            json!({ "principals": [{
                "email": "b@f18malformed.test", "name": "B",
                "roles": ["f18mal: dispatcher", "f18mal:dispatcher"]
            }] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["skippedRoles"],
        json!(["f18mal: dispatcher"]),
        "{body}"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_platform_catalogue_carries_the_platform_applications_id() {
    support::set_app_key();
    let app = TestApp::setup().await;
    let platform = app
        .repos
        .application_repo
        .find_by_code("platform")
        .await
        .unwrap()
        .expect("platform application")
        .id;

    seed_builtin_roles(&app.pool).await.expect("seed");
    let idless = |app: &TestApp| {
        let pool = app.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM iam_roles WHERE application_code = 'platform' AND application_id IS NULL",
            )
            .fetch_one(&pool)
            .await
            .expect("count")
        }
    };
    assert_eq!(idless(&app).await, 0, "the seed sets the id");
    let super_admin = app
        .repos
        .role_repo
        .find_by_name(&roles::super_admin().name)
        .await
        .unwrap()
        .expect("super admin");
    assert_eq!(super_admin.application_id, Some(platform.clone()));

    // Heal: a row written without its id is given it by the next sync.
    sqlx::query("UPDATE iam_roles SET application_id = NULL WHERE application_code = 'platform'")
        .execute(&app.pool)
        .await
        .expect("null the ids");
    assert!(idless(&app).await > 0);
    RoleSyncService::new(app.repos.role_repo.clone())
        .sync_code_defined_roles(&Caller::system())
        .await
        .expect("sync");
    assert_eq!(idless(&app).await, 0, "the platform role sync heals the id");
}
