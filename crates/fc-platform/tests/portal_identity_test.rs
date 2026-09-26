//! The portal identity plane against Go's wire contract (Go
//! `portalidentity/api`, `portalauth`, the bridge's portal hooks, the portal
//! branch of the reset-token and token endpoints): happy paths, permission
//! and validation errors, events and audit rows.
//!
//! Run with `cargo test -p fc-platform --test portal_identity_test -- --ignored`.

#[path = "support/mod.rs"]
mod support;

use std::sync::{Arc, Once};

use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use fc_platform::domain::{Principal, UserScope};
use fc_platform::identity_provider::entity::{IdentityProvider, IdentityProviderType};
use fc_platform::role::entity::roles;
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform::shared::encryption_service::EncryptionService;
use fc_platform::shared::rate_limit_store::PostgresRateLimitStore;
use fc_platform::Client;
use support::{assert_status, read_json, TestApp};

static APP_KEY: Once = Once::new();

/// CONFIDENTIAL portal apps hash a generated secret: every test binary run
/// uses one key.
fn set_app_key() {
    APP_KEY
        .call_once(|| std::env::set_var("FLOWCATALYST_APP_KEY", EncryptionService::generate_key()));
}

async fn setup() -> TestApp {
    set_app_key();
    TestApp::setup().await
}

async fn client(app: &TestApp, identifier: &str) -> String {
    let c = Client::new(format!("Client {identifier}"), identifier);
    app.repos
        .client_repo
        .insert(&c)
        .await
        .expect("insert client");
    c.id
}

/// A client-scoped user holding `platform:portal-administrator` for `client_id`.
async fn portal_admin_token(app: &TestApp, client_id: &str) -> String {
    let role = roles::portal_administrator();
    if app
        .repos
        .role_repo
        .find_by_name(&role.name)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        app.repos
            .role_repo
            .insert(&role)
            .await
            .expect("insert role");
    }
    let mut p = Principal::new_user("padmin@flowcatalyst.test", UserScope::Client)
        .with_client_id(client_id);
    p.roles = vec![RoleAssignment::new(role.name.clone())];
    app.auth_service.generate_access_token(&p).expect("token")
}

async fn send_raw(app: &TestApp, req: Request<Body>) -> Response<Body> {
    app.router.clone().oneshot(req).await.expect("oneshot")
}

async fn post_public(app: &TestApp, path: &str, body: Value) -> Response<Body> {
    send_raw(
        app,
        Request::post(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
}

fn location(resp: &Response<Body>) -> String {
    resp.headers()
        .get(header::LOCATION)
        .expect("location header")
        .to_str()
        .unwrap()
        .to_string()
}

fn query_param(url: &str, name: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(&if url.starts_with('/') {
        format!("http://localhost{url}")
    } else {
        url.to_string()
    })
    .ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.to_string())
}

fn jwt_payload(token: &str) -> Value {
    let payload = token.split('.').nth(1).expect("jwt payload");
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .expect("b64");
    serde_json::from_slice(&bytes).expect("json")
}

const VERIFIER: &str = "portal-test-verifier-0123456789-abcdefghijklmnopqrstuvwxyz";

fn challenge() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(VERIFIER.as_bytes()))
}

/// Start a flow at /portal/authorize; returns the flow id.
async fn authorize(
    app: &TestApp,
    oauth_client_id: &str,
    redirect_uri: &str,
    state: &str,
) -> String {
    let uri = format!(
        "/portal/authorize?response_type=code&client_id={}&redirect_uri={}&state={}&scope=openid%20profile%20email&code_challenge={}&code_challenge_method=S256&nonce=n-1",
        oauth_client_id,
        urlencoding::encode(redirect_uri),
        state,
        challenge()
    );
    let resp = send_raw(app, Request::get(uri).body(Body::empty()).unwrap()).await;
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    let loc = location(&resp);
    assert!(loc.starts_with("/portal/login?flow="), "{loc}");
    query_param(&loc, "flow").expect("flow id")
}

async fn create_app(
    app: &TestApp,
    token: &str,
    client_id: &str,
    code: &str,
    body_extra: Value,
) -> Value {
    let mut body = json!({
        "clientId": client_id,
        "code": code,
        "name": format!("{code} portal"),
        "clientType": "PUBLIC",
        "redirectUris": [format!("https://{code}.example.com/callback")],
    });
    if let (Some(b), Some(extra)) = (body.as_object_mut(), body_extra.as_object()) {
        for (k, v) in extra {
            b.insert(k.clone(), v.clone());
        }
    }
    assert_status(
        app.post("/api/portal-apps", token, body).await,
        StatusCode::CREATED,
    )
    .await
}

async fn ensure(app: &TestApp, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(app.post("/api/portal-users", token, body).await).await
}

/// Set the identity's password through its invite link.
async fn set_password(app: &TestApp, invite_url: &str, password: &str) -> Value {
    let token = query_param(invite_url, "token").expect("invite token");
    assert_status(
        post_public(
            app,
            "/auth/password-reset/confirm",
            json!({ "token": token, "password": password }),
        )
        .await,
        StatusCode::OK,
    )
    .await
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn portal_users_admin_surface_follows_go() {
    let app = setup().await;
    let anchor = app.anchor_admin_token().await;
    let client_id = client(&app, "portal-users").await;
    let other_client = client(&app, "portal-other").await;

    // Validation and permissions.
    // Go's huma answers a missing required member.
    let (status, body) = ensure(&app, &anchor, json!({ "email": "a@example.com" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "VALIDATION");
    let (status, body) = ensure(
        &app,
        &anchor,
        json!({ "clientId": "", "email": "a@example.com" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "CLIENT_ID_REQUIRED");
    let plain = app.client_user_token(&client_id);
    let (status, body) = ensure(
        &app,
        &plain,
        json!({ "clientId": client_id, "email": "a@example.com" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // A role-less user is stopped by Go's profile-only gate first.
    assert_eq!(body["error"], "NO_PLATFORM_ROLE");
    let padmin = portal_admin_token(&app, &client_id).await;
    let (status, body) = ensure(
        &app,
        &padmin,
        json!({ "clientId": other_client, "email": "a@example.com" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "SCOPE_FORBIDDEN");
    let (status, body) = ensure(
        &app,
        &anchor,
        json!({ "clientId": client_id, "email": "not-an-email" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "EMAIL_INVALID");
    let (status, body) = ensure(
        &app,
        &anchor,
        json!({ "clientId": "clt_0000000000000", "email": "a@example.com" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "Client_NOT_FOUND");
    let (status, body) = ensure(
        &app,
        &anchor,
        json!({ "clientId": client_id, "email": "a@example.com", "redirectUri": "https://evil.example.com/cb" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "REDIRECT_URI_INVALID");

    // Ensure (mailed invite), then again: idempotent.
    let (status, first) = ensure(
        &app,
        &padmin,
        json!({ "clientId": client_id, "email": "Pat.Jones@Example.com", "name": "Pat Jones" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["created"], true);
    assert_eq!(first["invited"], true);
    assert_eq!(first["hasPassword"], false);
    assert_eq!(first["state"], "INVITED");
    assert!(first.get("ssoManaged").is_none());
    let identity_id = first["identityId"].as_str().unwrap().to_string();
    assert!(identity_id.starts_with("ptu_"));
    let (_, again) = ensure(
        &app,
        &padmin,
        json!({ "clientId": client_id, "email": "pat.jones@example.com" }),
    )
    .await;
    assert_eq!(again["identityId"], identity_id);
    assert_eq!(again["created"], false);
    assert_eq!(
        app.event_count_by_type("platform:portal:identity:ensured")
            .await,
        2
    );
    assert_eq!(app.audit_count_for(&identity_id).await, 2);

    // returnInviteLink: the set-password link comes back instead.
    let (_, linked) = ensure(
        &app,
        &anchor,
        json!({ "clientId": client_id, "email": "jonas@example.com", "name": "Jonas Smith", "returnInviteLink": true }),
    )
    .await;
    let invite_url = linked["inviteUrl"].as_str().unwrap();
    assert!(
        invite_url.contains("/auth/set-password?token="),
        "{invite_url}"
    );
    assert_eq!(linked["invited"], false);

    // List, search, filters.
    let body = assert_status(
        app.get(&format!("/api/portal-users?clientId={client_id}"), &padmin)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["total"], 2);
    assert_eq!(body["page"], 0);
    assert_eq!(body["size"], 100);
    let row = &body["portalUsers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["identityId"] == identity_id.as_str())
        .unwrap()
        .clone();
    assert_eq!(row["email"], "pat.jones@example.com");
    assert_eq!(row["status"], "ACTIVE");
    assert_eq!(row["source"], "INVITE");
    assert!(row["invitedAt"].is_string() && row["inviteExpiresAt"].is_string());
    let body = assert_status(
        app.get(
            &format!("/api/portal-users?clientId={client_id}&q=JON"),
            &anchor,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["total"], 1, "prefix on name, case-insensitive: {body}");
    let (status, body) = read_json(app.get("/api/portal-users", &anchor).await).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "CLIENT_ID_REQUIRED");
    let (status, _) = read_json(
        app.get(&format!("/api/portal-users?clientId={client_id}"), &plain)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Suspend / reactivate.
    let (status, body) = read_json(
        app.post(
            &format!("/api/portal-users/{identity_id}/deactivate"),
            &padmin,
            json!({}),
        )
        .await,
    )
    .await;
    // Go's huma answers the missing required member.
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "VALIDATION");
    let body = assert_status(
        app.post(
            &format!("/api/portal-users/{identity_id}/deactivate"),
            &padmin,
            json!({ "clientId": client_id }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body, json!({ "message": "Portal user deactivated" }));
    let body = assert_status(
        app.get(
            &format!("/api/portal-users?clientId={client_id}&q=pat"),
            &padmin,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["portalUsers"][0]["state"], "SUSPENDED");
    assert_eq!(body["portalUsers"][0]["status"], "DISABLED");
    let body = assert_status(
        app.post(
            &format!("/api/portal-users/{identity_id}/activate"),
            &padmin,
            json!({ "clientId": client_id }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body, json!({ "message": "Portal user activated" }));
    assert_eq!(
        app.event_count_by_type("platform:portal:identity:status-set")
            .await,
        2
    );
    // Another client's gate cannot reach the identity.
    let (status, body) = read_json(
        app.post(
            &format!("/api/portal-users/{identity_id}/activate"),
            &anchor,
            json!({ "clientId": other_client }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "PortalIdentity_NOT_FOUND");

    // Delete, then again.
    let (status, _) = read_json(
        app.delete(&format!("/api/portal-users/{identity_id}"), &padmin)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body = assert_status(
        app.delete(
            &format!("/api/portal-users/{identity_id}?clientId={client_id}"),
            &padmin,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body, json!({ "message": "Portal user deleted" }));
    let (status, body) = read_json(
        app.delete(
            &format!("/api/portal-users/{identity_id}?clientId={client_id}"),
            &padmin,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "PortalIdentity_NOT_FOUND");
    assert_eq!(
        app.event_count_by_type("platform:portal:identity:deleted")
            .await,
        1
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn portal_apps_admin_surface_follows_go() {
    let app = setup().await;
    let anchor = app.anchor_admin_token().await;
    let client_id = client(&app, "portal-apps").await;
    let padmin = portal_admin_token(&app, &client_id).await;

    // Create: CONFIDENTIAL by default, with a one-time secret.
    let created = assert_status(
        app.post(
            "/api/portal-apps",
            &padmin,
            json!({
                "clientId": client_id, "code": "Customer-Portal", "name": "Customer Portal",
                "description": "the confidential app",
                "redirectUris": ["https://customers.example.com/callback"],
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(created["portalApp"]["code"], "customer-portal");
    assert_eq!(created["clientType"], "CONFIDENTIAL");
    assert!(created["clientSecret"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    assert_eq!(
        created["portalApp"]["oauthClients"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(created["portalApp"]["userCount"], 0);
    let app1 = created["portalApp"]["id"].as_str().unwrap().to_string();
    assert!(app1.starts_with("pta_"));
    let oc_row = created["oauthClientRowId"].as_str().unwrap().to_string();
    let oc = app
        .repos
        .oauth_client_repo
        .find_by_id(&oc_row)
        .await
        .unwrap()
        .expect("oauth client");
    assert_eq!(oc.portal_client_id.as_deref(), Some(client_id.as_str()));
    assert_eq!(oc.portal_app_id.as_deref(), Some(app1.as_str()));
    assert!(oc.pkce_required);
    assert_eq!(oc.client_name, "Customer Portal (portal)");
    assert_eq!(
        app.event_count_by_type("platform:portal:app:created").await,
        1
    );
    assert_eq!(
        app.event_count_by_type("platform:admin:oauth-client:created")
            .await,
        1
    );

    // Validation.
    for (body, code, status) in [
        (
            json!({ "clientId": client_id, "code": "CUSTOMER-PORTAL", "name": "Dup" }),
            "CODE_EXISTS",
            StatusCode::CONFLICT,
        ),
        (
            json!({ "clientId": client_id, "code": "w", "name": "W", "redirectUris": ["https://*.example.com/cb"] }),
            "REDIRECT_URI_INVALID",
            StatusCode::BAD_REQUEST,
        ),
        (
            // Go's huma enum check.
            json!({ "clientId": client_id, "code": "t", "name": "T", "clientType": "PARTNER" }),
            "VALIDATION",
            StatusCode::BAD_REQUEST,
        ),
        (
            // A missing member is huma's required-property check…
            json!({ "clientId": client_id, "code": "noname" }),
            "VALIDATION",
            StatusCode::BAD_REQUEST,
        ),
        (
            // …a blank one the use case's.
            json!({ "clientId": client_id, "code": "noname", "name": " " }),
            "NAME_REQUIRED",
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "clientId": client_id, "code": "-leading-dash", "name": "Bad" }),
            "CODE_INVALID",
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "code": "x", "name": "X" }),
            "VALIDATION",
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "clientId": "", "code": "x", "name": "X" }),
            "CLIENT_ID_REQUIRED",
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (got, resp) = read_json(app.post("/api/portal-apps", &padmin, body).await).await;
        assert_eq!(
            (got, resp["error"].as_str().unwrap()),
            (status, code),
            "{resp}"
        );
    }

    let public = create_app(&app, &padmin, &client_id, "suppliers", json!({})).await;
    assert_eq!(public["clientType"], "PUBLIC");
    assert!(public.get("clientSecret").is_none());
    let app2 = public["portalApp"]["id"].as_str().unwrap().to_string();

    // List.
    let body = assert_status(
        app.get(&format!("/api/portal-apps?clientId={client_id}"), &padmin)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["portalApps"].as_array().unwrap().len(), 2);
    assert_eq!(body["unassignedUsers"], 0);
    let (status, body) = read_json(app.get("/api/portal-apps", &padmin).await).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "CLIENT_ID_REQUIRED");
    let body = assert_status(app.get("/api/portal-apps", &anchor).await, StatusCode::OK).await;
    assert!(body.get("unassignedUsers").is_none());

    // Update.
    let body = assert_status(
        app.put(
            &format!("/api/portal-apps/{app2}"),
            &padmin,
            json!({ "clientId": client_id, "name": "Supplier Portal" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["name"], "Supplier Portal");
    let (status, body) = read_json(
        app.put(
            &format!("/api/portal-apps/{app2}"),
            &padmin,
            json!({ "clientId": client_id, "name": "  " }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "NAME_REQUIRED")
    );
    let (status, body) = read_json(
        app.put(
            "/api/portal-apps/pta_doesnotexist0",
            &padmin,
            json!({ "clientId": client_id, "name": "N" }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::NOT_FOUND, "PortalApp_NOT_FOUND")
    );

    // Grants and bulk assignment.
    let (_, u1) = ensure(
        &app,
        &padmin,
        json!({ "clientId": client_id, "email": "one@example.com", "returnInviteLink": true }),
    )
    .await;
    let (_, u2) = ensure(
        &app,
        &padmin,
        json!({ "clientId": client_id, "email": "two@example.com", "returnInviteLink": true }),
    )
    .await;
    let (_, u3) = ensure(&app, &padmin, json!({ "clientId": client_id, "email": "three@example.com", "portalAppCode": "SUPPLIERS", "returnInviteLink": true })).await;
    assert_eq!(u3["portalAppCode"], "suppliers");
    let (status, body) = ensure(
        &app,
        &padmin,
        json!({ "clientId": client_id, "email": "x@example.com", "portalAppCode": "nope" }),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::NOT_FOUND, "PortalApp_NOT_FOUND")
    );
    let body = assert_status(
        app.get(
            &format!("/api/portal-users?clientId={client_id}&unassigned=true"),
            &padmin,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["total"], 2);
    let (status, body) = read_json(
        app.get(
            &format!(
                "/api/portal-users?clientId={client_id}&unassigned=true&portalAppCode=suppliers"
            ),
            &padmin,
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "FILTER_CONFLICT")
    );
    let body = assert_status(
        app.get(
            &format!("/api/portal-users?clientId={client_id}&portalAppCode=suppliers"),
            &padmin,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["portalUsers"][0]["apps"][0]["code"], "suppliers");
    assert_eq!(body["portalUsers"][0]["apps"][0]["source"], "INVITE");

    let u1_id = u1["identityId"].as_str().unwrap();
    let body = assert_status(
        app.post(
            &format!("/api/portal-users/{u1_id}/apps"),
            &padmin,
            json!({ "clientId": client_id, "portalAppCode": "customer-portal" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body, json!({ "message": "Portal app access granted" }));
    assert_eq!(
        app.event_count_by_type("platform:portal:identity:app-granted")
            .await,
        1
    );
    let body = assert_status(
        app.delete(
            &format!("/api/portal-users/{u1_id}/apps/customer-portal?clientId={client_id}"),
            &padmin,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body, json!({ "message": "Portal app access revoked" }));
    assert_eq!(
        app.event_count_by_type("platform:portal:identity:app-revoked")
            .await,
        1
    );

    let body = assert_status(
        app.post(
            &format!("/api/portal-apps/{app1}/assign-unassigned"),
            &padmin,
            json!({ "clientId": client_id }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        body,
        json!({ "portalAppCode": "customer-portal", "assigned": 2 })
    );
    let body = assert_status(
        app.post(
            &format!("/api/portal-apps/{app1}/assign-unassigned"),
            &padmin,
            json!({ "clientId": client_id }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        body,
        json!({ "portalAppCode": "customer-portal", "assigned": 0 })
    );
    assert_eq!(
        app.event_count_by_type("platform:portal:identity:app-granted")
            .await,
        3
    );
    let (status, body) = read_json(
        app.post(
            &format!("/api/portal-apps/{app1}/assign-unassigned"),
            &padmin,
            json!({ "clientId": "" }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "CLIENT_ID_REQUIRED")
    );
    assert_status(
        app.put(
            &format!("/api/portal-apps/{app2}"),
            &padmin,
            json!({ "clientId": client_id, "active": false }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let (status, body) = read_json(
        app.post(
            &format!("/api/portal-apps/{app2}/assign-unassigned"),
            &padmin,
            json!({ "clientId": client_id }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "PORTAL_APP_INACTIVE")
    );
    let u2_id = u2["identityId"].as_str().unwrap();
    let (status, body) = read_json(
        app.post(
            &format!("/api/portal-users/{u2_id}/apps"),
            &padmin,
            json!({ "clientId": client_id, "portalAppCode": "suppliers" }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "PORTAL_APP_INACTIVE")
    );
    let body = assert_status(
        app.get(&format!("/api/portal-apps?clientId={client_id}"), &padmin)
            .await,
        StatusCode::OK,
    )
    .await;
    let customer = body["portalApps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == app1.as_str())
        .unwrap()
        .clone();
    assert_eq!(customer["userCount"], 2);

    // Delete takes its OAuth client with it.
    let body = assert_status(
        app.delete(
            &format!("/api/portal-apps/{app1}?clientId={client_id}"),
            &padmin,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        body,
        json!({ "message": "Portal app deleted with its OAuth client" })
    );
    assert!(app
        .repos
        .oauth_client_repo
        .find_by_id(&oc_row)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        app.event_count_by_type("platform:portal:app:deleted").await,
        1
    );
    assert_eq!(
        app.event_count_by_type("platform:admin:oauth-client:deleted")
            .await,
        1
    );
    let (status, _) = read_json(
        app.delete(
            &format!("/api/portal-apps/{app1}?clientId={client_id}"),
            &padmin,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = read_json(
        app.delete(&format!("/api/portal-apps/{app2}"), &padmin)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn portal_password_login_issues_a_portal_code() {
    let app = setup().await;
    let anchor = app.anchor_admin_token().await;
    let client_id = client(&app, "portal-login").await;
    let gate_a = create_app(&app, &anchor, &client_id, "gate-a", json!({})).await;
    let gate_b = create_app(&app, &anchor, &client_id, "gate-b", json!({})).await;
    let (a_client, b_client) = (
        gate_a["oauthClientId"].as_str().unwrap().to_string(),
        gate_b["oauthClientId"].as_str().unwrap().to_string(),
    );
    let (a_redirect, b_redirect) = (
        "https://gate-a.example.com/callback",
        "https://gate-b.example.com/callback",
    );

    // Invite through app A; the default post-set-password redirect is A's origin.
    let (_, user) = ensure(
        &app,
        &anchor,
        json!({ "clientId": client_id, "email": "portal.user@example.com", "portalAppCode": "gate-a", "returnInviteLink": true }),
    )
    .await;
    let identity_id = user["identityId"].as_str().unwrap().to_string();
    let invite_url = user["inviteUrl"].as_str().unwrap().to_string();
    let token = query_param(&invite_url, "token").unwrap();

    // The shared validate/confirm endpoints answer portal tokens.
    let body = assert_status(
        send_raw(
            &app,
            Request::get(format!("/auth/password-reset/validate?token={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        body,
        json!({ "valid": true, "reason": null, "requiresFactor": false, "portal": true })
    );
    let (status, body) = read_json(
        post_public(
            &app,
            "/auth/password-reset/confirm",
            json!({ "token": token, "password": "password" }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "PASSWORD_TOO_COMMON")
    );
    let body = set_password(&app, &invite_url, "Correct-Gate-Battery-4471").await;
    assert_eq!(
        body,
        json!({ "status": "ok", "message": "Password set successfully.", "portal": true, "redirectUri": "https://gate-a.example.com/" })
    );
    let (status, _) = read_json(
        post_public(
            &app,
            "/auth/password-reset/confirm",
            json!({ "token": token, "password": "Another-Battery-9931" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "the token is burned");

    // /portal/authorize validation.
    let bad = |q: &str| {
        Request::get(format!("/portal/authorize?{q}"))
            .body(Body::empty())
            .unwrap()
    };
    let (status, body) =
        read_json(send_raw(&app, bad(&format!("client_id={a_client}"))).await).await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "invalid_request")
    );
    let (status, body) = read_json(
        send_raw(
            &app,
            bad("state=s&client_id=nope&redirect_uri=x&response_type=code"),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "unauthorized_client")
    );
    let resp = send_raw(
        &app,
        bad(&format!(
            "state=s&client_id={a_client}&redirect_uri={}&response_type=token",
            urlencoding::encode(a_redirect)
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert!(location(&resp).contains("error=unsupported_response_type"));
    // The platform's /oauth/authorize refuses portal clients.
    let (status, body) = read_json(
        app.get_unauth(&format!(
            "/oauth/authorize?response_type=code&client_id={a_client}&redirect_uri={}&state=s&code_challenge={}",
            urlencoding::encode(a_redirect),
            challenge()
        ))
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "unauthorized_client")
    );

    let flow = authorize(&app, &a_client, a_redirect, "st-a").await;
    let body = assert_status(
        post_public(
            &app,
            "/portal/auth/check-domain",
            json!({ "flowId": flow, "email": "portal.user@example.com" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body, json!({ "method": "PASSWORD" }));
    let (status, body) = read_json(
        post_public(
            &app,
            "/portal/auth/check-domain",
            json!({ "flowId": "not-a-flow", "email": "portal.user@example.com" }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "FLOW_EXPIRED")
    );

    let (status, body) = read_json(
        post_public(
            &app,
            "/portal/auth/login",
            json!({ "flowId": flow, "email": "portal.user@example.com", "password": "wrong" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        body,
        json!({ "code": "INVALID_CREDENTIALS", "message": "Invalid email or password" })
    );
    let body = assert_status(
        post_public(&app, "/portal/auth/login", json!({ "flowId": flow, "email": "portal.user@example.com", "password": "Correct-Gate-Battery-4471" })).await,
        StatusCode::OK,
    )
    .await;
    let redirect_url = body["redirectUrl"].as_str().unwrap().to_string();
    assert!(redirect_url.starts_with(a_redirect));
    assert_eq!(query_param(&redirect_url, "state").as_deref(), Some("st-a"));
    let code = query_param(&redirect_url, "code").unwrap();
    let (status, body) = read_json(
        post_public(&app, "/portal/auth/login", json!({ "flowId": flow, "email": "portal.user@example.com", "password": "Correct-Gate-Battery-4471" })).await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "FLOW_EXPIRED")
    );

    // The code redeems at /oauth/token into portal-identity tokens.
    let redeem = |code: String, client: String, redirect: &'static str| {
        let form = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            urlencoding::encode(&code),
            urlencoding::encode(redirect),
            client,
            VERIFIER
        );
        Request::post("/oauth/token")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(form))
            .unwrap()
    };
    let tokens = assert_status(
        send_raw(&app, redeem(code, a_client.clone(), a_redirect)).await,
        StatusCode::OK,
    )
    .await;
    assert!(tokens.get("refresh_token").is_none());
    let id_token = jwt_payload(tokens["id_token"].as_str().unwrap());
    assert_eq!(id_token["sub"], identity_id);
    assert_eq!(id_token["email"], "portal.user@example.com");
    assert_eq!(id_token["portal_client_id"], client_id);
    assert_eq!(id_token["portal_app_code"], "gate-a");
    assert_eq!(id_token["roles"], json!([]));
    assert_eq!(id_token["nonce"], "n-1");

    // App gate: B needs its own grant.
    let flow_b = authorize(&app, &b_client, b_redirect, "st-b").await;
    let (status, body) = read_json(
        post_public(&app, "/portal/auth/login", json!({ "flowId": flow_b, "email": "portal.user@example.com", "password": "Correct-Gate-Battery-4471" })).await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "NO_PORTAL_ACCESS");
    assert_status(
        app.post(
            &format!("/api/portal-users/{identity_id}/apps"),
            &anchor,
            json!({ "clientId": client_id, "portalAppCode": "gate-b" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let body = assert_status(
        post_public(&app, "/portal/auth/login", json!({ "flowId": flow_b, "email": "portal.user@example.com", "password": "Correct-Gate-Battery-4471" })).await,
        StatusCode::OK,
    )
    .await;
    let code_b = query_param(body["redirectUrl"].as_str().unwrap(), "code").unwrap();
    // A revocation between issuance and redemption bites at the token endpoint.
    assert_status(
        app.delete(
            &format!("/api/portal-users/{identity_id}/apps/gate-b?clientId={client_id}"),
            &anchor,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let (status, body) =
        read_json(send_raw(&app, redeem(code_b, b_client.clone(), b_redirect)).await).await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "invalid_grant")
    );

    // Forgot-password: silent success; a known identity gets a reset token.
    let flow_r = authorize(&app, &a_client, a_redirect, "st-r").await;
    for email in ["portal.user@example.com", "nobody@example.com"] {
        let body = assert_status(
            post_public(
                &app,
                "/portal/auth/password-reset",
                json!({ "flowId": flow_r, "email": email }),
            )
            .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(
            body,
            json!({ "message": "If an account exists, a reset email has been sent." })
        );
    }
    let (resets,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM iam_password_reset_tokens WHERE principal_id = $1 AND purpose = 'reset' \
         AND redirect_uri = 'https://gate-a.example.com/'",
    )
    .bind(&identity_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(resets, 1);

    // The portal SSO start: parameters, unknown provider, burned flow.
    let get = |uri: String| Request::get(uri).body(Body::empty()).unwrap();
    let (status, body) =
        read_json(send_raw(&app, get("/portal/auth/oidc/login".into())).await).await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "MISSING_PARAM")
    );
    let (status, body) = read_json(
        send_raw(
            &app,
            get(format!(
                "/portal/auth/oidc/login?flow={flow_r}&provider_id=idp_nope"
            )),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::INTERNAL_SERVER_ERROR, "OIDC_RESOLVE_FAILED")
    );
    let (status, body) = read_json(
        send_raw(
            &app,
            get(format!(
                "/portal/auth/oidc/login?flow={flow_r}&provider_id=idp_nope"
            )),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "FLOW_EXPIRED")
    );

    // A suspended identity cannot sign in (uniform refusal).
    assert_status(
        app.post(
            &format!("/api/portal-users/{identity_id}/deactivate"),
            &anchor,
            json!({ "clientId": client_id }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let flow_s = authorize(&app, &a_client, a_redirect, "st-s").await;
    let (status, body) = read_json(
        post_public(&app, "/portal/auth/login", json!({ "flowId": flow_s, "email": "portal.user@example.com", "password": "Correct-Gate-Battery-4471" })).await,
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str().unwrap()),
        (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS")
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn sso_owned_domains_route_to_their_idp() {
    let app = setup().await;
    let anchor = app.anchor_admin_token().await;
    let client_id = client(&app, "portal-sso").await;
    let portal = create_app(&app, &anchor, &client_id, "sso-portal", json!({})).await;
    let oauth_client = portal["oauthClientId"].as_str().unwrap().to_string();
    let redirect = "https://sso-portal.example.com/callback";

    let mut idp = IdentityProvider::new("acme-sso", "Acme SSO", IdentityProviderType::Oidc);
    idp.oidc_issuer_url = Some("https://idp.acme.test".into());
    idp.oidc_client_id = Some("portal-client".into());
    idp.allowed_email_domains = vec!["acme.test".into()];
    app.repos.idp_repo.insert(&idp).await.expect("insert idp");

    // Ensure: SSO-managed, no set-password invite; the portal origin is mailed.
    let (status, body) = ensure(
        &app,
        &anchor,
        json!({ "clientId": client_id, "email": "sam@acme.test" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ssoManaged"], true);
    assert_eq!(body["invited"], true);
    assert_eq!(body["state"], "INVITED");
    let (_, body) = ensure(
        &app,
        &anchor,
        json!({ "clientId": client_id, "email": "sue@acme.test", "returnInviteLink": true }),
    )
    .await;
    assert_eq!(body["inviteUrl"], "https://sso-portal.example.com/");

    let flow = authorize(&app, &oauth_client, redirect, "st-sso").await;
    let body = assert_status(
        post_public(
            &app,
            "/portal/auth/check-domain",
            json!({ "flowId": flow, "email": "sam@ACME.test" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(body["method"], "SSO");
    assert_eq!(
        body["redirectUrl"],
        format!("/portal/auth/oidc/login?flow={flow}&provider_id={}", idp.id)
    );
    let (status, body) = read_json(
        post_public(
            &app,
            "/portal/auth/login",
            json!({ "flowId": flow, "email": "sam@acme.test", "password": "whatever-1234" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        body,
        json!({ "code": "SSO_REQUIRED", "message": "Sign in with your organisation account" })
    );

    // The SSO start parks a portal-flagged state and bounces to the IdP.
    let resp = send_raw(
        &app,
        Request::get(format!(
            "/portal/auth/oidc/login?flow={flow}&provider_id={}",
            idp.id
        ))
        .header(header::HOST, "platform.test")
        .body(Body::empty())
        .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FOUND);
    let to_idp = location(&resp);
    assert!(
        to_idp.starts_with("https://idp.acme.test/authorize?"),
        "{to_idp}"
    );
    let state = query_param(&to_idp, "state").unwrap();
    let (portal_client,): (Option<String>,) =
        sqlx::query_as("SELECT portal_client_id FROM oauth_oidc_login_states WHERE state = $1")
            .bind(&state)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(portal_client.as_deref(), Some(client_id.as_str()));

    // The callback sink: a first login JIT-creates the identity with the app
    // it came through, and mints a portal code.
    let portal_state = fc_platform::portal::PortalState::new(fc_platform::portal::PortalDeps {
        pool: app.pool.clone(),
        clients: app.repos.client_repo.clone(),
        oauth_clients: app.repos.oauth_client_repo.clone(),
        identity_providers: app.repos.idp_repo.clone(),
        auth_codes: app.repos.auth_code_repo.clone(),
        password_service: Arc::new(fc_platform::PasswordService::default()),
        unit_of_work: app.unit_of_work.clone(),
        email_service: Arc::new(fc_platform::shared::email_service::LogEmailService),
        encryption_service: None,
        rate_limit_store: Arc::new(fc_platform::shared::rate_limit_store::NoopRateLimitStore),
        external_base_url: "http://localhost".into(),
    });
    let parked = portal_state
        .oidc_states
        .consume_portal(&state)
        .await
        .unwrap()
        .expect("portal state");
    let resp =
        fc_platform::portal::oidc::complete(&portal_state, &parked, "jit@acme.test", "Jit User")
            .await;
    assert_eq!(resp.status(), StatusCode::FOUND);
    let back = location(&resp);
    assert!(
        back.starts_with(redirect) && back.contains("code=") && back.contains("state=st-sso"),
        "{back}"
    );
    let (source, grants): (String, i64) = sqlx::query_as(
        "SELECT pi.source, (SELECT COUNT(*) FROM portal_identity_apps g WHERE g.identity_id = pi.id) \
         FROM portal_identities pi WHERE pi.email = 'jit@acme.test'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!((source.as_str(), grants), ("JIT", 1));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn portal_login_is_budgeted_per_client_and_email() {
    set_app_key();
    std::env::set_var("FC_RL_PORTAL_LOGIN_PER_15MIN", "3");
    let app = TestApp::setup_with_rate_limit_store(|pool| {
        Arc::new(PostgresRateLimitStore::new(pool.clone()))
    })
    .await;
    std::env::remove_var("FC_RL_PORTAL_LOGIN_PER_15MIN");
    let anchor = app.anchor_admin_token().await;
    let client_id = client(&app, "portal-budget").await;
    let portal = create_app(&app, &anchor, &client_id, "budget", json!({})).await;
    let flow = authorize(
        &app,
        portal["oauthClientId"].as_str().unwrap(),
        "https://budget.example.com/callback",
        "st",
    )
    .await;
    let attempt = || {
        post_public(
            &app,
            "/portal/auth/login",
            json!({ "flowId": flow, "email": "x@example.com", "password": "nope" }),
        )
    };
    for _ in 0..3 {
        assert_eq!(attempt().await.status(), StatusCode::UNAUTHORIZED);
    }
    let resp = attempt().await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(resp.headers().contains_key(header::RETRY_AFTER));
    let (_, body) = read_json(resp).await;
    assert_eq!(body["error"], "TOO_MANY_REQUESTS");
    // Another address has its own budget.
    let other = post_public(
        &app,
        "/portal/auth/login",
        json!({ "flowId": flow, "email": "y@example.com", "password": "nope" }),
    )
    .await;
    assert_eq!(other.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn oauth_clients_carry_the_portal_flags() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let client_id = client(&app, "portal-flags").await;
    let other_client = client(&app, "portal-flags-other").await;

    // A legacy client-wide portal: portalClientId, no app.
    let created = assert_status(
        app.post(
            "/api/oauth-clients",
            &admin,
            json!({
                "clientName": "Legacy Portal", "clientType": "PUBLIC", "pkceRequired": false,
                "redirectUris": ["https://legacy.example.com/callback"], "portalClientId": client_id,
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(created["client"]["portalClientId"], client_id);
    assert!(created["client"].get("portalAppId").is_none());
    let legacy_row = created["client"]["id"].as_str().unwrap().to_string();
    let legacy_client_id = created["client"]["clientId"].as_str().unwrap().to_string();

    // The invite redirect validates against it; the portal signs in with no app gate.
    let (status, user) = ensure(
        &app,
        &admin,
        json!({
            "clientId": client_id, "email": "legacy.user@example.com", "returnInviteLink": true,
            "redirectUri": "https://legacy.example.com/callback",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{user}");
    let body = set_password(
        &app,
        user["inviteUrl"].as_str().unwrap(),
        "Correct-Legacy-Battery-5512",
    )
    .await;
    assert_eq!(body["redirectUri"], "https://legacy.example.com/callback");
    let resp = send_raw(
        &app,
        Request::get(format!(
            "/portal/authorize?response_type=code&client_id={legacy_client_id}&redirect_uri={}&state=s1",
            urlencoding::encode("https://legacy.example.com/callback")
        ))
        .body(Body::empty())
        .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    let flow = query_param(&location(&resp), "flow").unwrap();
    let body = assert_status(
        post_public(&app, "/portal/auth/login", json!({ "flowId": flow, "email": "legacy.user@example.com", "password": "Correct-Legacy-Battery-5512" })).await,
        StatusCode::OK,
    )
    .await;
    assert!(body["redirectUrl"]
        .as_str()
        .unwrap()
        .starts_with("https://legacy.example.com/callback?code="));

    // portalAppId links, and makes the app's client the portal owner.
    let portal = create_app(&app, &admin, &client_id, "flags", json!({})).await;
    let app_id = portal["portalApp"]["id"].as_str().unwrap().to_string();
    let linked = assert_status(
        app.post(
            "/api/oauth-clients",
            &admin,
            json!({ "clientName": "Second", "clientType": "PUBLIC", "redirectUris": ["https://second.example.com/cb"], "portalAppId": app_id }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(linked["client"]["portalClientId"], client_id);
    assert_eq!(linked["client"]["portalAppId"], app_id);
    let (status, body) = read_json(
        app.post(
            "/api/oauth-clients",
            &admin,
            json!({ "clientName": "Mismatch", "clientType": "PUBLIC", "portalAppId": app_id, "portalClientId": other_client }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, "PORTAL_APP_CLIENT_MISMATCH")
    );
    let (status, body) = read_json(
        app.post(
            "/api/oauth-clients",
            &admin,
            json!({ "clientName": "Unknown", "clientType": "PUBLIC", "portalAppId": "pta_doesnotexist0" }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str().unwrap()),
        (StatusCode::NOT_FOUND, "PortalApp_NOT_FOUND")
    );

    // Update: link, unlink, and clearing the portal owner clears the link.
    let path = format!("/api/oauth-clients/{legacy_row}");
    let (status, _) = read_json(
        app.put(&path, &admin, json!({ "portalAppId": app_id }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let got = assert_status(app.get(&path, &admin).await, StatusCode::OK).await;
    assert_eq!(got["portalAppId"], app_id);
    let (status, _) = read_json(app.put(&path, &admin, json!({ "portalAppId": "" })).await).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let got = assert_status(app.get(&path, &admin).await, StatusCode::OK).await;
    assert!(got.get("portalAppId").is_none());
    assert_eq!(got["portalClientId"], client_id);
    read_json(
        app.put(&path, &admin, json!({ "portalAppId": app_id }))
            .await,
    )
    .await;
    let (status, _) = read_json(
        app.put(&path, &admin, json!({ "portalClientId": "" }))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let got = assert_status(app.get(&path, &admin).await, StatusCode::OK).await;
    assert!(got.get("portalClientId").is_none() && got.get("portalAppId").is_none());
}
