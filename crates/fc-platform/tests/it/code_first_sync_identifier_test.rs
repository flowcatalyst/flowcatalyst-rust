//! Go's F5 (`3f0dba4`, docs/platform-bug-fixes.md in flowcatalyst-go): the
//! code-first connection and subscription syncs
//! (`/api/applications/{appCode}/connections|subscriptions/sync`) store the
//! sync client's identifier on the rows they create, as create does since
//! Go `60dc063`; a client-scoped code-first row showed as "Anchor-level".
//! Requires Docker (or `FC_TEST_PG_BIN`).

use axum::http::StatusCode;
use serde_json::json;

use crate::support::{self, assert_status, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::client::entity::Client;

#[tokio::test]
#[ignore = "requires Docker"]
async fn code_first_syncs_store_the_client_identifier() {
    support::set_app_key();
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;

    let acme = Client::new("ACME", "acme");
    app.repos
        .client_repo
        .insert(&acme)
        .await
        .expect("insert client");

    // An application with its service account, as the syncs need.
    let created = assert_status(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": "cfsync-bot", "name": "Bot" }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let mut application = Application::new("cfsync", "cfsync");
    application.service_account_id = Some(
        created["principalId"]
            .as_str()
            .expect("principalId")
            .to_string(),
    );
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");

    assert_status(
        app.post(
            "/api/applications/cfsync/connections/sync",
            &admin,
            json!({ "clientId": acme.id.as_str(),
                    "connections": [{ "code": "acme-conn", "name": "Acme" }] }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let (connection_id, identifier): (String, Option<String>) = sqlx::query_as(
        "SELECT id, client_identifier FROM msg_connections WHERE code = 'acme-conn'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("synced connection");
    assert_eq!(identifier.as_deref(), Some("acme"), "connection");

    assert_status(
        app.post(
            "/api/applications/cfsync/subscriptions/sync",
            &admin,
            json!({ "clientId": acme.id.as_str(), "subscriptions": [{
                "code": "acme-sub",
                "name": "Acme",
                "target": "https://receiver.example.test/hook",
                "connectionId": connection_id,
                "eventTypes": [{ "eventTypeCode": "cfsync:orders:order:created" }]
            }]}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let identifier: Option<String> = sqlx::query_scalar(
        "SELECT client_identifier FROM msg_subscriptions WHERE code = 'acme-sub'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("synced subscription");
    assert_eq!(identifier.as_deref(), Some("acme"), "subscription");
}
