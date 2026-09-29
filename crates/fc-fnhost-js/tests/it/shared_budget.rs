//! `FC_FN_MAX_EXECUTING` is one budget for the whole host: WASM guests and
//! JS isolates, loaded side by side in one host (the deployed assembly),
//! take their executing permits from the same place, only while they run
//! (never while they wait on I/O), and a guest waiting for a permit still
//! ends at its deadline.

use crate::support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use std::thread;
use support::{bundle, enc, entry, manifest, wasm_guest, wasm_manifest, JsHarness, Options};
use tokio::time;

const JS: &str = "app.orders.js";
const JS_HOG: &str = "app.orders.hog";
const WASM: &str = "app.orders.wasm";
const WASM_HTTP: &str = "app.orders.http";

fn js(address: &str, timeout_ms: u64) -> Value {
    entry(
        address,
        1,
        &bundle("guest.mjs"),
        manifest(json!({
            "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": timeout_ms}],
            "httpAllow": ["127.0.0.1"],
        })),
        json!({}),
    )
}

fn wasm(address: &str, guest: &str, timeout_ms: u64) -> Value {
    entry(
        address,
        1,
        &wasm_guest(guest),
        wasm_manifest(json!({
            "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": timeout_ms}],
            "httpAllow": ["127.0.0.1"],
        })),
        json!({}),
    )
}

async fn mixed(
    functions: Vec<Value>,
    max_executing: usize,
    js_workers: Option<usize>,
) -> JsHarness {
    JsHarness::start_with(
        functions,
        Options {
            max_executing,
            js_workers,
            wasm: true,
            ..Options::default()
        },
    )
    .await
}

/// Waits (briefly) for the budget to be idle: an isolate's teardown, which
/// holds a permit too, comes after its answer.
async fn settled(h: &JsHarness) {
    let until = Instant::now() + Duration::from_secs(2);
    while (h.budget.executing(), h.budget.waiting()) != (0, 0) {
        assert!(
            Instant::now() < until,
            "the budget stays busy: {:?}",
            h.budget
        );
        time::sleep(Duration::from_millis(5)).await;
    }
}

/// A loopback upstream whose every answer takes `delay`.
fn slow_upstream(delay: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                thread::sleep(delay);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 4\r\nconnection: close\r\n\r\nslow",
                );
            });
        }
    });
    port
}

/// Two CPU-bound WASM guests and two CPU-bound JS functions at once, with
/// `FC_FN_MAX_EXECUTING=2`. Each runtime has two threads of its own, so
/// without the shared budget all four would run side by side; with it, at
/// most two execute at any moment (the budget's own count, and the time
/// the four take).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn wasm_and_js_guests_share_one_executing_budget() {
    let h = mixed(vec![js(JS, 10_000), wasm(WASM, "spin", 10_000)], 2, None).await;
    // Size the WASM work to about as long as the JS work (300 ms).
    let rounds = {
        let started = Instant::now();
        assert_eq!(h.get_at(WASM, "/x?hash=20").await.status, 200);
        let per_round = started.elapsed().as_secs_f64() / 20.0;
        ((0.3 / per_round) as u64).clamp(5, 5_000)
    };
    let alone = {
        let started = Instant::now();
        assert_eq!(
            h.get_at(WASM, &format!("/x?hash={rounds}")).await.status,
            200
        );
        started.elapsed()
    };
    h.budget.reset_peak();
    let started = Instant::now();
    let wasm_path = format!("/x?hash={rounds}");
    let (a, b, c, d) = tokio::join!(
        h.get_at(JS, "/busy?ms=300"),
        h.get_at(JS, "/busy?ms=300"),
        h.get_at(WASM, &wasm_path),
        h.get_at(WASM, &wasm_path),
    );
    let took = started.elapsed();
    for reply in [&a, &b, &c, &d] {
        assert_eq!(reply.status, 200, "{}", reply.text());
    }
    assert_eq!(h.budget.peak(), 2, "two permits, both used, never more");
    // 2 × 300 ms of JavaScript plus 2 × `alone` of WASM over two permits.
    let floor = (Duration::from_millis(600) + alone * 2) / 2;
    assert!(
        took >= floor.mul_f64(0.8),
        "four guests on two permits take at least half their total work: {took:?} (floor {floor:?}, WASM alone {alone:?})"
    );
    settled(&h).await;
    h.close().await;
}

/// With `FC_FN_MAX_EXECUTING=1`, a guest waiting on a slow upstream holds
/// no permit: a guest of the other runtime computes meanwhile, and ends
/// long before the upstream answers. Both ways round.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_guest_waiting_on_io_holds_no_executing_permit() {
    let port = slow_upstream(Duration::from_millis(1500));
    let h = mixed(
        vec![
            js(JS, 10_000),
            wasm(WASM, "spin", 10_000),
            wasm(WASM_HTTP, "http", 10_000),
        ],
        1,
        None,
    )
    .await;
    let slow = enc(&format!("http://127.0.0.1:{port}/slow"));

    // WASM waits on the upstream; JS computes.
    let started = Instant::now();
    let path = format!("/x?url={slow}");
    let waiting = h.get_at(WASM_HTTP, &path);
    let computing = async {
        time::sleep(Duration::from_millis(200)).await;
        let reply = h.get_at(JS, "/busy?ms=200").await;
        (reply, started.elapsed())
    };
    let (waited, (computed, computed_at)) = tokio::join!(waiting, computing);
    assert_eq!(computed.text(), "busy");
    assert!(
        computed_at < Duration::from_millis(1200),
        "the JS computation ran while the WASM guest waited: done at {computed_at:?}"
    );
    assert_eq!(waited.json()["status"], 200, "{}", waited.text());

    // JS waits on the upstream; WASM computes.
    let started = Instant::now();
    let path = format!("/http?url={slow}");
    let waiting = h.get_at(JS, &path);
    let computing = async {
        time::sleep(Duration::from_millis(200)).await;
        let reply = h.get_at(WASM, "/x?n=1000000").await;
        (reply, started.elapsed())
    };
    let (waited, (computed, computed_at)) = tokio::join!(waiting, computing);
    assert_eq!(computed.status, 200, "{}", computed.text());
    assert!(
        computed_at < Duration::from_millis(1200),
        "the WASM computation ran while the JS function waited: done at {computed_at:?}"
    );
    assert_eq!(waited.json()["body"], "slow", "{}", waited.text());
    assert_eq!(h.budget.peak(), 1);
    h.close().await;
}

/// With `FC_FN_MAX_EXECUTING=1` held by a JS computation (JavaScript is not
/// preempted), a WASM guest and a JS function on another worker wait for
/// the permit, and each ends with a timeout at its own deadline, not when
/// the permit frees.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn the_deadline_applies_while_waiting_for_a_permit() {
    let h = mixed(
        vec![js(JS_HOG, 10_000), js(JS, 300), wasm(WASM, "echo", 300)],
        1,
        Some(2),
    )
    .await;
    let hog = h.get_at(JS_HOG, "/busy?ms=2000");
    let waiters = async {
        time::sleep(Duration::from_millis(200)).await;
        assert_eq!(h.budget.executing(), 1, "the hog holds the one permit");
        let started = Instant::now();
        let (wasm, js) = tokio::join!(h.get_at(WASM, "/x"), h.get_at(JS, "/echo"));
        (wasm, js, started.elapsed())
    };
    let (hog, (wasm, js, took)) = tokio::join!(hog, waiters);
    assert_eq!(wasm.status, 504, "{}", wasm.text());
    assert_eq!(js.status, 504, "{}", js.text());
    assert!(
        took < Duration::from_millis(1000),
        "both timed out at their 300 ms deadline, not when the hog let go: {took:?}"
    );
    assert_eq!(hog.text(), "busy");
    settled(&h).await;
    // The permit is usable again.
    assert_eq!(h.get_at(WASM, "/x").await.status, 200);
    assert_eq!(h.get_at(JS, "/echo").await.status, 200);
    h.close().await;
}

/// A WASM guest that spins until its deadline gives its permit back at
/// every epoch tick: with `FC_FN_MAX_EXECUTING=1`, JS calls are served
/// meanwhile, promptly.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_spinning_wasm_guest_does_not_starve_js() {
    let h = mixed(vec![js(JS, 10_000), wasm(WASM, "spin", 1500)], 1, None).await;
    let spinning = h.get_at(WASM, "/x");
    let served = async {
        time::sleep(Duration::from_millis(200)).await;
        let mut slowest = Duration::ZERO;
        for _ in 0..5 {
            let started = Instant::now();
            assert_eq!(h.get_at(JS, "/echo").await.status, 200);
            slowest = slowest.max(started.elapsed());
        }
        slowest
    };
    let (spun, slowest) = tokio::join!(spinning, served);
    assert_eq!(spun.status, 504, "the spinner ran to its deadline");
    assert!(
        slowest < Duration::from_millis(500),
        "JS was served beside the spinner: slowest call {slowest:?}"
    );
    h.close().await;
}
