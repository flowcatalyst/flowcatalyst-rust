//! The panic hook logs through `tracing` with the panicking code's span
//! context and a backtrace. Its own test binary: the hook and the global
//! subscriber are process-wide.

use fc_common::diagnostics;
use std::io;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

#[tokio::test]
async fn a_panic_in_a_task_is_logged_with_its_span_and_backtrace() {
    let capture = Capture::default();
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .flatten_event(true)
                .with_writer(capture.clone()),
        )
        .init();
    diagnostics::init();
    let before = diagnostics::panic_count();

    use tracing::Instrument;
    let task = tokio::spawn(
        async { panic!("mediator exploded") }.instrument(tracing::info_span!(
            "router.dispatch",
            message_id = "msg-42",
            pool = "P1"
        )),
    );
    assert!(task.await.unwrap_err().is_panic());

    assert_eq!(diagnostics::panic_count(), before + 1);
    let out = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    let line = out
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|l| l["target"] == "panic")
        .unwrap_or_else(|| panic!("no panic line in {out}"));
    assert_eq!(line["level"], "ERROR");
    assert_eq!(line["panic_message"], "mediator exploded");
    assert_eq!(line["span"]["message_id"], "msg-42");
    assert_eq!(line["span"]["pool"], "P1");
    assert!(line["panic_location"]
        .as_str()
        .unwrap()
        .contains("panic_hook_test.rs"));
    assert!(line["backtrace"].as_str().is_some_and(|b| !b.is_empty()));
}
