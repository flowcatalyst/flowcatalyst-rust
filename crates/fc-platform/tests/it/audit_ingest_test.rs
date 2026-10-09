//! `POST /api/audit-logs/batch` against a real database: the Go platform's
//! behaviour (`internal/platform/shared/sdk/audit_batch.go`). Items for a
//! client the caller cannot access are skipped while the rest land,
//! `performedAt` is stored as sent (now when it is unparseable), and an item
//! without a principal is refused in its own slot.

use crate::support;

use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use fc_platform_core::shared::id::ClientId;
use support::{assert_status, read_json, TestApp};

/// A persisted service principal with the all-applications flag (the audit
/// ingest confines application-scoped callers; these tests are not about it)
/// holding the audit batch-write permission the endpoint requires.
async fn all_apps_token(app: &TestApp, scope: UserScope, client_id: Option<&str>) -> String {
    token_with_perms(
        app,
        scope,
        client_id,
        &[permissions::admin::BATCH_AUDIT_LOGS_WRITE],
    )
    .await
}

async fn token_with_perms(
    app: &TestApp,
    scope: UserScope,
    client_id: Option<&str>,
    perms: &[&str],
) -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut principal = Principal::new_service(format!("svc-test-{n}"), "Test Service", scope);
    if let Some(client_id) = client_id {
        principal = principal.with_client_id(ClientId::parse(client_id).unwrap());
    }
    principal.all_applications = true;
    app.repos
        .principal_repo
        .insert(&principal)
        .await
        .expect("insert principal");
    let granted: Vec<String> = perms.iter().map(|p| p.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&principal, &granted, None)
        .expect("svc token")
}

async fn stored(app: &TestApp, entity_id: &str) -> Vec<(Option<String>, DateTime<Utc>)> {
    sqlx::query_as::<_, (Option<String>, DateTime<Utc>)>(
        "SELECT client_id, performed_at FROM aud_logs WHERE entity_id = $1",
    )
    .bind(entity_id)
    .fetch_all(&app.pool)
    .await
    .expect("read aud_logs")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn ingest_skips_inaccessible_clients_and_honours_performed_at() {
    let app = TestApp::setup().await;
    let mine = Client::new("Mine", "mine");
    let theirs = Client::new("Theirs", "theirs");
    app.repos.client_repo.insert(&mine).await.expect("client");
    app.repos.client_repo.insert(&theirs).await.expect("client");
    let token = all_apps_token(&app, UserScope::Client, Some(mine.id.as_str())).await;

    let before = Utc::now();
    let resp = app
        .post(
            "/api/audit-logs/batch",
            &token,
            json!({ "items": [
                {
                    "entityType": "Order", "entityId": "ord_mine", "operation": "CREATE",
                    "principalId": "prn_sdk", "clientCode": "mine",
                    "performedAt": "2025-03-04T05:06:07Z",
                    "operationData": { "password": "x", "name": "kept" },
                },
                {
                    "entityType": "Order", "entityId": "ord_theirs", "operation": "CREATE",
                    "principalId": "prn_sdk", "clientCode": "theirs",
                },
                {
                    "entityType": "Order", "entityId": "ord_bad_time", "operation": "CREATE",
                    "principalId": "prn_sdk", "clientCode": "mine",
                    "performedAt": "not a time",
                },
                {
                    "entityType": "Order", "entityId": "ord_no_actor", "operation": "CREATE",
                },
                {
                    "entityType": "Order", "entityId": "ord_unknown", "operation": "CREATE",
                    "principalId": "prn_sdk", "clientCode": "nobody",
                },
            ]}),
        )
        .await;
    let body = assert_status(resp, StatusCode::OK).await;
    let statuses: Vec<&str> = body["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|r| r["status"].as_str().expect("status"))
        .collect();
    assert_eq!(
        statuses,
        ["SUCCESS", "SKIPPED", "SUCCESS", "BAD_REQUEST", "SKIPPED"],
        "{body}"
    );
    assert_eq!(body["results"][1]["id"], "", "{body}");
    assert!(body["results"][1].get("error").is_none(), "{body}");
    assert_eq!(body["results"][3]["error"], "principalId is required");

    let rows = stored(&app, "ord_mine").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0.as_deref(), Some(mine.id.as_str()));
    let sent: DateTime<Utc> = "2025-03-04T05:06:07Z".parse().unwrap();
    assert_eq!(rows[0].1, sent, "performedAt is stored as sent");

    let rows = stored(&app, "ord_bad_time").await;
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].1 >= before - Duration::seconds(1) && rows[0].1 <= Utc::now(),
        "an unparseable performedAt falls back to now: {:?}",
        rows[0].1
    );

    for skipped in ["ord_theirs", "ord_no_actor", "ord_unknown"] {
        assert!(stored(&app, skipped).await.is_empty(), "{skipped} stored");
    }

    // The redaction backstop still runs on the batched path.
    let (json,): (serde_json::Value,) =
        sqlx::query_as("SELECT operation_json FROM aud_logs WHERE entity_id = 'ord_mine'")
            .fetch_one(&app.pool)
            .await
            .expect("read operation_json");
    assert_eq!(json, json!({ "password": "***", "name": "kept" }));
}

/// A full batch lands in one insert, and the wire errors match Go: an
/// oversized batch is `400 BATCH_TOO_LARGE`, a malformed body
/// `400 INVALID_JSON`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn ingest_takes_a_full_batch_and_rejects_bad_bodies_like_go() {
    let app = TestApp::setup().await;
    let token = all_apps_token(&app, UserScope::Anchor, None).await;

    let items: Vec<_> = (0..100)
        .map(|i| {
            json!({
                "entityType": "Order", "entityId": format!("ord_{i}"),
                "operation": "CREATE", "principalId": "prn_sdk",
                "clientCode": format!("c{}", i % 3),
            })
        })
        .collect();
    for code in ["c0", "c1", "c2"] {
        app.repos
            .client_repo
            .insert(&Client::new(code, code))
            .await
            .expect("client");
    }
    let resp = app
        .post("/api/audit-logs/batch", &token, json!({ "items": items }))
        .await;
    let body = assert_status(resp, StatusCode::OK).await;
    assert!(body["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["status"] == "SUCCESS"));
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM aud_logs WHERE entity_id LIKE 'ord\\_%'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(count, 100);

    let too_many: Vec<_> = (0..101).map(|_| json!({})).collect();
    let (status, body) = read_json(
        app.post(
            "/api/audit-logs/batch",
            &token,
            json!({ "items": too_many }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "BATCH_TOO_LARGE", "{body}");
    assert_eq!(body["message"], "Maximum 100 items per batch", "{body}");

    let (status, body) = read_json(
        app.post("/api/audit-logs/batch", &token, json!({ "items": "nope" }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "INVALID_JSON", "{body}");
}

/// The endpoint needs `platform:admin:batch:audit-logs-write`: a caller
/// without it (no permissions, or only the other two batch permissions) is
/// refused 403 `PERMISSION_REQUIRED` before the body is read (a malformed
/// body is still a 403, not a 400) and nothing is stored; a holder of the
/// permission, and a holder of the platform wildcard, are accepted.
#[tokio::test]
#[ignore = "requires Docker"]
async fn ingest_requires_the_audit_batch_write_permission() {
    let app = TestApp::setup().await;
    let item = |eid: &str| {
        json!({ "items": [{
            "entityType": "Order", "entityId": eid, "operation": "CREATE",
            "principalId": "prn_sdk",
        }]})
    };

    for perms in [
        &[][..],
        &[
            permissions::admin::BATCH_EVENTS_WRITE,
            permissions::admin::BATCH_DISPATCH_JOBS_WRITE,
        ][..],
    ] {
        let token = token_with_perms(&app, UserScope::Anchor, None, perms).await;
        let (status, body) = read_json(
            app.post("/api/audit-logs/batch", &token, item("perm_denied"))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"], "PERMISSION_REQUIRED", "{body}");

        let (status, body) = read_json(
            app.post("/api/audit-logs/batch", &token, json!({ "items": "nope" }))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    assert!(stored(&app, "perm_denied").await.is_empty());

    let holder = all_apps_token(&app, UserScope::Anchor, None).await;
    let resp = app
        .post("/api/audit-logs/batch", &holder, item("perm_holder"))
        .await;
    let body = assert_status(resp, StatusCode::OK).await;
    assert_eq!(body["results"][0]["status"], "SUCCESS", "{body}");
    assert_eq!(stored(&app, "perm_holder").await.len(), 1);

    // The platform wildcard (super-admin) covers the permission.
    let admin = app.anchor_admin_token_all_applications().await;
    let resp = app
        .post("/api/audit-logs/batch", &admin, item("perm_admin"))
        .await;
    let body = assert_status(resp, StatusCode::OK).await;
    assert_eq!(body["results"][0]["status"], "SUCCESS", "{body}");
    assert_eq!(stored(&app, "perm_admin").await.len(), 1);
}
