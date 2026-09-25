//! Self-service developer API credentials, as Go serves them
//! (principal/api/api.go developer-credential, developer-users;
//! oauthapi/token.go the developer client_credentials branch).

#[path = "support/mod.rs"]
mod support;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;

use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::roles;
use fc_platform::service_account::entity::RoleAssignment;
use support::{read_json, TestApp};

const APP_KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

async fn token_exchange(app: &TestApp, client_id: &str, secret: &str) -> (StatusCode, Value) {
    let body = format!(
        "grant_type=client_credentials&client_id={client_id}&client_secret={}",
        urlencoding::encode(secret)
    );
    let resp = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/oauth/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    read_json(resp).await
}

async fn developer(app: &TestApp, email: &str, with_role: bool) -> Principal {
    let mut p = Principal::new_user(email, UserScope::Anchor);
    if with_role {
        p.roles = vec![RoleAssignment::new("platform:developer")];
    }
    app.repos.principal_repo.insert(&p).await.unwrap();
    p
}

/// Go setDeveloperCredential / revokeDeveloperCredential /
/// listDeveloperUsers and the principal-as-client_id token grant.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_developer_mints_tokens_with_their_own_credential() {
    std::env::set_var("FLOWCATALYST_APP_KEY", APP_KEY);
    let app = TestApp::setup().await;
    app.repos
        .role_repo
        .insert(&roles::developer())
        .await
        .unwrap();
    let admin = app.anchor_admin_token().await;
    let dev = developer(&app, "dev@flowcatalyst.test", true).await;
    let other = developer(&app, "other@flowcatalyst.test", true).await;
    let plain = developer(&app, "plain@flowcatalyst.test", false).await;
    let dev_token = app.auth_service.generate_access_token(&dev).unwrap();

    let (status, list) = read_json(app.get("/api/principals/developer-users", &admin).await).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["total"], 2, "{list}");
    assert!(list["principals"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["hasDeveloperCredential"] == false));

    // Your own credential, with the self-service permission.
    let (status, set) = read_json(
        app.post(
            &format!("/api/principals/{}/developer-credential", dev.id),
            &dev_token,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["id"], dev.id.as_str());
    let secret = set["clientSecret"].as_str().unwrap().to_string();

    // Someone else's needs the user-admin permission.
    let resp = app
        .post(
            &format!("/api/principals/{}/developer-credential", other.id),
            &dev_token,
            json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let (status, body) = read_json(
        app.post(
            &format!("/api/principals/{}/developer-credential", plain.id),
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "NOT_A_DEVELOPER");

    let (status, token) = token_exchange(&app, &dev.id, &secret).await;
    assert_eq!(status, StatusCode::OK, "{token}");
    assert_eq!(token["token_type"], "Bearer");
    assert!(token["access_token"].as_str().is_some());
    let (status, body) = token_exchange(&app, &dev.id, "wrong").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "invalid_client");
    let (status, _) = token_exchange(&app, &plain.id, &secret).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (_, list) = read_json(app.get("/api/principals/developer-users", &admin).await).await;
    let mine = list["principals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == dev.id.as_str())
        .unwrap()
        .clone();
    assert_eq!(mine["hasDeveloperCredential"], true, "{mine}");
    assert!(mine["developerCredentialUpdatedAt"].is_string());

    let resp = app
        .delete(
            &format!("/api/principals/{}/developer-credential", dev.id),
            &dev_token,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let (status, _) = token_exchange(&app, &dev.id, &secret).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "revoked");

    assert_eq!(
        app.event_count_by_type("platform:iam:user:developer-credential-set")
            .await,
        1
    );
    assert_eq!(
        app.event_count_by_type("platform:iam:user:developer-credential-revoked")
            .await,
        1
    );
}
