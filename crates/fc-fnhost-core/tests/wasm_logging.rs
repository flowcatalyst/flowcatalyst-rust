//! A guest's log lines on the function's own logger, and a secret never on
//! any line (Java `WasmFunctionListenerTest`'s log and secret tests). Its own
//! test binary: guests run on the guest runtime's threads, so the capture is
//! the process-wide subscriber, at TRACE.

mod support;

use std::sync::{Arc, OnceLock};

use fc_fnhost_core::logging::SlogJsonLayer;
use parking_lot::Mutex;
use serde_json::{json, Value};
use support::wasm::{entry, guest, manifest, WasmHarness, ADDR};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn guest_log_lines_and_stdout_and_stderr_land_on_the_functions_own_logger() {
    captured();
    let h = WasmHarness::start(vec![entry(
        ADDR,
        1,
        &guest("log"),
        manifest(json!({})),
        json!({}),
    )])
    .await;
    let resp = h
        .get_headers(
            "/x?msg=wave&long=20000",
            &[("X-Correlation-Id", "corr-wasm-1")],
        )
        .await;
    assert_eq!(resp.status, 200, "{}", resp.text());
    let invocation_lines: Vec<Value> = lines()
        .into_iter()
        .filter(|l| l["logger"] == format!("fn.{ADDR}") && l["correlation_id"] == "corr-wasm-1")
        .collect();
    let rendered: Vec<String> = invocation_lines
        .iter()
        .map(|l| {
            format!(
                "{} {}",
                l["level"].as_str().unwrap(),
                l["msg"]
                    .as_str()
                    .unwrap()
                    .chars()
                    .take(40)
                    .collect::<String>()
            )
        })
        .collect();
    for want in [
        "INFO guest info: wave",
        "WARN guest warn: wave",
        "INFO guest stdout: wave",
        "WARN guest stderr: wave",
    ] {
        assert!(
            rendered.iter().any(|r| r == want),
            "{want} missing from {rendered:#?}"
        );
    }
    let info = invocation_lines
        .iter()
        .find(|l| l["msg"] == "guest info: wave")
        .unwrap();
    assert_eq!(info["function"], ADDR, "the invocation's fields ride along");
    assert_eq!(info["version"], 1);
    assert!(info["execution_id"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(
        info.get("fc_logger").is_none(),
        "the logger field is not written twice"
    );

    // 20000 bytes of stdout with no newline: 8 KiB pieces, the rest flushed
    // when the call ends.
    let pieces: Vec<usize> = invocation_lines
        .iter()
        .filter(|l| l["level"] == "INFO" && l["msg"].as_str().unwrap().starts_with("xxxx"))
        .map(|l| l["msg"].as_str().unwrap().len())
        .collect();
    assert_eq!(pieces, [8192, 8192, 20000 - 2 * 8192]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_secret_value_never_appears_on_a_log_line() {
    captured();
    let secret = "s3cr3t-value-7f1c-never-logged";
    let h = WasmHarness::start(vec![entry(
        "app.orders.secret",
        1,
        &guest("secret"),
        manifest(json!({"secrets": ["api_token"]})),
        json!({"secrets": {"api_token": secret, "not_declared": "zzz-undeclared"}}),
    )])
    .await;
    let reply = h
        .send(h.client.get(format!(
            "{}/functions/app.orders.secret/x?key=api_token",
            h.base
        )))
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["value"], secret);
    let text = String::from_utf8_lossy(&captured().lock()).into_owned();
    assert!(!text.is_empty(), "the capture saw the host at work");
    assert!(
        !text.contains(secret),
        "no line, field or error may carry the secret value"
    );
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
