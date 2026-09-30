//! Deleting a service account removes its principal's application-access and
//! client-access grants in the same transaction, as Go's
//! `serviceaccount.Repository.Delete` does. Neither grant table has an FK on
//! the principal, so a missed row outlives the principal, and the
//! application delete guard (decision #34) then refuses to delete the
//! application with `APPLICATION_HAS_REFERENCES`. Requires Docker.

use crate::support;

use axum::http::StatusCode;
use serde_json::json;

use fc_platform::application::entity::Application;
use fc_platform::client::entity::Client;
use support::{read_json, TestApp};

async fn grant_counts(app: &TestApp, principal_id: &str) -> (i64, i64) {
    let apps: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM iam_principal_application_access WHERE principal_id = $1",
    )
    .bind(principal_id)
    .fetch_one(&app.pool)
    .await
    .expect("count application access");
    let clients: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM iam_client_access_grants WHERE principal_id = $1")
            .bind(principal_id)
            .fetch_one(&app.pool)
            .await
            .expect("count client grants");
    (apps, clients)
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn deleting_a_service_account_removes_its_grants_and_frees_the_application() {
    support::set_app_key();
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;

    let application = Application::new("sa-delete-grants", "SA delete grants");
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");
    let mut client_ids = Vec::new();
    for identifier in ["sa-delete-grants-a", "sa-delete-grants-b"] {
        let client = Client::new(identifier.to_uppercase(), identifier);
        app.repos
            .client_repo
            .insert(&client)
            .await
            .expect("insert client");
        client_ids.push(client.id.to_string());
    }

    // Provisioning grants the account's principal access to the application.
    let (status, body) = read_json(
        app.post(
            &format!(
                "/api/applications/{}/provision-service-account",
                application.id
            ),
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    let principal_id = body["serviceAccount"]["principalId"]
        .as_str()
        .expect("principalId")
        .to_string();

    // Linking several clients (a PARTNER account) grants the principal
    // access to each; a single link is the principal's home client instead.
    let sa_path = format!("/api/service-accounts/{principal_id}");
    let resp = app
        .put(&sa_path, &admin, json!({ "clientIds": client_ids }))
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let (apps, clients) = grant_counts(&app, &principal_id).await;
    assert!(apps > 0, "provisioning left no application access grant");
    assert!(clients > 0, "linking clients left no client access grant");

    let resp = app.delete(&sa_path, &admin).await;
    assert!(resp.status().is_success(), "{}", resp.status());
    assert_eq!(grant_counts(&app, &principal_id).await, (0, 0));

    let (status, body) = read_json(
        app.delete(&format!("/api/applications/{}", application.id), &admin)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}
