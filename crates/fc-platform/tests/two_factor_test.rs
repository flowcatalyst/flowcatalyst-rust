//! Two-factor authentication as Go serves it (`internal/platform/mfa`,
//! `auth/login/twofactor*.go`), end to end against a real database.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use support::{read_json, TestApp};

/// An INTERNAL identity provider, created through the API; returns its id.
async fn create_internal_idp(app: &TestApp, token: &str, code: &str) -> String {
    let (status, body) = read_json(
        app.post(
            "/api/identity-providers",
            token,
            json!({ "code": code, "name": code, "type": "INTERNAL" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_string()
}

/// A mapping of `domain` to `idp` carrying `policy`'s 2FA fields; returns
/// the answer.
async fn create_mapping(
    app: &TestApp,
    token: &str,
    domain: &str,
    idp: &str,
    policy: Value,
) -> (StatusCode, Value) {
    let mut body = json!({
        "emailDomain": domain,
        "identityProviderId": idp,
        "scopeType": "ANCHOR"
    });
    for (k, v) in policy.as_object().unwrap() {
        body[k] = v.clone();
    }
    read_json(app.post("/api/email-domain-mappings", token, body).await).await
}

/// Go `validate2FA` + the mapping's 2FA fields on the wire
/// (emaildomainmapping/api/dto.go, operations/create.go:28-42,
/// operations/update.go:63-79).
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_domain_mapping_carries_its_two_factor_policy() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let idp = create_internal_idp(&app, &token, "internal").await;

    let (status, body) = create_mapping(
        &app,
        &token,
        "bad.test",
        &idp,
        json!({ "require2fa": true, "allowed2faMethods": ["SMS"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "INVALID_2FA_METHOD", "{body}");

    let (status, body) = create_mapping(
        &app,
        &token,
        "bad.test",
        &idp,
        json!({ "require2fa": true }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "2FA_METHOD_REQUIRED", "{body}");

    let (status, body) = create_mapping(
        &app,
        &token,
        "acme.test",
        &idp,
        json!({
            "require2fa": true,
            "allowed2faMethods": ["TOTP", "EMAIL_PIN"],
            "rememberDeviceEnabled": true,
            "rememberDeviceDays": 14
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().unwrap().to_string();

    let (status, got) = read_json(
        app.get(&format!("/api/email-domain-mappings/{id}"), &token)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert_eq!(got["require2fa"], true, "{got}");
    assert_eq!(
        got["allowed2faMethods"],
        json!(["TOTP", "EMAIL_PIN"]),
        "{got}"
    );
    assert_eq!(got["rememberDeviceEnabled"], true, "{got}");
    assert_eq!(got["rememberDeviceDays"], 14, "{got}");

    // Update: the resulting policy must hold, not just the change.
    let (status, body) = read_json(
        app.put(
            &format!("/api/email-domain-mappings/{id}"),
            &token,
            json!({ "allowed2faMethods": [] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "2FA_METHOD_REQUIRED", "{body}");

    let resp = app
        .put(
            &format!("/api/email-domain-mappings/{id}"),
            &token,
            json!({ "require2fa": false, "allowed2faMethods": ["EMAIL_PIN"] }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let (_, got) = read_json(
        app.get(&format!("/api/email-domain-mappings/{id}"), &token)
            .await,
    )
    .await;
    assert_eq!(got["require2fa"], false, "{got}");
    assert_eq!(got["allowed2faMethods"], json!(["EMAIL_PIN"]), "{got}");
    assert_eq!(got["rememberDeviceDays"], 14, "untouched: {got}");
}

// ── sign-in challenge, enrolment, self-service ───────────────────────────────

use axum::{
    body::Body,
    http::{Method, Request, Response},
};
use tower::ServiceExt;

use fc_platform::mfa::crypto::totp_code;
use fc_platform::mfa::entity::EmailPinPurpose;
use fc_platform::mfa::MfaRepository;

const PASSWORD: &str = "Correct-Horse-9!";

/// The platform key TOTP secrets are encrypted with; set before the router
/// is built (Go refuses TOTP without FLOWCATALYST_APP_KEY).
fn with_app_key() {
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=",
    );
}

async fn seed_user(app: &TestApp, email: &str) -> fc_platform::domain::Principal {
    use fc_platform::auth::password_service::PasswordService;
    use fc_platform::domain::{Principal, UserScope};
    let mut user = Principal::new_user(email, UserScope::Anchor);
    if let Some(identity) = user.user_identity.as_mut() {
        identity.password_hash = Some(PasswordService::default().hash_password(PASSWORD).unwrap());
    }
    app.repos.principal_repo.insert(&user).await.unwrap();
    user
}

/// A public JSON POST, optionally carrying a cookie header.
async fn post_public(
    app: &TestApp,
    path: &str,
    body: Value,
    cookie: Option<&str>,
) -> Response<Body> {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("content-type", "application/json")
        .header("user-agent", "TwoFactorTest/1.0");
    if let Some(c) = cookie {
        builder = builder.header("cookie", c);
    }
    app.router
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

fn cookie_of(resp: &Response<Body>, name: &str) -> Option<String> {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|c| c.strip_prefix(&format!("{name}=")))
        .and_then(|c| c.split(';').next())
        .map(String::from)
}

async fn login(
    app: &TestApp,
    email: &str,
    cookie: Option<&str>,
) -> (StatusCode, Value, Response<Body>) {
    let resp = post_public(
        app,
        "/auth/login",
        json!({ "email": email, "password": PASSWORD }),
        cookie,
    )
    .await;
    let status = resp.status();
    let headers = resp.headers().clone();
    let (_, body) = read_json(resp).await;
    let mut rebuilt = Response::new(Body::empty());
    *rebuilt.headers_mut() = headers;
    (status, body, rebuilt)
}

/// A signed-in session for a user with no 2FA yet.
async fn session_for(app: &TestApp, email: &str) -> String {
    let (status, body, resp) = login(app, email, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "ok", "{body}");
    cookie_of(&resp, "fc_session").expect("session cookie")
}

/// Enrol TOTP through self-service; the shared secret and recovery codes.
async fn enrol_totp(app: &TestApp, session: &str) -> (String, Vec<String>) {
    let (status, begin) = read_json(
        app.send_with_session(
            Method::POST,
            "/auth/2fa/methods/totp/begin",
            session,
            Some(json!({})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{begin}");
    let secret = begin["secret"].as_str().unwrap().to_string();
    assert!(begin["uri"]
        .as_str()
        .unwrap()
        .starts_with("otpauth://totp/"));
    assert!(begin["qr"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));

    let (status, body) = read_json(
        app.send_with_session(
            Method::POST,
            "/auth/2fa/methods/totp/confirm",
            session,
            Some(json!({ "code": "000000x" })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "INVALID_CODE");

    // The previous step's code: enrolment spends it, so the current step's
    // code is still good for the first sign-in.
    let now = chrono::Utc::now().timestamp();
    let (status, body) = read_json(
        app.send_with_session(
            Method::POST,
            "/auth/2fa/methods/totp/confirm",
            session,
            Some(json!({ "code": totp_code(&secret, now - 30).unwrap() })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let codes: Vec<String> = body["recoveryCodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    assert_eq!(codes.len(), 10);
    (secret, codes)
}

async fn pending_token(app: &TestApp, email: &str) -> String {
    let (status, body, _) = login(app, email, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "mfa_required", "{body}");
    assert!(body.get("principalId").is_none(), "no session yet: {body}");
    body["mfaToken"].as_str().unwrap().to_string()
}

/// Go `maybeChallenge2FA` + `handle2FAVerify`: an enrolled user's password
/// sign-in owes a code; TOTP, recovery codes (single use), and replay of a
/// spent step is refused even when two requests race for it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_password_sign_in_owes_the_enrolled_second_factor() {
    with_app_key();
    let app = TestApp::setup().await;
    let email = "totp@flowcatalyst.test";
    seed_user(&app, email).await;
    let session = session_for(&app, email).await;
    let (secret, recovery) = enrol_totp(&app, &session).await;

    let (status, st) = read_json(app.get_with_session("/auth/2fa/status", &session).await).await;
    assert_eq!(status, StatusCode::OK, "{st}");
    assert_eq!(st["methods"], json!(["TOTP"]));
    assert_eq!(st["required"], false);
    assert_eq!(st["allowedMethods"], json!(["TOTP", "EMAIL_PIN"]));
    assert_eq!(st["recoveryCodesLeft"], 10);

    // The challenge.
    let token = pending_token(&app, email).await;
    let (_, body, _) = login(&app, email, None).await;
    assert_eq!(body["methods"], json!(["TOTP"]), "{body}");

    let (status, body) = read_json(
        post_public(
            &app,
            "/auth/2fa/verify",
            json!({ "mfaToken": token, "method": "TOTP", "code": "123456" }),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["code"], "UNAUTHENTICATED");

    let (status, body) = read_json(
        post_public(
            &app,
            "/auth/2fa/verify",
            json!({ "mfaToken": token, "method": "SMS", "code": "1" }),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "INVALID_METHOD");

    // Two requests racing with the same (current) code: exactly one signs in.
    let code = totp_code(&secret, chrono::Utc::now().timestamp()).unwrap();
    let token2 = pending_token(&app, email).await;
    let body_a = json!({ "mfaToken": token, "method": "TOTP", "code": code });
    let body_b = json!({ "mfaToken": token2, "method": "TOTP", "code": code });
    let (a, b) = tokio::join!(
        post_public(&app, "/auth/2fa/verify", body_a, None),
        post_public(&app, "/auth/2fa/verify", body_b, None)
    );
    let mut statuses = [a.status(), b.status()];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::UNAUTHORIZED],
        "one code, one sign-in"
    );
    let winner = if a.status() == StatusCode::OK { a } else { b };
    assert!(cookie_of(&winner, "fc_session").is_some());
    let (_, done) = read_json(winner).await;
    assert_eq!(done["status"], "ok", "{done}");
    assert_eq!(done["email"], email);
    assert!(done["clientId"].is_null(), "{done}");
    assert_eq!(done["ssoManaged"], false);

    // A recovery code signs in once, in any case or spacing.
    let typed = recovery[0].to_lowercase().replace('-', " ");
    let token = pending_token(&app, email).await;
    let resp = post_public(
        &app,
        "/auth/2fa/verify",
        json!({ "mfaToken": token, "method": "RECOVERY_CODE", "code": typed }),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let token = pending_token(&app, email).await;
    let resp = post_public(
        &app,
        "/auth/2fa/verify",
        json!({ "mfaToken": token, "method": "RECOVERY_CODE", "code": recovery[0] }),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "burned");

    // A step token is no session, and a session is no step token.
    let resp = app.get_with_session("/auth/2fa/status", &token).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let (status, _) = read_json(
        post_public(
            &app,
            "/auth/2fa/verify",
            json!({ "mfaToken": session, "method": "TOTP", "code": code }),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Recovery codes regenerate; the old set dies.
    let (status, body) = read_json(
        app.send_with_session(
            Method::POST,
            "/auth/2fa/recovery-codes/regenerate",
            &session,
            None::<()>,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["recoveryCodes"].as_array().unwrap().len(), 10);
    let audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM aud_logs WHERE entity_type = 'PRINCIPAL' \
         AND operation IN ('2FA_TOTP_ENROLLED', '2FA_RECOVERY_REGENERATED')",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audit, 2);
}

/// A domain that requires 2FA sends an unenrolled user to enrolment, with
/// only its allowed methods; enrolling finishes the sign-in with a first
/// recovery-code set (Go `maybeChallenge2FA`, `handle2FAEnroll*`,
/// `completeEnrollment`). The last factor can't then be removed.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_domain_requiring_two_factor_enrols_before_signing_in() {
    with_app_key();
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let idp = create_internal_idp(&app, &token, "internal").await;
    let (status, body) = create_mapping(
        &app,
        &token,
        "strict.test",
        &idp,
        json!({ "require2fa": true, "allowed2faMethods": ["TOTP"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let email = "enrol@strict.test";
    seed_user(&app, email).await;

    let (status, body, _) = login(&app, email, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "enrollment_required", "{body}");
    assert_eq!(body["allowedMethods"], json!(["TOTP"]));
    let enroll = body["enrollToken"].as_str().unwrap().to_string();

    let (status, body) = read_json(
        post_public(
            &app,
            "/auth/2fa/enroll/email/begin",
            json!({ "enrollToken": enroll }),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "METHOD_NOT_ALLOWED");

    let (status, begin) = read_json(
        post_public(
            &app,
            "/auth/2fa/enroll/totp/begin",
            json!({ "enrollToken": enroll }),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{begin}");
    let secret = begin["secret"].as_str().unwrap();
    let code = totp_code(secret, chrono::Utc::now().timestamp()).unwrap();
    let resp = post_public(
        &app,
        "/auth/2fa/enroll/totp/confirm",
        json!({ "enrollToken": enroll, "code": code }),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let session = cookie_of(&resp, "fc_session").expect("signed in");
    let (_, body) = read_json(resp).await;
    assert_eq!(body["status"], "ok", "{body}");
    assert_eq!(body["recoveryCodes"].as_array().unwrap().len(), 10);

    // Enrolment spent that code's step: it can't sign in again.
    let pending = pending_token(&app, email).await;
    let resp = post_public(
        &app,
        "/auth/2fa/verify",
        json!({ "mfaToken": pending, "method": "TOTP", "code": code }),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let (status, body) = read_json(
        app.send_with_session(
            Method::DELETE,
            "/auth/2fa/methods/TOTP",
            &session,
            None::<()>,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "LAST_FACTOR");
    let (status, st) = read_json(app.get_with_session("/auth/2fa/status", &session).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(st["required"], true);
    assert_eq!(st["allowedMethods"], json!(["TOTP"]));
}

/// The email factor: enrolment proves the inbox, the challenge mails a PIN,
/// the PIN signs in once; remember-this-device skips the next challenge
/// (Go `handle2FAChallengeEmail`, `rememberDevice`, trusted devices).
#[tokio::test]
#[ignore = "requires Docker"]
async fn email_codes_and_remembered_devices() {
    with_app_key();
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let idp = create_internal_idp(&app, &token, "internal").await;
    let (status, body) = create_mapping(
        &app,
        &token,
        "mail.test",
        &idp,
        json!({ "rememberDeviceEnabled": true, "rememberDeviceDays": 7 }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let email = "pin@mail.test";
    let user = seed_user(&app, email).await;
    let mfa = MfaRepository::new(&app.pool);
    let known_pin = |purpose| {
        let mfa = &mfa;
        let id = user.id.clone();
        async move {
            mfa.replace_email_pin(
                &id,
                purpose,
                &fc_platform::mfa::crypto::sha256_hex("424242"),
                chrono::Utc::now() + chrono::Duration::minutes(10),
            )
            .await
            .unwrap();
        }
    };

    let session = session_for(&app, email).await;
    let (status, body) = read_json(
        app.send_with_session(
            Method::POST,
            "/auth/2fa/methods/email/begin",
            &session,
            None::<()>,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    known_pin(EmailPinPurpose::Enroll).await;
    let (status, body) = read_json(
        app.send_with_session(
            Method::POST,
            "/auth/2fa/methods/email/confirm",
            &session,
            Some(json!({ "code": " 424242 " })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["recoveryCodes"],
        json!([]),
        "email is its own recovery"
    );

    let pending = pending_token(&app, email).await;
    let (status, body) = read_json(
        post_public(
            &app,
            "/auth/2fa/challenge/email",
            json!({ "mfaToken": pending }),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["message"],
        "A verification code has been sent to your email."
    );
    known_pin(EmailPinPurpose::Login).await;
    let resp = post_public(
        &app,
        "/auth/2fa/verify",
        json!({ "mfaToken": pending, "method": "EMAIL_PIN", "code": "424242", "rememberDevice": true }),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let device = cookie_of(&resp, "fc_td").expect("remembered");
    // The PIN is spent.
    let resp = post_public(
        &app,
        "/auth/2fa/verify",
        json!({ "mfaToken": pending, "method": "EMAIL_PIN", "code": "424242" }),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // The remembered browser signs straight in.
    let (status, body, _) = login(&app, email, Some(&format!("fc_td={device}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok", "{body}");

    let (status, list) = read_json(
        app.get_with_session("/auth/2fa/trusted-devices", &session)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let devices = list["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0]["label"], "TwoFactorTest/1.0");
    let id = devices[0]["id"].as_str().unwrap();
    let resp = app
        .send_with_session(
            Method::DELETE,
            &format!("/auth/2fa/trusted-devices/{id}"),
            &session,
            None::<()>,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (_, body, _) = login(&app, email, Some(&format!("fc_td={device}"))).await;
    assert_eq!(body["status"], "mfa_required", "revoked: {body}");

    // Removing the factor (not required here) turns the challenge off.
    let resp = app
        .send_with_session(
            Method::DELETE,
            "/auth/2fa/methods/EMAIL_PIN",
            &session,
            None::<()>,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (_, body, _) = login(&app, email, None).await;
    assert_eq!(body["status"], "ok", "{body}");
}
