//! End to end against a fake platform over real HTTP: desired state →
//! fetch (`platform://` and `file://`) → digest check (signatures off, dev
//! mode) → a fake loader → heartbeat contents → `/ready` and `/metrics` →
//! unload → a `DRAINING` heartbeat on close.

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fc_fnhost_core::env::{EnvReader, HostEnv};
use fc_fnhost_core::host::{FnHost, Listener};
use fc_fnhost_core::invoke::{InvocationContext, InvokeError, Invoker};
use fc_fnhost_core::listener::FnListener;
use fc_fnhost_core::loader::{FunctionInstance, FunctionLoader, LoadOutcome, LoadRequest, Loaders};
use fc_function_abi::Response;
use parking_lot::Mutex;
use serde_json::json;

#[derive(Default)]
struct RecordingLoader {
    /// `address@version` → the artifact bytes the loader was handed.
    loaded: Mutex<Vec<(String, Vec<u8>)>>,
    instances: Mutex<Vec<(String, Arc<Instance>)>>,
}

struct Instance {
    closed: AtomicBool,
}

#[async_trait]
impl Invoker for Instance {
    async fn invoke(&self, _context: InvocationContext) -> Result<Response, InvokeError> {
        Ok(Response::ack())
    }
}

#[async_trait]
impl FunctionInstance for Instance {
    async fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[async_trait]
impl FunctionLoader for RecordingLoader {
    async fn load(&self, request: LoadRequest<'_>) -> LoadOutcome {
        let label = format!("{}@{}", request.entry.address, request.entry.version);
        let bytes = std::fs::read(request.artifact).unwrap();
        self.loaded.lock().push((label.clone(), bytes));
        let instance = Arc::new(Instance {
            closed: AtomicBool::new(false),
        });
        self.instances.lock().push((label, instance.clone()));
        LoadOutcome::Loaded(instance)
    }
}

impl RecordingLoader {
    fn closed(&self, label: &str) -> bool {
        self.instances
            .lock()
            .iter()
            .find(|(l, _)| l == label)
            .is_some_and(|(_, i)| i.closed.load(Ordering::SeqCst))
    }
}

#[tokio::test]
async fn desired_state_to_heartbeat_to_unload_over_http() {
    let (platform, url) = support::start().await;
    let cache = tempfile::tempdir().unwrap();

    // a warm function served from the platform, a lazy one from a file, and a JVM jar
    let warm_bytes = b"\0asm warm function".to_vec();
    let lazy_bytes = b"\0asm lazy function".to_vec();
    let lazy_path = cache.path().join("lazy.wasm");
    std::fs::write(&lazy_path, &lazy_bytes).unwrap();
    platform.artifacts.lock().insert(
        support::version_id("app.orders.ship", 3),
        warm_bytes.clone(),
    );
    let warm_ref = format!(
        "platform://fnc_1/{}",
        &support::sha256_digest(&warm_bytes)[7..]
    );
    platform.set_document(json!({
        "functions": [
            support::entry("app.orders.ship", 3, "wasm", "warm", &warm_ref, &warm_bytes),
            support::entry("app.orders.lazy", 1, "wasm", "lazy", &format!("file://{}", lazy_path.display()), &lazy_bytes),
            support::entry("app.legacy.jar", 7, "jvm", "warm", "platform://fnc_2/00", b"jar"),
        ],
        "unload": [],
        "publicRoutes": []
    }));

    let env = HostEnv::load(&EnvReader::from_pairs([
        ("FC_FN_PLATFORM_URL", url.as_str()),
        ("FC_FN_CLIENT_ID", "fn-host"),
        ("FC_FN_CLIENT_SECRET", "host-secret"),
        ("FC_FN_POOL", "blue"),
        ("FC_FN_HOST_ID", "it-host-1"),
        ("FC_FN_SIGNATURES", "off"),
        ("FLOWCATALYST_DEV_MODE", "true"),
        ("FC_METRICS_PORT", "0"),
        (
            "FC_FN_CACHE_DIR",
            cache.path().join("cache").to_str().unwrap(),
        ),
    ]))
    .unwrap();
    let loader = Arc::new(RecordingLoader::default());
    let mut host = FnHost::new(env, Loaders::none().with("wasm", loader.clone()), None).unwrap();
    host.start().await.unwrap();

    // the first reconcile ran inside start(): fetched, loaded, heartbeated
    assert_eq!(
        loader.loaded.lock().clone(),
        [("app.orders.ship@3".to_owned(), warm_bytes.clone())],
        "the loader got the verified bytes of the warm function only"
    );
    support::wait_for("the first heartbeat", || !platform.last_states().is_empty()).await;
    let beat = platform.last_heartbeat().unwrap();
    assert_eq!(beat["hostId"], "it-host-1");
    assert_eq!(beat["pool"], "blue");
    assert_eq!(beat["state"], "ACTIVE");
    assert_eq!(
        platform.last_states(),
        [
            "app.orders.ship@3 LOADED",
            "app.orders.lazy@1 REGISTERED",
            "app.legacy.jar@7 FAILED:RUNTIME_UNSUPPORTED",
        ]
    );
    assert!(
        platform.token_forms.lock()[0].contains("grant_type=client_credentials&client_id=fn-host")
    );

    // a lazy function loads on first call, from the file:// artifact
    let lazy = host
        .reconciler()
        .ensure_loaded(&fc_function_abi::FunctionAddress::parse("app.orders.lazy").unwrap())
        .await
        .unwrap();
    assert_eq!(lazy.version(), 1);
    assert_eq!(
        loader.loaded.lock()[1],
        ("app.orders.lazy@1".to_owned(), lazy_bytes.clone())
    );

    // observability
    let port = host.metrics_port().unwrap();
    let http = reqwest::Client::new();
    let ready = http
        .get(format!("http://127.0.0.1:{port}/ready"))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), 200);
    let body: serde_json::Value = ready.json().await.unwrap();
    assert_eq!(body["status"], "UP");
    assert!(body["memory"].is_object());
    let health = http
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        (health.status().as_u16(), health.text().await.unwrap()),
        (200, r#"{"status":"UP"}"#.to_owned())
    );
    let not_get = http
        .post(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(not_get.status(), 404);
    let metrics = http
        .get(format!("http://127.0.0.1:{port}/metrics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    for series in [
        "fc_fn_loaded 2",
        "fc_fn_warm 1",
        "fc_fn_reconcile_total{outcome=\"changed\"}",
        "fc_fn_load_errors_total{reason=\"RUNTIME_UNSUPPORTED\"}",
        "fc_fn_last_reconcile_success_timestamp_seconds",
    ] {
        assert!(metrics.contains(series), "{series} missing:\n{metrics}");
    }

    // an unchanged document is a 304 and reloads nothing
    host.trigger_reconcile();
    support::wait_for("a not-modified cycle", || {
        platform.not_modified.load(Ordering::SeqCst) >= 1
    })
    .await;
    assert_eq!(loader.loaded.lock().len(), 2);

    // the warm function leaves desired state: it is closed and drops out of the heartbeat
    platform.set_document(json!({
        "functions": [support::entry("app.orders.lazy", 1, "wasm", "lazy", &format!("file://{}", lazy_path.display()), &lazy_bytes)],
    }));
    let beats = platform.heartbeats.lock().len();
    host.trigger_reconcile();
    support::wait_for("the unload", || loader.closed("app.orders.ship@3")).await;
    support::wait_for("the next heartbeat", || {
        platform.heartbeats.lock().len() > beats
    })
    .await;
    assert_eq!(platform.last_states(), ["app.orders.lazy@1 LOADED"]);
    assert!(!loader.closed("app.orders.lazy@1"));

    // close: a DRAINING heartbeat, every function closed, observability gone
    host.close().await;
    assert_eq!(platform.last_heartbeat().unwrap()["state"], "DRAINING");
    assert!(loader.closed("app.orders.lazy@1"));
    let gone = http
        .get(format!("http://127.0.0.1:{port}/health"))
        .timeout(Duration::from_secs(2))
        .send()
        .await;
    assert!(
        gone.is_err(),
        "the observability listener closes last, but it closes"
    );
}

#[tokio::test]
async fn ready_reports_starting_then_platform_unreachable() {
    let cache = tempfile::tempdir().unwrap();
    let env = HostEnv::load(&EnvReader::from_pairs([
        ("FC_FN_PLATFORM_URL", "http://127.0.0.1:1"),
        ("FC_FN_CLIENT_ID", "id"),
        ("FC_FN_CLIENT_SECRET", "secret"),
        ("FC_FN_SIGNATURES", "off"),
        ("FLOWCATALYST_DEV_MODE", "true"),
        ("FC_METRICS_PORT", "0"),
        ("FC_FN_CACHE_DIR", cache.path().to_str().unwrap()),
    ]))
    .unwrap();
    let mut host = FnHost::new(env, Loaders::none(), None).unwrap();
    host.start().await.unwrap();
    let port = host.metrics_port().unwrap();
    let ready = reqwest::get(format!("http://127.0.0.1:{port}/ready"))
        .await
        .unwrap();
    assert_eq!(ready.status(), 503);
    let body: serde_json::Value = ready.json().await.unwrap();
    assert_eq!(body["status"], "PLATFORM_UNREACHABLE");
    let health = reqwest::get(format!("http://127.0.0.1:{port}/health"))
        .await
        .unwrap();
    assert_eq!(health.status(), 200, "an outage is not a liveness failure");
    host.close().await;
}

#[tokio::test]
async fn the_listeners_serve_through_the_host_and_close_with_it() {
    let (platform, url) = support::start().await;
    let cache = tempfile::tempdir().unwrap();
    let bytes = b"\0asm served".to_vec();
    platform
        .artifacts
        .lock()
        .insert(support::version_id("app.orders.ship", 1), bytes.clone());
    let artifact_ref = format!("platform://fnc_1/{}", &support::sha256_digest(&bytes)[7..]);
    let mut entry = support::entry("app.orders.ship", 1, "wasm", "warm", &artifact_ref, &bytes);
    entry["manifest"]["endpoints"] = json!([{"path": "/x", "auth": "none"}]);
    platform.set_document(json!({
        "functions": [entry],
        "publicRoutes": [{"hostname": "api.acme.com", "pathPrefix": "/", "address": "app.orders.ship"}]
    }));
    let env = HostEnv::load(&EnvReader::from_pairs([
        ("FC_FN_PLATFORM_URL", url.as_str()),
        ("FC_FN_CLIENT_ID", "id"),
        ("FC_FN_CLIENT_SECRET", "secret"),
        ("FC_FN_SIGNATURES", "off"),
        ("FLOWCATALYST_DEV_MODE", "true"),
        ("FC_METRICS_PORT", "0"),
        ("FC_FN_PORT", "0"),
        ("FC_FN_PUBLIC_PORT", "0"),
        ("FC_FN_CACHE_DIR", cache.path().to_str().unwrap()),
    ]))
    .unwrap();
    let listener: Arc<dyn Listener> = Arc::new(FnListener::from_env(&env));
    let loader = Arc::new(RecordingLoader::default());
    let mut host = FnHost::new(
        env,
        Loaders::none().with("wasm", loader.clone()),
        Some(listener),
    )
    .unwrap();
    host.start().await.unwrap();
    let port = host.port().unwrap();
    let public_port = host.public_port().unwrap();
    let http = reqwest::Client::new();

    let private = http
        .get(format!(
            "http://127.0.0.1:{port}/functions/app.orders.ship/x"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(private.status(), 200);
    let public = http
        .get(format!("http://127.0.0.1:{public_port}/x"))
        .header("Host", "api.acme.com")
        .send()
        .await
        .unwrap();
    assert_eq!(public.status(), 200);
    let ready: serde_json::Value = http
        .get(format!(
            "http://127.0.0.1:{}/ready",
            host.metrics_port().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ready["status"], "UP", "{ready}");
    let metrics = http
        .get(format!(
            "http://127.0.0.1:{}/metrics",
            host.metrics_port().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(metrics.contains("fc_fn_permits_available{scope=\"host\",address=\"\"} 512"));
    assert!(metrics.contains(
        "fc_fn_invocations_total{address=\"app.orders.ship\",version=\"1\",outcome=\"ok\",entry=\"public\"} 1"
    ));

    host.close().await;
    assert!(
        http.get(format!(
            "http://127.0.0.1:{port}/functions/app.orders.ship/x"
        ))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_err(),
        "the listener is closed"
    );
    assert!(loader.closed("app.orders.ship@1"));
}
