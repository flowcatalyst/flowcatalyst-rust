//! Guests written with the Rust guest SDK (`crates/fc-function-pdk`, plan
//! G1), end to end on the real host: the reconciler loads the committed
//! components (`tests/fixtures/wasm/{pdk,pdk-pure}.wasm`, rebuilt with
//! `tests/guests/build.sh pdk pdk-pure`), and real HTTP calls reach them
//! through the listener, with a fake control plane taking their events.
//!
//! Its own test binary, because it captures the process-wide log subscriber
//! (the guests run on the guest runtime's threads).

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use fc_fnhost_core::logging::SlogJsonLayer;
use fc_function_abi::EventEmitError;
use parking_lot::Mutex;
use serde_json::{json, Value};
use support::listener::{signed, timestamp};
use support::wasm::{enc, entry, guest, manifest, WasmHarness, ADDR};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

async fn pdk(manifest_extra: Value, entry_extra: Value) -> WasmHarness {
    WasmHarness::start(vec![entry(
        ADDR,
        1,
        &guest("pdk"),
        manifest(manifest_extra),
        entry_extra,
    )])
    .await
}

// ── the request, the invocation, the response builders ─────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pdk_guest_sees_its_request_and_invocation() {
    let bytes = std::fs::read(guest("pdk")).unwrap();
    let accepted =
        fc_fnhost_core::wasm::inspect::check(&bytes, "wasi_http_incoming_handler", 16 << 20)
            .unwrap();
    for interface in ["config", "secrets", "events", "log", "invocation"] {
        assert!(
            accepted
                .imports
                .iter()
                .any(|i| i.starts_with(&format!("flowcatalyst:function/{interface}@0.1.0"))),
            "the PDK binds flowcatalyst:function/{interface}: {:?}",
            accepted.imports
        );
    }

    let h = pdk(
        json!({"endpoints": [
            {"path": "/echo/{id}", "auth": "none"},
            {"path": "/*", "auth": "none"},
        ]}),
        json!({}),
    )
    .await;
    assert_eq!(
        h.heartbeat_states(),
        [(ADDR.to_owned(), 1, "LOADED".to_owned())]
    );
    let resp = h
        .send(
            h.client
                .post(format!(
                    "{}/functions/{ADDR}/echo/a%20b?y=hello+world&y=again&z=%2Fa",
                    h.base
                ))
                .header("X-Test-Custom", "hi")
                .header("X-Multi", "one")
                .header("X-Multi", "two")
                .header("X-Correlation-Id", "corr-pdk")
                .json(&json!({"order": {"id": 7}})),
        )
        .await;
    assert_eq!(resp.status, 200, "{}", resp.text());
    assert_eq!(
        resp.header("content-type").as_deref(),
        Some("application/json")
    );
    let echoed = resp.json();
    assert_eq!(echoed["method"], "POST");
    assert_eq!(echoed["path"], "/echo/a%20b");
    assert_eq!(echoed["rawQuery"], "y=hello+world&y=again&z=%2Fa");
    assert_eq!(
        echoed["query"],
        json!({"y": ["hello world", "again"], "z": ["/a"]}),
        "decoded as the host decodes it, repeats kept"
    );
    assert_eq!(echoed["header"], "hi", "found case-insensitively");
    assert_eq!(echoed["headerAll"], json!(["one", "two"]));
    assert!(echoed["authority"]
        .as_str()
        .unwrap()
        .starts_with("127.0.0.1:"));
    assert_eq!(echoed["body"], json!({"order": {"id": 7}}));
    assert_eq!(echoed["id"], "a b", "the path parameter, percent-decoded");
    assert!(!echoed["invocationId"].as_str().unwrap().is_empty());
    assert_eq!(echoed["address"], ADDR);
    assert_eq!(echoed["version"], 1);
    assert_eq!(echoed["caller"], json!({"kind": "anonymous"}));
    assert_eq!(echoed["correlationId"], "corr-pdk");
    assert_eq!(echoed["causationId"], Value::Null);
    assert_eq!(
        echoed["originalPath"],
        format!("/functions/{ADDR}/echo/a%20b")
    );
    assert_eq!(echoed["remoteAddress"], "127.0.0.1");

    let fail = h.get("/fail?msg=no+such+order").await;
    assert_eq!(fail.status, 500, "an Err is Java's fail");
    assert_eq!(fail.text(), r#"{"error":"no such order"}"#);

    let retry = h.get("/retry").await;
    assert_eq!(retry.status, 429);
    assert_eq!(retry.header("retry-after").as_deref(), Some("2"));

    let not_found = h.get("/nowhere").await;
    assert_eq!(not_found.status, 404);
    assert_eq!(not_found.error(), "no such route");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let clock = h.guest_json("/clock").await["epochMs"].as_i64().unwrap();
    assert!(
        (clock - now_ms).abs() < 60_000,
        "ctx.now() is the wall clock"
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_signed_delivery_parses_with_webhook_event_and_is_the_platform_caller() {
    let h = pdk(
        json!({"endpoints": [{"path": "/webhook", "auth": "webhook", "methods": ["POST"]}]}),
        json!({"webhookSigningSecret": "wh-1", "clientId": "clt_1"}),
    )
    .await;
    let body = br#"{"id":"evt-9","type":"a:b:c:d","attemptNumber":3,"data":{"x":[1,2]}}"#;
    let ts = timestamp(chrono::Utc::now());
    let resp = h
        .post(
            "/webhook",
            body,
            &[
                ("X-FlowCatalyst-Signature", &signed("wh-1", &ts, body)),
                ("X-FlowCatalyst-Timestamp", &ts),
            ],
        )
        .await;
    assert_eq!(resp.status, 200, "{}", resp.text());
    assert_eq!(
        resp.json(),
        json!({
            "id": "evt-9",
            "type": "a:b:c:d",
            "attempt": 3,
            "data": r#"{"x":[1,2]}"#,
            "caller": {"kind": "platform"},
        })
    );
    h.close().await;
}

// ── config, secrets, events ──────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_and_secrets_answer_declared_keys_only() {
    let h = pdk(
        json!({"config": ["GREETING"], "secrets": ["API_KEY"]}),
        json!({
            "config": {"GREETING": "hello", "EXTRA": "undeclared"},
            "secrets": {"API_KEY": "k-7f1c", "OTHER": "undeclared"},
        }),
    )
    .await;
    assert_eq!(
        h.guest_json("/config?key=GREETING").await,
        json!({"value": "hello", "require": {"Ok": "hello"}})
    );
    assert_eq!(
        h.guest_json("/config?key=EXTRA").await,
        json!({"value": null, "require": {"Err": "config key not declared: EXTRA"}})
    );
    assert_eq!(h.guest_json("/secret?key=API_KEY").await["value"], "k-7f1c");
    assert_eq!(
        h.guest_json("/secret?key=OTHER").await["value"],
        Value::Null
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn emit_reaches_the_control_plane_and_refusals_carry_javas_codes() {
    let h = pdk(json!({}), json!({})).await;
    let emit = |dedup: &'static str| {
        h.send(
            h.client
                .post(format!("{}/functions/{ADDR}/emit?dedupId={dedup}", h.base))
                .header("X-Correlation-Id", "corr-emit")
                .body(r#"{"n":1}"#),
        )
    };
    let ok = emit("d-1").await;
    assert_eq!(ok.json(), json!({"ok": true}), "{}", ok.text());
    let sent = h.control.emits.lock().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].address.render(), sent[0].version),
        (ADDR.to_owned(), 1)
    );
    let item = &sent[0].events[0];
    assert_eq!(item.event_type, "fixture:pdk:thing:happened");
    assert_eq!(item.dedup_id, "d-1");
    assert_eq!(item.subject.as_deref(), Some("thing-1"));
    assert_eq!(item.data, json!({"n": 1}));
    assert_eq!(
        item.correlation_id.as_deref(),
        Some("corr-emit"),
        "the invocation's correlation id by default"
    );

    *h.control.emit_refusal.lock() = Some(EventEmitError::new("EVENT_TYPE_NOT_OWNED", 403));
    assert_eq!(
        emit("d-2").await.json(),
        json!({
            "ok": false, "code": "EVENT_TYPE_NOT_OWNED", "status": 403, "retryable": false,
            "message": "emit refused: EVENT_TYPE_NOT_OWNED (403)",
        })
    );
    *h.control.emit_refusal.lock() = Some(EventEmitError::unavailable());
    assert_eq!(
        emit("d-3").await.json(),
        json!({
            "ok": false, "code": "UNAVAILABLE", "status": 503, "retryable": true,
            "message": "emit refused: UNAVAILABLE (503)",
        })
    );
    h.close().await;
}

// ── outbound HTTP ─────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn outbound_http_reaches_an_allowed_host_and_a_refusal_is_http_denied() {
    let upstream = Upstream::start(|_| (201, "upstream-ok".into()));
    let h = pdk(json!({"httpAllow": ["127.0.0.1"]}), json!({})).await;
    let allowed = h
        .guest_json(&format!(
            "/http?url={}",
            enc(&format!("http://127.0.0.1:{}/ok", upstream.port))
        ))
        .await;
    assert_eq!(
        allowed,
        json!({"status": 201, "upstream": "yes", "body": "upstream-ok"})
    );

    let posted = h
        .guest_json(&format!(
            "/http?body=hello+upstream&url={}",
            enc(&format!("http://127.0.0.1:{}/ok", upstream.port))
        ))
        .await;
    assert_eq!(posted["status"], 201, "{posted}");
    assert!(upstream.requests.lock()[1].ends_with("\r\n\r\nhello upstream"));

    // Loopback (so plain http would do), but not on httpAllow.
    let denied = h
        .guest_json(&format!(
            "/http?url={}",
            enc(&format!("http://localhost:{}/ok", upstream.port))
        ))
        .await;
    assert_eq!(denied["denied"], "localhost", "{denied}");
    assert!(denied["message"]
        .as_str()
        .unwrap()
        .starts_with("outbound call to 'localhost' refused"));
    let not_https = h
        .guest_json(&format!("/http?url={}", enc("http://example.com/x")))
        .await;
    assert_eq!(not_https["denied"], "example.com", "{not_https}");
    assert_eq!(
        upstream.requests.lock().len(),
        2,
        "the refusals never left the host"
    );
    h.close().await;
}

// ── logging ──────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_logger_the_log_crate_and_a_handler_error_reach_the_functions_logger() {
    captured();
    let h = pdk(json!({}), json!({})).await;
    let logged = h
        .get_headers("/log?msg=wave", &[("X-Correlation-Id", "corr-pdk-log")])
        .await;
    assert_eq!(logged.status, 200, "{}", logged.text());
    let failed = h
        .get_headers("/fail?msg=kaput", &[("X-Correlation-Id", "corr-pdk-log")])
        .await;
    assert_eq!(failed.status, 500);
    let rendered: Vec<String> = lines()
        .into_iter()
        .filter(|l| l["logger"] == format!("fn.{ADDR}") && l["correlation_id"] == "corr-pdk-log")
        .map(|l| {
            format!(
                "{} {}",
                l["level"].as_str().unwrap(),
                l["msg"].as_str().unwrap()
            )
        })
        .collect();
    for want in [
        "INFO pdk logger: wave",
        "WARN pdk log crate: wave",
        "ERROR the handler failed: kaput",
    ] {
        assert!(
            rendered.iter().any(|r| r == want),
            "{want} missing from {rendered:#?}"
        );
    }
    h.close().await;
}

// ── portability ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_the_flowcatalyst_feature_a_pdk_guest_is_a_plain_wasi_http_component() {
    let bytes = std::fs::read(guest("pdk-pure")).unwrap();
    let accepted =
        fc_fnhost_core::wasm::inspect::check(&bytes, "wasi:http/incoming-handler", 16 << 20)
            .unwrap();
    assert!(
        !accepted
            .imports
            .iter()
            .any(|i| i.starts_with("flowcatalyst:")),
        "imports nothing of ours: {:?}",
        accepted.imports
    );
    let h = WasmHarness::start(vec![entry(
        ADDR,
        1,
        &guest("pdk-pure"),
        manifest(json!({})),
        json!({}),
    )])
    .await;
    let resp = h.post("/any/where?x=1&x=2", br#"{"n":1}"#, &[]).await;
    assert_eq!(resp.status, 200, "{}", resp.text());
    assert_eq!(
        resp.json(),
        json!({
            "pure": true, "method": "POST", "path": "/any/where",
            "query": {"x": ["1", "2"]}, "body": {"n": 1},
        })
    );
    let bad = h.post("/x", b"nope", &[]).await;
    assert_eq!(bad.status, 500);
    assert!(bad.error().starts_with("expected ident"), "{}", bad.text());
    h.close().await;
}

// ── helpers ──────────────────────────────────────────────────────────────

/// A loopback HTTP/1.1 server that records each request (head and body) and
/// answers `answer(request)` with `x-upstream: yes`.
struct Upstream {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Upstream {
    fn start(answer: impl Fn(&str) -> (u16, String) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let answer = Arc::new(answer);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (seen, answer) = (seen.clone(), answer.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut head = String::new();
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            length = v.trim().parse().unwrap_or(0);
                        }
                        head.push_str(&line);
                        if line == "\r\n" {
                            break;
                        }
                    }
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let request = format!("{head}{}", String::from_utf8_lossy(&body));
                    let (status, reply) = answer(&request);
                    seen.lock().push(request);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} X\r\nx-upstream: yes\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                });
            }
        });
        Self { port, requests }
    }
}

fn captured() -> &'static Arc<Mutex<Vec<u8>>> {
    static LINES: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    LINES.get_or_init(|| {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let writer = {
            let lines = lines.clone();
            move || CaptureWriter(lines.clone())
        };
        // Not `init()`: that would also bridge the `log` crate, and
        // Cranelift logs every compiled function at TRACE through it.
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry()
                .with(EnvFilter::new("trace"))
                .with(SlogJsonLayer::new(writer)),
        )
        .expect("the only global subscriber in this binary");
        lines
    })
}

fn lines() -> Vec<Value> {
    String::from_utf8_lossy(&captured().lock())
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
