//! Audit rows never store passwords or secrets, through the real platform
//! (owner spec `docs/spec/audit-redaction.md` in the Java repo, test 2).
//!
//! Every unit-of-work commit redacts the command before writing
//! `aud_logs.operation_json`; these drive real HTTP writes and read the
//! rows back.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::json;

use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform::shared::encryption_service::EncryptionService;
use support::{assert_status, TestApp};

/// A platform whose routes can encrypt: service-account credentials and
/// SECRET config values are sealed at rest, and the routes read the key
/// from the environment when they are built.
async fn setup() -> TestApp {
    std::env::set_var("FLOWCATALYST_APP_KEY", EncryptionService::generate_key());
    TestApp::setup().await
}

/// An anchor admin that exists as a principal row, so application-scoped
/// routes (platform config) resolve its all-applications access.
async fn stored_admin_token(app: &TestApp) -> String {
    app.anchor_admin_token().await; // seeds the admin role
    let mut principal = Principal::new_user("audit-admin@flowcatalyst.test", UserScope::Anchor);
    principal.roles = vec![RoleAssignment::new("platform:test-admin")];
    app.repos
        .principal_repo
        .insert(&principal)
        .await
        .expect("insert principal");
    app.auth_service
        .generate_access_token(&principal)
        .expect("token")
}

/// Every `aud_logs.operation_json` in the database, as text.
async fn all_audit_json(app: &TestApp) -> Vec<(String, String)> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT operation, COALESCE(operation_json::text, '') FROM aud_logs ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .expect("read aud_logs")
}

#[track_caller]
fn assert_absent(rows: &[(String, String)], secret: &str) {
    for (operation, json) in rows {
        assert!(
            !json.contains(secret),
            "{operation} audit row holds a secret: {json}"
        );
    }
}

/// Creating a service account hands back a webhook auth token, an HMAC
/// signing secret and an OAuth client secret; none of them is audited.
#[tokio::test]
#[ignore = "requires Docker"]
async fn service_account_credentials_never_reach_the_audit_log() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;

    let resp = app
        .post(
            "/api/service-accounts",
            &token,
            json!({ "code": "audit-bot", "name": "Audit bot" }),
        )
        .await;
    // Service-account create answers 201, as Go does.
    let body = assert_status(resp, StatusCode::CREATED).await;
    let secrets = [
        body["webhook"]["authToken"].as_str().expect("auth token"),
        body["webhook"]["signingSecret"]
            .as_str()
            .expect("signing secret"),
        body["oauth"]["clientSecret"]
            .as_str()
            .expect("client secret"),
    ];

    let rows = all_audit_json(&app).await;
    assert!(
        rows.iter()
            .any(|(op, _)| op == "CreateServiceAccountCommand"),
        "the create was audited: {rows:?}"
    );
    for secret in secrets {
        assert!(!secret.is_empty());
        assert_absent(&rows, secret);
    }
}

/// A SECRET platform-config value is masked in the audit row — whether the
/// type is sent or omitted on an update — while a PLAIN value is recorded
/// as sent.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_secret_config_value_never_reaches_the_audit_log() {
    let app = setup().await;
    let token = stored_admin_token(&app).await;
    app.repos
        .application_repo
        .insert(&Application::new("mailer", "Mailer"))
        .await
        .expect("insert application");

    const SECRET: &str = "smtp-plaintext-password-1";
    const SECRET_UPDATE: &str = "smtp-plaintext-password-2";
    const PLAIN: &str = "smtp.example.com";

    let resp = app
        .put(
            "/api/config/mailer/email/smtpPassword",
            &token,
            json!({ "value": SECRET, "valueType": "SECRET" }),
        )
        .await;
    assert_status(resp, StatusCode::OK).await;
    // An update that omits the type keeps the stored SECRET type.
    let resp = app
        .put(
            "/api/config/mailer/email/smtpPassword",
            &token,
            json!({ "value": SECRET_UPDATE }),
        )
        .await;
    assert_status(resp, StatusCode::OK).await;
    let resp = app
        .put(
            "/api/config/mailer/email/smtpHost",
            &token,
            json!({ "value": PLAIN, "valueType": "PLAIN" }),
        )
        .await;
    assert_status(resp, StatusCode::OK).await;

    let rows = all_audit_json(&app).await;
    let set_rows: Vec<&(String, String)> = rows
        .iter()
        .filter(|(op, _)| op == "SetPlatformConfigPropertyCommand")
        .collect();
    assert_eq!(set_rows.len(), 3, "{rows:?}");
    assert_absent(&rows, SECRET);
    assert_absent(&rows, SECRET_UPDATE);
    assert!(
        set_rows.iter().any(|(_, json)| json.contains(PLAIN)),
        "a PLAIN value is still recorded: {set_rows:?}"
    );
}

/// The ingest backstop: an SDK-posted audit item is redacted by the name
/// rule before it is stored, whether `operationData` arrives as an object
/// or — as the TypeScript, Laravel and fc-sdk DTOs send it — as a
/// JSON-encoded string.
#[tokio::test]
#[ignore = "requires Docker"]
async fn ingested_audit_logs_are_redacted_before_they_are_stored() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;

    let resp = app
        .post(
            "/api/audit-logs/batch",
            &token,
            json!({ "items": [
                {
                    "entityType": "Order",
                    "entityId": "ord_1",
                    "operation": "CREATE",
                    "principalId": "prn_sdk",
                    "operationData": { "password": "x", "name": "kept" },
                },
                {
                    "entityType": "Order",
                    "entityId": "ord_2",
                    "operation": "CREATE",
                    "principalId": "prn_sdk",
                    "operationData": "{\"apiKey\":\"sk_live_9\",\"name\":\"kept\"}",
                },
            ]}),
        )
        .await;
    let body = assert_status(resp, StatusCode::OK).await;
    assert_eq!(body["results"][0]["status"], "SUCCESS", "{body}");
    assert_eq!(body["results"][1]["status"], "SUCCESS", "{body}");

    let stored = |entity_id: &'static str| {
        let pool = app.pool.clone();
        async move {
            sqlx::query_as::<_, (serde_json::Value,)>(
                "SELECT operation_json FROM aud_logs WHERE entity_id = $1",
            )
            .bind(entity_id)
            .fetch_all(&pool)
            .await
            .expect("read aud_logs")
        }
    };

    let rows = stored("ord_1").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, json!({ "password": "***", "name": "kept" }));

    let rows = stored("ord_2").await;
    assert_eq!(rows.len(), 1);
    let inner: serde_json::Value =
        serde_json::from_str(rows[0].0.as_str().expect("stored as a JSON string"))
            .expect("inner JSON");
    assert_eq!(inner, json!({ "apiKey": "***", "name": "kept" }));
}

// ── Temporary: the dashboard's redact-existing sweep ────────────────────────
//
// Owner spec "Temporary: redact existing rows from the dashboard". Remove
// with POST /bff/audit-logs/redact-existing.

async fn seed_audit_row(app: &TestApp, id: &str, operation: &str, json: serde_json::Value) {
    sqlx::query(
        "INSERT INTO aud_logs (id, entity_type, entity_id, operation, operation_json) \
         VALUES ($1, 'Seeded', $1, $2, $3)",
    )
    .bind(id)
    .bind(operation)
    .bind(json)
    .execute(&app.pool)
    .await
    .expect("seed aud_logs");
}

/// `(operation_json::text, xmin)`: the stored document, and the row version
/// (xmin changes on any UPDATE, even one writing the same value).
async fn stored_row(app: &TestApp, id: &str) -> (String, String) {
    sqlx::query_as::<_, (String, String)>(
        "SELECT operation_json::text, xmin::text FROM aud_logs WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("read seeded row")
}

/// S11 (Java b4a15fd8): a row stored before source-side redaction — every
/// Go-era row — is served redacted by the audit-log API whether or not the
/// sweep has run, and the stored row is left as it was.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_legacy_row_is_served_redacted_without_the_sweep() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    seed_audit_row(
        &app,
        "seed_legacy",
        "CreateOAuthClientCommand",
        json!({"note": "legacy", "clientSecret": "hunter2"}),
    )
    .await;

    let body = assert_status(
        app.get("/api/audit-logs/seed_legacy", &token).await,
        StatusCode::OK,
    )
    .await;
    let served = body["operationJson"].as_str().expect("operationJson");
    assert!(!served.contains("hunter2"), "{served}");
    assert!(served.contains("\"clientSecret\":\"***\""), "{served}");
    assert!(served.contains("\"note\":\"legacy\""), "{served}");

    let (stored, _) = stored_row(&app, "seed_legacy").await;
    assert!(stored.contains("hunter2"), "a read never rewrites the row");
}

async fn redact_existing(app: &TestApp, token: &str) -> (StatusCode, serde_json::Value) {
    support::read_json(
        app.post("/bff/audit-logs/redact-existing", token, json!({}))
            .await,
    )
    .await
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_sweep_redacts_existing_rows_once_and_touches_nothing_else() {
    let app = setup().await;
    let token = stored_admin_token(&app).await;
    sqlx::query("DELETE FROM aud_logs")
        .execute(&app.pool)
        .await
        .unwrap();

    // Rewritten.
    seed_audit_row(
        &app,
        "seed_webhook",
        "CreateServiceAccountCommand",
        json!({"code": "sa-1", "webhookCredentials": {"authType": "HMAC_SIGNATURE", "token": "tok-1", "signingSecret": "whsec-1"}}),
    )
    .await;
    seed_audit_row(
        &app,
        "seed_secret_cfg",
        "SetPlatformConfigPropertyCommand",
        json!({"property": "smtpPassword", "value": "cfg-secret-1", "valueType": "SECRET"}),
    )
    .await;
    seed_audit_row(
        &app,
        "seed_go_cfg",
        "SetPropertyCommand",
        json!({"property": "apiKey", "value": "cfg-secret-2"}),
    )
    .await;
    seed_audit_row(
        &app,
        "seed_user",
        "CreateUserCommand",
        json!({"email": "a@b.c", "password": "hunter2"}),
    )
    .await;
    seed_audit_row(
        &app,
        "seed_sdk_string",
        "CREATE",
        json!("{\"email\":\"a@b.c\",\"newPassword\":\"sdk-secret\"}"),
    )
    .await;
    // Candidates the exact rule keeps.
    let plain = json!({"property": "smtpHost", "value": "smtp.example.com", "valueType": "PLAIN"});
    seed_audit_row(
        &app,
        "seed_plain_cfg",
        "SetPlatformConfigPropertyCommand",
        plain,
    )
    .await;
    seed_audit_row(
        &app,
        "seed_lookalike",
        "IssueTokenCommand",
        json!({"tokenType": "Bearer", "secretKeys": ["A", "B"], "enforcePasswordComplexity": true}),
    )
    .await;
    // Not a candidate at all.
    seed_audit_row(
        &app,
        "seed_unrelated",
        "UpdateClientCommand",
        json!({"name": "Acme", "nested": {"n": 1.5, "list": [1, "two"]}}),
    )
    .await;
    // More than one batch of 500 secret rows.
    sqlx::query(
        "INSERT INTO aud_logs (id, entity_type, entity_id, operation, operation_json) \
         SELECT 'bulk_' || lpad(i::text, 5, '0'), 'Seeded', 'bulk', 'ResetPasswordCommand', \
                jsonb_build_object('principalId', 'prn_' || i, 'newPassword', 'bulk-secret-' || i) \
         FROM generate_series(1, 1100) AS i",
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let untouched = ["seed_plain_cfg", "seed_lookalike", "seed_unrelated"];
    let mut before = Vec::new();
    for id in untouched {
        before.push(stored_row(&app, id).await);
    }

    let (status, body) = redact_existing(&app, &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Candidates: 5 rewritten + plain config + look-alike + 1100 bulk.
    assert_eq!(body, json!({"scanned": 1107, "redacted": 1105}));

    for (id, row) in untouched.iter().zip(&before) {
        assert_eq!(&stored_row(&app, id).await, row, "{id} was rewritten");
    }
    let rows = all_audit_json(&app).await;
    for secret in [
        "tok-1",
        "whsec-1",
        "cfg-secret-1",
        "cfg-secret-2",
        "hunter2",
        "sdk-secret",
        "bulk-secret-",
    ] {
        assert_absent(&rows, secret);
    }
    let webhook: serde_json::Value =
        serde_json::from_str(&stored_row(&app, "seed_webhook").await.0).unwrap();
    assert_eq!(
        webhook,
        json!({"code": "sa-1", "webhookCredentials": {"authType": "HMAC_SIGNATURE", "token": "***", "signingSecret": "***"}})
    );
    let go_cfg: serde_json::Value =
        serde_json::from_str(&stored_row(&app, "seed_go_cfg").await.0).unwrap();
    assert_eq!(go_cfg, json!({"property": "apiKey", "value": "***"}));

    // Idempotent: nothing left to rewrite.
    let (status, body) = redact_existing(&app, &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({"scanned": 1107, "redacted": 0}));

    // Each run audits itself once, through the unit of work.
    let runs = sqlx::query_as::<_, (serde_json::Value,)>(
        "SELECT operation_json FROM aud_logs WHERE operation = 'RedactExistingAuditLogs' ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        runs.into_iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![
            json!({"scanned": 1107, "redacted": 1105}),
            json!({"scanned": 1107, "redacted": 0})
        ]
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_sweep_is_anchor_only_and_needs_audit_log_read() {
    let app = setup().await;
    seed_audit_row(
        &app,
        "seed_user",
        "CreateUserCommand",
        json!({"password": "hunter2"}),
    )
    .await;

    // A partner holding every permission, including audit-log read.
    app.anchor_admin_token().await; // seeds the admin role
    let mut partner = Principal::new_user("partner-admin@flowcatalyst.test", UserScope::Partner);
    partner.roles = vec![RoleAssignment::new("platform:test-admin")];
    let partner_admin = app.auth_service.generate_access_token(&partner).unwrap();

    // Anchor without the audit-log read permission; the partner admin;
    // a plain partner; a client user.
    for token in [
        app.anchor_token(),
        partner_admin,
        app.partner_token(),
        app.client_user_token("clt_1"),
    ] {
        let (status, _) = redact_existing(&app, &token).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert!(stored_row(&app, "seed_user").await.0.contains("hunter2"));
}
