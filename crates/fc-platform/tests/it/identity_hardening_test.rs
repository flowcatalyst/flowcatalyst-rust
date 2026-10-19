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
use fc_platform::shared::database::seed_fcdev_cli_client;
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
    seed_fcdev_cli_client(&app.pool).await.unwrap();
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

/// B1: a refresh token is revoked by its own client, not by another
/// authenticated client. The earlier behaviour is one env switch away
/// (`FC_OAUTH_REVOKE_ENFORCE_CLIENT=false`).
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

    // A Bearer caller carries no client identity and is not checked (Go
    // parity): it still revokes.
    let (access2, refresh2) = r.fcdev_login().await;
    let (status, _) = r
        .form("/oauth/revoke", &[("token", &refresh2)], Some(&access2))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!r.refresh_works(&refresh2).await, "a bearer's revoke lands");
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

// ── F28: a service account's scope against its client links ─────────────────

mod scope_reach {
    use super::*;
    use fc_platform::client::entity::Client;
    use fc_platform::shared::authorization_service::{AuthContext, Credential};
    use fc_platform::shared::id::PrincipalId;
    use fc_platform::usecase::{ExecutionContext, UseCase};
    use fc_platform::PrincipalType;
    use fc_platform_core::shared::encryption_service::EncryptionService;
    use fc_platform_core::shared::id::ClientId;
    use fc_platform_iam::service_account::operations::create::{
        CreateServiceAccountCommand, CreateServiceAccountUseCase,
    };
    use fc_platform_iam::service_account::operations::update::{
        UpdateServiceAccountCommand, UpdateServiceAccountUseCase,
    };
    use std::collections::HashSet;
    use std::sync::Arc;

    fn anchor() -> ExecutionContext {
        ExecutionContext::from_auth(&AuthContext {
            principal_id: PrincipalId::parse("prn_caller000001").unwrap(),
            principal_type: PrincipalType::User,
            scope: UserScope::Anchor,
            email: Some("anchor@f28.test".to_string()),
            name: "Anchor".to_string(),
            accessible_clients: vec![],
            permissions: HashSet::new(),
            roles: vec![],
            credential: Credential::BearerToken,
        })
    }

    async fn client(app: &TestApp, id: &str) -> ClientId {
        let c = Client::new(id.to_uppercase(), id);
        app.repos.client_repo.insert(&c).await.unwrap();
        ClientId::from_wire(c.id.as_str())
    }

    fn create_cmd(
        code: &str,
        scope: Option<UserScope>,
        clients: Vec<ClientId>,
    ) -> CreateServiceAccountCommand {
        CreateServiceAccountCommand {
            code: code.to_string(),
            name: code.to_string(),
            description: None,
            scope,
            client_ids: clients,
            application_id: None,
            all_applications: false,
        }
    }

    fn update_cmd(
        id: &PrincipalId,
        name: Option<&str>,
        scope: Option<UserScope>,
        clients: Option<Vec<ClientId>>,
    ) -> UpdateServiceAccountCommand {
        UpdateServiceAccountCommand {
            id: id.clone(),
            name: name.map(str::to_string),
            description: None,
            scope,
            client_ids: clients,
            webhook_credentials: None,
        }
    }

    /// Go F28 `TestServiceAccountScope_MustMatchReach`. With the check on, a
    /// scope that disagrees with the links is a 400 SCOPE_REACH_MISMATCH and
    /// nothing is stored; matching or unset scopes pass. Off (the default),
    /// today's behaviour holds.
    #[tokio::test]
    #[ignore = "requires Docker"]
    async fn scope_must_match_the_client_links_when_strict() {
        support::set_app_key();
        let app = TestApp::setup().await;
        let enc = Arc::new(
            EncryptionService::new("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap(),
        );
        let (c1, c2) = (client(&app, "f28a").await, client(&app, "f28b").await);
        let create = |strict: bool| {
            CreateServiceAccountUseCase::new(
                app.repos.service_account_repo.clone(),
                app.repos.client_repo.clone(),
                app.unit_of_work.clone(),
                Some(enc.clone()),
            )
            .with_strict_scope_reach(strict)
        };
        let update = |strict: bool| {
            UpdateServiceAccountUseCase::new(
                app.repos.service_account_repo.clone(),
                app.repos.client_repo.clone(),
                app.unit_of_work.clone(),
                Some(enc.clone()),
            )
            .with_strict_scope_reach(strict)
        };

        // Off: CLIENT with no links is stored, as today.
        let legacy = create(false)
            .run(
                create_cmd("legacy", Some(UserScope::Client), vec![]),
                anchor(),
            )
            .await
            .into_result()
            .expect("accepted while the switch is off");
        let stored = app
            .repos
            .service_account_repo
            .find_by_code("legacy")
            .await
            .unwrap()
            .expect("stored");
        assert_eq!(stored.requested_scope.as_deref(), Some("CLIENT"));

        // On: create refuses every disagreement and stores nothing.
        for (code, scope, links) in [
            ("m1", UserScope::Client, vec![]),
            ("m2", UserScope::Anchor, vec![c1.clone()]),
            ("m3", UserScope::Partner, vec![c1.clone()]),
            ("m4", UserScope::Client, vec![c1.clone(), c2.clone()]),
        ] {
            let err = create(true)
                .run(create_cmd(code, Some(scope), links), anchor())
                .await
                .into_result()
                .err()
                .expect("refused");
            assert_eq!(err.code(), "SCOPE_REACH_MISMATCH", "{code}");
            assert!(
                app.repos
                    .service_account_repo
                    .find_by_code(code)
                    .await
                    .unwrap()
                    .is_none(),
                "{code} stored"
            );
        }
        // ... and accepts agreement and an unset scope.
        let anchored = create(true)
            .run(
                create_cmd("ok-anchor", Some(UserScope::Anchor), vec![]),
                anchor(),
            )
            .await
            .into_result()
            .unwrap();
        create(true)
            .run(
                create_cmd("ok-client", Some(UserScope::Client), vec![c1.clone()]),
                anchor(),
            )
            .await
            .into_result()
            .unwrap();
        create(true)
            .run(
                create_cmd(
                    "ok-partner",
                    Some(UserScope::Partner),
                    vec![c1.clone(), c2.clone()],
                ),
                anchor(),
            )
            .await
            .into_result()
            .unwrap();
        create(true)
            .run(create_cmd("ok-unset", None, vec![c1.clone()]), anchor())
            .await
            .into_result()
            .unwrap();

        // Update: stored ANCHOR beside new links is refused; adding links is
        // refused; ANCHOR with the links cleared in the same call is accepted.
        let id = anchored.principal_id.clone();
        let err = update(true)
            .run(
                update_cmd(&id, None, None, Some(vec![c1.clone()])),
                anchor(),
            )
            .await
            .into_result()
            .unwrap_err();
        assert_eq!(
            err.code(),
            "SCOPE_REACH_MISMATCH",
            "links added to a stored ANCHOR"
        );
        update(true)
            .run(
                update_cmd(&id, None, Some(UserScope::Client), Some(vec![c1.clone()])),
                anchor(),
            )
            .await
            .into_result()
            .expect("scope and links changed together");
        let err = update(true)
            .run(
                update_cmd(&id, None, Some(UserScope::Anchor), None),
                anchor(),
            )
            .await
            .into_result()
            .unwrap_err();
        assert_eq!(
            err.code(),
            "SCOPE_REACH_MISMATCH",
            "ANCHOR with one link remaining"
        );
        update(true)
            .run(
                update_cmd(&id, None, Some(UserScope::Anchor), Some(vec![])),
                anchor(),
            )
            .await
            .into_result()
            .expect("ANCHOR with the links cleared");

        // A rename of a legacy-inconsistent account still works.
        let legacy_id = legacy.principal_id.clone();
        update(true)
            .run(
                update_cmd(&legacy_id, Some("legacy renamed"), None, None),
                anchor(),
            )
            .await
            .into_result()
            .expect("rename leaves the mismatch alone");
        let err = update(true)
            .run(
                update_cmd(&legacy_id, None, Some(UserScope::Client), None),
                anchor(),
            )
            .await
            .into_result()
            .unwrap_err();
        assert_eq!(
            err.code(),
            "SCOPE_REACH_MISMATCH",
            "touching scope re-checks it"
        );
    }
}
