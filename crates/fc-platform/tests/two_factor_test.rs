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
