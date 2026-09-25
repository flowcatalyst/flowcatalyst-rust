//! A service account's client tier follows its client links, as in Go (none
//! → ANCHOR, one → CLIENT, several → PARTNER), and lands on the principal its
//! tokens are built from. The requested scope is stored as sent and echoed,
//! but doesn't decide the tier. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use support::{read_json, TestApp};

/// Service-account creation encrypts the generated webhook credentials and
/// hashes the OAuth client secret; any 32-byte key will do.
async fn setup() -> TestApp {
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );
    TestApp::setup().await
}

async fn create_client(app: &TestApp, identifier: &str) -> String {
    let client = Client::new(identifier.to_uppercase(), identifier);
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");
    client.id
}

async fn create_sa(app: &TestApp, body: Value) -> (StatusCode, Value) {
    read_json(
        app.post(
            "/api/service-accounts",
            &app.anchor_admin_token().await,
            body,
        )
        .await,
    )
    .await
}

async fn principal(app: &TestApp, id: &str) -> Principal {
    app.repos
        .principal_repo
        .find_by_id(id)
        .await
        .expect("find principal")
        .expect("service account principal")
}

fn token(app: &TestApp, principal: &Principal) -> String {
    app.auth_service
        .generate_access_token(principal)
        .expect("token")
}

/// The 403 code an anchor-gated write (creating another service account)
/// made with `token` answers. The token's principal holds no permissions, so
/// an anchor caller passes the anchor check and stops at the permission
/// check (`PERMISSION_REQUIRED`); any other caller stops at the anchor check
/// (`ANCHOR_REQUIRED`), as Go's bodies have it.
async fn anchor_only_status(app: &TestApp, token: &str, code: &str) -> String {
    let (status, body) = read_json(
        app.post(
            "/api/service-accounts",
            token,
            json!({ "code": code, "name": code }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    body["error"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn client_scope_service_account_is_not_anchor() {
    let app = setup().await;
    let clt = create_client(&app, "scope-clt").await;

    let (status, body) = create_sa(
        &app,
        json!({ "code": "clt-bot", "name": "Client bot", "scope": "CLIENT", "clientIds": [clt] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["serviceAccount"]["scope"], "CLIENT");
    assert_eq!(body["serviceAccount"]["clientIds"], json!([clt]));
    assert_eq!(body["principalId"], body["serviceAccount"]["id"]);

    let id = body["serviceAccount"]["id"].as_str().unwrap();
    let p = principal(&app, id).await;
    assert_eq!(p.scope, UserScope::Client);
    assert_eq!(p.client_id.as_deref(), Some(clt.as_str()));

    // The token carries CLIENT and only its client.
    let token = token(&app, &p);
    let claims = app.auth_service.validate_token(&token).expect("claims");
    assert_eq!(claims.tier, UserScope::Client);
    assert_eq!(claims.clients.len(), 1);
    assert!(claims.clients[0].starts_with(&clt), "{:?}", claims.clients);

    // And the anchor check turns it away.
    assert_eq!(
        anchor_only_status(&app, &token, "escalated").await,
        "ANCHOR_REQUIRED"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn anchor_scope_service_account_is_still_anchor() {
    let app = setup().await;
    let (status, body) = create_sa(
        &app,
        json!({ "code": "anchor-bot", "name": "Anchor bot", "scope": "ANCHOR" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["serviceAccount"]["scope"], "ANCHOR");

    let p = principal(&app, body["serviceAccount"]["id"].as_str().unwrap()).await;
    assert_eq!(p.scope, UserScope::Anchor);
    let token = token(&app, &p);
    assert_eq!(
        app.auth_service.validate_token(&token).unwrap().tier,
        UserScope::Anchor
    );
    // It passes the anchor check; only the permission check stops it.
    assert_eq!(
        anchor_only_status(&app, &token, "made-by-anchor-bot").await,
        "PERMISSION_REQUIRED"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn unknown_scope_or_client_is_rejected() {
    let app = setup().await;
    let clt = create_client(&app, "scope-bad").await;

    // Unrecognised values are a 400 (X-06); Go stores them as sent.
    for scope in ["ROOT", "client", "Anchor", ""] {
        let (status, body) = create_sa(
            &app,
            json!({ "code": "bad-scope", "name": "x", "scope": scope, "clientIds": [clt] }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{scope:?}: {body}");
    }

    // A client that doesn't exist is named, not silently linked.
    let (status, body) = create_sa(
        &app,
        json!({ "code": "ghost", "name": "x", "scope": "CLIENT", "clientIds": ["clt_0NOSUCHCLIENT"] }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // Nothing was created by any of these.
    for code in ["bad-scope", "ghost"] {
        assert!(app
            .repos
            .service_account_repo
            .find_by_code(code)
            .await
            .unwrap()
            .is_none());
    }
}

/// A scope that disagrees with the links is stored as sent, and the tier
/// follows the links, as Go does (create_credentials.go:102, 125). CLIENT
/// with no clients therefore gives an ANCHOR principal: a known Go behaviour,
/// kept on purpose and flagged to the owner as a risk.
#[tokio::test]
#[ignore = "requires Docker"]
async fn requested_scope_is_stored_and_the_tier_follows_the_links() {
    let app = setup().await;
    let clt = create_client(&app, "scope-go").await;

    for (code, scope, clients, tier) in [
        ("anchor-one", "ANCHOR", json!([clt]), UserScope::Client),
        ("client-none", "CLIENT", json!([]), UserScope::Anchor),
        ("partner-none", "PARTNER", json!([]), UserScope::Anchor),
        ("partner-one", "PARTNER", json!([clt]), UserScope::Client),
    ] {
        let (status, body) = create_sa(
            &app,
            json!({ "code": code, "name": code, "scope": scope, "clientIds": clients }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{code}: {body}");
        assert_eq!(body["serviceAccount"]["scope"], scope, "{code}");
        assert_eq!(body["serviceAccount"]["clientIds"], clients, "{code}");
        let p = principal(&app, body["serviceAccount"]["id"].as_str().unwrap()).await;
        assert_eq!(p.scope, tier, "{code}");
    }
}

/// Without a scope, the scope follows the client links, as Go derives it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn absent_scope_follows_the_client_links() {
    let app = setup().await;
    let a = create_client(&app, "follow-a").await;
    let b = create_client(&app, "follow-b").await;

    for (code, clients, expected) in [
        ("none", json!([]), UserScope::Anchor),
        ("one", json!([a]), UserScope::Client),
        ("two", json!([a, b]), UserScope::Partner),
    ] {
        let (status, body) = create_sa(
            &app,
            json!({ "code": code, "name": code, "clientIds": clients }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{code}: {body}");
        assert!(body["serviceAccount"].get("scope").is_none(), "{code}");
        let p = principal(&app, body["serviceAccount"]["id"].as_str().unwrap()).await;
        assert_eq!(p.scope, expected, "{code}");
    }

    let p = principal(
        &app,
        &app.repos
            .service_account_repo
            .find_by_code("two")
            .await
            .unwrap()
            .unwrap()
            .id,
    )
    .await;
    let mut granted = p.assigned_clients.clone();
    granted.sort();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(granted, expected);
    assert!(p.client_id.is_none());
}

/// Changing the links moves the principal's reach with them, in the same
/// commit.
#[tokio::test]
#[ignore = "requires Docker"]
async fn update_moves_the_principal_reach() {
    let app = setup().await;
    let a = create_client(&app, "move-a").await;
    let b = create_client(&app, "move-b").await;
    let (_, body) = create_sa(
        &app,
        json!({ "code": "mover", "name": "Mover", "scope": "CLIENT", "clientIds": [a] }),
    )
    .await;
    let id = body["serviceAccount"]["id"].as_str().unwrap().to_string();
    let path = format!("/api/service-accounts/{id}");
    let admin = app.anchor_admin_token().await;

    let resp = app
        .put(
            &path,
            &admin,
            json!({ "scope": "PARTNER", "clientIds": [a, b] }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let p = principal(&app, &id).await;
    assert_eq!(p.scope, UserScope::Partner);
    assert!(p.client_id.is_none());
    assert_eq!(p.assigned_clients.len(), 2);
    let (_, read) = read_json(app.get(&path, &admin).await).await;
    assert_eq!(read["scope"], "PARTNER");
    assert_eq!(read["clientIds"].as_array().unwrap().len(), 2);

    // Links alone: the scope follows them, and the old grants go.
    let resp = app.put(&path, &admin, json!({ "clientIds": [b] })).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let p = principal(&app, &id).await;
    assert_eq!(p.scope, UserScope::Client);
    assert_eq!(p.client_id.as_deref(), Some(b.as_str()));
    assert!(p.assigned_clients.is_empty());

    // A scope alone is stored as sent and leaves the principal as it is, as
    // in Go; an unrecognised one is a 400.
    let resp = app.put(&path, &admin, json!({ "scope": "ANCHOR" })).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let (_, read) = read_json(app.get(&path, &admin).await).await;
    assert_eq!(read["scope"], "ANCHOR");
    assert_eq!(read["clientIds"], json!([b]));
    let p = principal(&app, &id).await;
    assert_eq!(p.scope, UserScope::Client);
    assert_eq!(p.client_id.as_deref(), Some(b.as_str()));
    let resp = app.put(&path, &admin, json!({ "scope": "anchor" })).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(principal(&app, &id).await.scope, UserScope::Client);

    // Explicitly back to ANCHOR with no links.
    let resp = app
        .put(&path, &admin, json!({ "scope": "ANCHOR", "clientIds": [] }))
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let p = principal(&app, &id).await;
    assert_eq!(p.scope, UserScope::Anchor);
    assert!(p.client_id.is_none());
}

/// An application's provisioned service account is ANCHOR with no client
/// links, as Go provisions it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn provisioned_service_account_is_anchor() {
    let app = setup().await;
    let application = fc_platform::application::entity::Application::new("prov-scope", "Prov");
    app.repos
        .application_repo
        .insert(&application)
        .await
        .expect("insert application");

    let (status, body) = read_json(
        app.post(
            &format!(
                "/api/applications/{}/provision-service-account",
                application.id
            ),
            &app.anchor_admin_token().await,
            json!({}),
        )
        .await,
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    let id = body["serviceAccount"]["principalId"].as_str().unwrap();
    let p = principal(&app, id).await;
    assert_eq!(p.scope, UserScope::Anchor);
    assert!(p.client_id.is_none());
    assert!(p.assigned_clients.is_empty());
}
