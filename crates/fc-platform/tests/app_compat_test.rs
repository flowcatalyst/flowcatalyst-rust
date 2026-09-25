//! The business apps that run against the platform (inhance integral, hr,
//! rfp through the Laravel SDK; AgentPlanner), replayed request for request
//! against the Rust platform. Each test names the app code it replays.
//! Requires Docker.

#[path = "support/mod.rs"]
mod support;

#[allow(unused_imports)]
use serde_json::{json, Value};

use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use support::{read_json, TestApp};

async fn insert_user(app: &TestApp, email: &str, active: bool) -> Principal {
    let mut p = Principal::new_user(email, UserScope::Anchor);
    p.active = active;
    app.repos
        .principal_repo
        .insert(&p)
        .await
        .expect("insert principal");
    p
}

/// hr `PrincipalDirectory.php:151` and rfp `PlatformPrincipalDirectory.php:216`
/// call `GET /api/principals?active=true` with no page size and expect every
/// active user back (Go returns all rows, principal/api/api.go:272-283); hr's
/// role import sends `?type=USER&active=true`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn hr_and_rfp_list_every_active_principal() {
    let app = TestApp::setup().await;
    for i in 0..25 {
        insert_user(&app, &format!("user{i:02}@inhance.test"), true).await;
    }
    insert_user(&app, "gone@inhance.test", false).await;
    let token = app.anchor_admin_token().await;

    let (status, body) = read_json(app.get("/api/principals?active=true", &token).await).await;
    assert_eq!(status, 200, "{body}");
    let principals = body["principals"].as_array().unwrap();
    assert_eq!(principals.len(), 25, "{body}");
    assert_eq!(body["total"], 25);
    assert!(principals.iter().all(|p| p["active"] == true));
    // Principal::fromArray requires id, type and name.
    for p in principals {
        assert!(p["id"].is_string() && p["type"] == "USER" && p["name"].is_string());
    }

    let (status, body) = read_json(
        app.get("/api/principals?type=USER&active=true", &token)
            .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["principals"].as_array().unwrap().len(), 25);

    let (_, body) = read_json(app.get("/api/principals?active=false", &token).await).await;
    assert_eq!(body["principals"].as_array().unwrap().len(), 1);

    // An explicit page size still pages.
    let (_, body) = read_json(
        app.get("/api/principals?active=true&page=1&pageSize=10", &token)
            .await,
    )
    .await;
    assert_eq!(body["principals"].as_array().unwrap().len(), 10);
    assert_eq!(body["total"], 25);
}

/// Go lists every application and every OAuth client when unfiltered
/// (application/api/api.go:63-81, auth/api/api.go:170-187); `?active=true`
/// parses.
#[tokio::test]
#[ignore = "requires Docker"]
async fn applications_and_oauth_clients_list_every_row_by_default() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let mut retired =
        fc_platform::application::entity::Application::new("retired-app", "Retired App");
    retired.active = false;
    app.repos
        .application_repo
        .insert(&retired)
        .await
        .expect("insert application");

    let (status, body) = read_json(app.get("/api/applications", &token).await).await;
    assert_eq!(status, 200, "{body}");
    let codes: Vec<&str> = body["applications"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"retired-app"), "{codes:?}");
    assert!(codes.contains(&"platform"), "{codes:?}");

    let (status, body) = read_json(app.get("/api/applications?active=true", &token).await).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["applications"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["active"] == true));

    let (status, body) = read_json(app.get("/api/oauth-clients?active=true", &token).await).await;
    assert_eq!(status, 200, "{body}");
    let (status, _) = read_json(app.get("/api/oauth-clients", &token).await).await;
    assert_eq!(status, 200);
}

#[allow(dead_code)]
async fn create_client(app: &TestApp, identifier: &str) -> String {
    let client = Client::new(identifier.to_uppercase(), identifier);
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");
    client.id
}
