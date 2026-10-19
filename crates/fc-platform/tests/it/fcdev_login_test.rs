//! F25, platform half (Go `oauthapi/fcdev_login_pg_test.go`): the built-in
//! public OAuth client `fcdev-cli` behind `fcdev login`. Seed shape and
//! re-assertion, the browser-login code flow with PKCE, RFC 8252 loopback
//! redirects for this client only, the ABSOLUTE refresh lifetime
//! (`FC_FCDEV_LOGIN_REFRESH_TTL_SECS`), `refresh_expires_in`, client binding,
//! account cut-off and revocation. Requires a Postgres (see `support::db`).

use crate::support;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use fc_platform::auth::fcdev;
use fc_platform::auth::oauth_entity::{GrantType, OAuthClient};
use fc_platform::auth::password_service::PasswordService;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::AuthRole;
use fc_platform::shared::database;
use fc_platform_core::shared::tsid;
use serde_json::Value;
use sha2::Digest as _;
use std::env;
use std::sync::OnceLock;
use std::time::Duration;
use support::{read_json, TestApp};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tower::ServiceExt;

const LOOPBACK_CB: &str = "http://127.0.0.1:53682/callback";
const ROLE_A: &str = "fcl:alpha";
const ROLE_B: &str = "fcl:beta";
const PERM_A: &str = "fcl:alpha:record:view";
const PERM_B: &str = "fcl:beta:record:view";

/// `FC_FCDEV_LOGIN_REFRESH_TTL_SECS` is read when a `TestApp` builds its
/// router, so the tests that care about it hold this lock from setup on.
fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct Rig {
    app: TestApp,
    user: Principal,
}

struct Pkce {
    verifier: String,
    challenge: String,
}

fn new_pkce(seed: &str) -> Pkce {
    let verifier = format!("{seed}-0123456789-0123456789-0123456789-abcdefghij");
    let challenge = URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    Pkce {
        verifier,
        challenge,
    }
}

fn unique_pkce() -> Pkce {
    new_pkce(&format!("v{}", tsid::generate_untyped()))
}

/// A platform with the fcdev client seeded and an anchor user holding two
/// roles that grant one permission each.
async fn rig(email: &str) -> Rig {
    support::set_app_key();
    let app = TestApp::setup().await;
    database::seed_fcdev_cli_client(&app.pool)
        .await
        .expect("seed fcdev-cli");
    for (name, perm) in [("alpha", PERM_A), ("beta", PERM_B)] {
        let role = AuthRole::new("fcl", name, name).with_permission(perm);
        app.repos
            .role_repo
            .insert(&role)
            .await
            .expect("insert role");
    }
    let mut user = Principal::new_user(email, UserScope::Anchor);
    if let Some(identity) = user.user_identity.as_mut() {
        identity.password_hash = Some(
            PasswordService::default()
                .hash_password("Pw-1234567!x")
                .unwrap(),
        );
    }
    user.assign_role(ROLE_A);
    user.assign_role(ROLE_B);
    app.repos.principal_repo.insert(&user).await.expect("user");
    Rig { app, user }
}

impl Rig {
    async fn send(&self, req: Request<Body>) -> Response<Body> {
        self.app.router.clone().oneshot(req).await.unwrap()
    }

    /// GET /oauth/authorize as fcdev (or `client_id`), signed in or not.
    async fn authorize(
        &self,
        client_id: &str,
        redirect: &str,
        challenge: Option<(&str, &str)>,
        signed_in: bool,
    ) -> Response<Body> {
        let mut uri = format!(
            "/oauth/authorize?response_type=code&client_id={client_id}&redirect_uri={}&state=s1&scope=openid%20offline_access",
            urlencoding::encode(redirect)
        );
        if let Some((c, m)) = challenge {
            uri.push_str(&format!("&code_challenge={c}&code_challenge_method={m}"));
        }
        let mut req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        if signed_in {
            let session = self
                .app
                .auth_service
                .generate_session_token(&self.user)
                .unwrap();
            req.headers_mut()
                .insert("cookie", format!("fc_session={session}").parse().unwrap());
        }
        self.send(req).await
    }

    async fn pkce_authorize(&self, p: &Pkce) -> Response<Body> {
        self.authorize(
            fcdev::FCDEV_CLI_CLIENT_ID,
            LOOPBACK_CB,
            Some((&p.challenge, "S256")),
            true,
        )
        .await
    }

    async fn token(&self, form: &[(&str, &str)]) -> (StatusCode, Value) {
        let body = form
            .iter()
            .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        read_json(
            self.send(
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

    async fn exchange(&self, code: &str, verifier: &str, redirect: &str) -> (StatusCode, Value) {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect),
            ("client_id", fcdev::FCDEV_CLI_CLIENT_ID),
        ];
        if !verifier.is_empty() {
            form.push(("code_verifier", verifier));
        }
        self.token(&form).await
    }

    async fn refresh(&self, client_id: &str, token: &str) -> (StatusCode, Value) {
        self.token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", token),
            ("client_id", client_id),
        ])
        .await
    }

    /// The whole browser login: authorize with the session, redeem the code.
    async fn login(&self) -> Value {
        let p = unique_pkce();
        let code = code_from(&self.pkce_authorize(&p).await);
        let (status, body) = self.exchange(&code, &p.verifier, LOOPBACK_CB).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// (created_at, expires_at) of the refresh row for `raw`.
    async fn refresh_expiry(&self, raw: &str) -> (DateTime<Utc>, DateTime<Utc>) {
        let (created, expires): (DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
            "SELECT created_at, expires_at FROM oauth_oidc_payloads
             WHERE type = 'RefreshToken' AND payload->>'tokenHash' = $1",
        )
        .bind(fc_platform::RefreshToken::hash_token(raw))
        .fetch_one(&self.app.pool)
        .await
        .unwrap();
        (created, expires)
    }
}

fn location(resp: &Response<Body>) -> String {
    resp.headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn code_from(resp: &Response<Body>) -> String {
    let loc = location(resp);
    loc.split("code=")
        .nth(1)
        .unwrap_or_else(|| panic!("no code in redirect {loc:?}"))
        .split('&')
        .next()
        .unwrap()
        .to_string()
}

fn claims_of(jwt: &str) -> Value {
    let payload = jwt.split('.').nth(1).expect("jwt payload");
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| STANDARD_NO_PAD.decode(payload))
        .expect("base64 payload");
    serde_json::from_slice(&bytes).expect("claims json")
}

fn strings(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = v
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|s| s.as_str().unwrap_or_default().to_string())
        .collect();
    out.sort();
    out
}

fn assert_secs_near(actual: i64, want: i64, slack: i64, what: &str) {
    assert!(
        (actual - want).abs() <= slack,
        "{what}: {actual}s, wanted about {want}s"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_client_is_seeded_and_re_asserted() {
    support::set_app_key();
    let app = TestApp::setup().await;
    database::seed_fcdev_cli_client(&app.pool).await.unwrap();
    database::seed_fcdev_cli_client(&app.pool)
        .await
        .expect("idempotent");
    let repo = &app.repos.oauth_client_repo;
    let assert_shape = |c: &OAuthClient| {
        assert!(c.is_public(), "PUBLIC");
        assert!(c.client_secret_ref.is_none());
        assert!(c.pkce_required);
        assert!(c.api_access);
        assert!(c.portal_client_id.is_none());
        assert!(c.application_ids.is_empty());
        assert!(c.allowed_origins.is_empty());
        let mut grants: Vec<String> = c
            .grant_types
            .iter()
            .map(|g| g.as_str().to_string())
            .collect();
        grants.sort();
        assert_eq!(grants, ["authorization_code", "refresh_token"]);
        let mut uris = c.redirect_uris.clone();
        uris.sort();
        assert_eq!(uris, ["http://127.0.0.1/callback", "http://[::1]/callback"]);
    };
    let c = repo
        .find_by_client_id(fcdev::FCDEV_CLI_CLIENT_ID)
        .await
        .unwrap()
        .expect("seeded");
    assert_shape(&c);
    assert!(c.active);

    // A console edit that widens the client is undone at the next start, but
    // an operator's deactivation is respected.
    sqlx::query(
        "UPDATE oauth_clients SET client_type='CONFIDENTIAL', client_secret_ref='x',
                pkce_required=FALSE, api_access=FALSE, active=FALSE WHERE client_id=$1",
    )
    .bind(fcdev::FCDEV_CLI_CLIENT_ID)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO oauth_client_redirect_uris (oauth_client_id, redirect_uri)
         SELECT id, 'https://evil.example/cb' FROM oauth_clients WHERE client_id=$1",
    )
    .bind(fcdev::FCDEV_CLI_CLIENT_ID)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO oauth_client_grant_types (oauth_client_id, grant_type)
         SELECT id, 'client_credentials' FROM oauth_clients WHERE client_id=$1",
    )
    .bind(fcdev::FCDEV_CLI_CLIENT_ID)
    .execute(&app.pool)
    .await
    .unwrap();
    database::seed_fcdev_cli_client(&app.pool).await.unwrap();
    // The repository caches by client_id; read the row afresh.
    let fresh = fc_platform::OAuthClientRepository::new(&app.pool);
    let c = fresh
        .find_by_client_id(fcdev::FCDEV_CLI_CLIENT_ID)
        .await
        .unwrap()
        .expect("still there");
    assert_shape(&c);
    assert!(
        !c.active,
        "seeding must not re-activate a deactivated client"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_login_yields_the_persons_own_authority_and_a_seven_day_family() {
    let _guard = env_lock().lock().await;
    let r = rig("fcdev-happy@login.test").await;
    let body = r.login().await;

    let claims = claims_of(body["access_token"].as_str().unwrap());
    assert_eq!(claims["token_use"], "api", "the person's own authority");
    assert_eq!(claims["tier"], "ANCHOR");
    assert_eq!(claims["sub"], r.user.id.as_str());
    assert_eq!(claims["email"], "fcdev-happy@login.test");
    assert_eq!(strings(&claims["roles"]), [ROLE_A, ROLE_B]);
    let scope = claims["scope"].as_str().unwrap_or_default();
    assert!(scope.contains(PERM_A) && scope.contains(PERM_B), "{scope}");
    assert!(body["refresh_token"]
        .as_str()
        .is_some_and(|t| !t.is_empty()));
    assert!(body["id_token"].as_str().is_some_and(|t| !t.is_empty()));
    assert_eq!(body["expires_in"], 3600, "the normal access-token TTL");

    let (created, expires) = r
        .refresh_expiry(body["refresh_token"].as_str().unwrap())
        .await;
    assert_secs_near(
        (expires - created).num_seconds(),
        fcdev::DEFAULT_FCDEV_CLI_REFRESH_TTL_SECS,
        5,
        "family lifetime",
    );
    assert_secs_near(
        body["refresh_expires_in"]
            .as_i64()
            .expect("refresh_expires_in"),
        fcdev::DEFAULT_FCDEV_CLI_REFRESH_TTL_SECS,
        10,
        "the client is told when the login ends",
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_lifetime_is_configurable_by_env() {
    let _guard = env_lock().lock().await;
    // Serialised by `env_lock`; only this module's tests read it.
    env::set_var(fcdev::FCDEV_REFRESH_TTL_ENV, "129600");
    let r = rig("fcdev-ttl@login.test").await;
    env::remove_var(fcdev::FCDEV_REFRESH_TTL_ENV);
    let body = r.login().await;
    let (created, expires) = r
        .refresh_expiry(body["refresh_token"].as_str().unwrap())
        .await;
    assert_secs_near(
        (expires - created).num_seconds(),
        36 * 3600,
        5,
        "configured lifetime",
    );
    assert_secs_near(
        body["refresh_expires_in"].as_i64().unwrap(),
        36 * 3600,
        10,
        "countdown",
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn login_is_required_and_pkce_is_enforced() {
    let _guard = env_lock().lock().await;
    let r = rig("fcdev-pkce@login.test").await;

    // Not signed in: sent to the login page, the client carried along.
    let p = unique_pkce();
    let resp = r
        .authorize(
            fcdev::FCDEV_CLI_CLIENT_ID,
            LOOPBACK_CB,
            Some((&p.challenge, "S256")),
            false,
        )
        .await;
    let loc = location(&resp);
    assert!(loc.starts_with("/auth/login?oauth=true"), "{loc}");
    assert!(loc.contains("client_id=fcdev-cli"), "{loc}");
    assert!(
        loc.contains(&format!("code_challenge={}", p.challenge)),
        "{loc}"
    );

    // No challenge: refused at authorize.
    let resp = r
        .authorize(fcdev::FCDEV_CLI_CLIENT_ID, LOOPBACK_CB, None, true)
        .await;
    let loc = location(&resp);
    assert!(
        loc.contains("error=invalid_request") && !loc.contains("code="),
        "{loc}"
    );
    // plain: refused.
    let plain = "a".repeat(43);
    let resp = r
        .authorize(
            fcdev::FCDEV_CLI_CLIENT_ID,
            LOOPBACK_CB,
            Some((&plain, "plain")),
            true,
        )
        .await;
    let loc = location(&resp);
    assert!(
        loc.contains("error=invalid_request") && !loc.contains("code="),
        "{loc}"
    );

    // Missing and wrong verifier: refused at the token endpoint.
    let p = unique_pkce();
    let code = code_from(&r.pkce_authorize(&p).await);
    let (status, body) = r.exchange(&code, "", LOOPBACK_CB).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");

    let p = unique_pkce();
    let code = code_from(&r.pkce_authorize(&p).await);
    let (status, body) = r
        .exchange(&code, &unique_pkce().verifier, LOOPBACK_CB)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");
    // Single use: the right verifier afterwards does not revive the code.
    let (status, _) = r.exchange(&code, &p.verifier, LOOPBACK_CB).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A plain-method verifier (verifier == challenge) must not pass for S256.
    let p = unique_pkce();
    let code = code_from(&r.pkce_authorize(&p).await);
    let (status, _) = r.exchange(&code, &p.challenge, LOOPBACK_CB).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The client is public: presenting a secret is refused.
    let p = unique_pkce();
    let code = code_from(&r.pkce_authorize(&p).await);
    let (status, body) = r
        .token(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", LOOPBACK_CB),
            ("client_id", fcdev::FCDEV_CLI_CLIENT_ID),
            ("client_secret", "guess"),
            ("code_verifier", &p.verifier),
        ])
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"], "invalid_client", "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn loopback_redirects_vary_the_port_for_fcdev_only() {
    let _guard = env_lock().lock().await;
    let r = rig("fcdev-redirect@login.test").await;
    let p = unique_pkce();
    let ch = Some((p.challenge.as_str(), "S256"));

    for ok in [
        "http://127.0.0.1:1/callback",
        "http://127.0.0.1:65535/callback",
        "http://[::1]:40000/callback",
    ] {
        let resp = r.authorize(fcdev::FCDEV_CLI_CLIENT_ID, ok, ch, true).await;
        assert!(
            location(&resp).starts_with(&format!("{ok}?code=")),
            "{ok}: {}",
            location(&resp)
        );
    }
    for bad in [
        "http://127.0.0.1.evil.com:5000/callback",
        "http://evil.com@127.0.0.1:5000/callback",
        "http://localhost:5000/callback",
        "https://127.0.0.1:5000/callback",
        "http://127.0.0.1:5000/callback/x",
        "http://127.0.0.1:5000/other",
        "http://127.0.0.1:0/callback",
        "http://127.0.0.1:65536/callback",
        "http://127.0.0.1:5000/callback?next=https://evil.com",
        "https://evil.example/cb",
    ] {
        let resp = r.authorize(fcdev::FCDEV_CLI_CLIENT_ID, bad, ch, true).await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "{bad} must be refused without a redirect"
        );
        assert_eq!(location(&resp), "", "{bad}");
    }

    // The same port-less registration on any other client: exact matching only.
    let mut imposter =
        OAuthClient::new("imposter", "Imposter").with_grant_type(GrantType::RefreshToken);
    for u in fcdev::FCDEV_CLI_REDIRECT_URIS {
        imposter = imposter.with_redirect_uri(u);
    }
    r.app
        .repos
        .oauth_client_repo
        .insert(&imposter)
        .await
        .unwrap();
    let resp = r.authorize("imposter", LOOPBACK_CB, ch, true).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // The token endpoint insists the redirect_uri equals the issued one.
    let code = code_from(&r.pkce_authorize(&p).await);
    let (status, body) = r
        .exchange(&code, &p.verifier, "http://127.0.0.1:53683/callback")
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn refresh_rotates_with_an_absolute_deadline_and_detects_reuse() {
    let _guard = env_lock().lock().await;
    let r = rig("fcdev-refresh@login.test").await;
    let body = r.login().await;
    let rt1 = body["refresh_token"].as_str().unwrap().to_string();
    let (created, exp1) = r.refresh_expiry(&rt1).await;

    // A refresh: new tokens, the same absolute deadline.
    sleep(Duration::from_millis(1100)).await;
    let (status, b2) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &rt1).await;
    assert_eq!(status, StatusCode::OK, "{b2}");
    let rt2 = b2["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(rt1, rt2, "refresh tokens rotate");
    assert_eq!(
        claims_of(b2["access_token"].as_str().unwrap())["token_use"],
        "api"
    );
    // The countdown counts DOWN; it is not reset.
    assert!(
        b2["refresh_expires_in"].as_i64().unwrap() < body["refresh_expires_in"].as_i64().unwrap(),
        "{} !< {}",
        b2["refresh_expires_in"],
        body["refresh_expires_in"]
    );
    let (_, exp2) = r.refresh_expiry(&rt2).await;
    assert_eq!(
        exp2, exp1,
        "a refresh must not extend the absolute lifetime"
    );
    assert_secs_near(
        (exp2 - created).num_seconds(),
        fcdev::DEFAULT_FCDEV_CLI_REFRESH_TTL_SECS,
        5,
        "lifetime from the sign-in",
    );

    // A third generation too.
    let (status, b3) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &rt2).await;
    assert_eq!(status, StatusCode::OK, "{b3}");
    let rt3 = b3["refresh_token"].as_str().unwrap().to_string();
    assert_eq!(r.refresh_expiry(&rt3).await.1, exp1);

    // Reuse detection: replaying an already-rotated token (past the sibling
    // leeway) burns the whole family.
    sqlx::query(
        "UPDATE oauth_oidc_payloads
         SET payload = jsonb_set(payload, '{revokedAt}', to_jsonb($2::text)), consumed_at = $3
         WHERE type = 'RefreshToken' AND payload->>'tokenHash' = $1",
    )
    .bind(fc_platform::RefreshToken::hash_token(&rt1))
    .bind((Utc::now() - chrono::Duration::seconds(60)).to_rfc3339())
    .bind(Utc::now() - chrono::Duration::seconds(60))
    .execute(&r.app.pool)
    .await
    .unwrap();
    let (status, body) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &rt1).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");
    let (status, _) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &rt3).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the family is revoked after a replay"
    );

    // Absolute expiry: once the deadline passes, refresh is refused.
    let body = r.login().await;
    let rt = body["refresh_token"].as_str().unwrap();
    sqlx::query(
        "UPDATE oauth_oidc_payloads SET expires_at = NOW() - INTERVAL '1 minute'
         WHERE type = 'RefreshToken' AND payload->>'tokenHash' = $1",
    )
    .bind(fc_platform::RefreshToken::hash_token(rt))
    .execute(&r.app.pool)
    .await
    .unwrap();
    let (status, body) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, rt).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn refresh_tokens_are_bound_to_their_client() {
    let _guard = env_lock().lock().await;
    let r = rig("fcdev-bound@login.test").await;
    let other = OAuthClient::new("other-public", "Other")
        .with_redirect_uri("https://other.example/cb")
        .with_grant_type(GrantType::RefreshToken);
    r.app.repos.oauth_client_repo.insert(&other).await.unwrap();
    let rt = r.login().await["refresh_token"]
        .as_str()
        .unwrap()
        .to_string();

    // Another client cannot redeem fcdev's refresh token.
    let (status, body) = r.refresh("other-public", &rt).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");

    // And fcdev cannot redeem another client's.
    let (other_raw, entity) = fc_platform::RefreshToken::generate_token_pair(r.user.id.clone());
    let entity = entity
        .with_oauth_client("other-public")
        .with_scopes(vec!["openid".into(), "offline_access".into()]);
    r.app
        .repos
        .refresh_token_repo
        .insert(&entity)
        .await
        .unwrap();
    let (status, body) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &other_raw).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");

    // The failed attempts did not burn the legitimate token.
    let (status, body) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &rt).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn removed_roles_narrow_and_a_deactivated_user_is_cut_off() {
    let _guard = env_lock().lock().await;
    let r = rig("fcdev-cutoff@login.test").await;

    // Removed roles take effect at the next refresh.
    let rt = r.login().await["refresh_token"]
        .as_str()
        .unwrap()
        .to_string();
    let mut user = r
        .app
        .repos
        .principal_repo
        .find_by_id(&r.user.id)
        .await
        .unwrap()
        .unwrap();
    user.roles.retain(|a| a.role != ROLE_B);
    r.app.repos.principal_repo.update(&user).await.unwrap();
    let (status, body) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &rt).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let claims = claims_of(body["access_token"].as_str().unwrap());
    assert_eq!(strings(&claims["roles"]), [ROLE_A]);
    let scope = claims["scope"].as_str().unwrap_or_default();
    assert!(scope.contains(PERM_A) && !scope.contains(PERM_B), "{scope}");
    let rt = body["refresh_token"].as_str().unwrap().to_string();

    // A code issued before the account is deactivated cannot be redeemed after.
    let p = unique_pkce();
    let code = code_from(&r.pkce_authorize(&p).await);
    let mut user = r
        .app
        .repos
        .principal_repo
        .find_by_id(&r.user.id)
        .await
        .unwrap()
        .unwrap();
    user.deactivate();
    r.app.repos.principal_repo.update(&user).await.unwrap();

    let (status, body) = r.exchange(&code, &p.verifier, LOOPBACK_CB).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");
    assert_eq!(body["error_description"], "Account is not active", "{body}");
    let (status, body) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &rt).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");
    assert!(
        body["error_description"]
            .as_str()
            .unwrap()
            .contains("not active"),
        "{body}"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn revoking_ends_the_session() {
    let _guard = env_lock().lock().await;
    let r = rig("fcdev-revoke@login.test").await;
    let rt = r.login().await["refresh_token"]
        .as_str()
        .unwrap()
        .to_string();

    let body = format!(
        "token={}&client_id={}",
        urlencoding::encode(&rt),
        fcdev::FCDEV_CLI_CLIENT_ID
    );
    let resp = r
        .send(
            Request::builder()
                .method("POST")
                .uri("/oauth/revoke")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let (status, body) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &rt).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant", "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_deactivated_client_is_refused() {
    let _guard = env_lock().lock().await;
    let r = rig("fcdev-clientoff@login.test").await;
    let rt = r.login().await["refresh_token"]
        .as_str()
        .unwrap()
        .to_string();
    // An operator deactivates it in the console (a repository update, which
    // also drops the client from the lookup cache).
    let mut client = r
        .app
        .repos
        .oauth_client_repo
        .find_by_client_id(fcdev::FCDEV_CLI_CLIENT_ID)
        .await
        .unwrap()
        .unwrap();
    client.active = false;
    r.app.repos.oauth_client_repo.update(&client).await.unwrap();
    let (status, body) = r.refresh(fcdev::FCDEV_CLI_CLIENT_ID, &rt).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let p = unique_pkce();
    let resp = r.pkce_authorize(&p).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
