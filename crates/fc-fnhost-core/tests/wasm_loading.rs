//! Loading a `runtime: wasm` version (Java `WasmFunctionLoaderTest`, for
//! components): the refusals and their heartbeat codes, the `.cwasm` cache,
//! and the committed guests' checksums.

mod support;

use std::path::Path;

use fc_fnhost_core::wasm::cwasm::{CwasmCache, Source};
use fc_fnhost_core::wasm::engine::{self, EngineSettings};
use serde_json::json;
use support::wasm::{
    artifact, entry, guest, manifest, sha256_hex, sums, WasmHarness, ADDR, GUESTS,
};

// ── the committed guests ────────────────────────────────────────────────

#[test]
fn every_committed_guest_matches_sha256sums() {
    let listed: Vec<String> = sums().into_iter().map(|(name, _)| name).collect();
    let expected: Vec<String> = GUESTS.iter().map(|g| format!("{g}.wasm")).collect();
    assert_eq!(listed, expected, "SHA256SUMS lists exactly the test guests");
    for name in GUESTS {
        guest(name); // panics on a mismatch
    }
}

// ── refusals ────────────────────────────────────────────────────────────

/// The heartbeat state of `ADDR@1` when `bytes` is published with
/// `manifest_extra`, plus the status a call gets.
async fn load(bytes: &[u8], manifest_extra: serde_json::Value) -> (String, u16) {
    let dir = tempfile::tempdir().unwrap();
    let path = artifact(dir.path(), "fn.wasm", bytes);
    let h = WasmHarness::start(vec![entry(
        ADDR,
        1,
        &path,
        manifest(manifest_extra),
        json!({}),
    )])
    .await;
    let state = h.heartbeat_states().remove(0).2;
    let status = h.get("/x").await.status;
    (state, status)
}

fn component(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).unwrap()
}

/// A component that exports an (empty) instance under the handler's name.
const NAMED_EXPORT: &str =
    r#"(instance $i) (export "wasi:http/incoming-handler@0.2.12" (instance $i))"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_core_module_is_refused_as_unsupported() {
    let (state, status) = load(
        &wat::parse_str("(module (func (export \"handle\")))").unwrap(),
        json!({}),
    )
    .await;
    assert_eq!(state, "FAILED:LOAD:WASM_CORE_MODULE_UNSUPPORTED");
    assert_eq!(status, 503);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bytes_that_are_not_wasm_are_invalid() {
    let (state, _) = load(b"definitely not wasm", json!({})).await;
    assert_eq!(state, "FAILED:LOAD:WASM_INVALID");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_import_the_host_does_not_provide_is_refused() {
    for import in [
        r#"(import "evil:thing/iface@1.0.0" (instance))"#,
        r#"(import "wasi:keyvalue/store@0.2.0" (instance))"#,
        r#"(import "wasi:http/types@0.3.0" (instance))"#,
        r#"(import "flowcatalyst:function/db@0.1.0" (instance))"#,
        r#"(import "bare-function" (func))"#,
    ] {
        let (state, _) = load(
            &component(&format!("(component {import} {NAMED_EXPORT})")),
            json!({}),
        )
        .await;
        assert_eq!(state, "FAILED:LOAD:WASM_IMPORT_NOT_ALLOWED", "{import}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_entrypoint_must_be_the_incoming_handler_and_exported() {
    let (state, _) = load(&component("(component)"), json!({})).await;
    assert_eq!(
        state, "FAILED:LOAD:WASM_ENTRYPOINT_NOT_EXPORTED",
        "no export at all"
    );

    let echo = std::fs::read(guest("echo")).unwrap();
    for entrypoint in [
        "handle",
        "wasi:http/outgoing-handler",
        "wasi:http/incoming-handler@0.2.4",
    ] {
        let (state, _) = load(&echo, json!({"entrypoint": entrypoint})).await;
        assert_eq!(
            state, "FAILED:LOAD:WASM_ENTRYPOINT_NOT_EXPORTED",
            "{entrypoint}"
        );
    }

    let (state, _) = load(
        &component(&format!("(component {NAMED_EXPORT})")),
        json!({}),
    )
    .await;
    assert_eq!(
        state, "FAILED:LOAD:WASM_ENTRYPOINT_NOT_EXPORTED",
        "exported under the right name but without the handler's shape"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_declared_minimum_memory_over_the_cap_is_refused() {
    // 100 pages is 6.25 MiB; the cap is 1 MiB.
    let bytes = component(&format!(
        "(component (core module (memory 100)) {NAMED_EXPORT})"
    ));
    let (state, _) = load(
        &bytes,
        json!({"limits": {"maxConcurrency": 1, "wasmMemoryMb": 1}}),
    )
    .await;
    assert_eq!(state, "FAILED:LOAD:WASM_MEMORY_OVER_CAP");

    let (state, _) = load(
        &bytes,
        json!({"limits": {"maxConcurrency": 1, "wasmMemoryMb": 7}}),
    )
    .await;
    assert_ne!(
        state, "FAILED:LOAD:WASM_MEMORY_OVER_CAP",
        "under the cap it gets past the memory check"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_component_that_does_not_compile_is_invalid() {
    let bytes = component(&format!(
        "(component (core module (func (result i32))) {NAMED_EXPORT})"
    ));
    let (state, _) = load(&bytes, json!({})).await;
    assert_eq!(state, "FAILED:LOAD:WASM_INVALID");
}

/// `runtime: component` (owner decision 5) loads through the same runtime,
/// with its entrypoint left to the default; the heartbeat says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_component_runtime_loads_with_the_default_entrypoint() {
    let mut m = manifest(json!({"runtime": "component"}));
    m.as_object_mut().unwrap().remove("entrypoint");
    let h = WasmHarness::start(vec![entry(ADDR, 1, &guest("echo"), m, json!({}))]).await;
    assert_eq!(h.heartbeat_states()[0].2, "LOADED");
    assert_eq!(h.get("/echo/1").await.status, 200);
    assert_eq!(
        h.control.last_heartbeat().runtimes,
        ["component", "wasm"],
        "the heartbeat names both runtimes"
    );
}

/// A core module under `runtime: component` is refused like one under
/// `wasm` (the platform refuses it at publish when it can read it).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_core_module_under_component_is_refused() {
    let (state, _) = load(
        &wat::parse_str("(module (func (export \"handle\")))").unwrap(),
        json!({"runtime": "component"}),
    )
    .await;
    assert_eq!(state, "FAILED:LOAD:WASM_CORE_MODULE_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_jvm_version_stays_runtime_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = artifact(dir.path(), "fn.jar", b"PK not really a jar");
    let h = WasmHarness::start(vec![entry(
        ADDR,
        1,
        &path,
        json!({"runtime": "jvm", "entrypoint": "com.acme.Fn", "endpoints": [{"path": "/*", "auth": "none"}]}),
        json!({}),
    )])
    .await;
    assert_eq!(h.heartbeat_states()[0].2, "FAILED:RUNTIME_UNSUPPORTED");
}

// ── the .cwasm cache ────────────────────────────────────────────────────

fn cwasm_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn the_cache_compiles_once_hits_after_and_recreates_a_corrupt_file() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine::engine(&EngineSettings::default()).unwrap();
    let cache = CwasmCache::new(dir.path(), &engine::fingerprint(&engine));
    let bytes = std::fs::read(guest("pure")).unwrap();
    let digest = sha256_hex(&bytes);

    let (_, first) = cache.load(&engine, &digest, &bytes).unwrap();
    assert_eq!(first, Source::Compiled);
    assert_eq!(
        cwasm_files(cache.dir()),
        [format!("{digest}.cwasm"), format!("{digest}.cwasm.sha256")],
        "the .cwasm and its checksum, and no temporary files left behind"
    );
    let (_, second) = cache.load(&engine, &digest, &bytes).unwrap();
    assert_eq!(second, Source::Hit);

    // A corrupted file (same length, one byte flipped) fails its checksum:
    // it is never mapped, and is compiled again.
    let path = cache.path_for(&digest);
    let mut cwasm = std::fs::read(&path).unwrap();
    let middle = cwasm.len() / 2;
    cwasm[middle] ^= 0xFF;
    std::fs::write(&path, &cwasm).unwrap();
    let (_, corrupt) = cache.load(&engine, &digest, &bytes).unwrap();
    assert_eq!(corrupt, Source::Replaced);
    let (_, after) = cache.load(&engine, &digest, &bytes).unwrap();
    assert_eq!(after, Source::Hit, "the recreated file is good");

    // A truncated file, and a missing checksum, are recreated too.
    std::fs::write(&path, &cwasm[..100]).unwrap();
    assert_eq!(
        cache.load(&engine, &digest, &bytes).unwrap().1,
        Source::Replaced
    );
    std::fs::remove_file(
        dir.path()
            .join("cwasm")
            .join(engine::fingerprint(&engine))
            .join(format!("{digest}.cwasm.sha256")),
    )
    .unwrap();
    assert_eq!(
        cache.load(&engine, &digest, &bytes).unwrap().1,
        Source::Replaced
    );
}

#[test]
fn a_different_engine_config_uses_its_own_cache_directory() {
    let dir = tempfile::tempdir().unwrap();
    let speed = engine::engine(&EngineSettings::default()).unwrap();
    let size = engine::engine(&EngineSettings {
        opt_level: wasmtime::OptLevel::SpeedAndSize,
        ..EngineSettings::default()
    })
    .unwrap();
    let bytes = std::fs::read(guest("pure")).unwrap();
    let digest = sha256_hex(&bytes);
    let a = CwasmCache::new(dir.path(), &engine::fingerprint(&speed));
    let b = CwasmCache::new(dir.path(), &engine::fingerprint(&size));
    assert_ne!(a.dir(), b.dir());
    assert_eq!(a.load(&speed, &digest, &bytes).unwrap().1, Source::Compiled);
    assert_eq!(
        b.load(&size, &digest, &bytes).unwrap().1,
        Source::Compiled,
        "a .cwasm from another config is never looked at"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_host_over_the_same_cache_loads_the_cwasm_without_rewriting_it() {
    let functions = || {
        vec![entry(
            ADDR,
            1,
            &guest("pure"),
            manifest(json!({})),
            json!({}),
        )]
    };
    let first = WasmHarness::start(functions()).await;
    assert_eq!(first.get("/x").await.status, 200);
    let cwasm_dir = first.runtime.cwasm_dir().to_owned();
    let files = cwasm_files(&cwasm_dir);
    assert_eq!(files.len(), 2, "{files:?}");
    let cwasm = cwasm_dir.join(files.iter().find(|f| f.ends_with(".cwasm")).unwrap());
    let written = std::fs::metadata(&cwasm).unwrap().modified().unwrap();
    first.close().await;
    let dir = {
        let WasmHarness { dir, .. } = first;
        dir
    };

    let second = WasmHarness::start_in(dir, functions(), Default::default()).await;
    assert_eq!(second.get("/x").await.status, 200);
    assert_eq!(second.runtime.cwasm_dir(), cwasm_dir);
    assert_eq!(
        std::fs::metadata(&cwasm).unwrap().modified().unwrap(),
        written,
        "loaded from the cache, not compiled and rewritten"
    );
}
