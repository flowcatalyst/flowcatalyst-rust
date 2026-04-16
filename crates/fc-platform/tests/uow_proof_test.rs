//! UoW proof tests: verify that every write handler migrated in Phase 2
//! persists both a `msg_events` row (domain event) and an `aud_logs` row
//! (audit log) atomically with the entity write.
//!
//! These tests are the critical end-to-end proof that the UoW machinery
//! set up in Phase 1+2 actually works under real HTTP traffic. They
//! complement the compile-time seal (`UseCaseResult::success`) and the
//! convention tests (`uow_convention_test`).
//!
//! Covers both patterns:
//!   - **UseCase flow** — handler calls `use_case.run(cmd, ctx)` which
//!     commits via `UnitOfWork::commit` (e.g. `create_client`).
//!   - **Inline `emit_event` flow** — handler writes via the repo then
//!     calls `unit_of_work.emit_event(event, &cmd)` (used in
//!     `auth/config_api.rs` and `auth/oauth_clients_api.rs` where the
//!     set of variant update routes is too wide for one narrow use case).

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::json;

use support::{assert_status, TestApp};

// ── UseCase pattern: create_client ───────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn create_client_emits_event_and_audit_log() {
    let app = TestApp::setup().await;
    let token = app.anchor_token();

    let resp = app
        .post(
            "/api/clients",
            &token,
            json!({ "identifier": "uow-test-1", "name": "UoW Test Client" }),
        )
        .await;
    let body = assert_status(resp, StatusCode::CREATED).await;

    let client_id = body
        .get("id")
        .and_then(|v| v.as_str())
        .expect("response should contain id")
        .to_string();

    // Exactly one event + one audit log for this aggregate.
    assert_eq!(
        app.event_count_for(&client_id).await,
        1,
        "expected one msg_events row for client {}",
        client_id
    );
    assert_eq!(
        app.audit_count_for(&client_id).await,
        1,
        "expected one aud_logs row for client {}",
        client_id
    );
    assert_eq!(
        app.event_count_by_type("platform:iam:client:created").await,
        1,
        "expected one client:created event"
    );
}

// ── UseCase pattern: update_client follow-up ────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn update_client_emits_second_event_and_audit_log() {
    let app = TestApp::setup().await;
    let token = app.anchor_token();

    let create = app
        .post(
            "/api/clients",
            &token,
            json!({ "identifier": "uow-test-2", "name": "Original" }),
        )
        .await;
    let body = assert_status(create, StatusCode::CREATED).await;
    let client_id = body.get("id").and_then(|v| v.as_str()).unwrap().to_string();

    let update = app
        .put(
            &format!("/api/clients/{}", client_id),
            &token,
            json!({ "name": "Renamed" }),
        )
        .await;
    // Update handlers return 204 No Content on success.
    assert!(
        update.status() == StatusCode::NO_CONTENT || update.status() == StatusCode::OK,
        "expected 204/200 on update, got {}",
        update.status()
    );

    assert_eq!(
        app.event_count_for(&client_id).await,
        2,
        "expected two msg_events rows (created + updated)"
    );
    assert_eq!(
        app.audit_count_for(&client_id).await,
        2,
        "expected two aud_logs rows (created + updated)"
    );
    assert_eq!(
        app.event_count_by_type("platform:iam:client:updated").await,
        1,
    );
}

// ── Inline emit_event pattern: anchor domain create ──────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn create_anchor_domain_emits_event_and_audit_log() {
    let app = TestApp::setup().await;
    let token = app.anchor_token();

    let resp = app
        .post(
            "/api/anchor-domains",
            &token,
            json!({ "domain": "uow-test-3.example.com" }),
        )
        .await;
    let body = assert_status(resp, StatusCode::OK).await;
    let anchor_id = body
        .get("id")
        .and_then(|v| v.as_str())
        .expect("response should contain id")
        .to_string();

    assert_eq!(app.event_count_for(&anchor_id).await, 1);
    assert_eq!(app.audit_count_for(&anchor_id).await, 1);
    assert_eq!(
        app.event_count_by_type("platform:iam:anchor-domain:created")
            .await,
        1
    );
}

// ── Inline emit_event pattern: anchor domain delete ──────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn delete_anchor_domain_emits_event_and_audit_log() {
    let app = TestApp::setup().await;
    let token = app.anchor_token();

    let created = app
        .post(
            "/api/anchor-domains",
            &token,
            json!({ "domain": "uow-test-4.example.com" }),
        )
        .await;
    let body = assert_status(created, StatusCode::OK).await;
    let id = body.get("id").and_then(|v| v.as_str()).unwrap().to_string();

    let del = app.delete(&format!("/api/anchor-domains/{}", id), &token).await;
    assert_eq!(
        del.status(),
        StatusCode::NO_CONTENT,
        "expected 204 on anchor-domain delete"
    );

    // created + deleted events/audits.
    assert_eq!(app.event_count_for(&id).await, 2);
    assert_eq!(app.audit_count_for(&id).await, 2);
    assert_eq!(
        app.event_count_by_type("platform:iam:anchor-domain:deleted")
            .await,
        1
    );
}

// ── Inline emit_event pattern: oauth client create + secret rotation ─────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn create_public_oauth_client_emits_event_and_audit_log() {
    let app = TestApp::setup().await;
    let token = app.anchor_token();

    let resp = app
        .post(
            "/api/oauth-clients",
            &token,
            json!({
                "clientName": "Test OAuth App",
                "clientType": "PUBLIC",
                "redirectUris": ["https://example.com/callback"],
            }),
        )
        .await;
    let body = assert_status(resp, StatusCode::CREATED).await;
    let oauth_id = body
        .get("client")
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_str())
        .expect("response should contain client.id")
        .to_string();

    assert_eq!(app.event_count_for(&oauth_id).await, 1);
    assert_eq!(app.audit_count_for(&oauth_id).await, 1);
    assert_eq!(
        app.event_count_by_type("platform:iam:oauth-client:created")
            .await,
        1
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn update_oauth_client_emits_second_event_and_audit_log() {
    let app = TestApp::setup().await;
    let token = app.anchor_token();

    let created = app
        .post(
            "/api/oauth-clients",
            &token,
            json!({
                "clientName": "Original Name",
                "clientType": "PUBLIC",
                "redirectUris": ["https://example.com/callback"],
            }),
        )
        .await;
    let body = assert_status(created, StatusCode::CREATED).await;
    let id = body
        .get("client")
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let update = app
        .put(
            &format!("/api/oauth-clients/{}", id),
            &token,
            json!({ "clientName": "Renamed" }),
        )
        .await;
    assert_eq!(update.status(), StatusCode::NO_CONTENT);

    assert_eq!(app.event_count_for(&id).await, 2);
    assert_eq!(app.audit_count_for(&id).await, 2);
    assert_eq!(
        app.event_count_by_type("platform:iam:oauth-client:updated")
            .await,
        1
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn deactivate_oauth_client_emits_event_and_audit_log() {
    let app = TestApp::setup().await;
    let token = app.anchor_token();

    let created = app
        .post(
            "/api/oauth-clients",
            &token,
            json!({
                "clientName": "To Deactivate",
                "clientType": "PUBLIC",
                "redirectUris": ["https://example.com/callback"],
            }),
        )
        .await;
    let body = assert_status(created, StatusCode::CREATED).await;
    let id = body
        .get("client")
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let deact = app
        .post(
            &format!("/api/oauth-clients/{}/deactivate", id),
            &token,
            json!({}),
        )
        .await;
    assert_eq!(deact.status(), StatusCode::OK);

    assert_eq!(app.event_count_for(&id).await, 2);
    assert_eq!(app.audit_count_for(&id).await, 2);
    assert_eq!(
        app.event_count_by_type("platform:iam:oauth-client:deactivated")
            .await,
        1
    );
}

// ── Inline emit_event pattern: idp role mapping ──────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn create_idp_role_mapping_emits_event_and_audit_log() {
    let app = TestApp::setup().await;
    let token = app.anchor_token();

    let resp = app
        .post(
            "/api/idp-role-mappings",
            &token,
            json!({
                "idpType": "azure-ad",
                "idpRoleName": "fc-admins",
                "platformRoleName": "admin",
            }),
        )
        .await;
    let body = assert_status(resp, StatusCode::OK).await;
    let id = body.get("id").and_then(|v| v.as_str()).unwrap().to_string();

    assert_eq!(app.event_count_for(&id).await, 1);
    assert_eq!(app.audit_count_for(&id).await, 1);
    assert_eq!(
        app.event_count_by_type("platform:iam:idp-role-mapping:created")
            .await,
        1
    );
}
