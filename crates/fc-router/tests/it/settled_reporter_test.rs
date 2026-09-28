//! Ledger A-01 (Go `Pool.ackBuffered` + `reportSettled`): with a settled
//! reporter wired, the siblings buffered behind a terminally failed
//! BLOCK_ON_ERROR head are ACKed — never delivered past the failure, never
//! handed back to be redelivered as a new head — and the dispatch jobs among
//! them (the ones carrying a scheduler-signed token) are reported to the
//! platform. Without a reporter the cascade NACKs them
//! (`cascade_dispatch_mode_test.rs`).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::oneshot;

use fc_common::{
    AckNack, BatchMessage, DispatchMode, MediationOutcome, Message, MessageCallback, PoolConfig,
};
use fc_router::{Mediator, ProcessPool, SettledJob, SettledReport, SettledReporter};

struct TestCallback {
    tx: parking_lot::Mutex<Option<oneshot::Sender<AckNack>>>,
}

#[async_trait]
impl MessageCallback for TestCallback {
    async fn ack(&self) {
        if let Some(tx) = self.tx.lock().take() {
            let _ = tx.send(AckNack::Ack);
        }
    }
    async fn nack(&self, delay_seconds: Option<u32>) {
        if let Some(tx) = self.tx.lock().take() {
            let _ = tx.send(AckNack::Nack { delay_seconds });
        }
    }
}

/// Fails `m1` permanently, succeeds everything else, records what it saw.
struct FailFirst {
    seen: parking_lot::Mutex<Vec<String>>,
}

#[async_trait]
impl Mediator for FailFirst {
    async fn mediate(&self, message: &Message) -> MediationOutcome {
        self.seen.lock().push(message.id.clone());
        if message.id == "m1" {
            MediationOutcome::error_config(400, "rejected".to_string())
        } else {
            MediationOutcome::success(200)
        }
    }
}

#[derive(Default)]
struct RecordingReporter {
    reports: parking_lot::Mutex<Vec<SettledReport>>,
}

#[async_trait]
impl SettledReporter for RecordingReporter {
    async fn report_settled(&self, report: &SettledReport) -> Result<(), String> {
        self.reports.lock().push(report.clone());
        Ok(())
    }
}

fn batch(id: &str, token: Option<&str>) -> (BatchMessage, oneshot::Receiver<AckNack>) {
    let (tx, rx) = oneshot::channel();
    let msg = BatchMessage {
        message: Message {
            id: id.to_string(),
            pool_code: "TEST".to_string(),
            auth_token: token.map(str::to_string),
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: "http://example.invalid/api/dispatch/process".to_string(),
            message_group_id: Some("g".to_string()),
            high_priority: false,
            dispatch_mode: DispatchMode::BlockOnError,
            dispatch_mode_specified: true,
        },
        receipt_handle: format!("receipt-{id}"),
        broker_message_id: Some(format!("broker-{id}")),
        queue_identifier: "test-queue".to_string(),
        batch_id: Some(Arc::from("batch-1")),
        callback: Box::new(TestCallback {
            tx: parking_lot::Mutex::new(Some(tx)),
        }),
    };
    (msg, rx)
}

async fn recv(rx: oneshot::Receiver<AckNack>) -> AckNack {
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("message settled within timeout")
        .expect("callback resolved")
}

#[tokio::test]
async fn block_on_error_acks_and_reports_siblings_when_a_reporter_is_wired() {
    let mediator = Arc::new(FailFirst {
        seen: parking_lot::Mutex::new(Vec::new()),
    });
    let reporter = Arc::new(RecordingReporter::default());
    let pool = Arc::new(
        ProcessPool::new(
            PoolConfig {
                code: "TEST".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            },
            mediator.clone(),
        )
        .with_settled_reporter(Some(reporter.clone())),
    );
    pool.start().await;

    let (b1, r1) = batch("m1", Some("tok-1"));
    let (b2, r2) = batch("m2", Some("tok-2"));
    let (b3, r3) = batch("m3", None);
    pool.submit(b1).await.unwrap();
    pool.submit(b2).await.unwrap();
    pool.submit(b3).await.unwrap();

    let (a1, a2, a3) = (recv(r1).await, recv(r2).await, recv(r3).await);
    assert!(matches!(a1, AckNack::Ack), "the failed head is ACKed away");
    assert!(matches!(a2, AckNack::Ack), "siblings are ACKed, as Go");
    assert!(matches!(a3, AckNack::Ack));
    assert_eq!(
        mediator.seen.lock().clone(),
        vec!["m1".to_string()],
        "nothing is delivered past the failed head"
    );

    // The report is fired on its own task after the ACKs.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while reporter.reports.lock().is_empty() {
        assert!(tokio::time::Instant::now() < deadline, "no settled report");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let reports = reporter.reports.lock().clone();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].pool_code, "TEST");
    assert_eq!(reports[0].group, "g");
    assert_eq!(reports[0].reason, "head failed under BLOCK_ON_ERROR");
    assert_eq!(
        reports[0].jobs,
        vec![SettledJob {
            id: "m2".to_string(),
            token: "tok-2".to_string()
        }],
        "only a message with a scheduler-signed token is a dispatch job to report"
    );
    assert_eq!(
        pool.queue_size(),
        0,
        "every sibling gave its queue slot back"
    );
}
