//! A fake platform for tests (Java `TestJwks`): discovery and JWKS served
//! on a loopback port, RS256 tokens minted with its key. Its own address is
//! the `platformUrl`; the discovery document's `issuer` is deliberately
//! something else, as in production (a Service Connect alias vs the
//! external URL).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use axum::routing;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use parking_lot::Mutex;
use rsa::pkcs1v15::SigningKey;
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;
use serde_json::{json, Value};
use tokio::net::TcpListener;

/// The issuer the fake platform's discovery document names.
pub const ISSUER: &str = "https://platform.example.test";

/// Its authorize URL, as a browser would be sent to.
pub const AUTHORIZATION_ENDPOINT: &str = "https://platform.example.test/oauth/authorize";

fn key(n: usize) -> RsaPrivateKey {
    static KEYS: OnceLock<Vec<RsaPrivateKey>> = OnceLock::new();
    KEYS.get_or_init(|| {
        (0..2)
            .map(|_| RsaPrivateKey::new(&mut rand_core::OsRng, 1024).unwrap())
            .collect()
    })[n]
        .clone()
}

/// A key the fake platform never publishes.
pub fn foreign_key() -> RsaPrivateKey {
    key(1)
}

/// The claims of a minted token. [`Claims::api`] is an authority-bearing
/// API token (`token_use = api`) holding `scope`.
#[derive(Clone, Debug)]
pub struct Claims {
    pub subject: String,
    pub principal_type: String,
    pub tier: String,
    pub scope: Option<String>,
    pub token_use: Option<String>,
    pub expires_in_seconds: i64,
    pub audience: Option<String>,
}

impl Claims {
    /// An API access token (`token_use = api`) with these permissions.
    pub fn api(scope: &[&str]) -> Self {
        Self {
            subject: "prn_test".into(),
            principal_type: "USER".into(),
            tier: "ANCHOR".into(),
            scope: Some(scope.join(" ")).filter(|s| !s.is_empty()),
            token_use: Some("api".into()),
            expires_in_seconds: 300,
            audience: None,
        }
    }

    pub fn token_use(mut self, token_use: Option<&str>) -> Self {
        self.token_use = token_use.map(str::to_owned);
        self
    }

    pub fn expires_in(mut self, seconds: i64) -> Self {
        self.expires_in_seconds = seconds;
        self
    }

    pub fn audience(mut self, audience: &str) -> Self {
        self.audience = Some(audience.to_owned());
        self
    }
}

struct State {
    self_url: Mutex<String>,
    jwks_requests: AtomicUsize,
}

/// A running fake platform. Drop it and the server keeps running until the
/// test's runtime ends (the servers are tiny).
pub struct TestPlatform {
    /// Where the platform is reached: the verifier's `platformUrl`.
    pub url: String,
    state: Arc<State>,
}

impl TestPlatform {
    /// Discovery and JWKS only.
    pub async fn start() -> Self {
        Self::start_with(axum::Router::new()).await
    }

    /// Discovery and JWKS, plus `extra` routes (e.g. a fake `/oauth/token`).
    pub async fn start_with(extra: axum::Router) -> Self {
        let state = Arc::new(State {
            self_url: Mutex::new(String::new()),
            jwks_requests: AtomicUsize::new(0),
        });
        let discovery = {
            let state = state.clone();
            move || {
                let state = state.clone();
                async move {
                    let base = state.self_url.lock().clone();
                    axum::Json(json!({
                        "issuer": ISSUER,
                        "jwks_uri": format!("{base}/.well-known/jwks.json"),
                        "authorization_endpoint": AUTHORIZATION_ENDPOINT,
                        "token_endpoint": format!("{ISSUER}/oauth/token"),
                    }))
                }
            }
        };
        let jwks = {
            let state = state.clone();
            move || {
                let state = state.clone();
                async move {
                    state.jwks_requests.fetch_add(1, Ordering::SeqCst);
                    axum::Json(json!({ "keys": [jwk("kid-1", &key(0))] }))
                }
            }
        };
        let app = axum::Router::new()
            .route("/.well-known/openid-configuration", routing::get(discovery))
            .route("/.well-known/jwks.json", routing::get(jwks))
            .merge(extra);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        *state.self_url.lock() = url.clone();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, state }
    }

    pub fn jwks_requests(&self) -> usize {
        self.state.jwks_requests.load(Ordering::SeqCst)
    }

    /// Signed with the published key, `iss` = the discovered issuer.
    pub fn mint(&self, claims: &Claims) -> String {
        mint(&key(0), "kid-1", ISSUER, claims)
    }

    /// Signed with the published key under another issuer.
    pub fn mint_with_issuer(&self, issuer: &str, claims: &Claims) -> String {
        mint(&key(0), "kid-1", issuer, claims)
    }

    /// Signed with a key the platform does not publish, under its `kid`.
    pub fn mint_forged(&self, claims: &Claims) -> String {
        mint(&foreign_key(), "kid-1", ISSUER, claims)
    }
}

fn jwk(kid: &str, key: &RsaPrivateKey) -> Value {
    json!({
        "kty": "RSA", "use": "sig", "alg": "RS256", "kid": kid,
        "n": URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),
        "e": URL_SAFE_NO_PAD.encode(key.e().to_bytes_be()),
    })
}

/// An RS256 JWT over `claims`.
pub fn mint(key: &RsaPrivateKey, kid: &str, issuer: &str, claims: &Claims) -> String {
    use rsa::signature::{SignatureEncoding, Signer};
    let header = json!({"alg": "RS256", "kid": kid, "typ": "JWT"});
    let now = Utc::now().timestamp();
    let mut payload = json!({
        "iss": issuer,
        "sub": claims.subject,
        "iat": now,
        "exp": now + claims.expires_in_seconds,
        "type": claims.principal_type,
        "tier": claims.tier,
        "clients": ["*"],
        "roles": [],
        "applications": [],
    });
    let obj = payload.as_object_mut().unwrap();
    if let Some(scope) = &claims.scope {
        obj.insert("scope".into(), json!(scope));
    }
    if let Some(token_use) = &claims.token_use {
        obj.insert("token_use".into(), json!(token_use));
    }
    if let Some(aud) = &claims.audience {
        obj.insert("aud".into(), json!(aud));
    }
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(payload.to_string())
    );
    let signer = SigningKey::<sha2::Sha256>::new(key.clone());
    let signature = signer.sign(input.as_bytes()).to_vec();
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
}
