//! The JS harness: the committed test bundles (verified against
//! `SHA256SUMS`), desired-state entries that point at them through
//! `file://`, and a host of the real pieces (artifact cache, `Reconciler`,
//! `JsLoader`, `FnListener`) fed by fc-fnhost-core's fake control plane.

#![allow(dead_code)]

#[path = "../../../fc-fnhost-core/tests/support/fakes.rs"]
pub mod fakes;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use fc_fnhost_core::artifact::{ArtifactCache, ArtifactStores, FileSource, DEFAULT_MAX_BYTES};
use fc_fnhost_core::clock::SystemClock;
use fc_fnhost_core::env::TrustedProxies;
use fc_fnhost_core::exec::ExecBudget;
use fc_fnhost_core::host::Listener;
use fc_fnhost_core::listener::{FnListener, ListenerConfig};
use fc_fnhost_core::loader::Loaders;
use fc_fnhost_core::metrics::FnMetrics;
use fc_fnhost_core::reconciler::Reconciler;
use fc_fnhost_core::registry::FunctionRegistry;
use fc_fnhost_core::signature::Signatures;
use fc_fnhost_core::wasm::{EngineSettings, WasmLoader, WasmRuntime, WasmSettings};
use fc_fnhost_js::{JsLoader, JsRuntime, JsSettings};
use serde_json::{json, Value};
use sha2::Digest as _;

use fakes::{Answer, FakeControlPlane};
use fc_fnhost_core::clock::SharedClock;
use fc_fnhost_core::db::DbSettings;
use reqwest::header::HeaderMap;
use std::fs;

pub const ADDR: &str = "app.orders.ship";

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/js")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

/// `name → sha256 hex` from the committed `SHA256SUMS`.
pub fn sums() -> Vec<(String, String)> {
    fs::read_to_string(fixtures_dir().join("SHA256SUMS"))
        .expect("SHA256SUMS is committed")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let (hash, name) = line.split_once("  ").expect("`<hash>  <name>` lines");
            (name.to_owned(), hash.to_owned())
        })
        .collect()
}

/// A committed bundle, verified against `SHA256SUMS`.
pub fn bundle(file: &str) -> PathBuf {
    let path = fixtures_dir().join(file);
    let bytes = fs::read(&path).unwrap_or_else(|_| panic!("{} is committed", path.display()));
    let expected = sums()
        .into_iter()
        .find(|(n, _)| n == file)
        .unwrap_or_else(|| panic!("{file} is not in SHA256SUMS"))
        .1;
    assert_eq!(
        sha256_hex(&bytes),
        expected,
        "{file} does not match SHA256SUMS: update it with `shasum -a 256`"
    );
    path
}

/// A committed WASM test guest of fc-fnhost-core's
/// (`tests/fixtures/wasm/<name>.wasm`), for hosts with `Options::wasm`.
pub fn wasm_guest(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fc-fnhost-core/tests/fixtures/wasm")
        .join(format!("{name}.wasm"))
}

/// A manifest for a component function (`wasi:http`): every path, no auth,
/// and `extra` merged on top.
pub fn wasm_manifest(extra: Value) -> Value {
    let mut manifest = json!({
        "runtime": "wasm",
        "entrypoint": "wasi:http/incoming-handler",
        "endpoints": [{"path": "/*", "auth": "none"}],
        "limits": {"maxConcurrency": 8, "wasmMemoryMb": 16},
    });
    for (k, v) in extra.as_object().unwrap() {
        manifest[k] = v.clone();
    }
    manifest
}

/// A manifest for a JS function: every path, no auth, and `extra` merged
/// on top.
pub fn manifest(extra: Value) -> Value {
    let mut manifest = json!({
        "runtime": "js",
        "entrypoint": "default",
        "endpoints": [{"path": "/*", "auth": "none"}],
        "limits": {"maxConcurrency": 8, "wasmMemoryMb": 32},
    });
    for (k, v) in extra.as_object().unwrap() {
        manifest[k] = v.clone();
    }
    manifest
}

/// A live, warm desired-state entry for `artifact`, `extra` merged in.
pub fn entry(address: &str, version: i32, artifact: &Path, manifest: Value, extra: Value) -> Value {
    let bytes = fs::read(artifact).unwrap();
    let mut e = fakes::entry(address, version, "live", "warm");
    e["digest"] = json!(format!("sha256:{}", sha256_hex(&bytes)));
    e["artifactRef"] = json!(format!("file://{}", artifact.display()));
    e["manifest"] = manifest;
    for (k, v) in extra.as_object().unwrap() {
        e[k] = v.clone();
    }
    e
}

pub struct Options {
    /// `FC_FN_MAX_EXECUTING`: the executing budget, and (unless
    /// `js_workers`) the JS workers.
    pub max_executing: usize,
    /// JS worker threads, when not `max_executing`.
    pub js_workers: Option<usize>,
    /// Also load `wasm` functions, on a WASM runtime sharing the budget (the
    /// deployed assembly).
    pub wasm: bool,
    pub host_max_concurrency: i32,
    pub init_timeout: Duration,
    /// `FC_FN_MAX_LOADED`.
    pub max_loaded: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_executing: 2,
            js_workers: None,
            wasm: false,
            host_max_concurrency: 64,
            init_timeout: Duration::from_secs(2),
            max_loaded: 50,
        }
    }
}

pub struct Reply {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.text()))
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn headers_all(&self, name: &str) -> Vec<String> {
        self.headers
            .get_all(name)
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect()
    }
}

pub struct JsHarness {
    /// The executing budget the runtimes share.
    pub budget: ExecBudget,
    pub control: Arc<FakeControlPlane>,
    pub reconciler: Arc<Reconciler>,
    pub listener: Arc<FnListener>,
    pub client: reqwest::Client,
    pub base: String,
    pub dir: tempfile::TempDir,
}

impl JsHarness {
    pub async fn start(functions: Vec<Value>) -> Self {
        Self::start_with(functions, Options::default()).await
    }

    pub async fn start_with(functions: Vec<Value>, options: Options) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock: SharedClock = Arc::new(SystemClock);
        let control = FakeControlPlane::new();
        let budget = ExecBudget::new(options.max_executing);
        let runtime = JsRuntime::new(JsSettings {
            workers: options.js_workers.unwrap_or(options.max_executing),
            budget: budget.clone(),
            init_timeout: options.init_timeout,
        })
        .unwrap();
        let mut loaders = Loaders::none();
        if options.wasm {
            let wasm = WasmRuntime::new(WasmSettings {
                engine: EngineSettings {
                    max_instances: 64,
                    ..EngineSettings::default()
                },
                threads: options.max_executing,
                budget: budget.clone(),
                cache_dir: dir.path().to_owned(),
                db: DbSettings::default(),
            })
            .unwrap();
            loaders = Arc::new(WasmLoader::new(wasm)).register(loaders);
        }
        let cache = ArtifactCache::new(dir.path(), DEFAULT_MAX_BYTES).unwrap();
        let artifacts = ArtifactStores::new(cache).with_source("file", Arc::new(FileSource));
        let registry = Arc::new(FunctionRegistry::new(options.max_loaded, clock.clone()));
        let reconciler = Arc::new(Reconciler::new(
            "default",
            "host-1",
            control.clone(),
            Arc::new(artifacts),
            Signatures::Off,
            Arc::new(JsLoader::new(runtime)).register(loaders),
            registry.clone(),
        ));
        let metrics = Arc::new(FnMetrics::new(registry));
        control.serve(Answer::Document(fakes::document_json(
            json!({ "functions": functions }),
        )));
        reconciler.reconcile_once(Utc::now()).await;
        let listener = Arc::new(FnListener::new(ListenerConfig {
            bind: "127.0.0.1".parse().unwrap(),
            port: 0,
            public_port: None,
            max_concurrency: options.host_max_concurrency,
            platform_url: "http://127.0.0.1:1".into(),
            trusted_proxies: TrustedProxies::default_list(),
            clock,
        }));
        listener.start(reconciler.clone(), metrics).await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.port().unwrap());
        Self {
            budget,
            control,
            reconciler,
            listener,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            base,
            dir,
        }
    }

    pub fn heartbeat_states(&self) -> Vec<(String, i32, String)> {
        fakes::states(&self.control.last_heartbeat())
    }

    /// The heartbeat's state for `address` (`LOADED`, `FAILED:<error>`, …).
    pub fn state_of(&self, address: &str) -> Option<String> {
        self.heartbeat_states()
            .into_iter()
            .find(|(a, _, _)| a == address)
            .map(|(_, _, state)| state)
    }

    pub async fn send(&self, request: reqwest::RequestBuilder) -> Reply {
        let response = request.send().await.expect("the request completes");
        Reply {
            status: response.status().as_u16(),
            headers: response.headers().clone(),
            body: response.bytes().await.unwrap().to_vec(),
        }
    }

    pub async fn get(&self, path: &str) -> Reply {
        self.get_at(ADDR, path).await
    }

    pub async fn get_at(&self, address: &str, path: &str) -> Reply {
        self.send(
            self.client
                .get(format!("{}/functions/{address}{path}", self.base)),
        )
        .await
    }

    pub async fn post(&self, path: &str, body: &[u8], headers: &[(&str, &str)]) -> Reply {
        let mut request = self
            .client
            .post(format!("{}/functions/{ADDR}{path}", self.base))
            .body(body.to_vec());
        for (k, v) in headers {
            request = request.header(*k, *v);
        }
        self.send(request).await
    }

    /// The guest's own JSON from a 200.
    pub async fn guest_json(&self, path: &str) -> Value {
        let reply = self.get(path).await;
        assert_eq!(reply.status, 200, "{}", reply.text());
        reply.json()
    }

    pub async fn close(&self) {
        self.listener.drain().await;
        self.listener.close(Duration::from_secs(5)).await;
    }
}

/// `%`-encodes a query value.
pub fn enc(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace(':', "%3A")
        .replace('/', "%2F")
        .replace('?', "%3F")
        .replace('=', "%3D")
        .replace('&', "%26")
}
