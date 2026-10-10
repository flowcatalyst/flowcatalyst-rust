//! Go's dispatch pool, connection and subscription bug fixes
//! (docs/platform-bug-fixes.md in flowcatalyst-go): create stores the
//! client identifier (60dc063); a pool edit can clear the rate limit and
//! description (d1158fa); subscription create accepts `clientScoped` and
//! `sequence` (bug 9, eb537a7) and create/update store the dispatch pool code
//! (bug 11, e6d1dc0); a blank `connectionId` on update leaves the connection
//! alone (bug 12, c3768c7). Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::client::entity::Client;
use fc_platform::shared::tsid;
use support::{assert_status, read_json, TestApp};

async fn create_client(app: &TestApp, identifier: &str) -> String {
    let client = Client::new(identifier.to_uppercase(), identifier);
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");
    client.id.to_string()
}

/// A CLIENT-tier service account of `client` and its SERVICE principal, as
/// Go's provisioning leaves them. Returns the account id.
async fn seed_client_account(app: &TestApp, code: &str, client: &str) -> String {
    let sac = tsid::generate(fc_platform::EntityType::ServiceAccount);
    let prn = tsid::generate(fc_platform::EntityType::Principal);
    sqlx::query(
        "INSERT INTO iam_service_accounts (id, code, name, active) VALUES ($1, $2, $2, true)",
    )
    .bind(&sac)
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("insert account");
    sqlx::query(
        "INSERT INTO iam_principals (id, type, scope, client_id, name, active, service_account_id) \
         VALUES ($1, 'SERVICE', 'CLIENT', $2, $3, true, $4)",
    )
    .bind(&prn)
    .bind(client)
    .bind(code)
    .bind(&sac)
    .execute(&app.pool)
    .await
    .expect("insert principal");
    sac
}

async fn created_id(app: &TestApp, path: &str, token: &str, body: Value) -> String {
    let created = assert_status(app.post(path, token, body).await, StatusCode::CREATED).await;
    created["id"].as_str().expect("id").to_string()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn pools_connections_and_subscriptions_store_what_they_are_given() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let acme = create_client(&app, "acme").await;

    // A client pool stores its client's identifier.
    let pool = created_id(
        &app,
        "/api/dispatch-pools",
        &admin,
        json!({ "code": "acme-pool", "name": "Acme Pool", "clientId": acme }),
    )
    .await;
    let pool_path = format!("/api/dispatch-pools/{pool}");
    let got = assert_status(app.get(&pool_path, &admin).await, StatusCode::OK).await;
    assert_eq!(got["clientIdentifier"], "acme", "{got}");

    // An edit can clear the rate limit (0) and the description ("").
    assert_status(
        app.put(
            &pool_path,
            &admin,
            json!({ "description": "Busy", "rateLimit": 60 }),
        )
        .await,
        StatusCode::NO_CONTENT,
    )
    .await;
    let got = assert_status(app.get(&pool_path, &admin).await, StatusCode::OK).await;
    assert_eq!(
        (got["description"].clone(), got["rateLimit"].clone()),
        (json!("Busy"), json!(60))
    );
    assert_status(
        app.put(
            &pool_path,
            &admin,
            json!({ "description": "", "rateLimit": 0 }),
        )
        .await,
        StatusCode::NO_CONTENT,
    )
    .await;
    let got = assert_status(app.get(&pool_path, &admin).await, StatusCode::OK).await;
    assert!(got.get("description").is_none(), "{got}");
    assert!(got.get("rateLimit").is_none(), "{got}");

    // A client connection stores its client's identifier.
    let account = seed_client_account(&app, "acme-svc", &acme).await;
    let connection = created_id(
        &app,
        "/api/connections",
        &admin,
        json!({ "code": "acme-conn", "name": "Acme", "serviceAccountId": account, "clientId": acme }),
    )
    .await;
    let got = assert_status(
        app.get(&format!("/api/connections/{connection}"), &admin)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(got["clientIdentifier"], "acme", "{got}");

    // A subscription: Client Scoped and Sequence are kept, the pool's code
    // and the client's identifier stored.
    let sub = created_id(
        &app,
        "/api/subscriptions",
        &admin,
        json!({
            "code": "acme-sub",
            "name": "Acme",
            "endpoint": "https://receiver.example.test/hook",
            "eventTypes": [{ "eventTypeCode": "acme:orders:order:created" }],
            "clientId": acme,
            "connectionId": connection,
            "dispatchPoolId": pool,
            "clientScoped": true,
            "sequence": 5,
        }),
    )
    .await;
    let sub_path = format!("/api/subscriptions/{sub}");
    let got = assert_status(app.get(&sub_path, &admin).await, StatusCode::OK).await;
    assert_eq!(got["clientScoped"], true, "{got}");
    assert_eq!(got["sequence"], 5, "{got}");
    assert_eq!(got["dispatchPoolCode"], "acme-pool", "{got}");
    assert_eq!(got["clientIdentifier"], "acme", "{got}");

    // A blank connectionId on update leaves the connection alone.
    assert_status(
        app.put(
            &sub_path,
            &admin,
            json!({ "name": "Acme 2", "connectionId": "" }),
        )
        .await,
        StatusCode::NO_CONTENT,
    )
    .await;
    let got = assert_status(app.get(&sub_path, &admin).await, StatusCode::OK).await;
    assert_eq!(got["connectionId"], connection.as_str(), "{got}");

    // Re-pointed at another pool, it stores that pool's code.
    let other_pool = created_id(
        &app,
        "/api/dispatch-pools",
        &admin,
        json!({ "code": "acme-fast", "name": "Acme Fast", "clientId": acme }),
    )
    .await;
    assert_status(
        app.put(&sub_path, &admin, json!({ "dispatchPoolId": other_pool }))
            .await,
        StatusCode::NO_CONTENT,
    )
    .await;
    let got = assert_status(app.get(&sub_path, &admin).await, StatusCode::OK).await;
    assert_eq!(got["dispatchPoolCode"], "acme-fast", "{got}");

    // A negative sequence is refused.
    let (status, body) = read_json(
        app.post(
            "/api/subscriptions",
            &admin,
            json!({
                "code": "acme-neg",
                "name": "Acme",
                "endpoint": "https://receiver.example.test/hook",
                "eventTypes": [{ "eventTypeCode": "acme:orders:order:created" }],
                "clientId": acme,
                "sequence": -1,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_SEQUENCE", "{body}");
}
