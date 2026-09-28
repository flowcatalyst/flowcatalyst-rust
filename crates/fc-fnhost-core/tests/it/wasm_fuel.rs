//! Fuel and memory metering (owner decision #13): every invocation's fuel
//! and peak linear memory reach `/metrics` per function and client, and a
//! manifest's `limits.maxFuel` stops a guest that spends more with
//! `500 FUNCTION_FUEL_EXHAUSTED`, as the deadline stops one with 504.
//!
//! The overhead of metering is the ignored measurement, run in release
//! (`docs/function-runner-density.md` §9 records its output):
//!
//! ```text
//! cargo test --release -p fc-fnhost-core --test it wasm_fuel:: -- --ignored --nocapture --test-threads 1
//! ```

use crate::support;

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use support::wasm::{entry, guest, manifest, Options, WasmHarness, ADDR};

async fn start(name: &str, manifest_extra: Value, entry_extra: Value, fuel: bool) -> WasmHarness {
    WasmHarness::start_with(
        vec![entry(
            ADDR,
            1,
            &guest(name),
            manifest(manifest_extra),
            entry_extra,
        )],
        Options {
            consume_fuel: fuel,
            ..Options::default()
        },
    )
    .await
}

/// The value of the first `/metrics` line starting with `series`.
fn scrape(h: &WasmHarness, series: &str) -> Option<f64> {
    let text = h.metrics.encode().unwrap();
    text.lines()
        .find(|line| line.starts_with(series))
        .and_then(|line| line.rsplit(' ').next())
        .and_then(|value| value.parse().ok())
}

/// Consumption is recorded when the invocation task ends, just after the
/// caller is answered.
async fn eventually(h: &WasmHarness, series: &str) -> f64 {
    let start = Instant::now();
    loop {
        if let Some(value) = scrape(h, series) {
            return value;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{series} never appeared in:\n{}",
            h.metrics.encode().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_invocation_reports_its_fuel_and_peak_memory_per_function_and_client() {
    let h = start(
        "alloc",
        json!({"limits": {"maxConcurrency": 2, "wasmMemoryMb": 32}}),
        json!({"clientId": "clt_acme"}),
        true,
    )
    .await;
    let reply = h.get("/x?mb=8").await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    let labels = format!(r#"{{address="{ADDR}",client="clt_acme"}}"#);
    let fuel = eventually(&h, &format!("fc_fn_fuel_total{labels}")).await;
    assert!(fuel > 10_000.0, "an 8 MiB fill spends real fuel: {fuel}");
    assert_eq!(
        eventually(&h, &format!("fc_fn_invocation_fuel_count{labels}")).await,
        1.0
    );
    let peak = eventually(
        &h,
        &format!("fc_fn_invocation_peak_memory_bytes_sum{labels}"),
    )
    .await;
    assert!(
        (8.0 * 1024.0 * 1024.0..=32.0 * 1024.0 * 1024.0).contains(&peak),
        "the 8 MiB allocation is in the peak, within the 32 MiB cap: {peak}"
    );

    // A second, smaller call adds to the total; the per-call peak is its own.
    assert_eq!(h.get("/x?mb=1").await.status, 200);
    let start = Instant::now();
    while scrape(&h, &format!("fc_fn_invocation_fuel_count{labels}")) != Some(2.0) {
        assert!(start.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(scrape(&h, &format!("fc_fn_fuel_total{labels}")).unwrap() > fuel);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_platform_function_is_labelled_platform() {
    let h = start("echo", json!({}), json!({}), true).await;
    assert_eq!(h.get("/x").await.status, 200);
    eventually(
        &h,
        &format!(r#"fc_fn_fuel_total{{address="{ADDR}",client="PLATFORM"}}"#),
    )
    .await;
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_guest_past_its_max_fuel_is_stopped_with_a_clear_code_and_the_next_call_is_served() {
    let h = start(
        "spin",
        json!({
            // A deadline far away: fuel, not time, stops it.
            "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 20000}],
            "limits": {"maxConcurrency": 1, "wasmMemoryMb": 16, "maxFuel": 5_000_000},
        }),
        json!({"clientId": "clt_acme"}),
        true,
    )
    .await;
    let started = Instant::now();
    let spun = h.get("/x").await;
    assert_eq!(spun.status, 500, "{}", spun.text());
    assert_eq!(spun.error(), "FUNCTION_FUEL_EXHAUSTED");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "stopped by fuel, long before the deadline: {:?}",
        started.elapsed()
    );
    let labels = format!(r#"{{address="{ADDR}",client="clt_acme"}}"#);
    assert_eq!(
        eventually(&h, &format!("fc_fn_fuel_total{labels}")).await,
        5_000_000.0,
        "a guest stopped for fuel spent exactly its budget"
    );
    eventually(
        &h,
        &format!(
            r#"fc_fn_invocations_total{{address="{ADDR}",version="1",outcome="fuel_exhausted",entry="private"}} 1"#
        ),
    )
    .await;

    // Within the budget, the same function answers.
    let bounded = h.get("/x?n=1000").await;
    assert_eq!(bounded.status, 200, "{}", bounded.text());
    assert_eq!(bounded.json()["spun"], 1000);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_max_fuel_a_spinning_guest_still_stops_at_its_deadline_and_reports_its_fuel() {
    let h = start(
        "spin",
        json!({
            "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 200}],
            "limits": {"maxConcurrency": 1, "wasmMemoryMb": 16},
        }),
        json!({}),
        true,
    )
    .await;
    let spun = h.get("/x").await;
    assert_eq!(spun.status, 504, "{}", spun.text());
    assert_eq!(spun.error(), "FUNCTION_TIMEOUT");
    let fuel = eventually(
        &h,
        &format!(r#"fc_fn_fuel_total{{address="{ADDR}",client="PLATFORM"}}"#),
    )
    .await;
    assert!(
        fuel > 1_000_000.0,
        "200 ms of spinning is reported even though the guest was interrupted: {fuel}"
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unmetered_engine_reports_memory_and_ignores_max_fuel() {
    let h = start(
        "spin",
        json!({"limits": {"maxConcurrency": 1, "wasmMemoryMb": 16, "maxFuel": 1000}}),
        json!({}),
        false,
    )
    .await;
    let bounded = h.get("/x?n=100000").await;
    assert_eq!(bounded.status, 200, "{}", bounded.text());
    eventually(
        &h,
        &format!(
            r#"fc_fn_invocation_peak_memory_bytes_count{{address="{ADDR}",client="PLATFORM"}}"#
        ),
    )
    .await;
    assert_eq!(
        scrape(&h, "fc_fn_fuel_total"),
        None,
        "no fuel series without metering"
    );
    h.close().await;
}

// ── the overhead measurement ─────────────────────────────────────────────

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1]
}

fn cwasm_bytes(dir: &Path) -> u64 {
    fn walk(dir: &Path, total: &mut u64) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, total);
            } else if path.extension().is_some_and(|e| e == "cwasm") {
                *total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    let mut total = 0;
    walk(&dir.join("cwasm"), &mut total);
    total
}

/// Per workload, fuel off vs on: the median and p99 wall time of one call
/// (loopback HTTP included), the first (compiling) call and the `.cwasm`
/// size. The two hosts run side by side and the calls alternate between
/// them, so background load hits both alike.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "measurement: run in release with --ignored --nocapture"]
async fn measure_fuel_metering_overhead() {
    let workloads: [(&str, &str, usize); 4] = [
        ("echo", "/x", 4000),
        ("spin", "/x?n=20000000", 150),
        ("spin", "/x?hash=200", 150),
        ("alloc", "/x?mb=32", 600),
    ];
    for (name, path, calls) in workloads {
        let mut hosts = Vec::new();
        let mut firsts = Vec::new();
        for fuel in [false, true] {
            let h = start(
                name,
                json!({
                    "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 60000}],
                    "limits": {"maxConcurrency": 4, "wasmMemoryMb": 64},
                }),
                json!({"mode": "lazy"}),
                fuel,
            )
            .await;
            let first = Instant::now();
            assert_eq!(h.get(path).await.status, 200);
            firsts.push(first.elapsed());
            for _ in 0..(calls / 10).max(3) {
                assert_eq!(h.get(path).await.status, 200);
            }
            hosts.push(h);
        }
        let mut samples = [Vec::with_capacity(calls), Vec::with_capacity(calls)];
        for i in 0..calls * 2 {
            let which = i % 2;
            let t = Instant::now();
            let reply = hosts[which].get(path).await;
            assert_eq!(reply.status, 200, "{}", reply.text());
            samples[which].push(t.elapsed());
        }
        let summary: Vec<(Duration, Duration, u64)> = (0..2)
            .map(|which| {
                let mut sorted = samples[which].clone();
                sorted.sort();
                (
                    median(sorted.clone()),
                    percentile(&sorted, 0.99),
                    cwasm_bytes(hosts[which].dir.path()),
                )
            })
            .collect();
        for h in &hosts {
            h.close().await;
        }
        let (off, on) = (summary[0], summary[1]);
        println!(
            "{name} {path}: off p50 {:.3} ms p99 {:.3} ms | on p50 {:.3} ms p99 {:.3} ms | p50 overhead {:+.1}% | first call off {:.1} ms on {:.1} ms | .cwasm off {} B on {} B ({:+.1}%)",
            off.0.as_secs_f64() * 1e3,
            off.1.as_secs_f64() * 1e3,
            on.0.as_secs_f64() * 1e3,
            on.1.as_secs_f64() * 1e3,
            (on.0.as_secs_f64() / off.0.as_secs_f64() - 1.0) * 100.0,
            firsts[0].as_secs_f64() * 1e3,
            firsts[1].as_secs_f64() * 1e3,
            off.2,
            on.2,
            (on.2 as f64 / off.2.max(1) as f64 - 1.0) * 100.0,
        );
    }
}
