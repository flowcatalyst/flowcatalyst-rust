//! Density and latency of JS functions: the measurements behind
//! `docs/function-runner-density.md` §10. Ignored (measurement only); run in
//! release, one at a time:
//!
//! ```text
//! cargo test --release -p fc-fnhost-js --test js_density -- --ignored --nocapture --test-threads 1
//! ```

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use support::{bundle, entry, manifest, JsHarness, Options};

const A: &str = "app.orders.a";

/// `(resident, footprint)` in bytes: macOS `proc_pid_rusage` (`ri_resident_size`,
/// `ri_phys_footprint`, the spike's metrics), Linux `/proc/self/status`
/// (`VmRSS`, `RssAnon`).
fn memory() -> (u64, u64) {
    #[cfg(target_os = "macos")]
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V2,
            &mut info as *mut _ as *mut libc::rusage_info_t,
        );
        (info.ri_resident_size, info.ri_phys_footprint)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let kb = |key: &str| {
            status
                .lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                * 1024
        };
        (kb("VmRSS:"), kb("RssAnon:"))
    }
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1048576.0
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1]
}

fn stats(mut samples: Vec<Duration>) -> String {
    samples.sort();
    format!(
        "p50 {:.3} ms, p99 {:.3} ms, max {:.2} ms",
        percentile(&samples, 0.50).as_secs_f64() * 1e3,
        percentile(&samples, 0.99).as_secs_f64() * 1e3,
        samples.last().unwrap().as_secs_f64() * 1e3
    )
}

async fn call(h: &JsHarness, address: &str, path: &str) -> u16 {
    h.send(
        h.client
            .get(format!("{}/functions/{address}{path}", h.base)),
    )
    .await
    .status
}

/// A bundle of about `kib` KiB: the hello handler plus a table of generated
/// functions and data, as a bundle with its npm dependencies inlined looks.
fn big_bundle(dir: &std::path::Path, kib: usize) -> std::path::PathBuf {
    let mut source = std::fs::read_to_string(bundle("hello.mjs")).unwrap();
    let mut i = 0;
    while source.len() < kib * 1024 {
        source.push_str(&format!(
            "export function helper{i}(x) {{ const t = [{i}, \"value-{i}\", {{ k: {i} }}]; return t.map((v) => typeof v === \"number\" ? v * x : v); }}\n"
        ));
        i += 1;
    }
    let path = dir.join(format!("big-{kib}k.mjs"));
    std::fs::write(&path, source).unwrap();
    path
}

/// First call and steady per-call latency through the listener (loopback
/// HTTP included), for the template's hello bundle and a larger one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "measurement: run in release with --ignored --nocapture"]
async fn measure_latency_through_the_listener() {
    let scratch = tempfile::tempdir().unwrap();
    for (name, artifact) in [
        ("hello (1 KiB)", bundle("hello.mjs")),
        ("hello + 256 KiB", big_bundle(scratch.path(), 256)),
        ("hello + 1 MiB", big_bundle(scratch.path(), 1024)),
    ] {
        let functions = vec![entry(
            A,
            1,
            &artifact,
            manifest(json!({
                "limits": {"maxConcurrency": 64, "wasmMemoryMb": 32},
                "endpoints": [{"path": "/hello/{name}", "auth": "none"}],
            })),
            json!({"mode": "lazy", "config": {"GREETING": "Hi"}}),
        )];
        let started = Instant::now();
        let h = Arc::new(
            JsHarness::start_with(
                functions,
                Options {
                    max_executing: 4,
                    ..Options::default()
                },
            )
            .await,
        );
        let reconciled = started.elapsed();
        let first = Instant::now();
        assert_eq!(call(&h, A, "/hello/Ada").await, 200);
        let first = first.elapsed();
        let second = Instant::now();
        assert_eq!(call(&h, A, "/hello/Ada").await, 200);
        let second = second.elapsed();
        let mut steady = Vec::new();
        for _ in 0..2000 {
            let t = Instant::now();
            assert_eq!(call(&h, A, "/hello/Ada").await, 200);
            steady.push(t.elapsed());
        }
        let concurrent = {
            let t = Instant::now();
            let calls: Vec<_> = (0..16)
                .map(|_| {
                    let h = h.clone();
                    tokio::spawn(async move {
                        for _ in 0..250 {
                            assert_eq!(call(&h, A, "/hello/Ada").await, 200);
                        }
                    })
                })
                .collect();
            for c in calls {
                c.await.unwrap();
            }
            4000.0 / t.elapsed().as_secs_f64()
        };
        println!(
            "{name}: reconcile {:.1} ms | first call (lazy load: check + code cache + request) {:.2} ms | second {:.2} ms | steady c=1 {} | c=16 {concurrent:.0} calls/s (FC_FN_MAX_EXECUTING=4)",
            reconciled.as_secs_f64() * 1e3,
            first.as_secs_f64() * 1e3,
            second.as_secs_f64() * 1e3,
            stats(steady),
        );
        h.close().await;
    }
}

/// Memory per loaded (idle) function, the slope between N=100 and N=1000
/// in one host, and memory per invocation in flight (isolates alive at
/// once).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "measurement: run in release with --ignored --nocapture"]
async fn measure_density() {
    let scratch = tempfile::tempdir().unwrap();
    for (name, artifact) in [
        ("hello (1 KiB)", bundle("hello.mjs")),
        ("hello + 256 KiB", big_bundle(scratch.path(), 256)),
    ] {
        let functions = |n: usize| -> Vec<serde_json::Value> {
            (0..n)
                .map(|i| {
                    entry(
                        &format!("app.density.f{i}"),
                        1,
                        &artifact,
                        manifest(json!({
                            "limits": {"maxConcurrency": 64, "wasmMemoryMb": 32},
                            "endpoints": [{"path": "/hello/{name}", "auth": "none"}],
                        })),
                        json!({"config": {"GREETING": "Hi"}}),
                    )
                })
                .collect()
        };
        let empty = memory();
        let h = JsHarness::start_with(
            Vec::new(),
            Options {
                max_executing: 4,
                max_loaded: 5000,
                ..Options::default()
            },
        )
        .await;
        let engine = memory();
        let mut points = Vec::new();
        for n in [100usize, 1000] {
            let started = Instant::now();
            h.control.serve(support::fakes::Answer::Document(
                support::fakes::document_json(json!({ "functions": functions(n) })),
            ));
            h.reconciler.reconcile_once(chrono::Utc::now()).await;
            let loaded_in = started.elapsed();
            let loaded = h
                .heartbeat_states()
                .iter()
                .filter(|(_, _, s)| s == "LOADED")
                .count();
            assert_eq!(loaded, n);
            tokio::time::sleep(Duration::from_millis(300)).await;
            let after = memory();
            println!(
                "{name}: N={n} reconciled in {:.1} s | rss {:.1} MiB | fp {:.1} MiB (process before the host {:.1}/{:.1}, with the engine {:.1}/{:.1})",
                loaded_in.as_secs_f64(),
                mib(after.0),
                mib(after.1),
                mib(empty.0),
                mib(empty.1),
                mib(engine.0),
                mib(engine.1),
            );
            points.push((n, after));
        }
        let ((n0, m0), (n1, m1)) = (points[0], points[1]);
        let slope = |a: u64, b: u64| (b as f64 - a as f64) / (n1 - n0) as f64 / 1024.0;
        println!(
            "{name}: per loaded function (slope {n0}->{n1}): rss {:.0} KiB, fp {:.0} KiB",
            slope(m0.0, m1.0),
            slope(m0.1, m1.1)
        );
        // Invocations in flight: 64 isolates alive at once, each waiting on
        // a timer.
        let mut with_guest = functions(1000);
        with_guest.push(entry(
            A,
            1,
            &bundle("guest.mjs"),
            manifest(json!({"limits": {"maxConcurrency": 128, "wasmMemoryMb": 32}})),
            json!({}),
        ));
        h.control.serve(support::fakes::Answer::Document(
            support::fakes::document_json(json!({ "functions": with_guest })),
        ));
        h.reconciler.reconcile_once(chrono::Utc::now()).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let idle = memory();
        let h = Arc::new(h);
        let calls: Vec<_> = (0..64)
            .map(|_| {
                let h = h.clone();
                tokio::spawn(async move { call(&h, A, "/sleep?ms=1500").await })
            })
            .collect();
        tokio::time::sleep(Duration::from_millis(900)).await;
        let busy = memory();
        for c in calls {
            assert_eq!(c.await.unwrap(), 200);
        }
        println!(
            "guest.mjs: 64 invocations in flight: rss +{:.1} MiB, fp +{:.1} MiB -> {:.0} KiB fp per invocation in flight",
            mib(busy.0.saturating_sub(idle.0)),
            mib(busy.1.saturating_sub(idle.1)),
            busy.1.saturating_sub(idle.1) as f64 / 64.0 / 1024.0
        );
        h.close().await;
    }
}

/// Where a request's time goes, in process (no HTTP): isolate creation (from
/// the base snapshot where [`fc_fnhost_js::engine::use_snapshot`]), loading
/// and running the bundle, the call, teardown. Also 2,000 isolates created
/// and disposed in a row: `FC_FN_JS_SNAPSHOT=true` on macOS aborted most runs
/// of it (see `use_snapshot`).
#[test]
#[ignore = "measurement: run in release with --ignored --nocapture"]
fn measure_request_phases() {
    use fc_fnhost_js::isolate::{Isolate, Limits};
    use fc_fnhost_js::modules::{FunctionModules, VersionCode};
    use fc_fnhost_js::ops::{HostState, VersionShared};
    use std::rc::Rc;

    let base = fc_fnhost_js::engine::init_v8().unwrap();
    let source = std::fs::read_to_string(bundle("hello.mjs")).unwrap();
    let control: Arc<dyn fc_fnhost_core::control_plane::ControlPlane> =
        support::fakes::FakeControlPlane::new();
    let version = Arc::new(VersionShared {
        address: fc_function_abi::FunctionAddress::parse(A).unwrap(),
        version: 1,
        logger: fc_fnhost_core::wasm::output::GuestLogger::for_address(A),
        config: Default::default(),
        secrets: Default::default(),
        allow: Default::default(),
        emitter: fc_fnhost_core::emit::Emitter {
            control_plane: control,
            host_id: "h".into(),
            host_runtime: None,
        },
        body_cap: 1 << 20,
        http: fc_fnhost_js::ops::http_client().unwrap(),
        host_runtime: None,
    });
    let prepared = fc_fnhost_js::prepare::prepare(
        base,
        source.as_bytes(),
        "default",
        Limits::of(32 << 20),
        version.clone(),
        Duration::from_secs(5),
    )
    .map_err(|r| r.detail)
    .unwrap();
    let code: VersionCode = prepared.code;
    println!(
        "code: bundle {} B, code cache {} B",
        code.bundle.len(),
        code.code_cache.as_ref().map_or(0, |c| c.len())
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (mut create, mut start, mut call, mut drop_) = (vec![], vec![], vec![], vec![]);
    for i in 0..2000 {
        let t = Instant::now();
        let host = Rc::new(HostState {
            version: version.clone(),
            invocation: None,
        });
        let mut isolate = Isolate::from_base(
            base,
            Limits::of(32 << 20),
            host,
            FunctionModules::new(code.clone()),
        )
        .unwrap();
        let t1 = Instant::now();
        runtime.block_on(isolate.start()).unwrap();
        isolate.set_host(Rc::new(HostState {
            version: version.clone(),
            invocation: Some(fc_fnhost_js::ops::InvocationState {
                context: fc_fnhost_js::ops::ContextOut {
                    invocation_id: "inv".into(),
                    address: A.into(),
                    version: 1,
                    caller: fc_fnhost_js::ops::CallerOut::Anonymous,
                    correlation_id: "inv".into(),
                    causation_id: None,
                    original_host: None,
                    original_path: None,
                    remote_address: None,
                    path_params: vec![("name".into(), "Ada".into())],
                },
                deadline: std::time::Instant::now() + Duration::from_secs(5),
                defaults: ("inv".into(), None),
            }),
        }));
        let t2 = Instant::now();
        let answer = runtime
            .block_on(isolate.call("GET", "http://localhost/hello/Ada", &[], &[]))
            .unwrap();
        assert_eq!(answer.0, 200);
        let t3 = Instant::now();
        drop(isolate);
        let t4 = Instant::now();
        if i >= 200 {
            create.push(t1 - t);
            start.push(t2 - t1);
            call.push(t3 - t2);
            drop_.push(t4 - t3);
        }
    }
    println!(
        "create ({}): {}",
        if base.is_some() {
            "base snapshot"
        } else {
            "no snapshot"
        },
        stats(create)
    );
    println!("start (load + top level): {}", stats(start));
    println!("call: {}", stats(call));
    println!("drop: {}", stats(drop_));
}

/// One function's `invoke` in process (no HTTP): the worker hand-off, the
/// watchdog and the isolate, without the listener.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "measurement: run in release with --ignored --nocapture"]
async fn measure_invoke_without_http() {
    let h = JsHarness::start_with(
        vec![entry(
            A,
            1,
            &bundle("hello.mjs"),
            manifest(json!({
                "limits": {"maxConcurrency": 64, "wasmMemoryMb": 32},
                "endpoints": [{"path": "/hello/{name}", "auth": "none"}],
            })),
            json!({"config": {"GREETING": "Hi"}}),
        )],
        Options {
            max_executing: 4,
            ..Options::default()
        },
    )
    .await;
    let function = h
        .reconciler
        .registry()
        .peek(&fc_function_abi::FunctionAddress::parse(A).unwrap())
        .expect("loaded");
    let context = || fc_fnhost_core::invoke::InvocationContext {
        invocation_id: "inv".into(),
        address: fc_function_abi::FunctionAddress::parse(A).unwrap(),
        version: 1,
        method: "GET".into(),
        path: "/hello/Ada".into(),
        original_host: Some("localhost".into()),
        original_path: None,
        path_params: [("name".to_string(), "Ada".to_string())]
            .into_iter()
            .collect(),
        query: Default::default(),
        raw_query: None,
        headers: Default::default(),
        body: Default::default(),
        remote_address: None,
        caller: fc_function_abi::Caller::Anonymous,
        deadline: std::time::Instant::now() + Duration::from_secs(5),
        interrupted: tokio_util::sync::CancellationToken::new(),
        correlation_id: "inv".into(),
        causation_id: None,
        usage: Default::default(),
    };
    let mut samples = Vec::new();
    for i in 0..2200 {
        let t = Instant::now();
        let response = function.instance().invoke(context()).await.unwrap();
        assert_eq!(response.status(), 200);
        if i >= 200 {
            samples.push(t.elapsed());
        }
    }
    println!("invoke in process, c=1: {}", stats(samples));
    h.close().await;
}
