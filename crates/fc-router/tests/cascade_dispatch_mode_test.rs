//! Dispatch-mode cascade behaviour (ledger A-01 router half).
//!
//! Mirrors the Go pool_release_test.go pair (as of commit `8804827`, the
//! last version before Go's later A-01 recovery-path work moved
//! BLOCK_ON_ERROR's successors to an ACK — see this crate's
//! `pool::disposition_of` module doc for why this port intentionally does
//! NOT follow that later Go commit: the platform-side settled/reaper half
//! A-01 requires before shipping the ACK branch doesn't exist yet here).
//!
//! What's pinned:
//! - NEXT_ON_ERROR: a terminally failed head (a permanent, non-retryable
//!   rejection) is ACKed away and the group CONTINUES — its successors are
//!   still attempted, in order.
//! - BLOCK_ON_ERROR: the same terminally failed head is ACKed away, but
//!   its successors are cascaded — NACKed back to the broker as they're
//!   dequeued, never attempted. This is the ONE place the two ordered
//!   modes diverge.
//! - A retryable head failure (target unreachable/unavailable) releases
//!   the WHOLE group — head plus every buffered successor — back to the
//!   broker, under BOTH ordered modes. Successors are never attempted.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

use fc_common::{
    AckNack, BatchMessage, DispatchMode, MediationOutcome, Message, MessageCallback, PoolConfig,
};
use fc_router::{Mediator, ProcessPool};

/// Test callback that records ack/nack via a oneshot channel. Same shape
/// as `pool_tests.rs`'s `TestCallback` — duplicated locally per this
/// crate's per-file test-mock convention (see `fifo_tests.rs`,
/// `pool_tests.rs`).
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

impl Drop for TestCallback {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.lock().take() {
            let _ = tx.send(AckNack::Nack {
                delay_seconds: Some(0),
            });
        }
    }
}

/// Mediator that fails ONE named message id with a fixed outcome and
/// succeeds (200) for every other message it sees. Mirrors Go's
/// `cascadeMediator{failID, failWith}`.
struct CascadeMediator {
    fail_id: String,
    fail_with: MediationOutcome,
    seen: parking_lot::Mutex<Vec<String>>,
}

impl CascadeMediator {
    fn new(fail_id: &str, fail_with: MediationOutcome) -> Self {
        Self {
            fail_id: fail_id.to_string(),
            fail_with,
            seen: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().clone()
    }
}

#[async_trait]
impl Mediator for CascadeMediator {
    async fn mediate(&self, message: &Message) -> MediationOutcome {
        self.seen.lock().push(message.id.clone());
        if message.id == self.fail_id {
            self.fail_with.clone()
        } else {
            MediationOutcome::success(200)
        }
    }
}

fn cascade_message(id: &str, group: &str, mode: DispatchMode) -> Message {
    Message {
        id: id.to_string(),
        pool_code: "TEST".to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: fc_common::MediationType::HTTP,
        mediation_target: "http://example.invalid/webhook".to_string(),
        message_group_id: Some(group.to_string()),
        high_priority: false,
        dispatch_mode: mode,
        dispatch_mode_specified: true,
    }
}

fn batch(id: &str, group: &str, mode: DispatchMode) -> (BatchMessage, oneshot::Receiver<AckNack>) {
    let (tx, rx) = oneshot::channel();
    let msg = BatchMessage {
        message: cascade_message(id, group, mode),
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

/// NEXT_ON_ERROR: a terminally failed head is ACKed away and the group
/// moves on — successors are still attempted, in order, and (here) succeed.
#[tokio::test]
async fn next_on_error_discards_terminal_head_and_continues() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(CascadeMediator::new(
        "m1",
        MediationOutcome::error_config(500, "rejected".to_string()),
    ));
    let pool = Arc::new(ProcessPool::new(config, mediator.clone()));
    pool.start().await;

    let (b1, r1) = batch("m1", "g", DispatchMode::NextOnError);
    let (b2, r2) = batch("m2", "g", DispatchMode::NextOnError);
    let (b3, r3) = batch("m3", "g", DispatchMode::NextOnError);
    pool.submit(b1).await.unwrap();
    pool.submit(b2).await.unwrap();
    pool.submit(b3).await.unwrap();

    let (a1, a2, a3) = (recv(r1).await, recv(r2).await, recv(r3).await);

    assert!(matches!(a1, AckNack::Ack), "the failed head is ACKed away");
    assert!(
        matches!(a2, AckNack::Ack),
        "NEXT_ON_ERROR moves on: successors still deliver"
    );
    assert!(matches!(a3, AckNack::Ack));
    assert_eq!(
        mediator.seen(),
        vec!["m1".to_string(), "m2".to_string(), "m3".to_string()],
        "every message is attempted, in order"
    );
}

/// BLOCK_ON_ERROR: the same terminally failed head is ACKed away, but its
/// successors are cascaded — NACKed as they're dequeued, never attempted.
/// This is the one place the two ordered modes diverge.
#[tokio::test]
async fn block_on_error_acks_terminal_head_and_cascades_nack_to_successors() {
    let config = PoolConfig {
        code: "TEST".to_string(),
        concurrency: 5,
        rate_limit_per_minute: None,
    };
    let mediator = Arc::new(CascadeMediator::new(
        "m1",
        MediationOutcome::error_config(500, "rejected".to_string()),
    ));
    let pool = Arc::new(ProcessPool::new(config, mediator.clone()));
    pool.start().await;

    let (b1, r1) = batch("m1", "g", DispatchMode::BlockOnError);
    let (b2, r2) = batch("m2", "g", DispatchMode::BlockOnError);
    let (b3, r3) = batch("m3", "g", DispatchMode::BlockOnError);
    pool.submit(b1).await.unwrap();
    pool.submit(b2).await.unwrap();
    pool.submit(b3).await.unwrap();

    let (a1, a2, a3) = (recv(r1).await, recv(r2).await, recv(r3).await);

    assert!(matches!(a1, AckNack::Ack), "the failed head is still ACKed away");
    assert!(
        matches!(a2, AckNack::Nack { .. }),
        "BLOCK_ON_ERROR cascades: successors are NACKed, not delivered past the failure"
    );
    assert!(matches!(a3, AckNack::Nack { .. }));
    assert_eq!(
        mediator.seen(),
        vec!["m1".to_string()],
        "successors must NOT be attempted at all under BLOCK_ON_ERROR"
    );
}

/// A retryable head failure (target unreachable) releases the WHOLE group
/// — head plus every buffered successor — back to the broker, under BOTH
/// ordered modes. Successors are never attempted: releasing only the head
/// while successors stayed buffered would put the head behind them on
/// redelivery, reordering a group whose entire purpose is ordering.
#[tokio::test]
async fn retryable_head_failure_releases_whole_group_under_both_modes() {
    for mode in [DispatchMode::NextOnError, DispatchMode::BlockOnError] {
        let config = PoolConfig {
            code: "TEST".to_string(),
            concurrency: 5,
            rate_limit_per_minute: None,
        };
        let mediator = Arc::new(CascadeMediator::new(
            "m1",
            MediationOutcome::error_connection("unreachable".to_string()),
        ));
        let pool = Arc::new(ProcessPool::new(config, mediator.clone()));
        pool.start().await;

        let (b1, r1) = batch("m1", "g", mode);
        let (b2, r2) = batch("m2", "g", mode);
        let (b3, r3) = batch("m3", "g", mode);
        pool.submit(b1).await.unwrap();
        pool.submit(b2).await.unwrap();
        pool.submit(b3).await.unwrap();

        let (a1, a2, a3) = (recv(r1).await, recv(r2).await, recv(r3).await);

        assert!(
            matches!(a1, AckNack::Nack { .. }),
            "mode {mode:?}: the head returns to the broker, not ACKed"
        );
        assert!(matches!(a2, AckNack::Nack { .. }), "mode {mode:?}");
        assert!(matches!(a3, AckNack::Nack { .. }), "mode {mode:?}");
        assert_eq!(
            mediator.seen(),
            vec!["m1".to_string()],
            "mode {mode:?}: successors must not be attempted against a target already known unreachable"
        );
    }
}
