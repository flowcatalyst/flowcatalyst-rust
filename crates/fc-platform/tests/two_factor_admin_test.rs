//! The admin 2FA reset, as Go serves it (principal/api/api.go
//! `resetTwoFactor`).

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::json;

use fc_platform::domain::{Principal, UserScope};
use support::{read_json, TestApp};

const APP_KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

async fn developer(app: &TestApp, email: &str, _with_role: bool) -> Principal {
    let p = Principal::new_user(email, UserScope::Anchor);
    app.repos.principal_repo.insert(&p).await.unwrap();
    p
}

/// Go resetTwoFactor: an administrator clears a user's 2FA; a service
/// account is NOT_USER; a missing id is 404.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_administrator_resets_a_users_two_factor() {
    std::env::set_var("FLOWCATALYST_APP_KEY", APP_KEY);
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let user = developer(&app, "lost@flowcatalyst.test", false).await;
    let mut method = fc_platform::mfa::entity::Method::new(
        &user.id,
        fc_platform::mfa::entity::MethodType::EmailPin,
    );
    method.confirmed_at = Some(chrono::Utc::now());
    fc_platform::mfa::MfaRepository::new(&app.pool)
        .replace_pending_method(&method)
        .await
        .unwrap();

    let (status, body) = read_json(
        app.post(
            &format!("/api/principals/{}/reset-2fa", user.id),
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Two-factor authentication reset");
    let left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM iam_user_mfa_methods WHERE principal_id = $1")
            .bind(&user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(left, 0);
    let audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM aud_logs WHERE entity_id = $1 AND operation = '2FA_RESET_BY_ADMIN'",
    )
    .bind(&user.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audit, 1);

    let svc = Principal::new_service("svc-2fa", "Service", UserScope::Anchor);
    app.repos.principal_repo.insert(&svc).await.unwrap();
    let (status, body) = read_json(
        app.post(
            &format!("/api/principals/{}/reset-2fa", svc.id),
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "NOT_USER");

    let resp = app
        .post(
            "/api/principals/prn_0000000000000/reset-2fa",
            &admin,
            json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Without the user-admin permission.
    let resp = app
        .post(
            &format!("/api/principals/{}/reset-2fa", user.id),
            &app.anchor_token(),
            json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
