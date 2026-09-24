//! The invocation's log fields (Java's MDC: `function`, `version`,
//! `execution_id`, `correlation_id`) on every line logged inside it. Its own
//! test binary: a thread-local subscriber shares callsite interest with any
//! test running beside it.

mod support;

use std::sync::Arc;

use serde_json::json;
use support::listener::{doc, entry, Harness};

const ADDR: &str = "app.orders.ship";

#[tokio::test]
async fn every_log_line_inside_an_invocation_carries_its_fields() {
    use tracing_subscriber::layer::SubscriberExt;
    let lines = Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
    let writer = {
        let lines = lines.clone();
        move || CaptureWriter(lines.clone())
    };
    let subscriber =
        tracing_subscriber::registry().with(fc_fnhost_core::logging::SlogJsonLayer::new(writer));
    let _default = tracing::subscriber::set_default(subscriber);

    let h = Harness::start(doc(vec![entry(
        ADDR,
        3,
        "live",
        "echo",
        5,
        json!([{"path": "/*", "auth": "none"}]),
    )]))
    .await;
    let resp = h
        .get(
            &format!("/functions/{ADDR}/x"),
            &[("X-Correlation-Id", "corr-9")],
        )
        .await;
    let invocation_id = resp.json()["invocationId"].as_str().unwrap().to_owned();
    let text = String::from_utf8(lines.lock().clone()).unwrap();
    let line: serde_json::Value = text
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|l| l["msg"] == "inside the function")
        .unwrap_or_else(|| panic!("no function line in:\n{text}"));
    assert_eq!(line["function"], ADDR);
    assert_eq!(line["version"], 3);
    assert_eq!(line["execution_id"], invocation_id);
    assert_eq!(line["correlation_id"], "corr-9");
}

struct CaptureWriter(Arc<parking_lot::Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
