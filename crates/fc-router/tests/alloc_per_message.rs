//! Heap allocations per message on the router's own path.
//!
//! A counting `#[global_allocator]` (std-only, this test binary only) wraps
//! `System`. Messages are built up front, then routed through the real
//! `QueueManager::route_batch` into a real pool whose mediator succeeds
//! instantly, and settled through an acking in-memory consumer. Everything
//! from the first `route_batch` call to the last ack is counted, so the
//! figure covers dedup, tracking, callback creation, pool submit, worker
//! pickup, circuit-breaker lookup, settlement and untracking.
//!
//! NOT covered: the real `HttpMediator` (the mediator here is a fake), the
//! SQS consumer and delete sender (`fc-queue`), and message construction.
//! Counts include background tokio worker noise, which is small beside the
//! per-message figure at this N.
//!
//! ```text
//! cargo test -p fc-router --release --test alloc_per_message -- --nocapture
//! ```
#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap, expect or panic is a failed test (clippy's test exemption covers #[test] fns and #[cfg(test)] modules, not the helpers of an integration-test crate)"
)]

use async_trait::async_trait;
use std::alloc::{GlobalAlloc, Layout, System};
use std::future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::task;

use fc_common::{
    DispatchMode, MediationOutcome, MediationType, Message, PoolConfig, QueuedMessage, RouterConfig,
};
use fc_queue::QueueConsumer;
use fc_router::{Mediator, QueueManager};

struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: forwards every call unchanged to `System`; only counts.
#[expect(
    unsafe_code,
    reason = "a counting GlobalAlloc must be an unsafe impl; every method forwards unchanged to System"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new as u64, Ordering::Relaxed);
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static A: Counting = Counting;

struct Instant200;

#[async_trait]
impl Mediator for Instant200 {
    async fn mediate(&self, _: &Message) -> MediationOutcome {
        MediationOutcome::success(200)
    }
}

struct Acker {
    acked: AtomicU64,
    total: u64,
    done: Notify,
}

#[async_trait]
impl QueueConsumer for Acker {
    fn identifier(&self) -> &str {
        "alloc-q"
    }
    async fn poll(&self, _: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
        future::pending().await
    }
    async fn ack(&self, _: &str) -> fc_queue::Result<()> {
        if self.acked.fetch_add(1, Ordering::Relaxed) + 1 == self.total {
            self.done.notify_one();
        }
        Ok(())
    }
    async fn nack(&self, _: &str, _: Option<u32>) -> fc_queue::Result<()> {
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

fn message(i: u64) -> QueuedMessage {
    let grouped = i.is_multiple_of(2);
    QueuedMessage {
        message: Message {
            id: format!("msg-{i:08}"),
            pool_code: "ALLOC".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://example.invalid/hook".to_string(),
            message_group_id: grouped.then(|| format!("g{}", i % 64)),
            high_priority: false,
            dispatch_mode: if grouped {
                DispatchMode::NextOnError
            } else {
                DispatchMode::Immediate
            },
            dispatch_mode_specified: true,
        },
        receipt_handle: format!("rh-{i}"),
        broker_message_id: Some(format!("b-{i}")),
        queue_identifier: "alloc-q".to_string(),
    }
}

async fn run(total: u64, start: u64) -> (u64, u64) {
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(Arc::new(
        Instant200,
    )));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "ALLOC".to_string(),
                concurrency: 64,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let consumer = Arc::new(Acker {
        acked: AtomicU64::new(0),
        total,
        done: Notify::new(),
    });
    // Batches of 10, kept inside the pool's capacity by waiting between
    // groups of 100 messages.
    let batches: Vec<Vec<QueuedMessage>> = (start..start + total)
        .collect::<Vec<_>>()
        .chunks(10)
        .map(|c| c.iter().map(|i| message(*i)).collect())
        .collect();
    let a0 = ALLOCS.load(Ordering::Relaxed);
    let b0 = BYTES.load(Ordering::Relaxed);
    let mut sent = 0u64;
    for batch in batches {
        sent += batch.len() as u64;
        manager
            .route_batch(batch, consumer.clone() as Arc<dyn QueueConsumer>)
            .await
            .unwrap();
        while sent - consumer.acked.load(Ordering::Relaxed) > 500 {
            task::yield_now().await;
        }
    }
    let done = consumer.done.notified();
    if consumer.acked.load(Ordering::Relaxed) < total {
        done.await;
    }
    (
        ALLOCS.load(Ordering::Relaxed) - a0,
        BYTES.load(Ordering::Relaxed) - b0,
    )
}

/// Allocation ceilings per message, a little above the measured figure
/// (24 allocs, ~5.5 KB; before the reductions: 46 allocs, ~6.5 KB).
const MAX_ALLOCS_PER_MSG: u64 = 30;
const MAX_BYTES_PER_MSG: u64 = 6_000;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn allocations_per_message() {
    const N: u64 = 2_000;
    // Warm up: pool, breaker and metric registrations are one-off.
    run(200, 1_000_000).await;
    let (allocs, bytes) = run(N, 0).await;
    let (a, b) = (allocs / N, bytes / N);
    println!("alloc_per_message messages={N} allocs/msg={a} bytes/msg={b}");
    assert!(
        a <= MAX_ALLOCS_PER_MSG,
        "allocs/msg {a} > {MAX_ALLOCS_PER_MSG}"
    );
    assert!(
        b <= MAX_BYTES_PER_MSG,
        "bytes/msg {b} > {MAX_BYTES_PER_MSG}"
    );
}
