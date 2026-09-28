//! Log correlation (docs/operations/diagnosing-stuck-processes.md): every
//! line logged while a message is in a worker carries its id, pool, group
//! and queue, from the `router.dispatch` span.
//!
//! Its own test binary: it installs a thread-local subscriber, and
//! tracing's callsite interest cache is process-wide, so a test running
//! beside it on another thread could register the dispatch span's callsite
//! first and leave it disabled here.

use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fc_common::{
    DispatchMode, MediationOutcome, MediationType, Message, PoolConfig, QueuedMessage, RouterConfig,
};
use fc_queue::QueueConsumer;
use fc_router::{Mediator, QueueManager};

// ── Doubles ────────────────────────────────────────────────────────────────

/// Answers by message id: ids starting `fail` get a 503 (ErrorProcess),
/// `hang` never answers, `log` logs a warning before succeeding; everything
/// else succeeds.
struct ByName;

#[async_trait]
impl Mediator for ByName {
    async fn mediate(&self, message: &Message) -> MediationOutcome {
        if message.id.starts_with("hang") {
            std::future::pending::<()>().await;
        }
        if message.id.starts_with("log") {
            tracing::warn!(status = 418, "target answered oddly");
        }
        if message.id.starts_with("fail") {
            return MediationOutcome::error_process(Some(30), "503".to_string());
        }
        MediationOutcome::success(200)
    }
}

#[derive(Default)]
struct Consumer {
    acked: Mutex<Vec<String>>,
    nacked: Mutex<Vec<(String, Option<u32>)>>,
}

#[async_trait]
impl QueueConsumer for Consumer {
    fn identifier(&self) -> &str {
        "q1"
    }
    async fn poll(&self, _: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
        std::future::pending().await
    }
    async fn ack(&self, receipt: &str) -> fc_queue::Result<()> {
        self.acked.lock().unwrap().push(receipt.to_string());
        Ok(())
    }
    async fn nack(&self, receipt: &str, delay: Option<u32>) -> fc_queue::Result<()> {
        self.nacked
            .lock()
            .unwrap()
            .push((receipt.to_string(), delay));
        Ok(())
    }
    async fn extend_visibility(&self, _: &str, _: u32) -> fc_queue::Result<()> {
        Ok(())
    }
    fn is_healthy(&self) -> bool {
        true
    }
    async fn stop(&self) {}
}

fn queued(id: &str, group: Option<&str>) -> QueuedMessage {
    QueuedMessage {
        message: Message {
            id: id.to_string(),
            pool_code: "P1".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://example.invalid/hook".to_string(),
            message_group_id: group.map(str::to_string),
            high_priority: false,
            dispatch_mode: if group.is_some() {
                DispatchMode::NextOnError
            } else {
                DispatchMode::Immediate
            },
            dispatch_mode_specified: true,
        },
        receipt_handle: format!("rh-{id}"),
        broker_message_id: Some(format!("b-{id}")),
        queue_identifier: "q1".to_string(),
    }
}

async fn manager(concurrency: u32) -> Arc<QueueManager> {
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(Arc::new(
        ByName,
    )));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "P1".to_string(),
                concurrency,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    manager
}

async fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..500 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

// ── Span correlation ───────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

/// A line the mediator logs carries the message's id, pool, group and
/// queue from the `router.dispatch` span — the JSON shape fc-server writes.
#[tokio::test]
async fn lines_logged_inside_a_delivery_carry_the_message_id() {
    use tracing_subscriber::layer::SubscriberExt;
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .flatten_event(true)
            .with_writer(capture.clone()),
    );
    // Current-thread runtime: every task runs on this thread, under this
    // subscriber.
    let _default = tracing::subscriber::set_default(subscriber);

    let manager = manager(4).await;
    let consumer = Arc::new(Consumer::default());
    manager
        .route_batch(vec![queued("log-7", Some("g-7"))], consumer.clone())
        .await
        .unwrap();
    eventually("the ack", || !consumer.acked.lock().unwrap().is_empty()).await;

    let out = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    let line = out
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|l| l["message"] == "target answered oddly")
        .unwrap_or_else(|| panic!("the mediator's line: {out}"));
    assert_eq!(line["span"]["name"], "router.dispatch", "{line}");
    assert_eq!(line["span"]["message_id"], "log-7");
    assert_eq!(line["span"]["pool"], "P1");
    assert_eq!(line["span"]["group"], "g-7");
    assert_eq!(line["span"]["queue"], "q1");
}
