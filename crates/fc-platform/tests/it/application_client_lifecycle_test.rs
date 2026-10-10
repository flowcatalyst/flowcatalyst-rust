//! Go's application and client fixes (docs/platform-bug-fixes.md in
//! flowcatalyst-go): deleting an active application is refused (bug 14,
//! 61dbcb9); client "Deactivate" sets INACTIVE instead of deleting the client
//! (bug 15, ff65ec4). Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::json;

use fc_platform::application::entity::Application;
use fc_platform::client::entity::Client;
use support::{assert_status, read_json, TestApp};

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_active_application_is_not_deleted() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let application = Application::new("del-active", "Del Active");
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");
    let path = format!("/api/applications/{}", application.id);

    let (status, body) = read_json(app.delete(&path, &admin).await).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "APPLICATION_ACTIVE", "{body}");
    assert_eq!(body["message"], "Deactivate the application first.");

    assert_status(
        app.post(&format!("{path}/deactivate"), &admin, json!({}))
            .await,
        StatusCode::OK,
    )
    .await;
    let (status, body) = read_json(app.delete(&path, &admin).await).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn deactivating_a_client_keeps_it_inactive() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let client = Client::new("Deactivated Co", "deactivated-co");
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");
    let path = format!("/api/clients/{}", client.id);

    let done = assert_status(
        app.post(
            &format!("{path}/deactivate"),
            &admin,
            json!({ "reason": "Manual deactivation" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(done["message"], "Client deactivated");

    // Kept, INACTIVE, with the reason; reactivated by activate.
    let got = assert_status(app.get(&path, &admin).await, StatusCode::OK).await;
    assert_eq!(got["status"], "INACTIVE", "{got}");
    assert_eq!(got["statusReason"], "Manual deactivation", "{got}");
    assert_eq!(
        app.event_count_by_type("platform:admin:client:deactivated")
            .await,
        1
    );
    assert_eq!(
        app.event_count_by_type("platform:admin:client:deleted")
            .await,
        0
    );
    assert_status(
        app.post(&format!("{path}/activate"), &admin, json!({}))
            .await,
        StatusCode::OK,
    )
    .await;
    let got = assert_status(app.get(&path, &admin).await, StatusCode::OK).await;
    assert_eq!(got["status"], "ACTIVE", "{got}");
}
