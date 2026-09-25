//! The password flows as Go serves them (`passwordreset/api/api.go`,
//! `auth/login/endpoint.go` check-domain): create-your-password invites,
//! factor-gated resets, the 2FA hand-off at confirm, the admin reset's
//! `reset2fa` option.

#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, Response, StatusCode},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use fc_platform::domain::{Principal, UserScope};
use fc_platform::mfa::crypto::totp_code;
use fc_platform::mfa::entity::{Method as MfaMethod, MethodType};
use fc_platform::mfa::MfaRepository;
use fc_platform::password_reset::entity::{PasswordResetToken, TokenPurpose};
use fc_platform::shared::encryption_service::EncryptionService;
use fc_platform::shared::rate_limit_store::PostgresRateLimitStore;
use support::{read_json, TestApp};

const APP_KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const NEW_PASSWORD: &str = "Brand-New-Secret-7!";

fn with_app_key() {
    std::env::set_var("FLOWCATALYST_APP_KEY", APP_KEY);
}

async fn post(app: &TestApp, path: &str, body: Value) -> Response<Body> {
    app.router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn sha256_hex(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

/// A token row for `principal` whose raw value is `raw`.
async fn plant_token(
    app: &TestApp,
    principal_id: &str,
    raw: &str,
    purpose: TokenPurpose,
    requires_factor: bool,
    reset_2fa: bool,
) {
    let mut token = PasswordResetToken::new(
        principal_id,
        sha256_hex(raw),
        chrono::Utc::now() + chrono::Duration::hours(1),
    );
    token.purpose = purpose;
    token.requires_factor = requires_factor;
    token.reset_2fa = reset_2fa;
    app.repos.password_reset_repo.create(&token).await.unwrap();
}

async fn token_row(app: &TestApp, principal_id: &str) -> Option<(String, String, bool, bool)> {
    sqlx::query_as(
        "SELECT id, purpose, requires_factor, reset_2fa FROM iam_password_reset_tokens \
         WHERE principal_id = $1",
    )
    .bind(principal_id)
    .fetch_optional(&app.pool)
    .await
    .unwrap()
}

/// A user with a confirmed TOTP factor; its shared secret.
async fn give_totp(app: &TestApp, principal_id: &str) -> String {
    let secret = fc_platform::mfa::crypto::new_totp_secret();
    let enc = EncryptionService::new(APP_KEY).unwrap();
    let mut m = MfaMethod::new(principal_id, MethodType::Totp);
    m.secret_encrypted = Some(enc.encrypt(&secret).unwrap());
    m.confirmed_at = Some(chrono::Utc::now());
    MfaRepository::new(&app.pool)
        .replace_pending_method(&m)
        .await
        .unwrap();
    secret
}

/// Go `requestPasswordSetup` + `withPasswordSetupRequired`: a passwordless
/// internal user is told to create a password and gets a 72-hour invite;
/// anyone else gets the same answer and nothing. Budgeted per address in
/// its own bucket: past it, nothing is issued and the answer is the same.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_passwordless_user_asks_for_a_set_password_link() {
    std::env::remove_var("FC_RL_PASSWORD_RESET_EMAIL_PER_HOUR");
    let app = TestApp::setup_with_rate_limit_store(|pool| {
        Arc::new(PostgresRateLimitStore::new(pool.clone()))
    })
    .await;
    let fresh = Principal::new_user("fresh@flowcatalyst.test", UserScope::Anchor);
    app.repos.principal_repo.insert(&fresh).await.unwrap();

    let (status, body) = read_json(
        post(
            &app,
            "/auth/check-domain",
            json!({ "email": "Fresh@flowcatalyst.test" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["authMethod"], "internal");
    assert_eq!(body["passwordSetupRequired"], true, "{body}");
    let (_, body) = read_json(
        post(
            &app,
            "/auth/check-domain",
            json!({ "email": "nobody@flowcatalyst.test" }),
        )
        .await,
    )
    .await;
    assert!(body.get("passwordSetupRequired").is_none(), "{body}");

    let expected = json!({
        "message": "If your account needs a password, we've emailed you a link to create it."
    });
    let (status, unknown) = read_json(
        post(
            &app,
            "/auth/password-setup/request",
            json!({ "email": "nobody@flowcatalyst.test" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unknown, expected);

    let mut ids = Vec::new();
    for _ in 0..7 {
        let (status, body) = read_json(
            post(
                &app,
                "/auth/password-setup/request",
                json!({ "email": " FRESH@flowcatalyst.test ", "redirectUri": "/dashboard" }),
            )
            .await,
        )
        .await;
        assert_eq!((status, &body), (StatusCode::OK, &expected));
        let (id, purpose, _, _) = token_row(&app, &fresh.id).await.expect("an invite");
        assert_eq!(purpose, "invite");
        ids.push(id);
    }
    ids.dedup();
    assert_eq!(
        ids.len(),
        5,
        "five within the budget, then nothing: {ids:?}"
    );
    let (expires, redirect): (chrono::DateTime<chrono::Utc>, Option<String>) = sqlx::query_as(
        "SELECT expires_at, redirect_uri FROM iam_password_reset_tokens WHERE principal_id = $1",
    )
    .bind(&fresh.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(expires > chrono::Utc::now() + chrono::Duration::hours(71));
    assert_eq!(redirect.as_deref(), Some("/dashboard"));
}

/// A confirmed invite sets the password and signs the user in (Go
/// `maybeEstablishSession`); a reset doesn't.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_confirmed_invite_signs_the_user_in() {
    with_app_key();
    let app = TestApp::setup().await;
    let user = Principal::new_user("invitee@flowcatalyst.test", UserScope::Anchor);
    app.repos.principal_repo.insert(&user).await.unwrap();
    plant_token(
        &app,
        &user.id,
        "raw-invite",
        TokenPurpose::Invite,
        false,
        false,
    )
    .await;

    let (status, v) = read_json(
        app.get_unauth("/auth/password-reset/validate?token=raw-invite")
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        v,
        json!({ "valid": true, "reason": null, "requiresFactor": false })
    );

    let resp = post(
        &app,
        "/auth/password-reset/confirm",
        json!({ "token": "raw-invite", "password": NEW_PASSWORD }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let signed_in = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .any(|c| c.to_str().unwrap().starts_with("fc_session="));
    assert!(signed_in);
    let (_, body) = read_json(resp).await;
    assert_eq!(body["status"], "ok", "{body}");
    assert_eq!(body["sessionEstablished"], true, "{body}");
    assert!(token_row(&app, &user.id).await.is_none(), "single use");

    plant_token(
        &app,
        &user.id,
        "raw-reset",
        TokenPurpose::Reset,
        false,
        false,
    )
    .await;
    let resp = post(
        &app,
        "/auth/password-reset/confirm",
        json!({ "token": "raw-reset", "password": "Another-Secret-8!" }),
    )
    .await;
    let (status, body) = read_json(resp).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("sessionEstablished").is_none(), "{body}");

    let (status, body) = read_json(
        post(
            &app,
            "/auth/password-reset/confirm",
            json!({ "token": "raw-reset", "password": NEW_PASSWORD }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "INVALID_TOKEN");
}

/// A user with an authenticator must prove it to reset (Go `tryIssueToken`
/// flags the token; confirm checks it). Five wrong codes burn the token set.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_reset_for_an_authenticator_user_needs_a_current_code() {
    with_app_key();
    let app = TestApp::setup().await;
    let user = Principal::new_user("guarded@flowcatalyst.test", UserScope::Anchor);
    app.repos.principal_repo.insert(&user).await.unwrap();
    let secret = give_totp(&app, &user.id).await;

    let resp = post(
        &app,
        "/auth/password-reset/request",
        json!({ "email": "guarded@flowcatalyst.test" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (_, purpose, requires_factor, _) = token_row(&app, &user.id).await.unwrap();
    assert_eq!((purpose.as_str(), requires_factor), ("reset", true));

    plant_token(
        &app,
        &user.id,
        "raw-guarded",
        TokenPurpose::Reset,
        true,
        false,
    )
    .await;
    let (_, v) = read_json(
        app.get_unauth("/auth/password-reset/validate?token=raw-guarded")
            .await,
    )
    .await;
    assert_eq!(v["requiresFactor"], true);
    for _ in 0..4 {
        let (status, body) = read_json(
            post(
                &app,
                "/auth/password-reset/confirm",
                json!({ "token": "raw-guarded", "password": NEW_PASSWORD, "factorCode": "000000" }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "INVALID_FACTOR");
    }
    let code = totp_code(&secret, chrono::Utc::now().timestamp()).unwrap();
    let (status, body) = read_json(
        post(
            &app,
            "/auth/password-reset/confirm",
            json!({ "token": "raw-guarded", "password": NEW_PASSWORD, "factorCode": code }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    plant_token(&app, &user.id, "raw-burn", TokenPurpose::Reset, true, false).await;
    let mut last = Value::Null;
    for _ in 0..5 {
        last = read_json(
            post(
                &app,
                "/auth/password-reset/confirm",
                json!({ "token": "raw-burn", "password": NEW_PASSWORD, "factorCode": "000000" }),
            )
            .await,
        )
        .await
        .1;
    }
    assert_eq!(last["code"], "INVALID_TOKEN", "{last}");
    assert!(token_row(&app, &user.id).await.is_none(), "burned");
}

/// The admin reset can also clear 2FA (Go `reset2fa`); confirming it on a
/// domain requiring 2FA hands back an enrolment token instead of `ok` (Go
/// `postResetTwoFactor`).
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_lost_device_reset_clears_two_factor_and_sends_the_user_to_enrol() {
    with_app_key();
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let (status, idp) = read_json(
        app.post(
            "/api/identity-providers",
            &admin,
            json!({ "code": "internal", "name": "Internal", "type": "INTERNAL" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{idp}");
    let (status, body) = read_json(
        app.post(
            "/api/email-domain-mappings",
            &admin,
            json!({
                "emailDomain": "strict.test",
                "identityProviderId": idp["id"],
                "scopeType": "ANCHOR",
                "require2fa": true,
                "allowed2faMethods": ["TOTP", "EMAIL_PIN"]
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let user = Principal::new_user("lost@strict.test", UserScope::Anchor);
    app.repos.principal_repo.insert(&user).await.unwrap();
    give_totp(&app, &user.id).await;

    let resp = app
        .post(
            &format!("/api/principals/{}/send-password-reset", user.id),
            &admin,
            json!({ "reset2fa": true }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (_, _, _, reset_2fa) = token_row(&app, &user.id).await.unwrap();
    assert!(reset_2fa);

    plant_token(&app, &user.id, "raw-lost", TokenPurpose::Reset, false, true).await;
    let (status, body) = read_json(
        post(
            &app,
            "/auth/password-reset/confirm",
            json!({ "token": "raw-lost", "password": NEW_PASSWORD }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "enrollment_required", "{body}");
    assert_eq!(body["allowedMethods"], json!(["TOTP", "EMAIL_PIN"]));
    assert!(body["enrollToken"].as_str().is_some());
    let factors: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM iam_user_mfa_methods WHERE principal_id = $1")
            .bind(&user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(factors, 0, "2FA cleared");
}

/// Go `InviteLink` (create-user's `returnInviteLink`): the invite is minted
/// and its set-password link returned, not emailed; it carries the redirect.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_invite_link_is_minted_for_the_caller() {
    use fc_platform::auth::password_reset_api::PasswordResetEmailer;
    let app = TestApp::setup().await;
    let user = Principal::new_user("linked@flowcatalyst.test", UserScope::Client);
    app.repos.principal_repo.insert(&user).await.unwrap();
    let emailer = PasswordResetEmailer {
        password_reset_repo: app.repos.password_reset_repo.clone(),
        email_service: Arc::new(fc_platform::shared::email_service::LogEmailService),
        unit_of_work: app.unit_of_work.clone(),
        external_base_url: "https://platform.test/".to_string(),
    };
    let link = emailer
        .invite_link(&user, Some("https://app.test/home".to_string()))
        .await
        .unwrap()
        .expect("a link");
    let raw = link
        .strip_prefix("https://platform.test/auth/set-password?token=")
        .expect("the set-password link");
    let (_, v) = read_json(
        app.get_unauth(&format!("/auth/password-reset/validate?token={raw}"))
            .await,
    )
    .await;
    assert_eq!(v["valid"], true, "{v}");
    let (purpose, redirect): (String, Option<String>) = sqlx::query_as(
        "SELECT purpose, redirect_uri FROM iam_password_reset_tokens WHERE principal_id = $1",
    )
    .bind(&user.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(purpose, "invite");
    assert_eq!(redirect.as_deref(), Some("https://app.test/home"));
}
