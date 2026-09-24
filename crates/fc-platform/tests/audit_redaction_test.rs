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
    let body = assert_status(resp, StatusCode::OK).await;
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
    assert_status(resp, StatusCode::CREATED).await;
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
    assert_status(resp, StatusCode::CREATED).await;

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
