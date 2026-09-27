//! A sync is planned in full and written in one transaction with its per-row
//! events and rollup, as Go's `usecaseop.Sync` / `usecasepgx.CommitSync`: a
//! bad row, or a row that fails to write, fails the whole sync and nothing is
//! written — no rows, no events, no audit rows. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use support::{read_json, TestApp};

/// An anchor admin that reaches every application (the syncs answer 404 out
/// of scope), and the application `code`.
async fn setup(code: &str) -> (TestApp, String) {
    let app = TestApp::setup().await;
    // Seeds the platform:test-admin role.
    app.anchor_admin_token().await;
    let mut caller = fc_platform::Principal::new_user(
        "sync-admin@flowcatalyst.test",
        fc_platform::UserScope::Anchor,
    );
    caller.all_applications = true;
    caller.roles = vec![fc_platform::service_account::entity::RoleAssignment::new(
        "platform:test-admin",
    )];
    app.repos
        .principal_repo
        .insert(&caller)
        .await
        .expect("insert caller");
    let token = app
        .auth_service
        .generate_access_token(&caller)
        .expect("caller token");
    app.repos
        .application_repo
        .insert(&fc_platform::application::entity::Application::new(
            code, code,
        ))
        .await
        .expect("insert application");
    (app, token)
}

async fn count(app: &TestApp, sql: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(sql)
        .fetch_one(&app.pool)
        .await
        .expect("count");
    n
}

/// Rows, events and audit rows, to show a refused sync wrote none.
async fn totals(app: &TestApp, table: &str) -> (i64, i64, i64) {
    (
        count(app, &format!("SELECT COUNT(*) FROM {table}")).await,
        count(app, "SELECT COUNT(*) FROM msg_events").await,
        count(app, "SELECT COUNT(*) FROM aud_logs").await,
    )
}

async fn post(app: &TestApp, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
    read_json(app.post(path, token, body).await).await
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_event_type_sync_with_a_bad_row_writes_nothing() {
    let (app, token) = setup("ets").await;
    let path = "/api/applications/ets/event-types/sync?removeUnlisted=true";

    // A first sync lands: two types.
    let (status, body) = post(
        &app,
        &token,
        path,
        json!({"eventTypes": [
            {"code": "ets:orders:order:created", "name": "Created"},
            {"code": "ets:orders:order:shipped", "name": "Shipped"}
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["created"], 2, "{body}");
    let before = totals(&app, "msg_event_types").await;

    // A rename, a new type, a removal (shipped is unlisted) and then a code
    // that is no event type code: the bad row is refused with its own code,
    // and none of the others is written.
    let (status, body) = post(
        &app,
        &token,
        path,
        json!({"eventTypes": [
            {"code": "ets:orders:order:created", "name": "Renamed"},
            {"code": "ets:orders:order:paid", "name": "Paid"},
            {"code": "not-a-code", "name": "Bad"}
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_EVENT_TYPE_CODE", "{body}");
    assert_eq!(totals(&app, "msg_event_types").await, before);
    let (name,): (String,) =
        sqlx::query_as("SELECT name FROM msg_event_types WHERE code = 'ets:orders:order:created'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(name, "Created");

    // The same sync without the bad row lands whole: the rows, one event and
    // audit row per row, and the rollup.
    let (status, body) = post(
        &app,
        &token,
        path,
        json!({"eventTypes": [
            {"code": "ets:orders:order:created", "name": "Renamed"},
            {"code": "ets:orders:order:paid", "name": "Paid"}
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (&body["created"], &body["updated"], &body["deleted"]),
        (&json!(1), &json!(1), &json!(1)),
        "{body}"
    );
    let (types, events, audits) = totals(&app, "msg_event_types").await;
    assert_eq!(types, before.0);
    assert_eq!(events, before.1 + 4, "created, updated, deleted, synced");
    assert_eq!(audits, before.2 + 4);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_process_sync_with_a_bad_row_writes_nothing() {
    let (app, token) = setup("prc").await;
    let path = "/api/applications/prc/processes/sync";
    let before = totals(&app, "msg_processes").await;

    let (status, body) = post(
        &app,
        &token,
        path,
        json!({"processes": [
            {"code": "prc:orders:fulfil", "name": "Fulfil", "body": "graph TD; A-->B"},
            {"code": "bad", "name": "Bad", "body": ""}
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_PROCESS_CODE", "{body}");
    assert_eq!(totals(&app, "msg_processes").await, before);

    // A row that fails only when written (the same new code twice; the code
    // is unique) rolls back the row written before it.
    let (status, body) = post(
        &app,
        &token,
        path,
        json!({"processes": [
            {"code": "prc:orders:fulfil", "name": "Fulfil", "body": "graph TD; A-->B"},
            {"code": "prc:orders:fulfil", "name": "Again", "body": "graph TD; A-->B"}
        ]}),
    )
    .await;
    assert!(!status.is_success(), "{status}: {body}");
    assert_eq!(totals(&app, "msg_processes").await, before);

    let (status, body) = post(
        &app,
        &token,
        path,
        json!({"processes": [
            {"code": "prc:orders:fulfil", "name": "Fulfil", "body": "graph TD; A-->B"}
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (rows, events, audits) = totals(&app, "msg_processes").await;
    assert_eq!(
        (rows, events, audits),
        (before.0 + 1, before.1 + 2, before.2 + 2)
    );
}

/// Go refuses to remove a role principals still hold; the roles planned
/// before that refusal are not written either.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_role_sync_refused_for_assignments_writes_nothing() {
    let (app, token) = setup("rls").await;
    let path = "/api/applications/rls/roles/sync";
    let (status, body) = post(
        &app,
        &token,
        path,
        json!({"roles": [{"name": "keeper", "permissions": ["rls:orders:read"]}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    sqlx::query(
        "INSERT INTO iam_principal_roles (principal_id, role_name, assigned_at) \
         SELECT id, 'rls:keeper', NOW() FROM iam_principals LIMIT 1",
    )
    .execute(&app.pool)
    .await
    .expect("assign role");
    let before = totals(&app, "iam_roles").await;

    let (status, body) = post(
        &app,
        &token,
        &format!("{path}?removeUnlisted=true"),
        json!({"roles": [{"name": "newcomer", "permissions": ["rls:orders:read"]}]}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "ROLE_HAS_ASSIGNMENTS", "{body}");
    assert_eq!(totals(&app, "iam_roles").await, before);
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM iam_roles WHERE name = 'rls:newcomer'"
        )
        .await,
        0
    );
}

/// The platform catalogue sync writes each type's schema as its spec version
/// "1.0" in the same transaction, and a repeat finds every schema unchanged.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_platform_sync_writes_schemas_with_the_types() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let (status, first) = post(&app, &token, "/bff/event-types/sync-platform", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let with_schema = first["schemas"]["created"].as_i64().unwrap()
        + first["schemas"]["updated"].as_i64().unwrap();
    assert!(with_schema > 0, "{first}");
    let stored = count(
        &app,
        "SELECT COUNT(*) FROM msg_event_type_spec_versions sv \
         JOIN msg_event_types et ON et.id = sv.event_type_id \
         WHERE et.application = 'platform' AND sv.version = '1.0'",
    )
    .await;
    assert!(stored >= with_schema, "{stored} < {with_schema}");

    let (status, again) = post(&app, &token, "/bff/event-types/sync-platform", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["schemas"]["created"], 0, "{again}");
    assert_eq!(again["schemas"]["updated"], 0, "{again}");
    assert_eq!(again["schemas"]["unchanged"], again["total"], "{again}");
}
