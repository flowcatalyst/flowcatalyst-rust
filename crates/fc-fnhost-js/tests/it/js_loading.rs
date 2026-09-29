//! Loading a JS function: what is refused, and with which heartbeat code
//! (the JS counterpart of fc-fnhost-core's `wasm_loading.rs`).

use crate::support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use fc_fnhost_core::control_plane::ControlPlane;
use fc_fnhost_core::emit::Emitter;
use fc_fnhost_core::wasm::output::GuestLogger;
use fc_fnhost_js::engine;
use fc_fnhost_js::isolate::Limits;
use fc_fnhost_js::ops;
use fc_fnhost_js::ops::VersionShared;
use fc_fnhost_js::prepare;
use fc_fnhost_js::prepare::Refusal;
use serde_json::json;
use std::fs;
use std::thread;
use support::{bundle, entry, manifest, JsHarness, Options, ADDR};

async fn load(file: &str, manifest_extra: serde_json::Value) -> JsHarness {
    JsHarness::start_with(
        vec![entry(
            ADDR,
            1,
            &bundle(file),
            manifest(manifest_extra),
            json!({}),
        )],
        Options {
            init_timeout: Duration::from_millis(500),
            ..Options::default()
        },
    )
    .await
}

/// The heartbeat reports the code; the detail is on the host's own log line
/// (and in [`refusal`]).
fn failed_with(h: &JsHarness, code: &str) {
    let state = h.state_of(ADDR).expect("the function is reported");
    assert_eq!(state, format!("FAILED:LOAD:{code}"));
}

/// `prepare` itself, for the refusal's detail (on a thread of its own, as
/// the loader runs it).
fn refusal(file: &str, entrypoint: &str) -> Refusal {
    let (file, entrypoint) = (file.to_owned(), entrypoint.to_owned());
    thread::spawn(move || refusal_on_this_thread(&file, &entrypoint))
        .join()
        .unwrap()
}

fn refusal_on_this_thread(file: &str, entrypoint: &str) -> Refusal {
    let base = engine::init_v8().unwrap();
    let bytes = fs::read(bundle(file)).unwrap();
    let control: Arc<dyn ControlPlane> = support::fakes::FakeControlPlane::new();
    let version = Arc::new(VersionShared {
        address: fc_function_abi::FunctionAddress::parse(ADDR).unwrap(),
        version: 1,
        logger: GuestLogger::for_address(ADDR),
        config: Default::default(),
        secrets: Default::default(),
        allow: Default::default(),
        emitter: Emitter {
            control_plane: control,
            host_id: "host-1".into(),
            host_runtime: None,
        },
        body_cap: 1 << 20,
        http: ops::http_client().unwrap(),
        host_runtime: None,
    });
    let result = prepare::prepare(
        base,
        &bytes,
        entrypoint,
        Limits::of(32 << 20),
        version,
        Duration::from_millis(500),
    );
    match result {
        Ok(_) => panic!("{file} loaded"),
        Err(refusal) => refusal,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bundle_that_imports_anything_but_the_host_modules_is_refused() {
    let h = load("bad-import.mjs", json!({})).await;
    failed_with(&h, "JS_IMPORT_NOT_ALLOWED");
    let why = refusal("bad-import.mjs", "default").detail;
    assert!(why.contains("'left-pad'"), "{why}");
    assert!(why.contains("flowcatalyst:function/config"), "{why}");
    // A refused version is not served.
    assert_eq!(h.get("/x").await.status, 503);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bundle_that_does_not_compile_or_is_not_utf8_is_invalid() {
    let h = load("syntax.mjs", json!({})).await;
    failed_with(&h, "JS_INVALID");
    h.close().await;
    let h = load("not-utf8.mjs", json!({})).await;
    failed_with(&h, "JS_INVALID");
    let why = refusal("not-utf8.mjs", "default").detail;
    assert!(why.contains("not UTF-8"), "{why}");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_entrypoint_must_be_an_exported_function_or_an_object_with_fetch() {
    let h = load("guest.mjs", json!({"entrypoint": "missing"})).await;
    failed_with(&h, "JS_ENTRYPOINT_NOT_EXPORTED");
    h.close().await;
    let why = refusal("guest.mjs", "missing").detail;
    assert!(why.contains("does not export 'missing'"), "{why}");
    let why = refusal("guest.mjs", "notAHandler").detail;
    assert!(why.contains("'notAHandler' is number"), "{why}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn top_level_code_that_throws_uses_a_request_api_or_spins_fails_the_load() {
    let h = load("init-throws.mjs", json!({})).await;
    failed_with(&h, "JS_INIT_FAILED");
    h.close().await;
    let why = refusal("init-throws.mjs", "default").detail;
    assert!(why.contains("top-level failure"), "{why}");
    let h = load("init-api.mjs", json!({})).await;
    failed_with(&h, "JS_INIT_FAILED");
    h.close().await;
    let why = refusal("init-api.mjs", "default").detail;
    assert!(why.contains("only while a request is handled"), "{why}");
    let started = Instant::now();
    let h = load("init-spin.mjs", json!({})).await;
    failed_with(&h, "JS_INIT_FAILED");
    assert!(started.elapsed() < Duration::from_secs(5));
    h.close().await;
    let why = refusal("init-spin.mjs", "default").detail;
    assert!(why.contains("did not finish within 500 ms"), "{why}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_host_reports_it_loads_js_beside_nothing_else_here() {
    let h = load("guest.mjs", json!({})).await;
    assert_eq!(h.control.last_heartbeat().runtimes, ["js"]);
    assert_eq!(h.state_of(ADDR).as_deref(), Some("LOADED"));
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_js_function_that_declares_a_database_fails_its_load() {
    let h = load(
        "guest.mjs",
        json!({
            "secrets": ["ORDERS_DB"],
            "db": [{"name": "orders", "secretRef": "ORDERS_DB"}],
        }),
    )
    .await;
    assert_eq!(h.state_of(ADDR).as_deref(), Some("FAILED:DB_UNSUPPORTED"));
    h.close().await;
}
