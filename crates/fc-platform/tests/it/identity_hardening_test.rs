//! Identity-review fixes (docs/go-mirror/2026-10-19-identity-fixes-and-catch-up.md):
//! one regression test per finding that needs a running platform. Requires a
//! Postgres (see `support::db`).

use crate::support;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use fc_platform::auth::fcdev;
use fc_platform::auth::oauth_entity::{GrantType, OAuthClient};
use fc_platform::auth::password_service::PasswordService;
use fc_platform::domain::{Principal, UserScope};
use serde_json::{json, Value};
use sha2::Digest as _;
use std::time::{Duration, Instant};
use support::{read_json, TestApp};
use tower::ServiceExt;

async fn post_json(app: &TestApp, path: &str, body: Value) -> Response<Body> {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.router.clone().oneshot(req).await.unwrap()
}

/// R2: a refused login for an email that is not registered spends the
/// Argon2id work a wrong password for a real account spends, so response time
/// does not enumerate emails. Before the fix the unknown email answered in
/// microseconds. The bound is loose (half) to stay stable on a busy machine.
#[tokio::test]
#[ignore = "requires Docker"]
async fn unknown_email_login_costs_what_a_wrong_password_costs() {
    let app = TestApp::setup().await;
    let mut user = Principal::new_user("timing@example.test", UserScope::Anchor);
    if let Some(identity) = user.user_identity.as_mut() {
        identity.password_hash = Some(
            PasswordService::default()
                .hash_password("Pw-1234567!x")
                .unwrap(),
        );
    }
    app.repos.principal_repo.insert(&user).await.unwrap();

    // Warm the dummy hash and the connection pool.
    post_json(
        &app,
        "/auth/login",
        json!({"email": "warm@example.test", "password": "x"}),
    )
    .await;

    // One attempt per distinct email: the per-(email, IP) backoff gives three
    // free failures, and each distinct email keeps this clear of it.
    async fn time(app: &TestApp, email: &str) -> Duration {
        let t = Instant::now();
        let resp = post_json(
            app,
            "/auth/login",
            json!({"email": email, "password": "Wrong-1234567!x"}),
        )
        .await;
        assert_eq!(resp.status(), 401);
        t.elapsed()
    }
    let known = time(&app, "timing@example.test").await;
    let mut unknown = Vec::new();
    for i in 0..3 {
        unknown.push(time(&app, &format!("nobody{i}@example.test")).await);
    }
    unknown.sort();
    let unknown = unknown[1];
    assert!(
        unknown * 2 > known,
        "unknown email answered in {unknown:?}, a real account's wrong password in {known:?}"
    );
}

// ── a signed-in user, the fcdev public client and a wildcard client ──────────

const CB: &str = "http://127.0.0.1:53682/callback";
const VERIFIER: &str = "v-0123456789-0123456789-0123456789-0123456789-abcdef";

struct Rig {
    app: TestApp,
    user: Principal,
}

async fn rig(email: &str) -> Rig {
    support::set_app_key();
    let app = TestApp::setup().await;
    fc_platform::shared::database::seed_fcdev_cli_client(&app.pool)
        .await
        .unwrap();
    let user = Principal::new_user(email, UserScope::Anchor);
    app.repos.principal_repo.insert(&user).await.unwrap();
    Rig { app, user }
}

impl Rig {
    async fn send(&self, req: Request<Body>) -> Response<Body> {
        self.app.router.clone().oneshot(req).await.unwrap()
    }

    /// GET /oauth/authorize as the signed-in user; the redirect's Location
    /// (empty when the request was refused with no redirect).
    async fn authorize(&self, client_id: &str, redirect: &str) -> (StatusCode, String) {
        let challenge = URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(VERIFIER.as_bytes()));
        let uri = format!(
            "/oauth/authorize?response_type=code&client_id={client_id}&redirect_uri={}&state=s1&scope=openid%20offline_access&code_challenge={challenge}&code_challenge_method=S256",
            urlencoding::encode(redirect)
        );
        let session = self
            .app
            .auth_service
            .generate_session_token(&self.user)
            .unwrap();
        let req = Request::builder()
            .uri(uri)
            .header("cookie", format!("fc_session={session}"))
            .body(Body::empty())
            .unwrap();
        let resp = self.send(req).await;
        let loc = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        (resp.status(), loc)
    }

    async fn form(
        &self,
        path: &str,
        form: &[(&str, &str)],
        bearer: Option<&str>,
    ) -> (StatusCode, Value) {
        let body = form
            .iter()
            .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/x-www-form-urlencoded");
        if let Some(b) = bearer {
            req = req.header("authorization", format!("Bearer {b}"));
        }
        read_json(self.send(req.body(Body::from(body)).unwrap()).await).await
    }

    /// The fcdev login: access and refresh token.
    async fn fcdev_login(&self) -> (String, String) {
        let (_, loc) = self.authorize(fcdev::FCDEV_CLI_CLIENT_ID, CB).await;
        let code = loc
            .split("code=")
            .nth(1)
            .unwrap_or_else(|| panic!("no code in {loc:?}"))
            .split('&')
            .next()
            .unwrap()
            .to_string();
        let (status, body) = self
            .form(
                "/oauth/token",
                &[
                    ("grant_type", "authorization_code"),
                    ("code", &code),
                    ("redirect_uri", CB),
                    ("client_id", fcdev::FCDEV_CLI_CLIENT_ID),
                    ("code_verifier", VERIFIER),
                ],
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        (
            body["access_token"].as_str().unwrap().to_string(),
            body["refresh_token"].as_str().unwrap().to_string(),
        )
    }

    async fn refresh_works(&self, token: &str) -> bool {
        let (status, _) = self
            .form(
                "/oauth/token",
                &[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", token),
                    ("client_id", fcdev::FCDEV_CLI_CLIENT_ID),
                ],
                None,
            )
            .await;
        status == StatusCode::OK
    }
}

/// R1 end to end: a client registering a wildcard redirect URI. The review's
/// bypass answers 400 with no redirect; the legitimate tenants still get a code.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_wildcard_redirect_uri_cannot_be_bent_to_another_host() {
    let r = rig("wild@login.test").await;
    let client = OAuthClient::new("wild", "Wild")
        .with_grant_type(GrantType::AuthorizationCode)
        .with_redirect_uri("https://*.example.com/callback");
    r.app.repos.oauth_client_repo.insert(&client).await.unwrap();

    for bad in [
        "https://evil%2Ecom/.example.com/callback",
        "https://evil%2Ecom#.example.com/callback",
        "https://evil.com/.example.com/callback",
        "https://a.example.com@evil.com/callback",
        "https://a.b.example.com/callback",
    ] {
        let (status, loc) = r.authorize("wild", bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        assert_eq!(loc, "", "{bad} must not redirect");
    }
    for good in [
        "https://tenant1.example.com/callback",
        "https://t-2.example.com/callback",
    ] {
        let (_, loc) = r.authorize("wild", good).await;
        assert!(loc.starts_with(&format!("{good}?code=")), "{good}: {loc}");
    }
}

/// B1: a refresh token is revoked by its own client or principal, not by any
/// other authenticated client. The earlier behaviour is one env switch away
/// (`FC_OAUTH_REVOKE_ANY_TOKEN`).
#[tokio::test]
#[ignore = "requires Docker"]
async fn revoke_is_confined_to_the_tokens_owner() {
    let r = rig("revoke@login.test").await;
    let other = OAuthClient::new("other-app", "Other").with_grant_type(GrantType::RefreshToken);
    r.app.repos.oauth_client_repo.insert(&other).await.unwrap();

    let (_, refresh) = r.fcdev_login().await;

    // Another client authenticates, names the token, gets the RFC 200, and
    // the token is untouched.
    let (status, _) = r
        .form(
            "/oauth/revoke",
            &[("token", &refresh), ("client_id", "other-app")],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        r.refresh_works(&refresh).await,
        "another client's revoke must not land"
    );
    let (_, again) = {
        // that refresh rotated the token; log in afresh for the next cases
        r.fcdev_login().await
    };

    // A bearer for a different, non-anchor principal cannot either.
    let stranger = Principal::new_user("stranger@login.test", UserScope::Client);
    r.app.repos.principal_repo.insert(&stranger).await.unwrap();
    let stranger_token = r.app.auth_service.generate_access_token(&stranger).unwrap();
    let (status, _) = r
        .form("/oauth/revoke", &[("token", &again)], Some(&stranger_token))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        r.refresh_works(&again).await,
        "a stranger's revoke must not land"
    );
    let (_, again) = r.fcdev_login().await;

    // Its own client revokes it.
    let (status, _) = r
        .form(
            "/oauth/revoke",
            &[("token", &again), ("client_id", fcdev::FCDEV_CLI_CLIENT_ID)],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !r.refresh_works(&again).await,
        "the owning client's revoke lands"
    );

    // Its own principal revokes it (the bearer is the user's access token).
    let (access2, refresh2) = r.fcdev_login().await;
    let (status, _) = r
        .form("/oauth/revoke", &[("token", &refresh2)], Some(&access2))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !r.refresh_works(&refresh2).await,
        "the principal's revoke lands"
    );
}

/// B1: introspection by default still answers for tokens issued to others
/// (resource servers do exactly that).
#[tokio::test]
#[ignore = "requires Docker"]
async fn introspection_keeps_answering_for_other_clients_tokens() {
    let r = rig("introspect@login.test").await;
    let other =
        OAuthClient::new("rs-app", "Resource server").with_grant_type(GrantType::RefreshToken);
    r.app.repos.oauth_client_repo.insert(&other).await.unwrap();
    let (access, _) = r.fcdev_login().await;
    let (status, body) = r
        .form(
            "/oauth/introspect",
            &[("token", &access), ("client_id", "rs-app")],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "{body}");
}
