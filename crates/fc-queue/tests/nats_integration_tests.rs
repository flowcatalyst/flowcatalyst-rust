//! NATS JetStream integration tests.
//!
//! Full-stack tests against a real NATS JetStream instance (via
//! testcontainers).
//!
//! **Item 2 (owner ruling 2026-09-07, applied to Java and Go already)**
//! replaced the old no-wait/waiting `fetch()` pair (G13,
//! `docs/go-mirror/2026-09-06-go-fix-list.md`) with one continuous pull
//! subscription per queue feeding a bounded client-side channel. That
//! retired the old "waiting phase is bounded by poll-timeout" test below —
//! `poll_timeout_ms` is now unused (see `NatsConfig::poll_timeout_ms`'s doc
//! comment) and an empty `poll()` blocks **untimed** until either a
//! message arrives or `stop()` is called, not until a fixed timeout — and
//! replaced it with the four behaviours item 2's brief calls out:
//!
//! - `item2_available_messages_return_without_waiting`: messages already
//!   on the stream come back immediately, in publish order, up to the
//!   requested max (was the G13 "no-wait" test — still true under the
//!   continuous subscription, now pinning order too).
//! - `item2_blocks_on_empty_then_returns_promptly_once_published`: an
//!   empty `poll()` genuinely blocks (not a short fixed wait) and resolves
//!   promptly once a message is published.
//! - `item2_stop_unblocks_a_parked_poll_within_500ms`: `stop()` must
//!   unblock a `poll()` parked on an empty queue quickly, not leave it
//!   hanging.
//! - `item2_bounded_channel_caps_ack_pending_before_any_poll`: the
//!   back-pressure bound — with zero `poll()` calls, the standing
//!   subscription must not pull far more than one channel's worth of
//!   messages off the broker.
//!
//! These tests require Docker to be running. They are ignored by default:
//!   cargo test -p fc-queue --features nats --test nats_integration_tests -- --ignored

#![cfg(feature = "nats")]

use std::time::{Duration, Instant};

use testcontainers::{runners::AsyncRunner, ImageExt};
use testcontainers_modules::nats::{Nats, NatsServerCmd};

use fc_common::{DispatchMode, MediationType, Message};
use fc_queue::nats::{NatsConfig, NatsQueueConsumer};
use fc_queue::{QueueConsumer, QueueError};

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
    js.publish(
        subject.to_string(),
        serde_json::to_vec(message).unwrap().into(),
    )
    .await
    .expect("publish request failed")
    .await
    .expect("publish ack failed");
}

/// Messages already sitting on the stream when the consumer's standing
/// subscription opens must come back on the very next `poll()` almost
/// immediately (never held back "waiting for a full batch"), and in the
/// order they were published (JetStream WorkQueue delivery order, single
/// consumer, single subject filter — no group partitioning at the broker
/// level).
///
/// Pins: (1) elapsed time close to zero, far under a would-be poll-timeout
/// (proves no artificial wait); (2) message id order matches publish order.
/// Mutant check: swapping `messages.push(first)` + the `try_recv` drain
/// loop for something that returns only the first message (never drains
/// the rest immediately available) — confirmed by hand while implementing
/// item 2 — makes the length assertion fail (1 instead of 3).
#[tokio::test]
#[ignore]
async fn item2_available_messages_return_without_waiting() {
    let (_container, servers) = start_jetstream().await;
    let stream = format!("ITEM2AVAIL{}", uuid::Uuid::new_v4().simple());
    let subject_filter = format!("{stream}.>");

    let config = NatsConfig {
        servers: servers.clone(),
        stream_name: stream.clone(),
        consumer_name: "router".to_string(),
        subject: subject_filter.clone(),
        max_messages_per_poll: 10,
        ..NatsConfig::default()
    };
    let consumer = NatsQueueConsumer::new(config)
        .await
        .expect("failed to build consumer");

    // Give the standing subscription a moment to open before publishing —
    // it's opened synchronously inside `new()` (awaited), so this is just
    // slack for the background task's first `stream.next()` to be parked.
    tokio::time::sleep(Duration::from_millis(200)).await;

    for i in 0..3 {
        publish_raw(
            &servers,
            &format!("{stream}.m{i}"),
            &healthy_message(&format!("item2-{i}")),
        )
        .await;
    }

    // Settle window: `publish_raw`'s ack only proves the broker has the
    // message, not that the background subscription task has already
    // pulled it off the wire and pushed it into the channel — that's an
    // async hop this test isn't trying to race. The timing claim under
    // test is entirely in `poll()` itself (measured below), not in how
    // fast the standing subscription drains a fresh publish.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let start = Instant::now();
    let messages = consumer.poll(10).await.expect("poll failed");
    let elapsed = start.elapsed();

    assert_eq!(
        messages.len(),
        3,
        "should have picked up all three already-published messages in one poll"
    );
    assert_eq!(
        messages
            .iter()
            .map(|m| m.message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["item2-0", "item2-1", "item2-2"],
        "delivery order must match publish order"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "poll took {:?} — already-available messages must return promptly, \
         not wait for anything",
        elapsed
    );
}

/// An empty queue's `poll()` must genuinely block (item 2: an **untimed**
/// await on the channel, not a fixed poll-timeout) and then resolve
/// promptly once a message actually arrives.
///
/// Pins both directions in one test: (1) still blocked well after 300ms
/// with nothing published (rules out an accidental instant-return or a
/// short fixed timeout masquerading as "untimed"); (2) resolves within
/// 500ms of a message landing (rules out a wait so long it might as well
/// be unbounded/broken).
///
/// Mutant check: swapping `rx.recv().await` for
/// `tokio::time::timeout(Duration::from_millis(50), rx.recv()).await`
/// (i.e. reintroducing a short fixed bound) — confirmed by hand while
/// implementing item 2 — makes assertion (1) fail: the poll task finishes
/// (with an empty/timeout result) well before the 300ms check.
#[tokio::test]
#[ignore]
async fn item2_blocks_on_empty_then_returns_promptly_once_published() {
    let (_container, servers) = start_jetstream().await;
    let stream = format!("ITEM2BLOCK{}", uuid::Uuid::new_v4().simple());
    let subject_filter = format!("{stream}.>");

    let config = NatsConfig {
        servers: servers.clone(),
        stream_name: stream.clone(),
        consumer_name: "router".to_string(),
        subject: subject_filter.clone(),
        max_messages_per_poll: 10,
        ..NatsConfig::default()
    };
    let consumer = std::sync::Arc::new(
        NatsQueueConsumer::new(config)
            .await
            .expect("failed to build consumer"),
    );

    let poll_consumer = consumer.clone();
    let handle = tokio::spawn(async move { poll_consumer.poll(10).await });

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !handle.is_finished(),
        "poll() on an empty queue must still be blocked after 300ms with \
         nothing published — it's an untimed wait, not a short fixed poll"
    );

    publish_raw(
        &servers,
        &format!("{stream}.m0"),
        &healthy_message("item2-block-0"),
    )
    .await;

    let messages = tokio::time::timeout(Duration::from_millis(500), handle)
        .await
        .expect("poll() must resolve within 500ms of a message being published")
        .expect("poll task must not panic")
        .expect("poll must succeed");

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.id, "item2-block-0");
}

/// `stop()` must unblock a `poll()` parked on an empty queue promptly —
/// the standing subscription's channel closes when the background task
/// exits on cancellation, which is what turns a would-be-forever
/// `rx.recv().await` into an immediate `Err(QueueError::Stopped)`.
///
/// Pins: elapsed time from `stop()` to the parked `poll()` resolving is
/// well under 500ms. Mutant check (temporarily made `stop()` a no-op —
/// dropped the `self.stream_cancel.cancel()` call — confirmed by hand
/// while implementing item 2, then restored): the parked `poll()` never
/// resolves and the `tokio::time::timeout` below fires instead, failing
/// the test.
#[tokio::test]
#[ignore]
async fn item2_stop_unblocks_a_parked_poll_within_500ms() {
    let (_container, servers) = start_jetstream().await;
    let stream = format!("ITEM2STOP{}", uuid::Uuid::new_v4().simple());
    let subject_filter = format!("{stream}.>");

    let config = NatsConfig {
        servers,
        stream_name: stream,
        consumer_name: "router".to_string(),
        subject: subject_filter,
        max_messages_per_poll: 10,
        ..NatsConfig::default()
    };
    let consumer = std::sync::Arc::new(
        NatsQueueConsumer::new(config)
            .await
            .expect("failed to build consumer"),
    );

    let poll_consumer = consumer.clone();
    let handle = tokio::spawn(async move { poll_consumer.poll(10).await });

    // Give the poll task a moment to actually be parked on `recv()`.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !handle.is_finished(),
        "poll() should still be parked before stop()"
    );

    let start = Instant::now();
    consumer.stop().await;

    let result = tokio::time::timeout(Duration::from_millis(500), handle)
        .await
        .expect("stop() must unblock the parked poll() within 500ms")
        .expect("poll task must not panic");
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(QueueError::Stopped)),
        "a poll() unblocked by stop() must report QueueError::Stopped, got {:?}",
        result
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "stop() -> poll() unblocking took {:?}, expected well under 500ms",
        elapsed
    );
}

/// Back-pressure bound: with **zero** `poll()` calls, the standing
/// subscription must not run ahead and pull the whole stream into local
/// memory — the client-side channel (capacity `max_messages_per_poll`) is
/// what stops it. Proven at the broker, not just locally: `num_ack_pending`
/// only counts messages the *server* has actually sent to a client, so an
/// unbounded local buffer would still show up here as the server having
/// handed over (and JetStream now tracking as unacked) far more than one
/// channel's worth.
///
/// Pins: `num_ack_pending` stays within a small multiple of
/// `max_messages_per_poll` (one batch the standing subscription's own
/// internal prefetch may hold, plus one channel's worth) even though 20
/// messages — 10x `max_messages_per_poll` — are sitting on the stream and
/// nothing has ever drained the channel.
///
/// Mutant check (temporarily changed the channel to
/// `mpsc::channel::<QueuedMessage>(100_000)` — an effectively unbounded
/// capacity — confirmed by hand while implementing item 2, then restored):
/// `num_ack_pending` climbs to (or very near) the full 20 published
/// messages well within the wait below, failing the bound.
#[tokio::test]
#[ignore]
async fn item2_bounded_channel_caps_ack_pending_before_any_poll() {
    let (_container, servers) = start_jetstream().await;
    let stream = format!("ITEM2BP{}", uuid::Uuid::new_v4().simple());
    let subject_filter = format!("{stream}.>");
    let max_messages: u32 = 2;
    let published: u32 = 20;

    // Publish well more than the channel capacity BEFORE the consumer (and
    // its standing subscription) exists, so every message is already
    // sitting on the stream the instant the subscription opens.
    {
        let client = async_nats::connect(&servers)
            .await
            .expect("failed to connect for publishing");
        let js = async_nats::jetstream::new(client);
        // Provision the stream up front (durable WorkQueue) so publishes
        // land before NatsQueueConsumer::new() also provisions it.
        js.get_or_create_stream(async_nats::jetstream::stream::Config {
            name: stream.clone(),
            subjects: vec![subject_filter.clone()],
            retention: async_nats::jetstream::stream::RetentionPolicy::WorkQueue,
            storage: async_nats::jetstream::stream::StorageType::Memory,
            ..Default::default()
        })
        .await
        .expect("create stream");
        for i in 0..published {
            js.publish(
                format!("{stream}.m{i}"),
                serde_json::to_vec(&healthy_message(&format!("bp-{i}")))
                    .unwrap()
                    .into(),
            )
            .await
            .expect("publish request failed")
            .await
            .expect("publish ack failed");
        }
    }

    let config = NatsConfig {
        servers: servers.clone(),
        stream_name: stream.clone(),
        consumer_name: "router".to_string(),
        subject: subject_filter,
        max_messages_per_poll: max_messages,
        ..NatsConfig::default()
    };
    let consumer = NatsQueueConsumer::new(config)
        .await
        .expect("failed to build consumer");

    // Never call poll() — give the background task plenty of time to run
    // away with the whole stream if its channel weren't bounded.
    tokio::time::sleep(Duration::from_millis(800)).await;

    let client = async_nats::connect(&servers)
        .await
        .expect("failed to connect for admin check");
    let js = async_nats::jetstream::new(client);
    let mut nats_consumer: async_nats::jetstream::consumer::PullConsumer = js
        .get_stream(&stream)
        .await
        .expect("get stream")
        .get_consumer("router")
        .await
        .expect("get consumer");
    let info = nats_consumer.info().await.expect("consumer info");

    let bound = max_messages * 4;
    assert!(
        (info.num_ack_pending as u32) <= bound,
        "num_ack_pending = {} with max_messages_per_poll = {} (bound {}) \
         and zero poll() calls, {} messages published — a bounded channel \
         must cap how far ahead of the consumer the standing subscription \
         can pull; an unbounded channel would drain the whole stream into \
         local memory almost immediately",
        info.num_ack_pending,
        max_messages,
        bound,
        published
    );
    assert!(
        (info.num_ack_pending as u32) < published,
        "num_ack_pending = {} must be well short of the {} published \
         messages — that's the actual back-pressure signal",
        info.num_ack_pending,
        published
    );

    // The bound isn't just "poll never happens to run" — draining via
    // poll() must still work and return real messages once we do call it.
    let messages = consumer.poll(max_messages).await.expect("poll failed");
    assert_eq!(messages.len(), max_messages as usize);
}

/// C4: `max_deliver` and `max_ack_pending` default to unlimited, as Go's do
/// (owner ruling 2026-09-22: "the router owns give-up") — and a durable
/// consumer provisioned earlier under the old finite defaults (10 / 1000)
/// is UPDATED on the next start, not reused untouched. Before this,
/// `get_or_create_consumer` returned the existing consumer as it was, so a
/// deployment's first-ever limits stayed in force for ever.
#[tokio::test]
#[ignore]
async fn c4_existing_durable_consumer_is_updated_to_unlimited_limits() {
    let (_container, servers) = start_jetstream().await;
    let stream = format!("C4LIMITS{}", uuid::Uuid::new_v4().simple());
    let subject_filter = format!("{stream}.>");

    // Provision stream + durable consumer the way the old code did.
    {
        let client = async_nats::connect(&servers).await.expect("connect");
        let js = async_nats::jetstream::new(client);
        let s = js
            .get_or_create_stream(async_nats::jetstream::stream::Config {
                name: stream.clone(),
                subjects: vec![subject_filter.clone()],
                retention: async_nats::jetstream::stream::RetentionPolicy::WorkQueue,
                ..Default::default()
            })
            .await
            .expect("create stream");
        let _: async_nats::jetstream::consumer::PullConsumer = s
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("router".to_string()),
                ack_wait: Duration::from_secs(120),
                max_deliver: 10,
                max_ack_pending: 1000,
                filter_subject: subject_filter.clone(),
                ..Default::default()
            })
            .await
            .expect("create legacy consumer");
    }

    let config = NatsConfig {
        servers: servers.clone(),
        stream_name: stream.clone(),
        consumer_name: "router".to_string(),
        subject: subject_filter,
        ..NatsConfig::default()
    };
    assert_eq!(config.max_deliver, -1);
    assert_eq!(config.max_ack_pending, -1);
    let consumer = NatsQueueConsumer::new(config)
        .await
        .expect("failed to build consumer");

    let client = async_nats::connect(&servers).await.expect("connect");
    let js = async_nats::jetstream::new(client);
    let mut nats_consumer: async_nats::jetstream::consumer::PullConsumer = js
        .get_stream(&stream)
        .await
        .expect("get stream")
        .get_consumer("router")
        .await
        .expect("get consumer");
    let info = nats_consumer.info().await.expect("consumer info");
    assert_eq!(
        info.config.max_deliver, -1,
        "the existing durable consumer's max_deliver must be updated to unlimited"
    );
    assert_eq!(
        info.config.max_ack_pending, -1,
        "the existing durable consumer's max_ack_pending must be updated to unlimited"
    );
    consumer.stop().await;
}
