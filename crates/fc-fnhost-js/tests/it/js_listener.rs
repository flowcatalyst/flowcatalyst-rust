//! JS functions through the real listener (the JS counterpart of
//! fc-fnhost-core's `wasm_listener.rs`): the reconciler, fed by the fake
//! control plane, fetches the committed bundles from `tests/fixtures/js/`
//! through the artifact cache and loads them with the real `JsLoader`, and
//! real HTTP calls reach them.

use crate::support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fc_function_abi::EventEmitError;
use serde_json::{json, Value};
use support::{bundle, enc, entry, manifest, JsHarness, Options, ADDR};

async fn guest(manifest_extra: Value, entry_extra: Value) -> JsHarness {
    JsHarness::start(vec![entry(
        ADDR,
        1,
        &bundle("guest.mjs"),
        manifest(manifest_extra),
        entry_extra,
    )])
    .await
}

/// Java's fixed failure body: nothing of the guest's own error reaches the
/// caller.
fn assert_generic_500(reply: &support::Reply) {
    assert_eq!(reply.status, 500, "{}", reply.text());
    assert_eq!(reply.json(), json!({"error": "the function failed"}));
}

// ── the request, the response, the context ──────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_request_field_and_context_value_reaches_the_function() {
    let h = guest(
        json!({"endpoints": [{"path": "/echo", "auth": "none"}]}),
        json!({}),
    )
    .await;
    assert_eq!(
        h.heartbeat_states(),
        [(ADDR.to_owned(), 1, "LOADED".to_owned())]
    );
    let reply = h
        .post(
            "/echo?y=hello+world&y=again&z=%2Fa",
            "héllo body".as_bytes(),
            &[("X-Test-Custom", "hi"), ("X-Correlation-Id", "corr-1")],
        )
        .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(
        reply.headers_all("x-guest"),
        ["echo", "twice"],
        "a repeated response header reaches the caller one line each"
    );
    let body = reply.json();
    assert_eq!(body["method"], "POST");
    assert_eq!(body["path"], "/echo");
    assert!(
        body["url"]
            .as_str()
            .unwrap()
            .ends_with("/echo?y=hello+world&y=again&z=%2Fa"),
        "the raw query, as received: {}",
        body["url"]
    );
    assert_eq!(body["queryAll"], json!(["hello world", "again"]));
    assert_eq!(body["query"]["z"], "/a");
    assert_eq!(body["headers"]["x-test-custom"], "hi");
    assert_eq!(body["body"], "héllo body");
    let context = &body["context"];
    assert_eq!(context["address"], ADDR);
    assert_eq!(context["version"], 1);
    assert_eq!(context["caller"], json!({"kind": "anonymous"}));
    assert_eq!(context["correlationId"], "corr-1");
    assert_eq!(context["originalPath"], format!("/functions/{ADDR}/echo"));
    assert_eq!(context["remoteAddress"], "127.0.0.1");
    assert!(context["invocationId"].as_str().unwrap().len() >= 13);
    assert!(context.get("causationId").is_none());
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_request_gets_a_fresh_isolate_from_the_snapshot() {
    let h = guest(json!({}), json!({})).await;
    let first = h.guest_json("/counter").await;
    let second = h.guest_json("/counter").await;
    assert_eq!(first["counter"], 1, "top-level state is the snapshot's");
    assert_eq!(second["counter"], 1, "nothing survives the request");
    assert_eq!(first["table"], 100, "top-level work is in the snapshot");
    assert_ne!(
        first["random"], second["random"],
        "Math.random is not frozen into the snapshot"
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn binary_bodies_statuses_and_headers_pass_through() {
    let h = guest(json!({}), json!({})).await;
    let reply = h.post("/binary", &[0u8, 1, 2, 255], &[]).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, [255u8, 2, 1, 0]);
    let reply = h.get("/status?code=418").await;
    assert_eq!(reply.status, 418);
    assert_eq!(reply.headers_all("set-cookie"), ["a=1"]);
    let reply = h.get("/nowhere").await;
    assert_eq!((reply.status, reply.text()), (404, "not found".to_owned()));
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_entrypoint_names_the_export_a_function_or_an_object_with_fetch() {
    let h = JsHarness::start(vec![
        entry(
            ADDR,
            1,
            &bundle("guest.mjs"),
            manifest(json!({"entrypoint": "named"})),
            json!({}),
        ),
        entry(
            "app.orders.object",
            1,
            &bundle("guest.mjs"),
            manifest(json!({"entrypoint": "objectStyle"})),
            json!({}),
        ),
    ])
    .await;
    assert_eq!(h.get("/x").await.text(), "named /x");
    assert_eq!(
        h.get_at("app.orders.object", "/y").await.text(),
        "object GET"
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_function_sees_the_web_subset_and_no_node_deno_wasm_or_atomics_wait() {
    let h = guest(json!({}), json!({})).await;
    let globals = h.guest_json("/globals").await;
    assert_eq!(
        globals,
        json!({
            "deno": "undefined",
            "host": "undefined",
            "process": "undefined",
            "require": "undefined",
            "wasm": "undefined",
            "uuid": true,
            "encoded": "héllo",
            "base64": "aGk=hi",
            "url": "https://example.test/b?x=1#h",
            "cloned": {"a": [1, 2]},
            "atomicsWait": "TypeError",
        })
    );
    h.close().await;
}

// ── failures: Java's fixed 500 ──────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_throw_a_rejection_a_non_response_or_a_promise_that_never_settles_is_the_generic_500() {
    let h = guest(json!({}), json!({})).await;
    for path in ["/throw", "/reject", "/not-response", "/never"] {
        let reply = h.get(path).await;
        assert_generic_500(&reply);
        assert!(
            !reply.text().contains("secret-ish"),
            "the guest's message never reaches the caller"
        );
    }
    // The function is fine afterwards.
    assert_eq!(h.guest_json("/counter").await["counter"], 1);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_response_body_over_the_memory_limit_is_the_generic_500() {
    let h = guest(json!({"limits": {"wasmMemoryMb": 8}}), json!({})).await;
    assert_eq!(h.get("/big?bytes=1024").await.status, 200);
    assert_generic_500(&h.get(&format!("/big?bytes={}", 8 << 20 | 1)).await);
    h.close().await;
}

// ── limits: deadline, memory, executing threads ─────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_deadline_stops_a_spinning_function_and_the_host_carries_on() {
    let h = guest(
        json!({"endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 200}]}),
        json!({}),
    )
    .await;
    let started = Instant::now();
    let reply = h.get("/spin").await;
    assert_eq!(reply.status, 504, "{}", reply.text());
    assert_eq!(reply.json()["error"], "FUNCTION_TIMEOUT");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "stopped at the deadline, not left spinning: {:?}",
        started.elapsed()
    );
    // A function waiting on a timer is stopped as well.
    let reply = h.get("/sleep?ms=5000").await;
    assert_eq!(reply.status, 504);
    // Both workers are free again.
    assert_eq!(h.get("/busy?ms=1").await.text(), "busy");
    assert_eq!(h.get("/busy?ms=1").await.text(), "busy");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_memory_limit_ends_the_call_with_the_generic_500_not_the_host() {
    let h = guest(json!({"limits": {"wasmMemoryMb": 16}}), json!({})).await;
    // The V8 heap.
    assert_generic_500(&h.get("/alloc").await);
    // ArrayBuffer storage, outside the heap: capped too.
    let reply = h.get("/buffer").await;
    assert_ne!(
        reply.text(),
        "no limit",
        "1000 MiB of buffers under a 16 MiB cap"
    );
    assert!(
        reply.status == 500 || reply.json()["error"] == "RangeError",
        "{}",
        reply.text()
    );
    // The next call is fine.
    assert_eq!(h.guest_json("/counter").await["counter"], 1);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn at_most_fc_fn_max_executing_functions_run_javascript_at_once() {
    async fn two_busy_calls(max_executing: usize) -> Duration {
        let h = JsHarness::start_with(
            vec![entry(
                ADDR,
                1,
                &bundle("guest.mjs"),
                manifest(json!({})),
                json!({}),
            )],
            Options {
                max_executing,
                ..Options::default()
            },
        )
        .await;
        let started = Instant::now();
        let (a, b) = tokio::join!(h.get("/busy?ms=400"), h.get("/busy?ms=400"));
        let took = started.elapsed();
        assert_eq!((a.status, b.status), (200, 200));
        h.close().await;
        took
    }
    let one = two_busy_calls(1).await;
    assert!(
        one >= Duration::from_millis(790),
        "one worker runs the two 400 ms computations one after the other: {one:?}"
    );
    let two = two_busy_calls(2).await;
    assert!(
        two * 4 < one * 3,
        "two workers run them side by side: {two:?} against {one:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_function_waiting_on_io_holds_no_worker() {
    let h = JsHarness::start_with(
        vec![entry(
            ADDR,
            1,
            &bundle("guest.mjs"),
            manifest(json!({})),
            json!({}),
        )],
        Options {
            max_executing: 1,
            ..Options::default()
        },
    )
    .await;
    let started = Instant::now();
    let calls: Vec<_> = (0..6).map(|_| h.get("/sleep?ms=300")).collect();
    for reply in futures::future::join_all(calls).await {
        assert_eq!(reply.text(), "slept");
    }
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "six sleeping calls share one worker (one after the other would be 1800 ms): {:?}",
        started.elapsed()
    );
    h.close().await;
}

// ── config, secrets ─────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_and_secrets_are_the_declared_keys_only() {
    let h = guest(
        json!({"config": ["GREETING"], "secrets": ["API_KEY", "EMPTY"]}),
        json!({
            "config": {"GREETING": "hello", "UNDECLARED": "x"},
            "secrets": {"API_KEY": "s3cr3t", "EMPTY": "", "OTHER": "y"},
        }),
    )
    .await;
    assert_eq!(
        h.guest_json("/config?key=GREETING").await,
        json!({"value": "hello", "viaAll": "hello"})
    );
    assert_eq!(
        h.guest_json("/config?key=UNDECLARED").await,
        json!({"value": null, "viaAll": null})
    );
    assert_eq!(
        h.guest_json("/secret?key=API_KEY").await,
        json!({"present": true, "length": 6})
    );
    for key in ["EMPTY", "OTHER"] {
        assert_eq!(
            h.guest_json(&format!("/secret?key={key}")).await,
            json!({"present": false, "length": 0}),
            "{key}"
        );
    }
    h.close().await;
}

// ── events ──────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn emit_answers_the_event_id_and_the_platform_gets_the_defaults() {
    let h = guest(json!({}), json!({})).await;
    let reply = h
        .send(
            h.client
                .get(format!("{}/functions/{ADDR}/emit?dedup=d-9", h.base))
                .header("X-Correlation-Id", "corr-7"),
        )
        .await;
    assert_eq!(reply.json(), json!({"ok": true, "id": "evt_1"}));
    let emits = h.control.emits.lock().clone();
    assert_eq!(emits.len(), 1);
    let sent = &emits[0];
    assert_eq!(sent.address.render(), ADDR);
    assert_eq!(sent.version, 1);
    let event = &sent.events[0];
    assert_eq!(event.event_type, "app:orders:order:shipped");
    assert_eq!(event.dedup_id, "d-9");
    assert_eq!(event.subject.as_deref(), Some("order/1"));
    assert_eq!(event.data, json!({"id": 1, "ok": true}));
    assert_eq!(event.correlation_id.as_deref(), Some("corr-7"));
    assert_eq!(event.causation_id, None);
    // The function's own correlation id wins.
    h.get("/emit?correlation=mine").await;
    assert_eq!(
        h.control.emits.lock()[1].events[0]
            .correlation_id
            .as_deref(),
        Some("mine")
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn emit_refusals_are_values_mirroring_emit_event_error() {
    let h = guest(json!({}), json!({})).await;
    assert_eq!(
        h.guest_json("/emit?dedup=").await,
        json!({"ok": false, "error": {"kind": "invalid", "code": "DEDUP_ID_REQUIRED"}})
    );
    assert_eq!(
        h.guest_json("/emit?type=").await,
        json!({"ok": false, "error": {"kind": "invalid", "code": "INVALID_EVENT: type is required"}})
    );
    assert_eq!(
        h.guest_json("/emit?badData=1").await,
        json!({"ok": false, "error": {"kind": "invalid", "code": "INVALID_EVENT: data is not JSON"}})
    );
    assert!(
        h.control.emits.lock().is_empty(),
        "none reached the platform"
    );
    *h.control.emit_refusal.lock() = Some(
        EventEmitError::new("EVENT_TYPE_NOT_OWNED", 422)
            .with_message("the event type is not owned by the function's application"),
    );
    assert_eq!(
        h.guest_json("/emit").await,
        json!({"ok": false, "error": {
            "kind": "refused",
            "code": "EVENT_TYPE_NOT_OWNED",
            "status": 422,
            "message": "the event type is not owned by the function's application",
        }})
    );
    *h.control.emit_refusal.lock() = Some(EventEmitError::unavailable());
    let unavailable = h.guest_json("/emit").await;
    assert_eq!(unavailable["ok"], false);
    assert_eq!(unavailable["error"]["kind"], "unavailable");
    h.close().await;
}

// ── outbound HTTP under httpAllow ───────────────────────────────────────

/// A loopback upstream: `/ok` answers 201 with `x-upstream`, `/redirect`
/// a 302, `/slow` after 5 s.
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
                let from = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("x-from: ")
                            .map(str::to_owned)
                    })
                    .unwrap_or_default();
                let reply = match path.as_str() {
                    "/ok" => {
                        let body = format!("upstream-ok from={}", from.trim());
                        format!(
                            "HTTP/1.1 201 Created\r\nx-upstream: yes\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    "/redirect" => "HTTP/1.1 302 Found\r\nlocation: /ok\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
                    "/slow" => {
                        std::thread::sleep(Duration::from_secs(5));
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
async fn fetch_reaches_only_allowed_hosts_https_only_except_loopback_and_never_follows_redirects() {
    let (port, served) = upstream();
    let h = guest(
        json!({
            "httpAllow": ["127.0.0.1", "*.allowed.test"],
            "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 2000}],
        }),
        json!({}),
    )
    .await;
    let call = |url: String| {
        let h = &h;
        async move { h.guest_json(&format!("/http?url={}", enc(&url))).await }
    };
    assert_eq!(
        call(format!("http://127.0.0.1:{port}/ok")).await,
        json!({"status": 201, "body": "upstream-ok from=guest", "header": "yes"}),
        "an allowed loopback host, over plain http"
    );
    assert_eq!(
        call(format!("http://127.0.0.1:{port}/redirect")).await["status"],
        302,
        "the redirect is the function's to see"
    );
    let denied = call(format!("http://localhost:{port}/ok")).await;
    assert_eq!(denied["error"], "HttpError");
    assert_eq!(denied["code"], "HTTP-request-denied", "{denied}");
    let https_only = call("http://api.allowed.test/ok".into()).await;
    assert_eq!(https_only["code"], "HTTP-request-denied");
    assert!(
        https_only["message"]
            .as_str()
            .unwrap()
            .contains("https only"),
        "{https_only}"
    );
    let apex = call("https://allowed.test/ok".into()).await;
    assert_eq!(apex["code"], "HTTP-request-denied", "never the apex");
    // The deadline caps the call: the upstream takes 5 s, the endpoint 2 s.
    let started = Instant::now();
    let slow = h
        .get(&format!(
            "/http?url={}",
            enc(&format!("http://127.0.0.1:{port}/slow"))
        ))
        .await;
    assert!(
        slow.status == 504 || slow.json()["code"] == "HTTP-response-timeout",
        "{}",
        slow.text()
    );
    assert!(started.elapsed() < Duration::from_millis(4500));
    assert_eq!(
        served.load(Ordering::SeqCst),
        3,
        "denied calls never left the host"
    );
    h.close().await;
}

// ── the template: templates/function-ts, built by esbuild ───────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_typescript_template_runs_on_the_host() {
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../templates/function-ts/manifest.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let h = JsHarness::start(vec![entry(
        ADDR,
        1,
        &bundle("hello.mjs"),
        manifest,
        json!({"config": {"GREETING": "Hi"}, "webhookSigningSecret": "wh-hello"}),
    )])
    .await;
    assert_eq!(
        h.guest_json("/hello/Ada").await,
        json!({"message": "Hi, Ada!"})
    );
    let body =
        json!({"id": "evt-1", "type": "hello:greeting:greeting:requested", "attemptNumber": 1})
            .to_string();
    let ts = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let reply = h
        .send(
            h.client
                .post(format!(
                    "{}/functions/{ADDR}/events/greeting-requested",
                    h.base
                ))
                .header(
                    "X-FlowCatalyst-Signature",
                    fc_fnhost_core::listener::webhook::sign("wh-hello", &ts, body.as_bytes()),
                )
                .header("X-FlowCatalyst-Timestamp", &ts)
                .body(body),
        )
        .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert!(
        reply.body.is_empty(),
        "an empty 200 acknowledges the delivery"
    );
    // Unsigned, the host refuses before the function runs.
    let unsigned = h.post("/events/greeting-requested", b"{}", &[]).await;
    assert_eq!(unsigned.status, 401, "{}", unsigned.text());
    h.close().await;
}
