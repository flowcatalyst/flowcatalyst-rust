//! Editing the payload of a dispatch job (Go `29e8cb11`,
//! `dispatchjob/api/edit_payload_pg_test.go` and
//! `operations/edit_payload_pg_test.go`): `PUT /api/dispatch-jobs/{id}/payload`
//! and its `/bff` twin. Requires a Postgres (see `support::db`).

use crate::support;

use axum::http::StatusCode;
use fc_platform::role::entity::AuthRole;
use fc_platform_core::permissions;
use serde_json::{json, Value};
use support::{read_json, TestApp};

/// One job in the write table (all `PUT` reads and writes go there).
async fn seed_job(app: &TestApp, id: &str, status: &str, client_id: Option<&str>, payload: &str) {
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs
             (id, code, source, target_url, status, client_id, attempt_count, last_error,
              payload, payload_content_type, created_at, updated_at)
         VALUES ($1, 'edptest:job:x:y', 'edp-test', 'http://example.invalid/hook', $2, $3, 3, 'boom',
                 $4, 'application/json', now() - interval '1 hour', now() - interval '1 hour')",
    )
    .bind(id)
    .bind(status)
    .bind(client_id)
    .bind(payload)
    .execute(&app.pool)
    .await
    .expect("seed job");
}

async fn stored(app: &TestApp, id: &str) -> (String, i32, Option<String>, String) {
    sqlx::query_as(
        "SELECT status, attempt_count, payload, payload_content_type FROM msg_dispatch_jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .unwrap()
}

async fn edit(app: &TestApp, token: &str, id: &str, body: Value) -> (StatusCode, Value) {
    read_json(
        app.put(&format!("/api/dispatch-jobs/{id}/payload"), token, body)
            .await,
    )
    .await
}

/// A token holding exactly `perms`, for a client user of `client` (or an
/// anchor-scoped caller when `client` is `None`).
async fn token_with(app: &TestApp, client: Option<&str>, perms: &[&str]) -> String {
    use fc_platform::domain::{Principal, UserScope};
    use fc_platform::service_account::entity::RoleAssignment;
    use fc_platform_core::shared::id::ClientId;
    let name = format!("edp-{}-{}", client.unwrap_or("anchor"), perms.len());
    let role_code = format!("edp:{name}");
    if app
        .repos
        .role_repo
        .find_by_name(&role_code)
        .await
        .unwrap()
        .is_none()
    {
        let mut role = AuthRole::new("edp", name, "EDP role");
        for p in perms {
            role = role.with_permission(*p);
        }
        app.repos.role_repo.insert(&role).await.unwrap();
    }
    let mut user = match client {
        Some(c) => Principal::new_user("who@edp.test", UserScope::Client)
            .with_client_id(ClientId::parse(c).unwrap()),
        None => Principal::new_user("who@edp.test", UserScope::Anchor),
    };
    user.roles = vec![RoleAssignment::new(role_code)];
    app.auth_service.generate_access_token(&user).unwrap()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_editable_job_takes_the_payload_on_both_mounts_and_keeps_status_and_attempts() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    seed_job(&app, "edpfail000001", "FAILED", None, r#"{"a":1}"#).await;
    seed_job(&app, "edppend000001", "PENDING", None, r#"{"a":1}"#).await;
    seed_job(&app, "edperr0000001", "ERROR", None, r#"{"a":1}"#).await;

    let (status, body) = edit(
        &app,
        &admin,
        "edpfail000001",
        json!({ "payload": r#"{"fixed":true}"# }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["payload"], r#"{"fixed":true}"#);
    assert_eq!(body["status"], "FAILED");
    assert_eq!(body["attemptCount"], 3);

    // The /bff twin.
    let (status, body) = read_json(
        app.put(
            "/bff/dispatch-jobs/edppend000001/payload",
            &admin,
            json!({ "payload": r#"{"fixed":"bff"}"# }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "PENDING");

    // The legacy ERROR spelling is editable and stays ERROR in storage.
    let (status, body) = edit(&app, &admin, "edperr0000001", json!({ "payload": "{}" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(stored(&app, "edperr0000001").await.0, "ERROR");

    // A content type change is persisted; an XML payload is then valid.
    let (status, body) = edit(
        &app,
        &admin,
        "edpfail000001",
        json!({ "payload": "<a/>", "payloadContentType": " application/xml " }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["payloadContentType"], "application/xml");
    let (st, attempts, payload, ct) = stored(&app, "edpfail000001").await;
    assert_eq!(
        (st.as_str(), attempts, payload.as_deref(), ct.as_str()),
        ("FAILED", 3, Some("<a/>"), "application/xml")
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_job_that_is_not_pending_or_failed_is_409_not_editable_and_unchanged() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    for (i, status) in ["QUEUED", "PROCESSING", "COMPLETED", "CANCELLED", "EXPIRED"]
        .into_iter()
        .enumerate()
    {
        let id = format!("edpno{i:08}");
        seed_job(&app, &id, status, None, r#"{"a":1}"#).await;
        let (code, body) = edit(&app, &admin, &id, json!({ "payload": r#"{"x":1}"# })).await;
        assert_eq!(code, StatusCode::CONFLICT, "{status}: {body}");
        assert_eq!(body["error"], "NOT_EDITABLE", "{status}: {body}");
        assert_eq!(stored(&app, &id).await.2.as_deref(), Some(r#"{"a":1}"#));
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_body_is_validated_before_anything_is_written() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    seed_job(&app, "edpval0000001", "FAILED", None, r#"{"a":1}"#).await;
    let id = "edpval0000001";

    let (code, body) = edit(&app, &admin, id, json!({ "payload": "{not json" })).await;
    assert_eq!(
        (code, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("PAYLOAD_NOT_JSON")),
        "{body}"
    );
    let (code, body) = edit(&app, &admin, id, json!({ "payload": "" })).await;
    assert_eq!(
        (code, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("PAYLOAD_REQUIRED")),
        "{body}"
    );
    let (code, body) = edit(
        &app,
        &admin,
        id,
        json!({ "payload": "a".repeat(256 * 1024 + 1) }),
    )
    .await;
    assert_eq!(
        (code, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("PAYLOAD_TOO_LARGE")),
        "{body}"
    );
    for ct in ["  ", "xml"] {
        let (code, body) = edit(
            &app,
            &admin,
            id,
            json!({ "payload": "<a/>", "payloadContentType": ct }),
        )
        .await;
        assert_eq!(
            (code, body["error"].as_str()),
            (StatusCode::BAD_REQUEST, Some("CONTENT_TYPE_INVALID")),
            "{ct}: {body}"
        );
    }
    // A switch back to JSON validates against JSON; XML under XML is fine.
    let (code, body) = edit(
        &app,
        &admin,
        id,
        json!({ "payload": "<a/>", "payloadContentType": "application/json" }),
    )
    .await;
    assert_eq!(
        (code, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("PAYLOAD_NOT_JSON")),
        "{body}"
    );
    // The cap itself is allowed.
    let (code, body) = edit(
        &app,
        &admin,
        id,
        json!({ "payload": "a".repeat(256 * 1024), "payloadContentType": "text/plain" }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");

    let (code, body) = edit(&app, &admin, "edpnosuchjob001", json!({ "payload": "{}" })).await;
    assert_eq!(
        (code, body["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("DispatchJob_NOT_FOUND")),
        "{body}"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_edit_needs_its_own_permission_and_the_jobs_client_scope() {
    let app = TestApp::setup().await;
    seed_job(&app, "edpscp0000001", "FAILED", None, r#"{"a":1}"#).await;
    seed_job(
        &app,
        "edpscp0000002",
        "FAILED",
        Some("clt_edpclient00a"),
        r#"{"a":1}"#,
    )
    .await;
    seed_job(
        &app,
        "edpscp0000003",
        "FAILED",
        Some("clt_edpclient00b"),
        r#"{"a":1}"#,
    )
    .await;

    // `dispatch-job:view` alone does not edit.
    let viewer = token_with(&app, None, &[permissions::admin::DISPATCH_JOB_READ]).await;
    let (code, body) = edit(&app, &viewer, "edpscp0000001", json!({ "payload": "{}" })).await;
    assert_eq!(
        (code, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("PERMISSION_REQUIRED")),
        "{body}"
    );

    let editor = token_with(
        &app,
        Some("clt_edpclient00a"),
        &[permissions::admin::DISPATCH_JOB_EDIT_PAYLOAD],
    )
    .await;
    // Another tenant's job, and a platform job, are out of reach.
    for id in ["edpscp0000003", "edpscp0000001"] {
        let (code, body) = edit(&app, &editor, id, json!({ "payload": "{}" })).await;
        assert_eq!(
            (code, body["error"].as_str()),
            (StatusCode::FORBIDDEN, Some("SCOPE_FORBIDDEN")),
            "{id}: {body}"
        );
        assert_eq!(stored(&app, id).await.2.as_deref(), Some(r#"{"a":1}"#));
    }
    let (code, body) = edit(&app, &editor, "edpscp0000002", json!({ "payload": "{}" })).await;
    assert_eq!(code, StatusCode::OK, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_event_and_the_audit_row_hold_sizes_and_digests_never_the_payload() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    let id = "edpaud0000001";
    seed_job(&app, id, "FAILED", None, r#"{"email":"old@example.com"}"#).await;
    let new = r#"{"email":"new@example.com"}"#;
    let (code, body) = edit(&app, &admin, id, json!({ "payload": new })).await;
    assert_eq!(code, StatusCode::OK, "{body}");

    let (data,): (Value,) = sqlx::query_as(
        "SELECT data::jsonb FROM msg_events WHERE type = 'platform:messaging:dispatch-job:payload-edited' AND subject = $1",
    )
    .bind(format!("platform.dispatchjob.{id}"))
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(!data.to_string().contains("example.com"), "{data}");
    assert_eq!(data["dispatchJobId"], id);
    assert_eq!(data["status"], "FAILED");
    assert_eq!(data["previousSize"], r#"{"email":"old@example.com"}"#.len());
    assert_eq!(data["size"], new.len());
    assert_eq!(data["sha256"].as_str().unwrap().len(), 64);
    assert_ne!(data["sha256"], data["previousSha256"]);
    assert_eq!(data["previousContentType"], "application/json");
    assert_eq!(data["contentType"], "application/json");

    let (op,): (Value,) = sqlx::query_as(
        "SELECT operation_json::jsonb FROM aud_logs WHERE entity_id = $1 AND operation = 'EditPayloadCommand'",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(
        !op.to_string().contains("example.com"),
        "the audit row must not hold the payload: {op}"
    );
    assert!(op.to_string().contains(id));
}
