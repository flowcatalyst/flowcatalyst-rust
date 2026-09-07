//! NATS JetStream integration tests (G13: fetch semantics).
//!
//! Full-stack tests against a real NATS JetStream instance (via
//! testcontainers) pinning the poll loop's fetch shape:
//!
//! - a `no_wait` pull tries first and returns whatever is already on the
//!   stream immediately — it must never block toward `poll-timeout-ms`
//!   waiting to fill out a full batch when a message is already available;
//! - only when nothing is immediately available does the router fall
//!   through to a genuine *waiting* pull bounded by `poll-timeout-ms`.
//!
//! Both are timing claims, so both are pinned with a timing assertion
//! (`docs/go-mirror/2026-09-06-go-fix-list.md` G12/G13's own convention):
//! elapsed time close to "immediate" vs. elapsed time close to the
//! configured bound.
//!
//! These tests require Docker to be running. They are ignored by default:
//!   cargo test -p fc-queue --features nats --test nats_integration_tests -- --ignored

#![cfg(feature = "nats")]

use std::time::{Duration, Instant};

use testcontainers::{runners::AsyncRunner, ImageExt};
use testcontainers_modules::nats::{Nats, NatsServerCmd};

use fc_common::{DispatchMode, MediationType, Message};
use fc_queue::nats::{NatsConfig, NatsQueueConsumer};
use fc_queue::QueueConsumer;

/// Start a NATS JetStream testcontainer and return its client-facing
/// `servers` URI (`nats://127.0.0.1:<port>`).
async fn start_jetstream() -> (testcontainers::ContainerAsync<Nats>, String) {
    let cmd = NatsServerCmd::default().with_jetstream();
    let container = Nats::default()
        .with_cmd(&cmd)
        .start()
        .await
        .expect("failed to start nats container");
    let port = container
        .get_host_port_ipv4(4222)
        .await
        .expect("failed to get nats port");
    (container, format!("nats://127.0.0.1:{port}"))
}

fn healthy_message(id: &str) -> Message {
    Message {
        id: id.to_string(),
        pool_code: "TEST".to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: "http://localhost:8080".to_string(),
        message_group_id: None,
        high_priority: false,
        dispatch_mode: DispatchMode::default(),
        dispatch_mode_specified: true,
    }
}

/// Publish one message straight onto a JetStream subject, independent of
/// (and before/after, order doesn't matter for a durable WorkQueue
/// consumer) `NatsQueueConsumer` — pins behaviour purely at the consumer's
/// `poll()`, not through any publisher path.
async fn publish_raw(servers: &str, subject: &str, message: &Message) {
    let client = async_nats::connect(servers)
        .await
        .expect("failed to connect to publish");
    let js = async_nats::jetstream::new(client);
    js.publish(subject.to_string(), serde_json::to_vec(message).unwrap().into())
        .await
        .expect("publish request failed")
        .await
        .expect("publish ack failed");
}

/// G13: a message already sitting on the stream must come back on the
/// no-wait phase almost immediately — never held back to fill a batch of
/// 10 when only 1 is available.
///
/// Pins: elapsed time close to zero, far under `poll_timeout_ms` (set
/// deliberately high at 5s). Mutant check: reverting to a single
/// `.batch().max_messages(10).expires(5s).messages()` call (no no-wait
/// phase first) makes this fail at ~5s actual vs. the 1.5s bound, because
/// a `Batch` waits for either the full requested count or the expiry
/// before yielding a batch smaller than requested.
#[tokio::test]
#[ignore]
async fn g13_no_wait_phase_returns_available_message_without_waiting_for_full_batch() {
    let (_container, servers) = start_jetstream().await;
    let stream = format!("G13AVAIL{}", uuid::Uuid::new_v4().simple());
    let subject_filter = format!("{stream}.>");

    let config = NatsConfig {
        servers: servers.clone(),
        stream_name: stream.clone(),
        consumer_name: "router".to_string(),
        subject: subject_filter.clone(),
        max_messages_per_poll: 10,
        poll_timeout_ms: 5000,
        ..NatsConfig::default()
    };
    let consumer = NatsQueueConsumer::new(config)
        .await
        .expect("failed to build consumer");

    publish_raw(&servers, &format!("{stream}.test"), &healthy_message("g13-m1")).await;

    let start = Instant::now();
    let messages = consumer.poll(10).await.expect("poll failed");
    let elapsed = start.elapsed();

    assert_eq!(
        messages.len(),
        1,
        "should have picked up the one already-published message"
    );
    assert!(
        elapsed < Duration::from_millis(1500),
        "poll took {:?} — the no-wait phase should have returned the \
         already-available message almost immediately instead of waiting \
         toward the 5s poll-timeout to fill a batch of 10",
        elapsed
    );
}

/// G13: when nothing is available, `poll()` must fall through to a genuine
/// *waiting* pull bounded by `poll-timeout-ms` — not return instantly (that
/// would mean it's still only ever no-waiting), and not hang indefinitely.
///
/// Pins: elapsed time close to the configured bound (400ms), both from
/// below (rules out an instant-return no-wait-only implementation) and
/// from above (rules out an unbounded/hanging wait).
#[tokio::test]
#[ignore]
async fn g13_waiting_phase_is_bounded_by_poll_timeout_when_nothing_available() {
    let (_container, servers) = start_jetstream().await;
    let stream = format!("G13EMPTY{}", uuid::Uuid::new_v4().simple());
    let subject_filter = format!("{stream}.>");

    let config = NatsConfig {
        servers,
        stream_name: stream,
        consumer_name: "router".to_string(),
        subject: subject_filter,
        max_messages_per_poll: 10,
        poll_timeout_ms: 400,
        ..NatsConfig::default()
    };
    let consumer = NatsQueueConsumer::new(config)
        .await
        .expect("failed to build consumer");

    let start = Instant::now();
    let messages = consumer.poll(10).await.expect("poll failed");
    let elapsed = start.elapsed();

    assert!(messages.is_empty(), "queue was never published to");
    assert!(
        elapsed >= Duration::from_millis(300),
        "empty poll returned in {:?} — should have genuinely waited close \
         to poll-timeout (400ms) via the waiting-pull phase, not returned \
         instantly (that would mean poll() is still no-wait-only)",
        elapsed
    );
    assert!(
        elapsed < Duration::from_millis(3000),
        "empty poll took {:?} — should be bounded by poll-timeout (400ms) \
         plus slack, not hang",
        elapsed
    );
}
