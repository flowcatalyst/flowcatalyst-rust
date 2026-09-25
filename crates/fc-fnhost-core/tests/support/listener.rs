//! The listener harness (a port of Java's `FnHttpTestSupport`, `TestJwks`
//! and the fixture functions of `FnHttpServerTest`): a real [`Reconciler`]
//! fed by the fake control plane, a scripted runtime whose instances
//! implement [`Invoker`] (echo, park, fail, panic, unavailable, …), the
//! listeners on ephemeral ports, and a local OIDC discovery + JWKS server.

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use fc_fnhost_core::clock::{Clock, ManualClock, SharedClock};
use fc_fnhost_core::env::TrustedProxies;
use fc_fnhost_core::host::Listener;
use fc_fnhost_core::invoke::{InvocationContext, InvokeError, Invoker};
use fc_fnhost_core::listener::{FnListener, ListenerConfig};
use fc_fnhost_core::loader::{FunctionInstance, FunctionLoader, LoadOutcome, LoadRequest, Loaders};
use fc_fnhost_core::metrics::FnMetrics;
use fc_fnhost_core::reconciler::Reconciler;
use fc_fnhost_core::registry::FunctionRegistry;
use fc_fnhost_core::signature::Signatures;
use fc_function_abi::{Caller, MultiMap, Response};
use parking_lot::Mutex;
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;
use serde_json::{json, Value};
use tokio::sync::{Notify, Semaphore};

use super::fakes::{self, Answer, FakeControlPlane, FakeStore};

// ── desired-state entries ─────────────────────────────────────────────────

/// A desired-state entry whose manifest declares `endpoints` and runs the
/// scripted behaviour named by `entrypoint`.
pub fn entry(
    address: &str,
    version: i32,
    role: &str,
    entrypoint: &str,
    max_concurrency: i32,
    endpoints: Value,
) -> Value {
    let mut e = fakes::entry(address, version, role, "warm");
    e["manifest"] = json!({
        "runtime": "wasm",
        "entrypoint": entrypoint,
        "limits": {"maxConcurrency": max_concurrency},
        "endpoints": endpoints,
    });
    e
}

pub fn with(mut entry: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        entry[k] = v.clone();
    }
    entry
}

pub fn doc(functions: Vec<Value>) -> Value {
    json!({ "functions": functions })
}

pub fn doc_with_routes(functions: Vec<Value>, routes: Value) -> Value {
    json!({ "functions": functions, "publicRoutes": routes })
}

// ── the scripted runtime ──────────────────────────────────────────────────

/// What the scripted instances report back to a test.
pub struct Probes {
    invocations: Mutex<HashMap<String, usize>>,
    pub started: AtomicUsize,
    started_notify: Notify,
    /// A parked call returns once it gets a permit here.
    pub release: Semaphore,
    pub interrupted: AtomicUsize,
    pub finished: AtomicUsize,
}

impl Default for Probes {
    fn default() -> Self {
        Self {
            invocations: Mutex::new(HashMap::new()),
            started: AtomicUsize::new(0),
            started_notify: Notify::new(),
            release: Semaphore::new(0),
            interrupted: AtomicUsize::new(0),
            finished: AtomicUsize::new(0),
        }
    }
}

impl Probes {
    pub fn invocations(&self, label: &str) -> usize {
        self.invocations.lock().get(label).copied().unwrap_or(0)
    }

    pub fn total_invocations(&self) -> usize {
        self.invocations.lock().values().sum()
    }

    /// Waits until `n` parked calls have started.
    pub async fn await_started(&self, n: usize) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let notified = self.started_notify.notified();
                if self.started.load(Ordering::SeqCst) >= n {
                    return;
                }
                notified.await;
            }
        })
        .await
        .expect("the parked call never started");
    }

    pub fn release_one(&self) {
        self.release.add_permits(1);
    }
}

pub struct ScriptedInstance {
    pub label: String,
    behaviour: String,
    probes: Arc<Probes>,
    pub closed: AtomicBool,
    /// Whether a parked call was still running when close ran.
    pub closed_while_running: AtomicBool,
    running: AtomicUsize,
}

#[async_trait]
impl Invoker for ScriptedInstance {
    async fn invoke(&self, context: InvocationContext) -> Result<Response, InvokeError> {
        *self
            .probes
            .invocations
            .lock()
            .entry(self.label.clone())
            .or_default() += 1;
        tracing::info!("inside the function");
        match self.behaviour.as_str() {
            "echo" => Ok(echo(&self.label, &context)),
            "park" | "park-interruptible" => {
                self.running.fetch_add(1, Ordering::SeqCst);
                self.probes.started.fetch_add(1, Ordering::SeqCst);
                self.probes.started_notify.notify_waiters();
                let interruptible = self.behaviour == "park-interruptible";
                let outcome = tokio::select! {
                    permit = self.probes.release.acquire() => {
                        permit.unwrap().forget();
                        Ok(Response::json(200, json!({"tag": self.label}).to_string()).unwrap())
                    }
                    _ = context.interrupted.cancelled(), if interruptible => {
                        self.probes.interrupted.fetch_add(1, Ordering::SeqCst);
                        Err(InvokeError::Timeout)
                    }
                };
                self.running.fetch_sub(1, Ordering::SeqCst);
                self.probes.finished.fetch_add(1, Ordering::SeqCst);
                outcome
            }
            "fail" => Err(InvokeError::Failed(
                "do-not-leak-this-message-to-the-caller".into(),
            )),
            "panic" => panic!("do-not-leak-this-panic-to-the-caller"),
            "unavailable" => Err(InvokeError::Unavailable("no instance".into())),
            "timeout" => Err(InvokeError::Timeout),
            "cors-setter" => {
                let mut headers = MultiMap::new();
                headers.insert(
                    "access-control-allow-origin".into(),
                    vec!["https://attacker.example.com".into()],
                );
                headers.insert(
                    "access-control-allow-credentials".into(),
                    vec!["false".into()],
                );
                Ok(Response::http(200, headers, b"{}".to_vec()).unwrap())
            }
            other => panic!("unknown scripted behaviour {other}"),
        }
    }
}

#[async_trait]
impl FunctionInstance for ScriptedInstance {
    async fn close(&self) {
        if self.running.load(Ordering::SeqCst) > 0 {
            self.closed_while_running.store(true, Ordering::SeqCst);
        }
        self.closed.store(true, Ordering::SeqCst);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Everything the function saw, as JSON; the status from `?status=`.
fn echo(label: &str, c: &InvocationContext) -> Response {
    let caller = match &c.caller {
        Caller::Platform => json!({"kind": "Platform"}),
        Caller::Anonymous => json!({"kind": "Anonymous"}),
        Caller::Principal(p) => json!({
            "kind": "Principal",
            "id": p.id,
            "type": p.principal_type,
            "tier": p.tier,
            "clientId": p.client_id(),
            "clients": p.clients,
            "roles": p.roles,
            "applications": p.applications,
            "allApplications": p.all_applications,
            "permissions": p.permissions,
        }),
    };
    let body = json!({
        "label": label,
        "invocationId": c.invocation_id,
        "address": c.address.render(),
        "version": c.version,
        "method": c.method,
        "path": c.path,
        "originalPath": c.original_path,
        "originalHost": c.original_host,
        "pathParams": c.path_params,
        "query": c.query,
        "headers": c.headers,
        "bodyLength": c.body.len(),
        "remoteAddress": c.remote_address,
        "caller": caller,
        "correlationId": c.correlation_id,
        "causationId": c.causation_id,
        "remainingMs": c.remaining().as_millis() as u64,
    });
    let status = c
        .query
        .get("status")
        .and_then(|v| v.first())
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let mut headers = MultiMap::new();
    headers.insert("X-Multi".into(), vec!["a".into(), "b".into()]);
    headers.insert("Connection".into(), vec!["keep-alive".into()]);
    headers.insert("Content-Length".into(), vec!["999".into()]);
    headers.insert("Content-Type".into(), vec!["application/json".into()]);
    Response::http(status, headers, body.to_string().into_bytes()).unwrap()
}

#[derive(Default)]
pub struct ScriptedLoader {
    pub probes: Arc<Probes>,
    pub refusals: Mutex<Vec<String>>,
    pub loads: Mutex<Vec<String>>,
    pub instances: Mutex<Vec<Arc<ScriptedInstance>>>,
    pub delay: Mutex<Option<Duration>>,
}

impl ScriptedLoader {
    pub fn load_count(&self, label: &str) -> usize {
        self.loads.lock().iter().filter(|l| *l == label).count()
    }

    pub fn instance(&self, label: &str) -> Arc<ScriptedInstance> {
        self.instances
            .lock()
            .iter()
            .rev()
            .find(|i| i.label == label)
            .cloned()
            .unwrap_or_else(|| panic!("{label} was never loaded"))
    }
}

#[async_trait]
impl FunctionLoader for ScriptedLoader {
    async fn load(&self, request: LoadRequest<'_>) -> LoadOutcome {
        let label = format!("{}@{}", request.entry.address, request.entry.version);
        let delay = *self.delay.lock();
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        if self.refusals.lock().contains(&label) {
            return LoadOutcome::Refused {
                reason: "WASM_INVALID".into(),
                detail: "scripted".into(),
            };
        }
        self.loads.lock().push(label.clone());
        let instance = Arc::new(ScriptedInstance {
            label,
            behaviour: request.entry.manifest.entrypoint.clone(),
            probes: self.probes.clone(),
            closed: AtomicBool::new(false),
            closed_while_running: AtomicBool::new(false),
            running: AtomicUsize::new(0),
        });
        self.instances.lock().push(instance.clone());
        LoadOutcome::Loaded(instance)
    }
}

// ── the harness ───────────────────────────────────────────────────────────

pub struct Options {
    pub max_concurrency: i32,
    pub platform_url: Option<String>,
    pub public: bool,
    pub max_loaded: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_concurrency: 512,
            platform_url: None,
            public: true,
            max_loaded: 50,
        }
    }
}

pub struct Harness {
    pub control: Arc<FakeControlPlane>,
    pub store: Arc<FakeStore>,
    pub reconciler: Arc<Reconciler>,
    pub metrics: Arc<FnMetrics>,
    pub listener: Arc<FnListener>,
    pub loader: Arc<ScriptedLoader>,
    pub clock: ManualClock,
    pub client: reqwest::Client,
    pub base: String,
    pub public_base: Option<String>,
}

pub struct Reply {
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&self.body)))
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .get(name)
            .map(|v| v.to_str().unwrap().to_owned())
    }

    pub fn headers_all(&self, name: &str) -> Vec<String> {
        self.headers
            .get_all(name)
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect()
    }

    pub fn error(&self) -> String {
        self.json()["error"].as_str().unwrap_or_default().to_owned()
    }
}

impl Harness {
    pub async fn start(document: Value) -> Self {
        Self::start_with(document, Options::default()).await
    }

    pub async fn start_with(document: Value, options: Options) -> Self {
        let clock = ManualClock::new(Utc::now());
        let shared: SharedClock = Arc::new(clock.clone());
        let control = FakeControlPlane::new();
        let loader = Arc::new(ScriptedLoader::default());
        let registry = Arc::new(FunctionRegistry::new(options.max_loaded, shared.clone()));
        let store = FakeStore::new();
        let reconciler = Arc::new(Reconciler::new(
            "default",
            "host-1",
            control.clone(),
            store.clone(),
            Signatures::Off,
            Loaders::none().with("wasm", loader.clone()),
            registry.clone(),
        ));
        let metrics = Arc::new(FnMetrics::new(registry));
        control.serve(Answer::Document(fakes::document_json(document)));
        reconciler.reconcile_once(clock.now()).await;
        let listener = Arc::new(FnListener::new(ListenerConfig {
            bind: "127.0.0.1".parse().unwrap(),
            port: 0,
            public_port: options.public.then_some(0),
            max_concurrency: options.max_concurrency,
            platform_url: options
                .platform_url
                .unwrap_or_else(|| "http://127.0.0.1:1".into()),
            trusted_proxies: TrustedProxies::default_list(),
            clock: shared,
        }));
        listener
            .start(reconciler.clone(), metrics.clone())
            .await
            .unwrap();
        let base = format!("http://127.0.0.1:{}", listener.port().unwrap());
        let public_base = listener
            .public_port()
            .map(|p| format!("http://127.0.0.1:{p}"));
        Self {
            control,
            store,
            reconciler,
            metrics,
            listener,
            loader,
            clock,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            base,
            public_base,
        }
    }

    pub fn probes(&self) -> &Arc<Probes> {
        &self.loader.probes
    }

    /// A new document, then one reconcile.
    pub async fn publish(&self, document: Value) {
        self.control
            .serve(Answer::Document(fakes::document_json(document)));
        self.reconciler.reconcile_once(self.clock.now()).await;
    }

    pub async fn send(&self, request: reqwest::RequestBuilder) -> Reply {
        let response = request.send().await.expect("the request completes");
        Reply {
            status: response.status().as_u16(),
            headers: response.headers().clone(),
            body: response.bytes().await.unwrap().to_vec(),
        }
    }

    pub async fn get(&self, path: &str, headers: &[(&str, &str)]) -> Reply {
        let mut request = self.client.get(format!("{}{path}", self.base));
        for (k, v) in headers {
            request = request.header(*k, *v);
        }
        self.send(request).await
    }

    pub async fn post(&self, path: &str, body: &[u8], headers: &[(&str, &str)]) -> Reply {
        let mut request = self
            .client
            .post(format!("{}{path}", self.base))
            .body(body.to_vec());
        for (k, v) in headers {
            request = request.header(*k, *v);
        }
        self.send(request).await
    }

    /// A request to the public listener with an explicit `Host`.
    pub async fn public(
        &self,
        method: &str,
        host: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> Reply {
        let port = self.listener.public_port().expect("a public listener");
        raw(
            port,
            method,
            path,
            &[&[("Host", host)], headers].concat(),
            b"",
        )
        .await
    }

    pub fn function_permits(&self, address: &str) -> Option<usize> {
        self.listener
            .permits()
            .unwrap()
            .function_available(&fc_function_abi::FunctionAddress::parse(address).unwrap())
    }

    pub fn host_permits(&self) -> usize {
        self.listener.permits().unwrap().host_available()
    }

    pub fn scrape(&self) -> String {
        self.metrics.encode().unwrap()
    }

    pub async fn close(&self, timeout: Duration) {
        self.listener.drain().await;
        self.listener.close(timeout).await;
    }
}

/// A plain HTTP/1.1 exchange over a raw socket (Java `RawHttpClient`): any
/// `Host`, any header, and nothing written that the test did not ask for.
pub async fn raw(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Reply {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    // Read the head, then exactly `Content-Length` body bytes: the server
    // may keep the connection open (draining a refused body).
    let mut out = Vec::new();
    let read = async {
        let mut chunk = [0u8; 8192];
        loop {
            if let Some(end) = out.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&out[..end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map_or(0, |v| v.trim().parse::<usize>().unwrap());
                if out.len() >= end + 4 + length {
                    return;
                }
            }
            let n = stream.read(&mut chunk).await.unwrap();
            if n == 0 {
                return;
            }
            out.extend_from_slice(&chunk[..n]);
        }
    };
    tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .expect("a response within 10 s");
    parse_response(&out)
}

fn parse_response(bytes: &[u8]) -> Reply {
    let split = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no header end in {:?}", String::from_utf8_lossy(bytes)));
    let head = String::from_utf8_lossy(&bytes[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut headers = reqwest::header::HeaderMap::new();
    for line in lines {
        let (k, v) = line.split_once(':').unwrap();
        headers.append(
            reqwest::header::HeaderName::from_bytes(k.trim().as_bytes()).unwrap(),
            v.trim().parse().unwrap(),
        );
    }
    Reply {
        status,
        headers,
        body: bytes[split + 4..].to_vec(),
    }
}

// ── webhook signing ───────────────────────────────────────────────────────

/// `yyyy-MM-ddTHH:mm:ss.SSSZ`, as the platform's `WebhookSigner.timestamp`.
pub fn timestamp(at: chrono::DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub fn signed(secret: &str, ts: &str, body: &[u8]) -> String {
    fc_fnhost_core::listener::webhook::sign(secret, ts, body)
}

// ── the platform's discovery document and JWKS ───────────────────────────

/// Test keys, generated once per test binary (RSA key generation is slow
/// in a debug build).
fn key(n: usize) -> RsaPrivateKey {
    static KEYS: OnceLock<Vec<RsaPrivateKey>> = OnceLock::new();
    KEYS.get_or_init(|| {
        (0..3)
            .map(|_| RsaPrivateKey::new(&mut rand_core::OsRng, 1024).unwrap())
            .collect()
    })[n]
        .clone()
}

pub fn foreign_key() -> RsaPrivateKey {
    key(2)
}

/// Claims for [`TestJwks::mint`].
#[derive(Clone)]
pub struct Claims {
    pub subject: String,
    pub principal_type: String,
    pub tier: String,
    pub scope: String,
    pub clients: Vec<String>,
    pub roles: Vec<String>,
    pub applications: Vec<String>,
    pub all_applications: bool,
    pub token_use: Option<String>,
    pub expires_in_seconds: i64,
}

impl Claims {
    pub fn new(subject: &str, tier: &str, scope: &str, clients: &[&str]) -> Self {
        Self {
            subject: subject.into(),
            principal_type: "SERVICE".into(),
            tier: tier.into(),
            scope: scope.into(),
            clients: clients.iter().map(|s| s.to_string()).collect(),
            roles: Vec::new(),
            applications: Vec::new(),
            all_applications: true,
            token_use: None,
            expires_in_seconds: 300,
        }
    }

    pub fn applications(mut self, applications: &[&str], all: bool) -> Self {
        self.applications = applications.iter().map(|s| s.to_string()).collect();
        self.all_applications = all;
        self
    }

    pub fn token_use(mut self, token_use: &str) -> Self {
        self.token_use = Some(token_use.into());
        self
    }

    pub fn roles(mut self, roles: &[&str]) -> Self {
        self.roles = roles.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn expires_in(mut self, seconds: i64) -> Self {
        self.expires_in_seconds = seconds;
        self
    }
}

struct JwksState {
    /// `kid` → key currently published.
    published: Mutex<Vec<(String, RsaPrivateKey)>>,
    current: Mutex<(String, RsaPrivateKey)>,
    self_url: Mutex<String>,
    discovery_down: AtomicBool,
    foreign_jwks_uri: Mutex<Option<String>>,
    discovery_requests: AtomicUsize,
    jwks_requests: AtomicUsize,
}

/// A fake platform for bearer auth (Java `TestJwks`): its own address is
/// the `platformUrl`; the discovery document's `issuer` is deliberately
/// something else.
pub struct TestJwks {
    pub url: String,
    pub discovery_issuer: String,
    state: Arc<JwksState>,
}

impl TestJwks {
    pub async fn start() -> Self {
        let state = Arc::new(JwksState {
            published: Mutex::new(vec![("kid-1".into(), key(0))]),
            current: Mutex::new(("kid-1".into(), key(0))),
            self_url: Mutex::new(String::new()),
            discovery_down: AtomicBool::new(false),
            foreign_jwks_uri: Mutex::new(None),
            discovery_requests: AtomicUsize::new(0),
            jwks_requests: AtomicUsize::new(0),
        });
        let discovery = {
            let state = state.clone();
            move || {
                let state = state.clone();
                async move {
                    state.discovery_requests.fetch_add(1, Ordering::SeqCst);
                    if state.discovery_down.load(Ordering::SeqCst) {
                        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, String::new());
                    }
                    let jwks_uri = state.foreign_jwks_uri.lock().clone().unwrap_or_else(|| {
                        format!("{}/.well-known/jwks.json", state.self_url.lock())
                    });
                    (
                        axum::http::StatusCode::OK,
                        json!({"issuer": "https://platform.example.test", "jwks_uri": jwks_uri})
                            .to_string(),
                    )
                }
            }
        };
        let jwks = {
            let state = state.clone();
            move || {
                let state = state.clone();
                async move {
                    state.jwks_requests.fetch_add(1, Ordering::SeqCst);
                    let keys: Vec<Value> = state
                        .published
                        .lock()
                        .iter()
                        .map(|(kid, key)| jwk(kid, key))
                        .collect();
                    json!({ "keys": keys }).to_string()
                }
            }
        };
        let app = axum::Router::new()
            .route(
                "/.well-known/openid-configuration",
                axum::routing::get(discovery),
            )
            .route("/.well-known/jwks.json", axum::routing::get(jwks));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        *state.self_url.lock() = url.clone();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            discovery_issuer: "https://platform.example.test".into(),
            state,
        }
    }

    pub fn jwks_requests(&self) -> usize {
        self.state.jwks_requests.load(Ordering::SeqCst)
    }

    pub fn discovery_requests(&self) -> usize {
        self.state.discovery_requests.load(Ordering::SeqCst)
    }

    /// A new current key under a new `kid`, published beside the old one.
    pub fn rotate(&self) {
        let next = ("kid-2".to_owned(), key(1));
        self.state.published.lock().push(next.clone());
        *self.state.current.lock() = next;
    }

    pub fn break_discovery(&self) {
        self.state.discovery_down.store(true, Ordering::SeqCst);
    }

    pub fn fix_discovery(&self) {
        self.state.discovery_down.store(false, Ordering::SeqCst);
    }

    pub fn use_foreign_jwks_uri(&self, uri: &str) {
        *self.state.foreign_jwks_uri.lock() = Some(uri.to_owned());
    }

    /// Signed with the current key, `iss` = the discovered issuer.
    pub fn mint(&self, claims: &Claims) -> String {
        let (kid, key) = self.state.current.lock().clone();
        mint(&key, &kid, &self.discovery_issuer, claims)
    }

    pub fn mint_with_issuer(&self, issuer: &str, claims: &Claims) -> String {
        let (kid, key) = self.state.current.lock().clone();
        mint(&key, &kid, issuer, claims)
    }
}

fn jwk(kid: &str, key: &RsaPrivateKey) -> Value {
    json!({
        "kty": "RSA", "use": "sig", "alg": "RS256", "kid": kid,
        "n": URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),
        "e": URL_SAFE_NO_PAD.encode(key.e().to_bytes_be()),
    })
}

pub fn mint(key: &RsaPrivateKey, kid: &str, issuer: &str, claims: &Claims) -> String {
    use rsa::signature::{SignatureEncoding, Signer};
    let header = json!({"alg": "RS256", "kid": kid, "typ": "JWT"});
    let now = Utc::now().timestamp();
    let payload = json!({
        "iss": issuer,
        "sub": claims.subject,
        "iat": now,
        "exp": now + claims.expires_in_seconds,
        "type": claims.principal_type,
        "tier": claims.tier,
        "scope": claims.scope,
        "clients": claims.clients,
        "roles": claims.roles,
        "applications": claims.applications,
        "all_applications": claims.all_applications,
        "token_use": claims.token_use,
    });
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(payload.to_string())
    );
    let signer = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(key.clone());
    let signature = signer.sign(input.as_bytes()).to_vec();
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
}

/// The peer address a test's own connections come from.
pub fn loopback() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}
