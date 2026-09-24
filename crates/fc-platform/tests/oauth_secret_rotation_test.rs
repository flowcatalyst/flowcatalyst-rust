//! OAuth client secret rotation with an overlap window, as Go does it: the
//! outgoing secret keeps authenticating until it lapses or is revoked, its
//! use is stamped, and lapsed overlaps are purged. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use support::{read_json, TestApp};

async fn setup() -> TestApp {
    // Secrets are stored as keyed hashes; any 32-byte key will do.
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );
    TestApp::setup().await
}

/// A client_credentials exchange with `secret`; returns the status.
async fn token_status(app: &TestApp, client_id: &str, secret: &str) -> StatusCode {
    let body = format!(
        "grant_type=client_credentials&client_id={}&client_secret={}",
        urlencoding::encode(client_id),
        urlencoding::encode(secret)
    );
    let req = Request::builder()
        .method("POST")
        .uri("/oauth/token")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    app.router
        .clone()
        .oneshot(req)
        .await
        .expect("oneshot")
        .status()
}

async fn rotate(app: &TestApp, id: &str, body: Option<Value>) -> (StatusCode, Value) {
    let path = format!("/api/oauth-clients/{id}/rotate-secret");
    let token = app.anchor_token();
    let resp = match body {
        Some(b) => app.post(&path, &token, b).await,
        None => {
            let req = Request::builder()
                .method("POST")
                .uri(&path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request");
            app.router.clone().oneshot(req).await.expect("oneshot")
        }
    };
    read_json(resp).await
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn rotated_secret_overlaps_until_revoked_or_lapsed() {
    let app = setup().await;
    let admin = app.anchor_token();

    // A service account comes with a CONFIDENTIAL client_credentials client.
    let (status, body) = read_json(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": "rotor", "name": "Rotor" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let client_id = body["oauth"]["clientId"].as_str().unwrap().to_string();
    let first = body["oauth"]["clientSecret"].as_str().unwrap().to_string();
    let row_id = app
        .repos
        .oauth_client_repo
        .find_by_client_id(&client_id)
        .await
        .unwrap()
        .expect("oauth client")
        .id;
    assert_eq!(token_status(&app, &client_id, &first).await, StatusCode::OK);

    // Body-less rotation keeps the old secret for the default window.
    let (status, rotated) = rotate(&app, &row_id, None).await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    assert_eq!(rotated["clientId"], client_id.as_str());
    assert!(rotated["previousSecretExpiresAt"].is_string(), "{rotated}");
    let second = rotated["clientSecret"].as_str().unwrap().to_string();
    assert_ne!(first, second);

    assert_eq!(
        token_status(&app, &client_id, &second).await,
        StatusCode::OK
    );
    assert_eq!(token_status(&app, &client_id, &first).await, StatusCode::OK);
    assert_eq!(
        token_status(&app, &client_id, "not-a-secret").await,
        StatusCode::UNAUTHORIZED
    );

    // Using the old secret is stamped and shows on the client.
    let (_, read) = read_json(
        app.get(&format!("/api/oauth-clients/{row_id}"), &admin)
            .await,
    )
    .await;
    assert!(read["previousSecretExpiresAt"].is_string(), "{read}");
    assert!(read["previousSecretLastUsedAt"].is_string(), "{read}");

    // Revoking ends the overlap now; it's idempotent.
    for _ in 0..2 {
        let resp = app
            .post(
                &format!("/api/oauth-clients/{row_id}/revoke-previous-secret"),
                &admin,
                json!({}),
            )
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
    assert_eq!(
        token_status(&app, &client_id, &first).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        token_status(&app, &client_id, &second).await,
        StatusCode::OK
    );

    // graceSeconds 0 is an immediate cutover; a negative one is a 400.
    let (status, body) = rotate(&app, &row_id, Some(json!({ "graceSeconds": -1 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, cut) = rotate(&app, &row_id, Some(json!({ "graceSeconds": 0 }))).await;
    assert_eq!(status, StatusCode::OK, "{cut}");
    assert!(cut.get("previousSecretExpiresAt").is_none(), "{cut}");
    let third = cut["clientSecret"].as_str().unwrap().to_string();
    assert_eq!(
        token_status(&app, &client_id, &second).await,
        StatusCode::UNAUTHORIZED
    );

    // A lapsed overlap is refused, and the purge clears it from the row.
    let (status, _) = rotate(&app, &row_id, Some(json!({ "graceSeconds": 3600 }))).await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query(
        "UPDATE oauth_clients SET previous_secret_expires_at = NOW() - INTERVAL '1 second' \
         WHERE id = $1",
    )
    .bind(&row_id)
    .execute(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        token_status(&app, &client_id, &third).await,
        StatusCode::UNAUTHORIZED
    );
    let cleared = app
        .repos
        .oauth_client_repo
        .purge_lapsed_previous_secrets()
        .await
        .unwrap();
    assert_eq!(cleared, 1);
    let (ref_left,): (Option<String>,) =
        sqlx::query_as("SELECT previous_secret_ref FROM oauth_clients WHERE id = $1")
            .bind(&row_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(ref_left.is_none());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn only_confidential_clients_rotate() {
    let app = setup().await;
    let admin = app.anchor_token();
    let (status, body) = read_json(
        app.post(
            "/api/oauth-clients",
            &admin,
            json!({
                "clientName": "SPA",
                "clientType": "PUBLIC",
                "redirectUris": ["https://spa.example/cb"],
                "grantTypes": ["authorization_code"]
            }),
        )
        .await,
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    let id = body["client"]["id"]
        .as_str()
        .or_else(|| body["id"].as_str())
        .expect("client id")
        .to_string();
    let (status, body) = rotate(&app, &id, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}
