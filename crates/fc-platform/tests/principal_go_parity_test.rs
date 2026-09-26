//! `/api/principals` against Go's behaviour: a no-op update, application
//! access read and written in batches, client grants reported with their own
//! dates, and users created with every application. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::application::entity::Application;
use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use support::{read_json, TestApp};

async fn create_client(app: &TestApp, identifier: &str) -> String {
    let client = Client::new(identifier.to_uppercase(), identifier);
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");
    client.id
}

async fn create_app(app: &TestApp, code: &str) -> Application {
    let application = Application::new(code, code.to_uppercase());
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");
    application
}

async fn create_user(app: &TestApp, token: &str, email: &str, client_id: &str) -> Value {
    let (status, body) = read_json(
        app.post(
            "/api/principals/users",
            token,
            json!({"email": email, "name": "Ada Lovelace", "clientId": client_id,
                   "sendInvitation": false}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// Go `UpdateUser` (principal/operations/update.go) saves and records
/// `UserUpdated` whatever was sent: the SPA sends the unchanged name on every
/// save before a tier or client change, so refusing it ("No changes
/// detected") broke those saves. A blank name and a different email are
/// refused as Go refuses them.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_update_that_changes_nothing_saves_and_records_the_event() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let client_id = create_client(&app, "acme").await;
    let user = create_user(&app, &token, "ada@acme.test", &client_id).await;
    let id = user["id"].as_str().unwrap();
    let path = format!("/api/principals/{id}");
    let updated = || app.event_count_by_type("platform:iam:user:updated");
    assert_eq!(updated().await, 0);

    // The name the user already has.
    let (status, body) = read_json(
        app.put(&path, &token, json!({"name": "Ada Lovelace"}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "Ada Lovelace");
    assert_eq!(updated().await, 1);

    // Nothing at all, and the stored email asserted back.
    let (status, body) = read_json(app.put(&path, &token, json!({})).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = read_json(
        app.put(
            &path,
            &token,
            json!({"name": "Ada", "email": " ADA@acme.test "}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "Ada");
    assert_eq!(updated().await, 3);

    let (status, body) = read_json(app.put(&path, &token, json!({"name": "  "})).await).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "NAME_REQUIRED", "{body}");
    let (status, body) = read_json(
        app.put(&path, &token, json!({"email": "someone@else.test"}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "EMAIL_IMMUTABLE", "{body}");
    assert_eq!(updated().await, 3);
}

/// The application-access read and write answer each granted application
/// in the order given, from one batch read; an id with no application is
/// skipped on the read (Go `resolveApplications`) and refused on the write.
#[tokio::test]
#[ignore = "requires Docker"]
async fn application_access_is_read_and_written_as_a_set() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let client_id = create_client(&app, "acme").await;
    let user = create_user(&app, &token, "ada@acme.test", &client_id).await;
    let id = user["id"].as_str().unwrap();
    let path = format!("/api/principals/{id}/application-access");
    let (a, b, c) = (
        create_app(&app, "app-a").await,
        create_app(&app, "app-b").await,
        create_app(&app, "app-c").await,
    );

    let (status, body) = read_json(
        app.put(
            &path,
            &token,
            json!({"applicationIds": [c.id, a.id, b.id], "allApplications": false}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let codes = |body: &Value| -> Vec<String> {
        body["applications"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["applicationCode"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(codes(&body), ["app-c", "app-a", "app-b"]);
    assert_eq!(body["added"], 3);
    assert_eq!(body["allApplications"], false);

    // The read: ordered by application id (Go's hydration order), with a
    // dropped application skipped.
    sqlx::query("DELETE FROM app_applications WHERE id = $1")
        .bind(&b.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, body) = read_json(app.get(&path, &token).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut expected = vec![(a.id.clone(), "app-a"), (c.id.clone(), "app-c")];
    expected.sort();
    assert_eq!(
        codes(&body),
        expected
            .iter()
            .map(|(_, c)| c.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(body["total"], 2);

    let (status, body) = read_json(
        app.put(
            &path,
            &token,
            json!({"applicationIds": [a.id, "app_missing"]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// Each client grant answers with its own id and grant date (Go
/// `clientAccessGrantFromEntity`), and a later save of the principal keeps
/// the grant rows it still holds rather than rewriting them.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_client_grant_reports_its_own_id_and_date() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let home = create_client(&app, "home").await;
    let other = create_client(&app, "other").await;
    let partner = Principal::new_user("pat@partner.test", UserScope::Partner).with_client_id(&home);
    app.repos
        .principal_repo
        .insert(&partner)
        .await
        .expect("insert partner");
    let grants_path = format!("/api/principals/{}/client-access", partner.id);

    let (status, granted) = read_json(
        app.post(&grants_path, &token, json!({"clientId": other}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{granted}");
    let grant_id = granted["id"].as_str().unwrap().to_string();
    assert!(grant_id.starts_with("gnt_"), "{granted}");
    assert_eq!(granted["clientId"], other.as_str());

    // Date the grant well away from the principal's creation.
    sqlx::query("UPDATE iam_client_access_grants SET granted_at = '2024-02-03T04:05:06.123456Z' WHERE id = $1")
        .bind(&grant_id)
        .execute(&app.pool)
        .await
        .unwrap();

    // A save of the principal (a no-op name update) leaves the grant alone.
    let (status, body) = read_json(
        app.put(
            &format!("/api/principals/{}", partner.id),
            &token,
            json!({"name": "Pat"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = read_json(app.get(&grants_path, &token).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({"grants": [{
            "id": grant_id,
            "clientId": other,
            "grantedAt": "2024-02-03T04:05:06.123456Z"
        }]})
    );
}

/// Go's `principal.NewUser` gives a user every application
/// (`AllApplications: true`), whether an administrator creates it or a sync
/// does; only portal identities and service accounts start without.
#[tokio::test]
#[ignore = "requires Docker"]
async fn new_users_have_every_application() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let client_id = create_client(&app, "acme").await;

    let created = create_user(&app, &token, "ada@acme.test", &client_id).await;
    let (status, body) = read_json(
        app.post(
            "/api/principals/sync",
            &token,
            json!({"principals": [{"email": "bob@acme.test", "name": "Bob", "roles": []}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let synced = app
        .repos
        .principal_repo
        .find_by_email("bob@acme.test")
        .await
        .unwrap()
        .expect("synced user");

    for id in [created["id"].as_str().unwrap(), synced.id.as_str()] {
        let (status, body) = read_json(
            app.get(&format!("/api/principals/{id}/application-access"), &token)
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["allApplications"], true, "{id}: {body}");
        assert_eq!(body["total"], 0);
    }
}
