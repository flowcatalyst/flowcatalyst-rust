//! Noisy neighbours and rough latency, through the real listener.
//!
//! The neighbour test runs by default with a deliberately generous bound, so
//! it catches containment breaking (A stuck behind B for most of B's
//! deadline) without flaking on a busy machine. The measurements behind
//! `docs/function-runner-plan.md` §5 H4 are the ignored tests, run in
//! release:
//!
//! ```text
//! cargo test --release -p fc-fnhost-core --test wasm_neighbour -- --ignored --nocapture --test-threads 1
//! ```

mod support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use support::wasm::{entry, guest, manifest, Options, WasmHarness};

const A: &str = "app.orders.a";
const B: &str = "app.orders.b";

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1]
}

struct Stats {
    p50: Duration,
    p99: Duration,
    max: Duration,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "p50 {:.2} ms, p99 {:.2} ms, max {:.2} ms",
            self.p50.as_secs_f64() * 1e3,
            self.p99.as_secs_f64() * 1e3,
            self.max.as_secs_f64() * 1e3
        )
    }
}

fn stats(mut samples: Vec<Duration>) -> Stats {
    samples.sort();
    Stats {
        p50: percentile(&samples, 0.50),
        p99: percentile(&samples, 0.99),
        max: *samples.last().unwrap(),
    }
}

async fn call(h: &WasmHarness, address: &str, path: &str) -> u16 {
    h.send(
        h.client
            .get(format!("{}/functions/{address}{path}", h.base)),
    )
    .await
    .status
}

/// A at `rate` calls/s, open loop, for `seconds`; each latency from its
/// scheduled start. Every call must succeed.
async fn drive_a(h: &Arc<WasmHarness>, rate: f64, seconds: f64) -> Vec<Duration> {
    let start = tokio::time::Instant::now();
    let total = (rate * seconds) as usize;
    let mut calls = Vec::with_capacity(total);
    for i in 0..total {
        tokio::time::sleep_until(start + Duration::from_secs_f64(i as f64 / rate)).await;
        let h = h.clone();
        calls.push(tokio::spawn(async move {
            let sent = Instant::now();
            let status = call(&h, A, "/x").await;
            assert_eq!(status, 200, "A must always be served");
            sent.elapsed()
        }));
    }
    let mut samples = Vec::with_capacity(total);
    for c in calls {
        samples.push(c.await.unwrap());
    }
    samples
}

/// `workers` loops calling B until `stop`; B spins to its deadline or is
/// refused BUSY.
fn hammer_b(
    h: &Arc<WasmHarness>,
    workers: usize,
    stop: &Arc<AtomicBool>,
) -> (Vec<tokio::task::JoinHandle<()>>, Arc<AtomicUsize>) {
    let spun = Arc::new(AtomicUsize::new(0));
    let handles = (0..workers)
        .map(|_| {
            let (h, stop, spun) = (h.clone(), stop.clone(), spun.clone());
            tokio::spawn(async move {
                while !stop.load(Ordering::Relaxed) {
                    match call(&h, B, "/x").await {
                        504 => {
                            spun.fetch_add(1, Ordering::Relaxed);
                        }
                        _ => tokio::time::sleep(Duration::from_millis(5)).await,
                    }
                }
            })
        })
        .collect();
    (handles, spun)
}

async fn neighbour_run(
    max_executing: usize,
    b_concurrency: usize,
    seconds: f64,
) -> (Stats, Stats, usize) {
    let h = Arc::new(
        WasmHarness::start_with(
            vec![
                entry(
                    A,
                    1,
                    &guest("echo"),
                    manifest(json!({"limits": {"maxConcurrency": 16, "wasmMemoryMb": 16}})),
                    json!({}),
                ),
                entry(
                    B,
                    1,
                    &guest("spin"),
                    manifest(json!({
                        "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 200}],
                        "limits": {"maxConcurrency": b_concurrency, "wasmMemoryMb": 16},
                    })),
                    json!({}),
                ),
            ],
            Options {
                max_executing,
                max_instances: 64,
                host_max_concurrency: 64,
            },
        )
        .await,
    );
    drive_a(&h, 100.0, 0.5).await; // warm-up
    let alone = stats(drive_a(&h, 100.0, seconds).await);
    let stop = Arc::new(AtomicBool::new(false));
    let (workers, spun) = hammer_b(&h, b_concurrency * 3, &stop);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let busy = stats(drive_a(&h, 100.0, seconds).await);
    stop.store(true, Ordering::Relaxed);
    for w in workers {
        w.await.unwrap();
    }
    (alone, busy, spun.load(Ordering::Relaxed))
}

/// B spins at its `maxConcurrency` (2) below the executing cap (3): a guest
/// thread is always free for A, and the listener never runs a guest. A's p99
/// must stay far below B's 200 ms deadline (without containment A queues
/// behind B's spinners for most of it).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_spinning_neighbour_under_the_executing_cap_leaves_a_within_bounds() {
    let (alone, busy, spun) = neighbour_run(3, 2, 2.0).await;
    println!("A alone: {alone}\nA busy:  {busy} (B spun {spun} times)");
    assert!(spun >= 10, "B really spun: {spun}");
    assert!(
        busy.p99 < Duration::from_millis(100).max(alone.p99 * 10),
        "A's p99 with B spinning: {busy} (alone: {alone})"
    );
}

/// The same with B allowed more spinners than there are guest threads: A
/// shares the threads round-robin at the 1 ms epoch tick. Measurement only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "measurement: run in release with --ignored --nocapture"]
async fn measure_neighbour_saturating_the_executing_cap() {
    for (cap, b) in [(3, 2), (3, 8), (8, 8)] {
        let (alone, busy, spun) = neighbour_run(cap, b, 5.0).await;
        println!("FC_FN_MAX_EXECUTING={cap} B maxConcurrency={b}: A alone {alone} | A busy {busy} | B spun {spun}");
    }
}

/// First call and steady per-call latency through the listener (loopback
/// HTTP included), per guest. Measurement only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "measurement: run in release with --ignored --nocapture"]
async fn measure_latency_through_the_listener() {
    for name in ["pure", "echo"] {
        let dir = tempfile::tempdir().unwrap();
        let functions = || {
            vec![entry(
                A,
                1,
                &guest(name),
                manifest(json!({"limits": {"maxConcurrency": 64, "wasmMemoryMb": 16}})),
                json!({"mode": "lazy"}),
            )]
        };
        let started = Instant::now();
        let h = Arc::new(WasmHarness::start_in(dir, functions(), Options::default()).await);
        let reconciled = started.elapsed();
        let first = Instant::now();
        assert_eq!(call(&h, A, "/x").await, 200);
        let first = first.elapsed();
        let second = Instant::now();
        assert_eq!(call(&h, A, "/x").await, 200);
        let second = second.elapsed();
        let mut steady = Vec::new();
        for _ in 0..2000 {
            let t = Instant::now();
            assert_eq!(call(&h, A, "/x").await, 200);
            steady.push(t.elapsed());
        }
        let steady = stats(steady);
        let concurrent = {
            let t = Instant::now();
            let calls: Vec<_> = (0..16)
                .map(|_| {
                    let h = h.clone();
                    tokio::spawn(async move {
                        for _ in 0..250 {
                            assert_eq!(call(&h, A, "/x").await, 200);
                        }
                    })
                })
                .collect();
            for c in calls {
                c.await.unwrap();
            }
            4000.0 / t.elapsed().as_secs_f64()
        };
        h.close().await;
        let dir = Arc::try_unwrap(h).ok().unwrap().dir;
        // A second host over the same cache: the lazy first call deserializes
        // the .cwasm instead of compiling.
        let cold = WasmHarness::start_in(dir, functions(), Options::default()).await;
        let t = Instant::now();
        assert_eq!(call(&cold, A, "/x").await, 200);
        let from_cwasm = t.elapsed();
        println!(
            "{name}: reconcile {:.1} ms | first call (lazy load: compile + instantiate) {:.2} ms | first call from .cwasm {:.2} ms | second {:.2} ms | steady c=1 {steady} | c=16 {concurrent:.0} calls/s",
            reconciled.as_secs_f64() * 1e3,
            first.as_secs_f64() * 1e3,
            from_cwasm.as_secs_f64() * 1e3,
            second.as_secs_f64() * 1e3,
        );
    }
}
