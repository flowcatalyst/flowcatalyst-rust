//! Go's F17 (`c74650d5`, docs/platform-bug-fixes.md in flowcatalyst-go): a
//! code-first connection sync (`POST /api/applications/{appCode}/connections/sync`)
//! stored the application's PRINCIPAL id (`prn_...`) as the connection's
//! `service_account_id`, but the delivery signer looks up the service
//! account's own id (`sac_...`), so every code-first connection delivered
//! unsigned. The sync now resolves the principal to the account behind it,
//! refuses a principal with none (400 `APPLICATION_SERVICE_ACCOUNT_REQUIRED`,
//! nothing persisted), and heals a stale row on the next sync; `PUT
//! /api/connections/{id}` accepts an optional `serviceAccountId`, which must
//! name an existing account. Requires Docker (or `FC_TEST_DATABASE_URL`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::support::{self, assert_status, read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};

struct Fixture {
    app: TestApp,
    admin: String,
    /// The account's own id (`sac_...`).
    sac: String,
    /// Its SERVICE principal's id (`prn_...`): what the application holds.
    prn: String,
}

/// An application whose service account is a real one, as provisioning leaves it.
async fn fixture(code: &str) -> Fixture {
    support::set_app_key();
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    let created = assert_status(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": format!("{code}-bot"), "name": "Bot" }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let sac = created["serviceAccount"]["id"]
        .as_str()
        .expect("service account id")
        .to_string();
    let prn = created["principalId"]
        .as_str()
        .expect("principalId")
        .to_string();
    assert!(sac.starts_with("sac_"), "{sac}");
    assert!(prn.starts_with("prn_"), "{prn}");
    let mut application = Application::new(code, code);
    application.service_account_id = Some(prn.clone());
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");
    Fixture {
        app,
        admin,
        sac,
        prn,
    }
}

async fn sync(f: &Fixture, code: &str, connections: Value) -> (StatusCode, Value) {
    read_json(
        f.app
            .post(
                &format!("/api/applications/{code}/connections/sync"),
                &f.admin,
                json!({ "connections": connections }),
            )
            .await,
    )
    .await
}

async fn stored_account(f: &Fixture, connection_code: &str) -> Option<String> {
    sqlx::query_scalar("SELECT service_account_id FROM msg_connections WHERE code = $1")
        .bind(connection_code)
        .fetch_optional(&f.app.pool)
        .await
        .expect("read connection")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn sync_stores_the_service_account_not_the_applications_principal() {
    let f = fixture("f17sync").await;

    let (status, body) = sync(
        &f,
        "f17sync",
        json!([{ "code": "f17-conn", "name": "One" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        stored_account(&f, "f17-conn").await.as_deref(),
        Some(f.sac.as_str()),
        "the connection names the service account, not principal {}",
        f.prn
    );

    // A second sync updates the row and keeps the account.
    let (status, body) = sync(
        &f,
        "f17sync",
        json!([{ "code": "f17-conn", "name": "Renamed" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["updated"], 1, "{body}");
    assert_eq!(
        stored_account(&f, "f17-conn").await.as_deref(),
        Some(f.sac.as_str())
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn sync_heals_a_principal_id_stored_by_an_earlier_build() {
    let f = fixture("f17heal").await;
    let (status, body) = sync(&f, "f17heal", json!([{ "code": "f17-old", "name": "Old" }])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    sqlx::query("UPDATE msg_connections SET service_account_id = $1 WHERE code = 'f17-old'")
        .bind(&f.prn)
        .execute(&f.app.pool)
        .await
        .expect("plant the old value");

    let (status, body) = sync(&f, "f17heal", json!([{ "code": "f17-old", "name": "Old" }])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        stored_account(&f, "f17-old").await.as_deref(),
        Some(f.sac.as_str()),
        "the next sync re-points the stale row"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn sync_refuses_a_principal_with_no_service_account_and_persists_nothing() {
    support::set_app_key();
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    // A principal that is not a service account's: a bare user row.
    let bare = Principal::new_user("bare@f17.test", UserScope::Anchor);
    app.repos
        .principal_repo
        .insert(&bare)
        .await
        .expect("insert principal");
    let mut application = Application::new("f17bare", "f17bare");
    application.service_account_id = Some(bare.id.to_string());
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");

    let (status, body) = read_json(
        app.post(
            "/api/applications/f17bare/connections/sync",
            &admin,
            json!({ "connections": [
                { "code": "f17-a", "name": "A" },
                { "code": "f17-b", "name": "B" }
            ] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["error"], "APPLICATION_SERVICE_ACCOUNT_REQUIRED",
        "{body}"
    );
    let rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM msg_connections WHERE code LIKE 'f17-%'")
            .fetch_one(&app.pool)
            .await
            .expect("count");
    assert_eq!(rows, 0, "nothing persisted");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn put_connection_sets_the_service_account_only_to_an_existing_one() {
    let f = fixture("f17put").await;
    let (status, body) = sync(&f, "f17put", json!([{ "code": "f17-put", "name": "P" }])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id: String = sqlx::query_scalar("SELECT id FROM msg_connections WHERE code = 'f17-put'")
        .fetch_one(&f.app.pool)
        .await
        .expect("id");

    // A second account to move the connection to.
    let other = assert_status(
        f.app
            .post(
                "/api/service-accounts",
                &f.admin,
                json!({ "code": "f17put-other", "name": "Other" }),
            )
            .await,
        StatusCode::CREATED,
    )
    .await["serviceAccount"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    let put = |body: Value| {
        let (f, id) = (&f, id.clone());
        async move {
            read_json(
                f.app
                    .put(&format!("/api/connections/{id}"), &f.admin, body)
                    .await,
            )
            .await
        }
    };

    // Omitted: left alone.
    let (status, body) = put(json!({ "name": "P2" })).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(
        stored_account(&f, "f17-put").await.as_deref(),
        Some(f.sac.as_str())
    );

    // An existing account: set.
    let (status, body) = put(json!({ "name": "P2", "serviceAccountId": other })).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(
        stored_account(&f, "f17-put").await.as_deref(),
        Some(other.as_str())
    );

    // The application's principal id is not a service account.
    let (status, body) = put(json!({ "name": "P2", "serviceAccountId": f.prn })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "SERVICE_ACCOUNT_NOT_FOUND", "{body}");

    // Blank is refused.
    let (status, body) = put(json!({ "name": "P2", "serviceAccountId": "  " })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "SERVICE_ACCOUNT_REQUIRED", "{body}");
    assert_eq!(
        stored_account(&f, "f17-put").await.as_deref(),
        Some(other.as_str()),
        "a refusal changes nothing"
    );
}
