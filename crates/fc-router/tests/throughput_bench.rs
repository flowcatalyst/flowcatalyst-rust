//! Router throughput, in process: how many messages a second the manager
//! routes, dispatches and settles when the target costs nothing, so the
//! router's own work (tracking, pools, spans, flight recorder, logging) is
//! what is measured. Used to price the observability added in
//! `docs/operations/diagnosing-stuck-processes.md`.
//!
//! ```text
//! cargo test -p fc-router --release --test throughput_bench -- --ignored --nocapture
//! ```
//!
//! Each run routes `FC_BENCH_MESSAGES` (default 200 000) messages in
//! batches of 10 — half in 64 ordered groups, half IMMEDIATE — and reports
//! the rate, under two logging setups: none, and the production JSON layer
//! at INFO writing to a sink (spans enabled and formatted, as fc-server
//! runs). `FC_BENCH_RECORDER=0` turns the flight recorder off.

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use fc_common::{
    DispatchMode, MediationOutcome, MediationType, Message, PoolConfig, QueuedMessage, RouterConfig,
};
use fc_queue::QueueConsumer;
use fc_router::flight_recorder::FlightRecorder;
use fc_router::{Mediator, QueueManager};
use std::env;
use std::future;
use std::io;
use std::mem;
use tokio::sync::Notify;
use tokio::sync::Semaphore;
use tokio::task;
use tracing::subscriber;
use tracing_subscriber::fmt;

struct Instant200;

#[async_trait]
impl Mediator for Instant200 {
    async fn mediate(&self, _: &Message) -> MediationOutcome {
        task::yield_now().await;
        MediationOutcome::success(200)
    }
}

struct Counting {
    acked: AtomicU64,
    /// Admission window: the router takes at most 1000 unsettled messages
    /// (under the pool's 64 × 20 capacity), without spinning.
    window: Semaphore,
    done: Notify,
    total: u64,
}

#[async_trait]
impl QueueConsumer for Counting {
    fn identifier(&self) -> &str {
        "bench-q"
    }
    async fn poll(&self, _: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
        future::pending().await
    }
    async fn ack(&self, _: &str) -> fc_queue::Result<()> {
        if self.acked.fetch_add(1, Ordering::Relaxed) + 1 == self.total {
            self.done.notify_one();
        }
        self.window.add_permits(1);
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
            pool_code: "BENCH".to_string(),
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
        queue_identifier: "bench-q".to_string(),
    }
}

/// Process CPU time (user + system) so far, in seconds.
fn cpu_seconds() -> f64 {
    // SAFETY: getrusage only writes the zeroed struct it is handed.
    unsafe {
        let mut usage: libc::rusage = mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        let secs = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
        secs(usage.ru_utime) + secs(usage.ru_stime)
    }
}

/// One run's figures: messages a second, end to end (routed and acked), and
/// process CPU microseconds per message (steadier than the rate on a busy
/// machine).
#[derive(Clone, Copy)]
struct Figures {
    rate: f64,
    cpu_us: f64,
}

async fn run(total: u64) -> Figures {
    let cpu_before = cpu_seconds();
    let manager = Arc::new(new_manager());
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "BENCH".to_string(),
                concurrency: 64,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let consumer = Arc::new(Counting {
        acked: AtomicU64::new(0),
        window: Semaphore::new(1000),
        done: Notify::new(),
        total,
    });
    let started = Instant::now();
    let mut next = 0u64;
    while next < total {
        let batch: Vec<_> = (next..(next + 10).min(total)).map(message).collect();
        consumer
            .window
            .acquire_many(batch.len() as u32)
            .await
            .unwrap()
            .forget();
        next += batch.len() as u64;
        manager
            .route_batch(batch, consumer.clone() as Arc<dyn QueueConsumer>)
            .await
            .unwrap();
    }
    let done = consumer.done.notified();
    if consumer.acked.load(Ordering::Relaxed) < total {
        done.await;
    }
    Figures {
        rate: total as f64 / started.elapsed().as_secs_f64(),
        cpu_us: (cpu_seconds() - cpu_before) * 1e6 / total as f64,
    }
}

// BASELINE-START (main has no flight recorder: use
// `QueueManager::with_shared_mediator_for_testing(Arc::new(Instant200))`)
fn new_manager() -> QueueManager {
    let recorder = match env::var("FC_BENCH_RECORDER").as_deref() {
        Ok("0") => FlightRecorder::new(0),
        _ => FlightRecorder::default(),
    };
    QueueManager::builder_with_shared_mediator(Arc::new(Instant200))
        .flight_recorder(Arc::new(recorder))
        .build()
}
// BASELINE-END

fn total() -> u64 {
    env::var("FC_BENCH_MESSAGES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200_000)
}

/// Median rate and median CPU per message of `FC_BENCH_RUNS` (default 5)
/// runs.
async fn median(total: u64) -> Figures {
    let runs: usize = env::var("FC_BENCH_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let mut all = Vec::with_capacity(runs);
    for _ in 0..runs {
        all.push(run(total).await);
    }
    let mut rates: Vec<f64> = all.iter().map(|f| f.rate).collect();
    let mut cpus: Vec<f64> = all.iter().map(|f| f.cpu_us).collect();
    rates.sort_by(|a, b| a.partial_cmp(b).unwrap());
    cpus.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Figures {
        rate: rates[runs / 2],
        cpu_us: cpus[runs / 2],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "benchmark: run with --release --ignored --nocapture"]
async fn router_throughput() {
    let total = total();
    // Warm up allocators and the runtime.
    run(total / 10).await;

    let bare = median(total).await;

    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(
            fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_file(true)
                .with_line_number(true)
                .with_target(true)
                .flatten_event(true)
                .with_writer(io::sink),
        );
    subscriber::set_global_default(subscriber).unwrap();
    let logged = median(total).await;

    println!(
        "router_throughput messages={total} no_subscriber={:.0}/s cpu={:.2}us/msg json_info_subscriber={:.0}/s cpu={:.2}us/msg",
        bare.rate, bare.cpu_us, logged.rate, logged.cpu_us
    );
}
