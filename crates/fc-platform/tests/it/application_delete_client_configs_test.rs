//! Only an enabled client config blocks an application delete (owner
//! decision #55, refining the #34 guard): an application enabled and then
//! disabled for a client is deleted, and its disabled config row goes with
//! it in the same transaction; one still enabled is refused with
//! `APPLICATION_HAS_REFERENCES`, counting the enabled configs only.
//! Requires Docker.

use crate::support;

use axum::http::StatusCode;
use serde_json::json;

use fc_platform::application::entity::Application;
use fc_platform::client::entity::Client;
use support::{read_json, TestApp};

async fn config_rows(app: &TestApp, application_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM app_client_configs WHERE application_id = $1")
        .bind(application_id)
        .fetch_one(&app.pool)
        .await
        .expect("count client configs")
}

async fn application(app: &TestApp, code: &str) -> Application {
    let application = Application::new(code, code);
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");
    application
}

async fn client(app: &TestApp, identifier: &str) -> Client {
    let client = Client::new(identifier.to_uppercase(), identifier);
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");
    client
}

async fn toggle(
    app: &TestApp,
    admin: &str,
    application: &Application,
    client: &Client,
    action: &str,
) {
    let resp = app
        .post(
            &format!(
                "/api/applications/{}/clients/{}/{action}",
                application.id, client.id
            ),
            admin,
            json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "{action}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_disabled_client_config_no_longer_blocks_the_delete_and_goes_with_it() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let client_a = client(&app, "app-del-cfg-a").await;
    let client_b = client(&app, "app-del-cfg-b").await;

    // Enabled then disabled for one client, still enabled for none: deleted,
    // and no config row is left behind.
    let freed = application(&app, "app-del-cfg-freed").await;
    toggle(&app, &admin, &freed, &client_a, "enable").await;
    toggle(&app, &admin, &freed, &client_a, "disable").await;
    assert_eq!(
        config_rows(&app, &freed.id).await,
        1,
        "disable keeps the row"
    );
    let (status, body) = read_json(
        app.delete(&format!("/api/applications/{}", freed.id), &admin)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(config_rows(&app, &freed.id).await, 0);
    assert!(app
        .repos
        .application_repo
        .find_by_id(&freed.id)
        .await
        .unwrap()
        .is_none());

    // Still enabled for one client (and disabled for another): refused,
    // counting the enabled config only, and nothing is deleted.
    let held = application(&app, "app-del-cfg-held").await;
    toggle(&app, &admin, &held, &client_a, "enable").await;
    toggle(&app, &admin, &held, &client_b, "enable").await;
    toggle(&app, &admin, &held, &client_b, "disable").await;
    let (status, body) = read_json(
        app.delete(&format!("/api/applications/{}", held.id), &admin)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "APPLICATION_HAS_REFERENCES", "{body}");
    assert_eq!(
        body["message"],
        "Cannot delete application 'app-del-cfg-held' — 1 client configs still reference it. \
         Remove those before deleting.",
        "{body}"
    );
    assert_eq!(
        config_rows(&app, &held.id).await,
        2,
        "a refused delete removes nothing"
    );
}
