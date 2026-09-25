//! The WASM harness (Java `WasmFixtures` + `FnHttpTestSupport` for Wasm): the
//! committed test guests (verified against `SHA256SUMS`), desired-state
//! entries that point at them through `file://`, and a host of the real
//! pieces: artifact cache, [`Reconciler`], [`WasmLoader`] and [`FnListener`],
//! fed by the fake control plane.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use fc_fnhost_core::artifact::{ArtifactCache, ArtifactStores, FileSource, DEFAULT_MAX_BYTES};
use fc_fnhost_core::clock::SystemClock;
use fc_fnhost_core::env::TrustedProxies;
use fc_fnhost_core::host::Listener;
use fc_fnhost_core::listener::{FnListener, ListenerConfig};
use fc_fnhost_core::loader::Loaders;
use fc_fnhost_core::metrics::FnMetrics;
use fc_fnhost_core::reconciler::Reconciler;
use fc_fnhost_core::registry::FunctionRegistry;
use fc_fnhost_core::signature::Signatures;
use fc_fnhost_core::wasm::{EngineSettings, WasmLoader, WasmRuntime, WasmSettings};
use serde_json::{json, Value};
use sha2::Digest as _;

use super::fakes::{self, Answer, FakeControlPlane};
use super::listener::Reply;

pub const ADDR: &str = "app.orders.ship";
pub const ENTRYPOINT: &str = "wasi:http/incoming-handler";

/// Every committed guest, in `SHA256SUMS` (and `tests/guests/build.sh`)
/// order. `pdk`, `pdk-pure` and `hello` are written with the guest SDK
/// (`crates/fc-function-pdk`; `hello` is `examples/function-hello-rust`).
pub const GUESTS: [&str; 13] = [
    "echo", "spin", "alloc", "fail", "config", "secret", "http", "emit", "log", "pure", "pdk",
    "pdk-pure", "hello",
];

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm")
}

/// `name → sha256 hex` from the committed `SHA256SUMS`.
pub fn sums() -> Vec<(String, String)> {
    std::fs::read_to_string(fixtures_dir().join("SHA256SUMS"))
        .expect("SHA256SUMS is committed")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let (hash, name) = line.split_once("  ").expect("`<hash>  <name>` lines");
            (name.to_owned(), hash.to_owned())
        })
        .collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

/// A committed guest, verified against `SHA256SUMS`.
pub fn guest(name: &str) -> PathBuf {
    let path = fixtures_dir().join(format!("{name}.wasm"));
    let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("{} is committed", path.display()));
    let file = format!("{name}.wasm");
    let expected = sums()
        .into_iter()
        .find(|(n, _)| *n == file)
        .unwrap_or_else(|| panic!("{file} is not in SHA256SUMS"))
        .1;
    assert_eq!(
        sha256_hex(&bytes),
        expected,
        "{file} does not match SHA256SUMS: rebuild with tests/guests/build.sh"
    );
    path
}

/// Writes `bytes` into `dir` as an artifact (for hand-made components).
pub fn artifact(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// A manifest for a component function: every path, no auth, and `extra`
/// merged on top (limits, config, secrets, httpAllow, …).
pub fn manifest(extra: Value) -> Value {
    let mut manifest = json!({
        "runtime": "wasm",
        "entrypoint": ENTRYPOINT,
        "endpoints": [{"path": "/*", "auth": "none"}],
        "limits": {"maxConcurrency": 4, "wasmMemoryMb": 16},
    });
    for (k, v) in extra.as_object().unwrap() {
        manifest[k] = v.clone();
    }
    manifest
}

/// A live, warm desired-state entry for `artifact`, with `extra` merged
/// into the entry (config and secret values, mode, …).
pub fn entry(address: &str, version: i32, artifact: &Path, manifest: Value, extra: Value) -> Value {
    let bytes = std::fs::read(artifact).unwrap();
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
    pub max_executing: usize,
    pub max_instances: u32,
    pub host_max_concurrency: i32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_executing: 4,
            max_instances: 32,
            host_max_concurrency: 64,
        }
    }
}

pub struct WasmHarness {
    pub control: Arc<FakeControlPlane>,
    pub reconciler: Arc<Reconciler>,
    pub listener: Arc<FnListener>,
    pub runtime: Arc<WasmRuntime>,
    pub client: reqwest::Client,
    pub base: String,
    /// Holds the artifact and `.cwasm` caches.
    pub dir: tempfile::TempDir,
}

impl WasmHarness {
    pub async fn start(functions: Vec<Value>) -> Self {
        Self::start_with(functions, Options::default()).await
    }

    pub async fn start_with(functions: Vec<Value>, options: Options) -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self::start_in(dir, functions, options).await
    }

    /// With the caches in `dir` (a second host over the same cache).
    pub async fn start_in(dir: tempfile::TempDir, functions: Vec<Value>, options: Options) -> Self {
        let clock: fc_fnhost_core::clock::SharedClock = Arc::new(SystemClock);
        let control = FakeControlPlane::new();
        let runtime = WasmRuntime::new(WasmSettings {
            engine: EngineSettings {
                max_instances: options.max_instances,
                ..EngineSettings::default()
            },
            max_executing: options.max_executing,
            cache_dir: dir.path().to_owned(),
        })
        .unwrap();
        let cache = ArtifactCache::new(dir.path(), DEFAULT_MAX_BYTES).unwrap();
        let artifacts = ArtifactStores::new(cache).with_source("file", Arc::new(FileSource));
        let registry = Arc::new(FunctionRegistry::new(50, clock.clone()));
        let reconciler = Arc::new(Reconciler::new(
            "default",
            "host-1",
            control.clone(),
            Arc::new(artifacts),
            Signatures::Off,
            Loaders::none().with("wasm", Arc::new(WasmLoader::new(runtime.clone()))),
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
            control,
            reconciler,
            listener,
            runtime,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            base,
            dir,
        }
    }

    pub async fn publish(&self, functions: Vec<Value>) {
        self.control.serve(Answer::Document(fakes::document_json(
            json!({ "functions": functions }),
        )));
        self.reconciler.reconcile_once(Utc::now()).await;
    }

    pub fn heartbeat_states(&self) -> Vec<(String, i32, String)> {
        fakes::states(&self.control.last_heartbeat())
    }

    pub async fn send(&self, request: reqwest::RequestBuilder) -> Reply {
        let response = request.send().await.expect("the request completes");
        Reply {
            status: response.status().as_u16(),
            headers: response.headers().clone(),
            body: response.bytes().await.unwrap().to_vec(),
        }
    }

    /// `GET /functions/<ADDR><path>`.
    pub async fn get(&self, path: &str) -> Reply {
        self.get_headers(path, &[]).await
    }

    pub async fn get_headers(&self, path: &str, headers: &[(&str, &str)]) -> Reply {
        let mut request = self
            .client
            .get(format!("{}/functions/{ADDR}{path}", self.base));
        for (k, v) in headers {
            request = request.header(*k, *v);
        }
        self.send(request).await
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

    pub fn function_permits(&self) -> Option<usize> {
        self.listener
            .permits()
            .unwrap()
            .function_available(&fc_function_abi::FunctionAddress::parse(ADDR).unwrap())
    }

    pub async fn close(&self) {
        self.listener.drain().await;
        self.listener.close(Duration::from_secs(5)).await;
    }
}

/// `%`-encodes a query value (what the guests' `q` decodes).
pub fn enc(value: &str) -> String {
    value
        .replace(':', "%3A")
        .replace('/', "%2F")
        .replace('?', "%3F")
        .replace('=', "%3D")
        .replace('&', "%26")
}
