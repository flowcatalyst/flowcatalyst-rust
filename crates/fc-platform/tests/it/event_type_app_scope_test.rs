//! An application's service account lists its event types and pushes their
//! schemas through the plain event-type endpoints (the SDK's schema sync does
//! exactly this). It holds the application-service permissions, not the
//! messaging ones, and is confined to the applications it is bound to.
//! Requires Docker or a PostgreSQL (see `support`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::application_scope_test::create_app;
use crate::support::{read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::AuthRole;
use fc_platform::service_account::entity::RoleAssignment;

const MINE: &str = "etscope-mine:orders:order:created";
const FOREIGN: &str = "etscope-other:orders:order:created";

/// Persist a service account bound to `bound` and holding a role made of
/// `permissions`, and mint a token for it.
async fn token_with(
    app: &TestApp,
    name: &str,
    bound: &Application,
    permissions: &[&str],
) -> String {
    let mut role = AuthRole::new("etscope", name, name);
    for p in permissions {
        role = role.with_permission(*p);
    }
    app.repos
        .role_repo
        .insert(&role)
        .await
        .expect("insert role");

    let mut principal =
        Principal::new_service(name, name, UserScope::Anchor).with_application_id(bound.id.clone());
    principal.roles = vec![RoleAssignment::new(role.name.clone())];
    app.repos
        .principal_repo
        .insert(&principal)
        .await
        .expect("insert principal");
    principal.accessible_application_ids = vec![bound.id.clone()];
    app.repos
        .principal_repo
        .update(&principal)
        .await
        .expect("grant application access");
    app.auth_service
        .generate_access_token(&principal)
        .expect("token")
}

async fn create_event_type(app: &TestApp, admin: &str, code: &str) -> String {
    let (status, body) = read_json(
        app.post(
            "/api/event-types",
            admin,
            json!({ "code": code, "name": "Order created" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("event type id").to_string()
}

async fn get(app: &TestApp, token: &str, path: &str) -> (StatusCode, Value) {
    read_json(app.get(path, token).await).await
}

async fn add_schema(app: &TestApp, token: &str, id: &str, version: &str) -> (StatusCode, Value) {
    read_json(
        app.post(
            &format!("/api/event-types/{id}/versions"),
            token,
            json!({ "version": version, "schema": { "type": "object" } }),
        )
        .await,
    )
    .await
}

fn codes(list: &Value) -> Vec<String> {
    list["items"]
        .as_array()
        .expect("items array")
        .iter()
        .filter_map(|i| i["code"].as_str().map(str::to_string))
        .collect()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn application_service_account_event_types_are_confined_to_its_application() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let mine = create_app(&app, "etscope-mine").await;
    create_app(&app, "etscope-other").await;
    let own_id = create_event_type(&app, &admin, MINE).await;
    let foreign_id = create_event_type(&app, &admin, FOREIGN).await;

    let svc = token_with(
        &app,
        "svc",
        &mine,
        &[
            "platform:application-service:event-type:view",
            "platform:application-service:event-type:create",
            "platform:application-service:event-type:update",
        ],
    )
    .await;

    // List shows only its own application's event types.
    let (status, list) = get(&app, &svc, "/api/event-types?status=CURRENT").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let listed = codes(&list);
    assert!(listed.contains(&MINE.to_string()), "{listed:?}");
    assert!(!listed.contains(&FOREIGN.to_string()), "{listed:?}");
    assert!(
        listed.iter().all(|c| c.starts_with("etscope-mine:")),
        "unexpected event type in a confined list: {listed:?}"
    );

    // A list filtered to another application is empty.
    let (status, list) = get(&app, &svc, "/api/event-types?application=etscope-other").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert!(codes(&list).is_empty());

    // Reads its own event type by id and by code.
    assert_eq!(
        get(&app, &svc, &format!("/api/event-types/{own_id}"))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        get(&app, &svc, &format!("/api/event-types/by-code/{MINE}"))
            .await
            .0,
        StatusCode::OK
    );

    // Is refused another application's event type.
    assert_eq!(
        get(&app, &svc, &format!("/api/event-types/{foreign_id}"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(&app, &svc, &format!("/api/event-types/by-code/{FOREIGN}"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    // Adds a schema version to its own event type.
    let (status, body) = add_schema(&app, &svc, &own_id, "1.0").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body["specVersions"]
        .as_array()
        .expect("specVersions")
        .is_empty());

    // Cannot add one to another application's event type, and nothing is stored.
    let (status, _) = add_schema(&app, &svc, &foreign_id, "1.0").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, after) = get(&app, &admin, &format!("/api/event-types/{foreign_id}")).await;
    assert!(
        after["specVersions"]
            .as_array()
            .expect("specVersions")
            .is_empty(),
        "a refused schema push must not be stored: {after}"
    );

    // A view-only service account cannot add a schema.
    let view_only = token_with(
        &app,
        "viewonly",
        &mine,
        &["platform:application-service:event-type:view"],
    )
    .await;
    let (status, _) = add_schema(&app, &view_only, &own_id, "1.1").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The plain create/update/delete endpoints stay messaging-only.
    let (status, _) = read_json(
        app.post(
            "/api/event-types",
            &svc,
            json!({ "code": "etscope-mine:orders:order:paid", "name": "Order paid" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The messaging permissions still read and write any application.
    assert_eq!(
        get(&app, &admin, &format!("/api/event-types/{foreign_id}"))
            .await
            .0,
        StatusCode::OK
    );
    let (status, body) = add_schema(&app, &admin, &foreign_id, "1.0").await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
