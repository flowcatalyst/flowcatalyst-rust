//! `/api/principals` against Go's behaviour: a no-op update, application
//! access read and written in batches, client grants reported with their own
//! dates, users created with every application, and provision-service-account
//! recorded as Go's one command. Requires Docker.

use crate::support;
use fc_platform_core::shared::id::ClientId;

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
    client.id.to_string()
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

async fn count(app: &TestApp, sql: &str) -> i64 {
    let row: (i64,) = sqlx::query_as(sql)
        .fetch_one(&app.pool)
        .await
        .expect("count");
    row.0
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
    let mut expected = [(a.id.clone(), "app-a"), (c.id.clone(), "app-c")];
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
    let partner = Principal::new_user("pat@partner.test", UserScope::Partner)
        .with_client_id(ClientId::parse(&home).unwrap());
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

/// Go records provision-service-account as its one
/// `ProvisionServiceAccountCommand` on each of the three audit rows it
/// writes (service account, application, OAuth client), one event each,
/// all in one transaction.
#[tokio::test]
#[ignore = "requires Docker"]
async fn provision_service_account_records_one_command() {
    support::set_app_key();
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let application = create_app(&app, "mailer").await;
    let (audits, events) = (
        count(&app, "SELECT COUNT(*) FROM aud_logs").await,
        count(&app, "SELECT COUNT(*) FROM msg_events").await,
    );

    let (status, body) = read_json(
        app.post(
            &format!(
                "/api/applications/{}/provision-service-account",
                application.id
            ),
            &token,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM msg_events").await,
        events + 3
    );
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM aud_logs").await,
        audits + 3
    );
    let rows: Vec<(String, String, Value)> = sqlx::query_as(
        "SELECT entity_type, operation, operation_json FROM aud_logs ORDER BY id DESC LIMIT 3",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    let mut entity_types: Vec<&str> = rows.iter().map(|(t, _, _)| t.as_str()).collect();
    entity_types.sort();
    assert_eq!(entity_types.len(), 3);
    assert!(entity_types.windows(2).all(|w| w[0] != w[1]), "{rows:?}");
    for (_, operation, json) in &rows {
        assert_eq!(operation, "ProvisionServiceAccountCommand");
        assert_eq!(json, &json!({"applicationId": application.id}));
    }

    // A refused provision (already provisioned) writes nothing.
    let (status, _) = read_json(
        app.post(
            &format!(
                "/api/applications/{}/provision-service-account",
                application.id
            ),
            &token,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM aud_logs").await,
        audits + 3
    );
}

/// Every event a service account's lifecycle writes names the account's own
/// id (`sac_…`), as Go's `subjectFor(sa.ID)` does, never its SERVICE
/// principal's (`prn_…`): subject, message group, `serviceAccountId` and the
/// audit row's `entity_id`. Provisioning names the account too, while the
/// application and the OAuth client still point at the principal.
#[tokio::test]
#[ignore = "requires Docker"]
async fn service_account_events_carry_the_account_id() {
    support::set_app_key();
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;

    let (status, body) = read_json(
        app.post(
            "/api/service-accounts",
            &token,
            json!({"code": "orders-bot", "name": "Orders bot"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let sac = body["serviceAccount"]["id"].as_str().unwrap().to_string();
    let prn = body["principalId"].as_str().unwrap().to_string();
    assert!(sac.starts_with("sac_"), "{body}");
    assert!(prn.starts_with("prn_"), "{body}");

    let path = format!("/api/service-accounts/{sac}");
    let resp = app
        .put(&path, &token, json!({"name": "Orders bot 2"}))
        .await;
    assert!(resp.status().is_success(), "{}", resp.status());
    let resp = app
        .put(
            &format!("{path}/roles"),
            &token,
            json!({"roles": ["platform:viewer"]}),
        )
        .await;
    assert!(resp.status().is_success(), "{}", resp.status());
    for suffix in [
        "regenerate-auth-token",
        "regenerate-signing-secret",
        "deactivate",
    ] {
        let resp = app
            .post(&format!("{path}/{suffix}"), &token, json!({}))
            .await;
        assert!(resp.status().is_success(), "{suffix}: {}", resp.status());
    }
    let resp = app.delete(&path, &token).await;
    assert!(resp.status().is_success(), "{}", resp.status());

    let rows: Vec<(String, String, String, Value)> = sqlx::query_as(
        "SELECT type, subject, message_group, data FROM msg_events \
         WHERE type LIKE 'platform:iam:serviceaccount:%' ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    let types: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    for kind in [
        "created",
        "updated",
        "roles-assigned",
        "token-regenerated",
        "secret-regenerated",
        "deactivated",
        "deleted",
    ] {
        assert!(
            types.contains(&format!("platform:iam:serviceaccount:{kind}").as_str()),
            "{kind} missing from {types:?}"
        );
    }
    for (event_type, subject, group, data) in &rows {
        assert_eq!(
            subject,
            &format!("platform.serviceaccount.{sac}"),
            "{event_type}"
        );
        assert_eq!(
            group,
            &format!("platform:serviceaccount:{sac}"),
            "{event_type}"
        );
        assert_eq!(data["serviceAccountId"], sac.as_str(), "{event_type}");
    }
    let audit_ids: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT entity_id FROM aud_logs WHERE entity_type = 'Serviceaccount'",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(audit_ids, vec![(sac.clone(),)]);

    // Provisioning: the created and provisioned events name the account; the
    // application and the OAuth client point at the principal.
    let application = create_app(&app, "mailer").await;
    let (status, body) = read_json(
        app.post(
            &format!(
                "/api/applications/{}/provision-service-account",
                application.id
            ),
            &token,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let principal_id = body["serviceAccount"]["principalId"]
        .as_str()
        .unwrap_or_else(|| panic!("{body}"))
        .to_string();
    assert!(principal_id.starts_with("prn_"), "{body}");
    let (account_id,): (String,) =
        sqlx::query_as("SELECT service_account_id FROM iam_principals WHERE id = $1")
            .bind(&principal_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(account_id.starts_with("sac_"));
    let (created,): (Value,) = sqlx::query_as(
        "SELECT data FROM msg_events WHERE type = 'platform:iam:serviceaccount:created' \
         AND data->>'code' = 'app:mailer'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(created["serviceAccountId"], account_id.as_str());
    let (provisioned,): (Value,) = sqlx::query_as(
        "SELECT data FROM msg_events WHERE type LIKE '%:application:service-account-provisioned'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(provisioned["serviceAccountId"], account_id.as_str());
    let stored = app
        .repos
        .application_repo
        .find_by_id(&application.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.service_account_id.as_deref(),
        Some(principal_id.as_str())
    );
    let (oauth_principal,): (Option<String>,) = sqlx::query_as(
        "SELECT service_account_principal_id FROM oauth_clients \
         WHERE service_account_principal_id = $1",
    )
    .bind(&principal_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(oauth_principal.as_deref(), Some(principal_id.as_str()));
}
