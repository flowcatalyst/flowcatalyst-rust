//! SSO login CSRF: the OIDC login state is bound to the browser that
//! started it (Go bridge `login_binding.go`, docs F16 follow-up; Go
//! internal/platform/auth/bridge/sso_binding_pg_test.go), through the whole
//! platform router and a real OIDC handshake against a test identity
//! provider served in-process.
//!
//! Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, Response, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tower::ServiceExt;

use fc_platform::auth::auth_service::{AuthConfig, AuthService};
use fc_platform::auth::login_binding;
use fc_platform::identity_provider::entity::{IdentityProvider, IdentityProviderType};
use fc_platform::ScopeType;
use support::{read_json, TestApp};

const CLIENT_ID: &str = "sso-binding-client";
const DOMAIN: &str = "sso-binding.test";

/// A test identity provider: discovery, keys, and a token endpoint that
/// answers an ID token for the nonce of the login in flight.
struct Idp {
    base: String,
    nonce: Arc<Mutex<String>>,
}

#[derive(Clone)]
struct IdpState {
    base: String,
    jwks: Value,
    kid: String,
    private_pem: String,
    nonce: Arc<Mutex<String>>,
}

async fn token(State(s): State<IdpState>) -> Json<Value> {
    let now = chrono::Utc::now().timestamp();
    let nonce = s.nonce.lock().unwrap().clone();
    let claims = json!({
        "iss": s.base, "aud": CLIENT_ID, "sub": "sso-binding-subject",
        "email": format!("attacker@{DOMAIN}"), "name": "Attacker",
        "nonce": nonce, "iat": now, "exp": now + 300,
    });
    let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(s.kid.clone());
    let id_token = encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(s.private_pem.as_bytes()).unwrap(),
    )
    .unwrap();
    Json(json!({ "access_token": "at", "token_type": "Bearer", "id_token": id_token }))
}

async fn start_idp() -> Idp {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (private_pem, public_pem) = AuthConfig::load_or_generate_rsa_keys(None, None).unwrap();
    let keys = AuthService::new(AuthConfig {
        rsa_private_key: Some(private_pem.clone()),
        rsa_public_key: Some(public_pem),
        rsa_public_key_previous: None,
        secret_key: String::new(),
        issuer: base.clone(),
        audience: base.clone(),
        access_token_expiry_secs: 60,
        session_token_expiry_secs: 60,
        refresh_token_expiry_secs: 60,
    });
    let (kid, components) = keys.all_jwks_keys()[0];
    let state = IdpState {
        base: base.clone(),
        jwks: json!({ "keys": [{
            "kty": "RSA", "use": "sig", "alg": "RS256", "kid": kid,
            "n": components.n, "e": components.e,
        }]}),
        kid: kid.to_string(),
        private_pem,
        nonce: Arc::new(Mutex::new(String::new())),
    };
    let nonce = state.nonce.clone();
    let app = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(|State(s): State<IdpState>| async move {
                Json(json!({
                    "issuer": s.base,
                    "authorization_endpoint": format!("{}/authorize", s.base),
                    "token_endpoint": format!("{}/token", s.base),
                    "jwks_uri": format!("{}/jwks", s.base),
                }))
            }),
        )
        .route(
            "/jwks",
            get(|State(s): State<IdpState>| async move { Json(s.jwks) }),
        )
        .route("/token", post(token))
        .with_state(state);
    tokio::spawn(async move { axum::serve(listener, app).await });
    Idp { base, nonce }
}

async fn send(app: &TestApp, path: &str, cookie: Option<String>) -> Response<Body> {
    let mut b = Request::get(path).header(header::HOST, "platform.test");
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    app.router
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn query_param(url: &str, name: &str) -> String {
    url.split_once('?')
        .unwrap()
        .1
        .split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{name}=")))
        .map(|v| urlencoding::decode(v).unwrap().into_owned())
        .unwrap()
}

fn set_cookies(resp: &Response<Body>) -> Vec<String> {
    resp.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect()
}

fn signs_in(resp: &Response<Body>) -> bool {
    set_cookies(resp)
        .iter()
        .any(|c| c.starts_with("fc_session=") && !c.starts_with("fc_session=;"))
}

/// Start a login: the IdP redirect's state, and the binding cookie the
/// start set (as a `Cookie` request header).
async fn start(app: &TestApp, idp: &Idp) -> (String, Option<String>) {
    let resp = send(app, &format!("/auth/oidc/login?domain={DOMAIN}"), None).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let to_idp = resp.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        to_idp.starts_with(&format!("{}/authorize?", idp.base)),
        "{to_idp}"
    );
    *idp.nonce.lock().unwrap() = query_param(&to_idp, "nonce");
    let state = query_param(&to_idp, "state");
    let binding = set_cookies(&resp)
        .into_iter()
        .find(|c| c.starts_with("fc_sso_"))
        .map(|c| c.split(';').next().unwrap().to_string());
    (state, binding)
}

fn callback(state: &str) -> String {
    format!("/auth/oidc/callback?state={state}&code=the-code")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_sso_callback_completes_only_in_the_browser_that_started_it() {
    let app = TestApp::setup().await;
    let idp = start_idp().await;
    let mut provider = IdentityProvider::new("sso-binding", "SSO", IdentityProviderType::Oidc);
    provider.oidc_issuer_url = Some(idp.base.clone());
    provider.oidc_client_id = Some(CLIENT_ID.into());
    app.repos.idp_repo.insert(&provider).await.unwrap();
    let mapping =
        fc_platform::EmailDomainMapping::new(DOMAIN, provider.id.clone(), ScopeType::Anchor);
    app.repos.edm_repo.insert(&mapping).await.unwrap();

    // The attack: the attacker starts a login and signs in at the IdP, then
    // sends the callback link to a victim, whose browser never started it.
    let (state, binding) = start(&app, &idp).await;
    let resp = send(&app, &callback(&state), None).await;
    assert!(
        !signs_in(&resp),
        "a browser that never started the login was signed in"
    );
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let (_, body) = read_json(resp).await;
    assert_eq!(
        body,
        json!({
            "error": "LOGIN_NOT_STARTED_HERE",
            "message": "This sign-in was not started in this browser. Start the sign-in again.",
        })
    );
    // The refused link is spent: even the starting browser cannot finish it.
    let resp = send(&app, &callback(&state), binding.clone()).await;
    assert!(!signs_in(&resp));

    // The binding: one cookie per login, HttpOnly, Lax, ten minutes.
    let set = {
        let resp = send(&app, &format!("/auth/oidc/login?domain={DOMAIN}"), None).await;
        set_cookies(&resp)
            .into_iter()
            .find(|c| c.starts_with("fc_sso_"))
            .expect("the start sets the binding cookie")
    };
    for part in ["HttpOnly", "SameSite=Lax", "Path=/", "Max-Age=600"] {
        assert!(set.contains(part), "{set}");
    }
    assert!(!set.contains("Secure"), "plain-http test platform: {set}");

    // Another login's cookie, and a forged value, do not bind.
    let (_, first_binding) = start(&app, &idp).await;
    let (second, _) = start(&app, &idp).await;
    let resp = send(&app, &callback(&second), first_binding.clone()).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(!signs_in(&resp));
    let (third, _) = start(&app, &idp).await;
    let forged = format!("{}=forged", login_binding::cookie_name(&third, false));
    let resp = send(&app, &callback(&third), Some(forged)).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(!signs_in(&resp));

    // The browser that started the login finishes it, and the binding is
    // cleared.
    let (state, binding) = start(&app, &idp).await;
    let resp = send(&app, &callback(&state), binding.clone()).await;
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "{:?}",
        set_cookies(&resp)
    );
    assert!(signs_in(&resp), "{:?}", set_cookies(&resp));
    let name = login_binding::cookie_name(&state, false);
    assert!(
        set_cookies(&resp)
            .iter()
            .any(|c| c.starts_with(&format!("{name}=;")) && c.contains("Max-Age=0")),
        "{:?}",
        set_cookies(&resp)
    );
}
