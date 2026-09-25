//! Auth and session security fixes (Java security sweep S2–S15 and the
//! owner rulings of 2026-09-25), end to end against a real database.

#[path = "support/mod.rs"]
mod support;

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;

use fc_platform::email_domain_mapping::entity::{EmailDomainMapping, ScopeType};
use support::{read_json, TestApp};

/// Send a hand-built request through the full router.
async fn send(app: &TestApp, req: Request<Body>) -> Response<Body> {
    app.router.clone().oneshot(req).await.expect("oneshot")
}

/// A multi-tenant (Entra-style) OIDC identity provider, created through the
/// API; returns its id.
async fn create_oidc_idp(app: &TestApp, token: &str, code: &str, multi_tenant: bool) -> String {
    let (status, body) = read_json(
        app.post(
            "/api/identity-providers",
            token,
            json!({
                "code": code,
                "name": code,
                "type": "OIDC",
                "oidcIssuerUrl": "https://login.microsoftonline.com/organizations/v2.0",
                "oidcClientId": "our-client",
                "oidcMultiTenant": multi_tenant,
                "oidcIssuerPattern": "^https://login\\.microsoftonline\\.com/[^/]+/v2\\.0$"
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_string()
}

fn mapping_body(domain: &str, idp_id: &str, pin: Option<&str>) -> Value {
    let mut body = json!({
        "emailDomain": domain,
        "identityProviderId": idp_id,
        "scopeType": "ANCHOR"
    });
    if let Some(pin) = pin {
        body["requiredOidcTenantId"] = json!(pin);
    }
    body
}

/// Owner ruling 2026-09-25, item 3(a) (Java ecb622fe): a mapping to a
/// multi-tenant provider must pin the tenant, on create, on update and on a
/// move; and a provider cannot become multi-tenant while one of its
/// mappings pins nothing.
#[tokio::test]
#[ignore = "requires Docker"]
async fn multi_tenant_mappings_must_pin_the_tenant_on_save() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let multi = create_oidc_idp(&app, &token, "entra-multi", true).await;
    let single = create_oidc_idp(&app, &token, "entra-single", false).await;

    // Create: unpinned (absent or blank) is refused; pinned is accepted.
    for pin in [None, Some(""), Some("  ")] {
        let (status, body) = read_json(
            app.post(
                "/api/email-domain-mappings",
                &token,
                mapping_body("acme.test", &multi, pin),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{pin:?}: {body}");
        assert_eq!(body["code"], "TENANT_PIN_REQUIRED", "{body}");
    }
    let (status, body) = read_json(
        app.post(
            "/api/email-domain-mappings",
            &token,
            mapping_body("acme.test", &multi, Some("tenant-a")),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let pinned_id = body["id"].as_str().unwrap().to_string();

    // Update: clearing the pin is refused.
    let (status, body) = read_json(
        app.put(
            &format!("/api/email-domain-mappings/{pinned_id}"),
            &token,
            json!({ "requiredOidcTenantId": "" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "TENANT_PIN_REQUIRED", "{body}");

    // A single-tenant provider needs no pin...
    let (status, body) = read_json(
        app.post(
            "/api/email-domain-mappings",
            &token,
            mapping_body("beta.test", &single, None),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unpinned_id = body["id"].as_str().unwrap().to_string();

    // ...but moving that unpinned mapping to the multi-tenant one is refused.
    let (status, body) = read_json(
        app.put(
            &format!("/api/email-domain-mappings/{unpinned_id}"),
            &token,
            json!({ "identityProviderId": multi }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "TENANT_PIN_REQUIRED", "{body}");

    // Switching the single-tenant provider to multi-tenant is refused while
    // its unpinned mapping exists.
    let (status, body) = read_json(
        app.put(
            &format!("/api/identity-providers/{single}"),
            &token,
            json!({ "oidcMultiTenant": true }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "TENANT_PIN_REQUIRED", "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("beta.test"),
        "{body}"
    );

    // Once the mapping pins its tenant, the switch goes through.
    let resp = app
        .put(
            &format!("/api/email-domain-mappings/{unpinned_id}"),
            &token,
            json!({ "requiredOidcTenantId": "tenant-b" }),
        )
        .await;
    assert!(resp.status().is_success(), "{}", resp.status());
    let resp = app
        .put(
            &format!("/api/identity-providers/{single}"),
            &token,
            json!({ "oidcMultiTenant": true }),
        )
        .await;
    assert!(resp.status().is_success(), "{}", resp.status());
}

fn oidc_login_request(domain: &str) -> Request<Body> {
    Request::builder()
        .uri(format!("/auth/oidc/login?domain={domain}"))
        .header("host", "platform.test")
        .body(Body::empty())
        .unwrap()
}

/// Owner ruling 2026-09-25, item 3(b): a mapping saved before the rule, to
/// a multi-tenant provider and pinning no tenant, is refused at login with
/// 403 `TENANT_NOT_PINNED`; a pinned one proceeds to the provider.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_unpinned_multi_tenant_mapping_cannot_sign_in() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let multi = create_oidc_idp(&app, &token, "entra-multi", true).await;

    // A legacy row, written straight to the table as the rule never saw it.
    let legacy = EmailDomainMapping::new("legacy.test", &multi, ScopeType::Anchor);
    app.repos.edm_repo.insert(&legacy).await.unwrap();

    let (status, body) = read_json(send(&app, oidc_login_request("legacy.test")).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "TENANT_NOT_PINNED", "{body}");

    let mut pinned = EmailDomainMapping::new("pinned.test", &multi, ScopeType::Anchor);
    pinned.required_oidc_tenant_id = Some("tenant-a".to_string());
    app.repos.edm_repo.insert(&pinned).await.unwrap();
    let resp = send(&app, oidc_login_request("pinned.test")).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
}

/// Owner ruling 2026-09-25, item 8 (Java 93367448): an unmapped domain is a
/// 404 carrying `EMAIL_DOMAIN_NOT_MAPPED`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_unmapped_domain_is_404_email_domain_not_mapped() {
    let app = TestApp::setup().await;
    let (status, body) = read_json(send(&app, oidc_login_request("nowhere.test")).await).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "EMAIL_DOMAIN_NOT_MAPPED", "{body}");
    assert!(body["error"].as_str().unwrap().contains("nowhere.test"));
}
