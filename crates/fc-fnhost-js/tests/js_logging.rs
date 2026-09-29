//! A JS function's log lines on the function's own logger, inside the
//! invocation's span (the JS counterpart of `wasm_logging.rs`), and a
//! secret never on any line. Its own test binary: functions run on the
//! JS workers' threads, so the capture is the process-wide subscriber.

mod support;

use std::sync::{Arc, OnceLock};

use fc_fnhost_core::logging::SlogJsonLayer;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::io;
use std::io::Write;
use support::{bundle, entry, manifest, JsHarness, ADDR};
use tracing::subscriber;
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
        subscriber::set_global_default(
            tracing_subscriber::registry()
                .with(EnvFilter::new("trace"))
                .with(SlogJsonLayer::new(writer)),
        )
        .expect("the only global subscriber in this binary");
        lines
    })
}

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn lines() -> Vec<Value> {
    String::from_utf8_lossy(&captured().lock())
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn console_and_log_lines_land_on_the_functions_logger_with_the_invocations_fields() {
    captured();
    let h = JsHarness::start(vec![entry(
        ADDR,
        1,
        &bundle("guest.mjs"),
        manifest(json!({"secrets": ["API_KEY"]})),
        json!({"secrets": {"API_KEY": "top-secret-value"}}),
    )])
    .await;
    let reply = h
        .send(
            h.client
                .get(format!("{}/functions/{ADDR}/log", h.base))
                .header("X-Correlation-Id", "corr-js-1"),
        )
        .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    h.get("/secret?key=API_KEY").await;
    let rendered: Vec<String> = lines()
        .into_iter()
        .filter(|l| l["logger"] == format!("fn.{ADDR}") && l["correlation_id"] == "corr-js-1")
        .map(|l| {
            format!(
                "{} {}",
                l["level"].as_str().unwrap(),
                l["msg"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        rendered,
        [
            "INFO guest loaded",
            "INFO line one",
            "INFO line two",
            "ERROR an error line",
            "WARN a warn line",
            "INFO an info line",
        ],
        "one line per \\n, at the level asked for, in order; the top-level line runs in \
         every request's isolate"
    );
    let all = String::from_utf8_lossy(&captured().lock()).into_owned();
    assert!(
        !all.contains("top-secret-value"),
        "a secret is never logged"
    );
    let invocation = lines()
        .into_iter()
        .find(|l| l["msg"] == "line one")
        .unwrap();
    assert_eq!(invocation["function"], ADDR);
    assert_eq!(invocation["version"], 1);
    assert!(invocation["execution_id"].as_str().is_some());
    h.close().await;
}
