//! Component functions through the real listener (Java
//! `WasmFunctionListenerTest`, adapted to WASI 0.2 components): the
//! reconciler, fed by a fake control plane, fetches the committed guests
//! from `tests/fixtures/wasm/` through the artifact cache and loads them with
//! the real `WasmLoader`, and real HTTP calls reach them. Each guest is its
//! own component (a component has one entrypoint, `wasi:http/incoming-handler`,
//! so behaviours are separate artifacts rather than separate exports).

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fc_function_abi::EventEmitError;
use serde_json::{json, Value};
use support::listener::{signed, timestamp};
use support::wasm::{enc, entry, guest, manifest, WasmHarness, ADDR};

async fn one(name: &str, manifest_extra: Value, entry_extra: Value) -> WasmHarness {
    WasmHarness::start(vec![entry(
        ADDR,
        1,
        &guest(name),
        manifest(manifest_extra),
        entry_extra,
    )])
    .await
}

// ── the request and the invocation context ──────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_request_field_and_context_value_reaches_the_guest() {
    let h = one(
        "echo",
        json!({"endpoints": [{"path": "/echo/{id}", "auth": "none"}]}),
        json!({}),
    )
    .await;
    assert_eq!(
        h.heartbeat_states(),
        [(ADDR.to_owned(), 1, "LOADED".to_owned())],
        "a component loads for runtime: wasm"
    );
    let resp = h
        .post(
            "/echo/42?y=hello+world&y=again&z=%2Fa",
            "héllo body".as_bytes(),
            &[("X-Test-Custom", "hi"), ("X-Correlation-Id", "corr-1")],
        )
        .await;
    assert_eq!(resp.status, 200, "{}", resp.text());
    assert_eq!(
        resp.headers_all("x-guest"),
        ["echo", "twice"],
        "the guest's own multi-valued header reaches the caller"
    );
    let echoed = resp.json();
    assert_eq!(echoed["method"], "POST");
    assert_eq!(
        echoed["pathWithQuery"], "/echo/42?y=hello+world&y=again&z=%2Fa",
        "the function path, prefix stripped, with the query exactly as sent"
    );
    assert!(echoed["authority"]
        .as_str()
        .unwrap()
        .starts_with("127.0.0.1:"));
    let headers: Vec<(String, String)> = serde_json::from_value(echoed["headers"].clone()).unwrap();
    assert!(headers.contains(&("x-test-custom".into(), "hi".into())));
    assert_eq!(echoed["body"], "héllo body");
    assert_eq!(
        echoed["calls"], 1,
        "instance per request: always a fresh instance"
    );
    assert!(!echoed["invocationId"].as_str().unwrap().is_empty());
    assert_eq!(echoed["address"], ADDR);
    assert_eq!(echoed["version"], 1);
    assert_eq!(echoed["caller"]["kind"], "anonymous");
    assert_eq!(echoed["correlationId"], "corr-1");
    assert_eq!(echoed["causationId"], Value::Null);
    assert!(echoed["originalHost"]
        .as_str()
        .unwrap()
        .starts_with("127.0.0.1:"));
    assert_eq!(echoed["originalPath"], format!("/functions/{ADDR}/echo/42"));
    assert_eq!(echoed["remoteAddress"], "127.0.0.1");
    assert_eq!(echoed["pathParams"]["id"], "42");

    let again = h.get("/echo/7").await.json();
    assert_eq!(again["calls"], 1, "the next call is a fresh instance too");
    assert_ne!(again["invocationId"], echoed["invocationId"]);
    assert_eq!(
        again["correlationId"], again["invocationId"],
        "without X-Correlation-Id, events correlate to the invocation id"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_verified_webhook_delivery_is_the_platform_caller_with_the_events_ids() {
    let h = one(
        "echo",
        json!({"endpoints": [{"path": "/events/*", "auth": "webhook"}]}),
        json!({"webhookSigningSecret": "wh-secret-1", "clientId": "clt_1"}),
    )
    .await;
    let body =
        br#"{"id":"evt-9","type":"a:b:c:d","attemptNumber":1,"correlationId":"flow-7","data":{}}"#;
    let ts = timestamp(chrono::Utc::now());
    let sig = signed("wh-secret-1", &ts, body);
    let resp = h
        .post(
            "/events/x",
            body,
            &[
                ("X-FlowCatalyst-Signature", &sig),
                ("X-FlowCatalyst-Timestamp", &ts),
            ],
        )
        .await;
    assert_eq!(resp.status, 200, "{}", resp.text());
    let echoed = resp.json();
    assert_eq!(echoed["caller"]["kind"], "platform");
    assert_eq!(echoed["correlationId"], "flow-7");
    assert_eq!(echoed["causationId"], "evt-9");
    let headers = echoed["headers"].to_string().to_lowercase();
    assert!(
        !headers.contains("x-flowcatalyst-signature"),
        "the headers the host consumed are not passed on"
    );
}

// ── the deadline, memory, failure ────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_spinning_guest_is_stopped_at_its_deadline_and_the_next_call_is_served() {
    let h = one(
        "spin",
        json!({
            "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 200}],
            "limits": {"maxConcurrency": 1, "wasmMemoryMb": 16},
        }),
        json!({}),
    )
    .await;
    let start = Instant::now();
    let spun = h.get("/x").await;
    assert_eq!(spun.status, 504, "{}", spun.text());
    assert_eq!(spun.error(), "FUNCTION_TIMEOUT");
    assert!(start.elapsed() < Duration::from_secs(5));

    // The permit comes back only once the guest has really stopped.
    let freed = Instant::now();
    while h.function_permits() == Some(0) && freed.elapsed() < Duration::from_secs(3) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        h.function_permits(),
        Some(1),
        "the spinning guest let its permit go"
    );
    assert!(
        start.elapsed() < Duration::from_millis(200 + 1000),
        "the guest stops at the deadline, not whenever it likes: {:?}",
        start.elapsed()
    );

    let next = h.get("/x?spin=false").await;
    assert_eq!(next.status, 200, "{}", next.text());
    assert_eq!(next.json()["spun"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_allocation_past_the_memory_cap_is_a_500_and_the_next_call_is_served() {
    let h = one(
        "alloc",
        json!({"limits": {"maxConcurrency": 1, "wasmMemoryMb": 16}}),
        json!({}),
    )
    .await;
    let small = h.get("/x?mb=1").await;
    assert_eq!(small.status, 200, "{}", small.text());

    let too_big = h.get("/x?mb=64").await;
    assert_eq!(
        too_big.status,
        500,
        "64 MiB past a 16 MiB cap: {}",
        too_big.text()
    );
    assert_eq!(
        too_big.text(),
        r#"{"error":"the function failed"}"#,
        "a fixed reason, never the guest's own message"
    );

    let fits = h.get("/x?mb=8").await;
    assert_eq!(fits.status, 200, "{}", fits.text());
    assert_eq!(fits.json()["allocatedMb"], 8);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_response_body_past_the_memory_cap_is_a_500() {
    let h = one(
        "echo",
        json!({"limits": {"maxConcurrency": 1, "wasmMemoryMb": 2}}),
        json!({}),
    )
    .await;
    let fits = h.get("/x?bodyBytes=2000000").await;
    assert_eq!(fits.status, 200, "{}", fits.text());
    assert_eq!(fits.body.len(), 2_000_000);
    let over = h.get("/x?bodyBytes=3000000").await;
    assert_eq!(over.status, 500, "a 3 MB body past the 2 MiB cap");
    assert_eq!(over.text(), r#"{"error":"the function failed"}"#);
    assert_eq!(h.function_permits(), Some(1), "and the guest is gone");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_trap_is_a_500_that_never_shows_the_guests_message() {
    let h = one("fail", json!({}), json!({})).await;
    let failed = h.get("/x").await;
    assert_eq!(failed.status, 500);
    assert_eq!(failed.text(), r#"{"error":"the function failed"}"#);
    assert!(!failed.text().contains("do-not-leak"));
    let after = h.get("/x?fail=false").await;
    assert_eq!(after.status, 200, "{}", after.text());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn max_concurrency_calls_run_in_parallel_each_on_its_own_instance() {
    let n = 4usize;
    let ms = 400u64;
    let h = one(
        "echo",
        json!({"limits": {"maxConcurrency": n, "wasmMemoryMb": 16}}),
        json!({}),
    )
    .await;
    assert_eq!(h.get("/x?sleepMs=1").await.status, 200, "warm-up");
    let h = Arc::new(h);
    let start = Instant::now();
    let calls: Vec<_> = (0..n)
        .map(|_| {
            let h = h.clone();
            tokio::spawn(async move { h.get(&format!("/x?sleepMs={ms}")).await })
        })
        .collect();
    let mut bodies = Vec::new();
    for call in calls {
        let resp = call.await.unwrap();
        assert_eq!(resp.status, 200, "{}", resp.text());
        bodies.push(resp.json());
    }
    let elapsed = start.elapsed();
    let latest_start = bodies
        .iter()
        .map(|b| b["startNanos"].as_u64().unwrap())
        .max()
        .unwrap();
    let earliest_end = bodies
        .iter()
        .map(|b| b["endNanos"].as_u64().unwrap())
        .min()
        .unwrap();
    assert!(
        latest_start < earliest_end,
        "every guest run overlapped every other one"
    );
    assert!(
        elapsed < Duration::from_millis((ms + n as u64 * ms) / 2),
        "closer to one call ({ms} ms) than to {n} in series: {elapsed:?}"
    );
    let over = h.get("/x").await;
    assert_eq!(over.status, 200, "{}", over.text());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_call_past_max_concurrency_is_busy() {
    let h = Arc::new(
        one(
            "echo",
            json!({"limits": {"maxConcurrency": 1, "wasmMemoryMb": 16}}),
            json!({}),
        )
        .await,
    );
    let slow = {
        let h = h.clone();
        tokio::spawn(async move { h.get("/x?sleepMs=500").await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    let busy = h.get("/x").await;
    assert_eq!(busy.status, 429, "{}", busy.text());
    assert_eq!(slow.await.unwrap().status, 200);
}

// ── config and secrets ──────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_answers_a_declared_key_and_nothing_for_an_undeclared_one() {
    let h = one(
        "config",
        json!({"config": ["greeting"]}),
        json!({"config": {"greeting": "hello", "extra": "not-declared"}}),
    )
    .await;
    assert_eq!(h.guest_json("/x?key=greeting").await["value"], "hello");
    assert_eq!(
        h.guest_json("/x?key=extra").await["value"],
        Value::Null,
        "a key the platform holds but the manifest does not declare"
    );
    assert_eq!(h.guest_json("/x?key=missing").await["value"], Value::Null);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn secrets_answer_a_declared_key_and_nothing_for_an_undeclared_or_empty_one() {
    let h = one(
        "secret",
        json!({"secrets": ["api_token", "empty"]}),
        json!({"secrets": {"api_token": "s3cr3t-value-7f1c", "empty": "", "not_declared": "zzz"}}),
    )
    .await;
    assert_eq!(
        h.guest_json("/x?key=api_token").await["value"],
        "s3cr3t-value-7f1c"
    );
    assert_eq!(
        h.guest_json("/x?key=not_declared").await["value"],
        Value::Null
    );
    assert_eq!(h.guest_json("/x?key=empty").await["value"], Value::Null);
    assert_eq!(h.guest_json("/x?key=missing").await["value"], Value::Null);
}

// ── outbound HTTP ─────────────────────────────────────────────────────────

/// A loopback HTTP/1.1 server: `/ok` 201 with `x-upstream: yes` and the
/// guest's `x-from-guest`; `/redirect` 302 to `/ok`; `/slow` after 3 s.
fn upstream() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = Arc::new(AtomicUsize::new(0));
    let count = served.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let count = count.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                count.fetch_add(1, Ordering::SeqCst);
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let from_guest = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("x-from-guest: ")
                            .map(str::to_owned)
                    })
                    .unwrap_or_default();
                let reply = match path.as_str() {
                    "/ok" => {
                        let body = format!("upstream-ok from-guest={}", from_guest.trim());
                        format!(
                            "HTTP/1.1 201 Created\r\nx-upstream: yes\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    "/redirect" => "HTTP/1.1 302 Found\r\nlocation: /ok\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
                    "/slow" => {
                        std::thread::sleep(Duration::from_secs(3));
                        "HTTP/1.1 200 OK\r\ncontent-length: 4\r\nconnection: close\r\n\r\nslow".into()
                    }
                    _ => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
                };
                let _ = stream.write_all(reply.as_bytes());
            });
        }
    });
    (port, served)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn outbound_http_reaches_an_allowlisted_host_and_every_other_call_is_a_typed_denial() {
    let (port, served) = upstream();
    let h = one(
        "http",
        json!({
            "httpAllow": ["127.0.0.1"],
            "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 5000}],
        }),
        json!({}),
    )
    .await;

    let allowed = h
        .guest_json(&format!(
            "/x?url={}",
            enc(&format!("http://127.0.0.1:{port}/ok"))
        ))
        .await;
    assert_eq!(allowed["status"], 201, "{allowed}");
    assert_eq!(allowed["body"], "upstream-ok from-guest=yes");
    assert_eq!(allowed["xUpstream"], "yes");
    assert_eq!(served.load(Ordering::SeqCst), 1);

    // localhost is loopback (so plain http is permitted) but not on httpAllow.
    let denied = h
        .guest_json(&format!(
            "/x?url={}",
            enc(&format!("http://localhost:{port}/ok"))
        ))
        .await;
    assert_eq!(denied["error"], "ErrorCode::HttpRequestDenied", "{denied}");
    assert_eq!(
        served.load(Ordering::SeqCst),
        1,
        "the denied call never left the host"
    );

    let not_https = h
        .guest_json(&format!("/x?url={}", enc("http://example.com/ok")))
        .await;
    assert_eq!(
        not_https["error"], "ErrorCode::HttpRequestDenied",
        "{not_https}"
    );

    let redirect = h
        .guest_json(&format!(
            "/x?url={}",
            enc(&format!("http://127.0.0.1:{port}/redirect"))
        ))
        .await;
    assert_eq!(
        redirect["status"], 302,
        "the guest sees the redirect: {redirect}"
    );
    assert_eq!(redirect["location"], "/ok");
    assert_eq!(served.load(Ordering::SeqCst), 2, "and it was not followed");

    // The guest's own (shorter) timeout applies.
    let own_timeout = h
        .guest_json(&format!(
            "/x?timeoutMs=300&url={}",
            enc(&format!("http://127.0.0.1:{port}/slow"))
        ))
        .await;
    assert!(
        own_timeout["error"]
            .as_str()
            .is_some_and(|e| e.contains("Timeout")),
        "{own_timeout}"
    );
    assert!(own_timeout["ms"].as_u64().unwrap() < 2000, "{own_timeout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_outbound_call_never_outlives_the_invocation_deadline() {
    let (port, _) = upstream();
    let h = one(
        "http",
        json!({
            "httpAllow": ["127.0.0.1"],
            "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 800}],
            "limits": {"maxConcurrency": 1, "wasmMemoryMb": 16},
        }),
        json!({}),
    )
    .await;
    let start = Instant::now();
    // The guest asks for no timeout of its own; the upstream takes 3 s.
    let resp = h
        .get(&format!(
            "/x?url={}",
            enc(&format!("http://127.0.0.1:{port}/slow"))
        ))
        .await;
    assert_eq!(resp.status, 504, "{}", resp.text());
    let freed = Instant::now();
    while h.function_permits() == Some(0) && freed.elapsed() < Duration::from_secs(3) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        start.elapsed() < Duration::from_millis(2000),
        "the call and its guest ended with the deadline, not the upstream: {:?}",
        start.elapsed()
    );
}

// ── events ────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn emit_reaches_the_control_plane_and_a_refusal_is_a_typed_value() {
    let h = one("emit", json!({}), json!({})).await;

    let resp = h
        .get_headers("/x?dedupId=d-1", &[("X-Correlation-Id", "corr-emit")])
        .await;
    assert_eq!(resp.status, 200, "{}", resp.text());
    assert_eq!(resp.json()["result"]["ok"], true, "{}", resp.text());
    let sent = h.control.emits.lock().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].host_id, "host-1");
    assert_eq!(sent[0].address.render(), ADDR);
    assert_eq!(sent[0].version, 1);
    let item = &sent[0].events[0];
    assert_eq!(item.event_type, "fixture:guest:thing:happened");
    assert_eq!(item.dedup_id, "d-1");
    assert_eq!(item.subject.as_deref(), Some("thing-1"));
    assert_eq!(item.message_group.as_deref(), Some("group-1"));
    assert_eq!(item.data, json!({"from": "wasm"}));
    assert_eq!(
        item.correlation_id.as_deref(),
        Some("corr-emit"),
        "the invocation's correlation id is the default"
    );
    assert_eq!(item.causation_id, None);

    let explicit = h.guest_json("/x?dedupId=d-2&correlationId=mine").await;
    assert_eq!(explicit["result"]["ok"], true);
    assert_eq!(
        h.control.emits.lock()[1].events[0]
            .correlation_id
            .as_deref(),
        Some("mine"),
        "the event's own correlation id wins"
    );

    *h.control.emit_refusal.lock() = Some(EventEmitError::new("EVENT_TYPE_NOT_OWNED", 403));
    let refused = h.guest_json("/x?dedupId=d-3").await;
    assert_eq!(
        refused["result"],
        json!({"ok": false, "error": "refused", "code": "EVENT_TYPE_NOT_OWNED", "status": 403})
    );

    *h.control.emit_refusal.lock() = Some(EventEmitError::unavailable());
    let unavailable = h.guest_json("/x?dedupId=d-4").await;
    assert_eq!(
        unavailable["result"],
        json!({"ok": false, "error": "unavailable"})
    );

    *h.control.emit_refusal.lock() = None;
    let before = h.control.emits.lock().len();
    let no_dedup = h.guest_json("/x").await;
    assert_eq!(
        no_dedup["result"],
        json!({"ok": false, "error": "invalid", "code": "DEDUP_ID_REQUIRED"})
    );
    let not_json = h.guest_json("/x?dedupId=d-5&data=nope").await;
    assert_eq!(
        not_json["result"],
        json!({"ok": false, "error": "invalid", "code": "INVALID_EVENT: data is not JSON"})
    );
    let no_type = h.guest_json("/x?dedupId=d-6&type=+").await;
    assert_eq!(
        no_type["result"]["code"], "INVALID_EVENT: type is required",
        "{no_type}"
    );
    assert_eq!(
        h.control.emits.lock().len(),
        before,
        "refused before it reached the platform"
    );
}

// ── portability ───────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plain_wasi_http_component_with_no_flowcatalyst_imports_runs() {
    let bytes = std::fs::read(guest("pure")).unwrap();
    let accepted =
        fc_fnhost_core::wasm::inspect::check(&bytes, "wasi:http/incoming-handler", 16 << 20)
            .unwrap();
    assert!(
        !accepted
            .imports
            .iter()
            .any(|i| i.starts_with("flowcatalyst:")),
        "the pure guest imports nothing of ours: {:?}",
        accepted.imports
    );
    let h = one("pure", json!({}), json!({})).await;
    let resp = h.post("/any/where?q=1", b"plain", &[]).await;
    assert_eq!(resp.status, 200, "{}", resp.text());
    assert_eq!(
        resp.json(),
        json!({
            "pure": true,
            "method": "POST",
            "pathWithQuery": "/any/where?q=1",
            "authority": resp.json()["authority"].clone(),
            "body": "plain",
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_entrypoint_may_name_the_export_version() {
    let h = one(
        "pure",
        json!({"entrypoint": "wasi:http/incoming-handler@0.2.12"}),
        json!({}),
    )
    .await;
    assert_eq!(h.get("/x").await.status, 200);
}

// ── the instance pool ─────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_every_pool_slot_in_use_a_call_is_unavailable_not_a_failure() {
    let h = Arc::new(
        WasmHarness::start_with(
            vec![entry(
                ADDR,
                1,
                &guest("echo"),
                manifest(json!({"limits": {"maxConcurrency": 4, "wasmMemoryMb": 16}})),
                json!({}),
            )],
            support::wasm::Options {
                max_instances: 1,
                ..Default::default()
            },
        )
        .await,
    );
    let slow = {
        let h = h.clone();
        tokio::spawn(async move { h.get("/x?sleepMs=500").await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    let refused = h.get("/x").await;
    assert_eq!(refused.status, 503, "{}", refused.text());
    assert_eq!(refused.error(), "FUNCTION_UNAVAILABLE");
    assert_eq!(refused.header("retry-after").as_deref(), Some("15"));
    assert_eq!(slow.await.unwrap().status, 200);
    assert_eq!(h.get("/x").await.status, 200, "the slot is free again");
}
