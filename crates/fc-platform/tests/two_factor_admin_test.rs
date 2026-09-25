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
    assert_eq!(body["error"], "NOT_USER");

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

/// Go's lost-device reset approval queue (resetapproval/api/api.go): list
/// pending requests, approve once (the user is emailed a reset link that
/// clears their 2FA), refuse a second decision.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_client_administrator_decides_lost_device_resets() {
    std::env::set_var("FLOWCATALYST_APP_KEY", APP_KEY);
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let user = developer(&app, "stranded@flowcatalyst.test", false).await;
    for (id, principal) in [
        ("rar_0000000000001", &user.id),
        ("rar_0000000000002", &user.id),
    ] {
        sqlx::query(
            "INSERT INTO iam_reset_approval_requests (id, principal_id, client_id, status, reset_2fa, expires_at) \
             VALUES ($1, $2, 'clt_0000000000001', 'PENDING', TRUE, NOW() + INTERVAL '1 day')",
        )
        .bind(id)
        .bind(principal)
        .execute(&app.pool)
        .await
        .unwrap();
    }

    let (status, list) = read_json(app.get("/api/reset-approvals", &admin).await).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let requests = list["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["email"], "stranded@flowcatalyst.test");
    assert_eq!(requests[0]["clientId"], "clt_0000000000001");

    // Another client's administrator can't reach it.
    let other = app.client_user_token("clt_0000000000009");
    let resp = app
        .post(
            "/api/reset-approvals/rar_0000000000001/approve",
            &other,
            json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let (status, body) = read_json(
        app.post(
            "/api/reset-approvals/rar_0000000000001/approve",
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["message"],
        "Reset approved — the user has been emailed a link"
    );
    let reset_2fa: bool = sqlx::query_scalar(
        "SELECT reset_2fa FROM iam_password_reset_tokens WHERE principal_id = $1",
    )
    .bind(&user.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(reset_2fa);

    let (status, body) = read_json(
        app.post(
            "/api/reset-approvals/rar_0000000000001/deny",
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "ALREADY_DECIDED");
    let (status, body) = read_json(
        app.post(
            "/api/reset-approvals/rar_0000000000002/deny",
            &admin,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Reset request denied");
    let resp = app
        .post(
            "/api/reset-approvals/rar_0000000000404/deny",
            &admin,
            json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let (_, list) = read_json(app.get("/api/reset-approvals", &admin).await).await;
    assert_eq!(list["requests"], json!([]));
}
