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

/// A user in the database, with a password, for the session tests.
async fn seed_user(app: &TestApp, email: &str, password: &str) -> fc_platform::domain::Principal {
    use fc_platform::auth::password_service::PasswordService;
    use fc_platform::domain::{Principal, UserScope};
    let mut user = Principal::new_user(email, UserScope::Anchor);
    if let Some(identity) = user.user_identity.as_mut() {
        identity.password_hash = Some(PasswordService::default().hash_password(password).unwrap());
    }
    app.repos.principal_repo.insert(&user).await.unwrap();
    user
}

/// The `fc_session` value a response sets.
fn session_cookie_of(resp: &Response<Body>) -> String {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|c| c.strip_prefix("fc_session="))
        .and_then(|c| c.split(';').next())
        .expect("fc_session cookie set")
        .to_string()
}

/// Decision #26: the session cookie carries the subject only (Go's shape)
/// and the principal is reloaded on every request — a role granted after
/// sign-in applies at once, and a deactivation signs the session out on
/// the next request.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_session_cookie_is_the_subject_reloaded_per_request() {
    use axum::http::Method;
    use base64::Engine as _;

    let app = TestApp::setup().await;
    // Seeds the platform:test-admin role (platform:*:*:*).
    let _ = app.anchor_admin_token().await;
    let user = seed_user(&app, "ada@flowcatalyst.test", "Correct-Horse-9!").await;

    let resp = send(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": "ada@flowcatalyst.test", "password": "Correct-Horse-9!"})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = session_cookie_of(&resp);

    // Go's claim shape: identity only.
    let payload: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(cookie.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    let mut keys: Vec<&str> = payload
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "all_applications",
            "email",
            "exp",
            "iat",
            "iss",
            "nbf",
            "sub",
            "tier"
        ]
    );
    assert_eq!(payload["sub"], user.id.as_str());

    let (status, body) = read_json(app.get_with_session("/auth/me", &cookie).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["principalId"], user.id.as_str());

    // No role yet: a permission-gated read is refused...
    let resp = app.get_with_session("/api/event-types", &cookie).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    // ...and a role granted now applies to the same cookie.
    let mut granted = app
        .repos
        .principal_repo
        .find_by_id(&user.id)
        .await
        .unwrap()
        .unwrap();
    granted.assign_role("platform:test-admin");
    app.repos.principal_repo.update(&granted).await.unwrap();
    let resp = app.get_with_session("/api/event-types", &cookie).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // The cookie is no bearer.
    let resp = app.get("/auth/me", &cookie).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Deactivation signs the session out on the next request.
    granted.deactivate();
    app.repos.principal_repo.update(&granted).await.unwrap();
    let resp = app.get_with_session("/auth/me", &cookie).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp = app.get_with_session("/api/event-types", &cookie).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// An access token replayed as the cookie is no session, even for an
/// active user (it may be narrowed or delegated to an OAuth client).
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_access_token_is_no_session_cookie() {
    let app = TestApp::setup().await;
    let user = seed_user(&app, "bob@flowcatalyst.test", "Correct-Horse-9!").await;
    let access = app.auth_service.generate_access_token(&user).unwrap();
    let resp = app.get_with_session("/auth/me", &access).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let session = app.auth_service.generate_session_token(&user).unwrap();
    let resp = app.get_with_session("/auth/me", &session).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

const PLANNER_REDIRECT: &str = "https://planner.example.test/callback";
const PKCE_VERIFIER: &str = "verifier-0123456789-0123456789-0123456789-abcdef";

/// A public PKCE client, as AgentPlanner is registered.
async fn seed_public_client(app: &TestApp) {
    let client = fc_platform::auth::oauth_entity::OAuthClient::new("agent-planner", "Planner")
        .with_redirect_uri(PLANNER_REDIRECT)
        .with_grant_type(fc_platform::auth::oauth_entity::GrantType::RefreshToken);
    app.repos.oauth_client_repo.insert(&client).await.unwrap();
}

fn authorize_request(prompt: Option<&str>) -> Request<Body> {
    use base64::Engine as _;
    use sha2::Digest as _;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(PKCE_VERIFIER.as_bytes()));
    let mut uri = format!(
        "/oauth/authorize?response_type=code&client_id=agent-planner&redirect_uri={}&state=s1&scope=openid%20offline_access&code_challenge={challenge}&code_challenge_method=S256",
        urlencoding::encode(PLANNER_REDIRECT)
    );
    if let Some(p) = prompt {
        uri.push_str(&format!("&prompt={p}"));
    }
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn location(resp: &Response<Body>) -> String {
    resp.headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn with_header(mut req: Request<Body>, name: &'static str, value: String) -> Request<Body> {
    req.headers_mut().insert(name, value.parse().unwrap());
    req
}

/// POST /oauth/token with a form body.
async fn token_request(app: &TestApp, form: &[(&str, &str)]) -> (StatusCode, Value) {
    let body = form
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    read_json(
        send(
            app,
            Request::builder()
                .method("POST")
                .uri("/oauth/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await,
    )
    .await
}

/// Owner ruling 2026-09-25, item 6 (Java 66281bc7, S2.2): /oauth/authorize
/// signs in from the session cookie only — never a Bearer — and only an
/// active user's; a code is not redeemed for a principal deactivated since.
#[tokio::test]
#[ignore = "requires Docker"]
async fn authorize_reads_only_an_active_users_session_cookie() {
    let app = TestApp::setup().await;
    seed_public_client(&app).await;
    let user = seed_user(&app, "ada@flowcatalyst.test", "Correct-Horse-9!").await;

    // A Bearer is no session: sent to log in, and prompt=none is refused.
    let access = app.auth_service.generate_access_token(&user).unwrap();
    let resp = send(
        &app,
        with_header(
            authorize_request(None),
            "authorization",
            format!("Bearer {access}"),
        ),
    )
    .await;
    assert!(
        location(&resp).starts_with("/auth/login?"),
        "{}",
        location(&resp)
    );
    let resp = send(
        &app,
        with_header(
            authorize_request(Some("none")),
            "authorization",
            format!("Bearer {access}"),
        ),
    )
    .await;
    assert!(
        location(&resp).contains("error=login_required"),
        "{}",
        location(&resp)
    );
    // Nor is an access token carried in the cookie.
    let resp = send(
        &app,
        with_header(
            authorize_request(None),
            "cookie",
            format!("fc_session={access}"),
        ),
    )
    .await;
    assert!(
        location(&resp).starts_with("/auth/login?"),
        "{}",
        location(&resp)
    );

    // The session cookie of an active user gets a code.
    let session = app.auth_service.generate_session_token(&user).unwrap();
    let resp = send(
        &app,
        with_header(
            authorize_request(None),
            "cookie",
            format!("fc_session={session}"),
        ),
    )
    .await;
    let loc = location(&resp);
    assert!(loc.starts_with(PLANNER_REDIRECT), "{loc}");
    let code = loc
        .split("code=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();

    // Deactivated after the code was issued: the code buys no tokens...
    let mut inactive = app
        .repos
        .principal_repo
        .find_by_id(&user.id)
        .await
        .unwrap()
        .unwrap();
    inactive.deactivate();
    app.repos.principal_repo.update(&inactive).await.unwrap();
    let (status, body) = token_request(
        &app,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", PLANNER_REDIRECT),
            ("client_id", "agent-planner"),
            ("code_verifier", PKCE_VERIFIER),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");
    assert_eq!(body["error_description"], "Account is not active", "{body}");

    // ...and the session no longer signs in.
    let resp = send(
        &app,
        with_header(
            authorize_request(None),
            "cookie",
            format!("fc_session={session}"),
        ),
    )
    .await;
    assert!(
        location(&resp).starts_with("/auth/login?"),
        "{}",
        location(&resp)
    );
}

/// Triage S10 (Java 6a06a7f0 S2.1, S2.4): client selection and passkey
/// self-service take the session cookie only. The accessible list is the
/// caller's active clients (one batch load, no per-client query).
#[tokio::test]
#[ignore = "requires Docker"]
async fn client_selection_and_passkeys_take_the_session_cookie_only() {
    use axum::http::Method;
    use fc_platform::domain::{Principal, UserScope};
    use fc_platform::Client;

    let app = TestApp::setup().await;
    let mut clients = Vec::new();
    for (name, identifier) in [("Beta", "beta"), ("Alpha", "alpha"), ("Gone", "gone")] {
        let mut client = Client::new(name, identifier);
        if identifier == "gone" {
            client.suspend("test");
        }
        app.repos.client_repo.insert(&client).await.unwrap();
        clients.push(client);
    }
    let partner = Principal::new_user("pat@flowcatalyst.test", UserScope::Partner);
    app.repos.principal_repo.insert(&partner).await.unwrap();
    for client in &clients {
        app.repos
            .principal_repo
            .grant_client_access(&partner.id, &client.id)
            .await
            .unwrap();
    }
    let partner = app
        .repos
        .principal_repo
        .find_by_id(&partner.id)
        .await
        .unwrap()
        .unwrap();
    let bearer = app.auth_service.generate_access_token(&partner).unwrap();
    let session = app.auth_service.generate_session_token(&partner).unwrap();

    // A bearer — the user's own, full-authority one — is refused.
    for path in [
        "/auth/client/accessible",
        "/auth/client/current",
        "/auth/webauthn/credentials",
    ] {
        let resp = app.get(path, &bearer).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
    let resp = app
        .post(
            "/auth/client/switch",
            &bearer,
            json!({ "clientId": clients[0].id }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp = app
        .post("/auth/webauthn/register/begin", &bearer, json!({}))
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // The session cookie is accepted.
    let (status, body) = read_json(
        app.get_with_session("/auth/client/accessible", &session)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let names: Vec<&str> = body["clients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Alpha", "Beta"], "active clients, by name: {body}");
    assert_eq!(body["globalAccess"], false);

    let (status, body) = read_json(
        app.send_with_session(
            Method::POST,
            "/auth/client/switch",
            &session,
            Some(json!({ "clientId": clients[1].id })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["client"]["identifier"], "alpha");

    let resp = app
        .get_with_session("/auth/webauthn/credentials", &session)
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// Sign `user` in to the planner client through the code flow and return
/// its refresh token.
async fn planner_refresh_token(app: &TestApp, user: &fc_platform::domain::Principal) -> String {
    let session = app.auth_service.generate_session_token(user).unwrap();
    let resp = send(
        app,
        with_header(
            authorize_request(None),
            "cookie",
            format!("fc_session={session}"),
        ),
    )
    .await;
    let loc = location(&resp);
    let code = loc
        .split("code=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let (status, body) = token_request(
        app,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", PLANNER_REDIRECT),
            ("client_id", "agent-planner"),
            ("code_verifier", PKCE_VERIFIER),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["refresh_token"]
        .as_str()
        .expect("refresh_token")
        .to_string()
}

async fn refresh(app: &TestApp, token: &str) -> (StatusCode, Value) {
    token_request(
        app,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", token),
            ("client_id", "agent-planner"),
        ],
    )
    .await
}

/// (expires_at, consumed, revoked, replacedBy, family) of the row for `raw`.
async fn refresh_row(
    app: &TestApp,
    raw: &str,
) -> (
    chrono::DateTime<chrono::Utc>,
    bool,
    bool,
    Option<String>,
    Option<String>,
) {
    let hash = fc_platform::RefreshToken::hash_token(raw);
    sqlx::query_as(
        "SELECT expires_at, consumed_at IS NOT NULL,
                coalesce((payload->>'revoked')::boolean, false),
                payload->>'replacedBy', grant_id
         FROM oauth_oidc_payloads WHERE type = 'RefreshToken' AND payload->>'tokenHash' = $1",
    )
    .bind(hash)
    .fetch_one(&app.pool)
    .await
    .unwrap()
}

/// Triage S12 (Java 6a06a7f0 S2.5, 477db983; Go grantstore.Rotate):
/// rotation is single-use, keeps the family and its absolute expiry; a
/// client racing itself is not signed out; a replay after the 10 s leeway
/// revokes the whole family.
#[tokio::test]
#[ignore = "requires Docker"]
async fn refresh_rotation_is_atomic_with_family_reuse_detection() {
    let app = TestApp::setup().await;
    seed_public_client(&app).await;
    let user = seed_user(&app, "ada@flowcatalyst.test", "Correct-Horse-9!").await;
    let first = planner_refresh_token(&app, &user).await;
    let (first_expiry, ..) = refresh_row(&app, &first).await;

    // A rotation: the new token inherits the expiry and the family; the old
    // one is consumed and linked to it.
    let (status, body) = refresh(&app, &first).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let second = body["refresh_token"].as_str().unwrap().to_string();
    let (expiry, consumed, revoked, replaced_by, family) = refresh_row(&app, &first).await;
    assert!(consumed && revoked);
    assert_eq!(
        replaced_by.as_deref(),
        Some(fc_platform::RefreshToken::hash_token(&second).as_str())
    );
    let (second_expiry, _, _, _, second_family) = refresh_row(&app, &second).await;
    assert_eq!(
        second_expiry.timestamp(),
        first_expiry.timestamp(),
        "no fresh 30 days"
    );
    assert_eq!(expiry, first_expiry);
    assert!(family.is_some());
    assert_eq!(second_family, family);

    // Two concurrent presentations of the same token both succeed, and
    // exactly one consumed it (the other got a sibling).
    let (a, b) = tokio::join!(refresh(&app, &second), refresh(&app, &second));
    assert_eq!(a.0, StatusCode::OK, "{}", a.1);
    assert_eq!(b.0, StatusCode::OK, "{}", b.1);
    let third_a = a.1["refresh_token"].as_str().unwrap().to_string();
    let third_b = b.1["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(third_a, third_b);
    let (live,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM oauth_oidc_payloads
         WHERE type = 'RefreshToken' AND grant_id = $1 AND consumed_at IS NULL",
    )
    .bind(&family)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(live, 2, "the winner's token and the sibling");

    // A replay of `second` after the leeway is reuse: the family dies.
    sqlx::query(
        "UPDATE oauth_oidc_payloads
         SET payload = jsonb_set(payload, '{revokedAt}', to_jsonb($2::text)),
             consumed_at = $3
         WHERE type = 'RefreshToken' AND payload->>'tokenHash' = $1",
    )
    .bind(fc_platform::RefreshToken::hash_token(&second))
    .bind((chrono::Utc::now() - chrono::Duration::seconds(60)).to_rfc3339())
    .bind(chrono::Utc::now() - chrono::Duration::seconds(60))
    .execute(&app.pool)
    .await
    .unwrap();
    let (status, body) = refresh(&app, &second).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    for token in [&third_a, &third_b] {
        let (status, body) = refresh(&app, token).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    }
}

/// Triage S2 (Go handleRefresh): /auth/refresh authenticates no client, so
/// a token issued to an OAuth client is refused there and not consumed.
#[tokio::test]
#[ignore = "requires Docker"]
async fn auth_refresh_refuses_a_client_bound_token() {
    use axum::http::Method;
    let app = TestApp::setup().await;
    seed_public_client(&app).await;
    let user = seed_user(&app, "ada@flowcatalyst.test", "Correct-Horse-9!").await;
    let token = planner_refresh_token(&app, &user).await;

    let (status, body) = read_json(
        send(
            &app,
            Request::builder()
                .method(Method::POST)
                .uri("/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "refreshToken": token }).to_string()))
                .unwrap(),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("not issued to this client"),
        "{body}"
    );
    let (_, consumed, ..) = refresh_row(&app, &token).await;
    assert!(!consumed, "a refusal consumes nothing");
    let (status, body) = refresh(&app, &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
