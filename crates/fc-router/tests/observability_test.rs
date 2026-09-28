//! Router observability and robustness (docs/operations/diagnosing-stuck-processes.md):
//!
//! - the flight recorder tells a message's story (routed, dispatched,
//!   settled) and records what a group decided;
//! - (`log_correlation_test.rs`) every line logged while a message is in a
//!   worker carries its id, pool and group (the `router.dispatch` span);
//! - `/diagnostics/messages/{id}` says where a message is now;
//! - shutdown hands back deliveries still in hand past the drain budget
//!   (Go's `Stop` dropped them without a nack);
//! - a reconfigure applies everything it can and retries what it could not
//!   (Go's aborted at the first failing queue).

use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fc_common::{
    DispatchMode, MediationOutcome, MediationType, Message, PoolConfig, QueuedMessage, RouterConfig,
};
use fc_queue::QueueConsumer;
use fc_router::flight_recorder::EventKind;
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

fn kinds(manager: &QueueManager, id: &str) -> Vec<EventKind> {
    manager
        .flight_recorder()
        .for_message(id)
        .iter()
        .map(|e| e.kind)
        .collect()
}

// ── Flight recorder ────────────────────────────────────────────────────────

#[tokio::test]
async fn the_flight_recorder_tells_a_message_story() {
    let manager = manager(4).await;
    let consumer = Arc::new(Consumer::default());
    manager
        .route_batch(vec![queued("ok-1", None)], consumer.clone())
        .await
        .unwrap();
    eventually("the ack", || !consumer.acked.lock().unwrap().is_empty()).await;
    eventually("the Acked event", || {
        kinds(&manager, "ok-1").last() == Some(&EventKind::Acked)
    })
    .await;

    assert_eq!(
        kinds(&manager, "ok-1"),
        vec![
            EventKind::Routed,
            EventKind::DispatchStarted,
            EventKind::DispatchFinished,
            EventKind::Acked
        ]
    );
    let finished = manager
        .flight_recorder()
        .for_message("ok-1")
        .into_iter()
        .find(|e| e.kind == EventKind::DispatchFinished)
        .unwrap();
    assert_eq!(finished.facts.outcome, Some("Success"));
    assert_eq!(finished.facts.status, Some(200));
    assert_eq!(finished.facts.action, Some("Ack"));
    assert_eq!(finished.facts.attempt, Some(1));
    assert_eq!(finished.pool(), Some("P1"));
    assert_eq!(finished.queue(), Some("q1"));
}

#[tokio::test]
async fn a_released_group_is_recorded_as_a_group_decision() {
    let manager = manager(4).await;
    let consumer = Arc::new(Consumer::default());
    manager
        .route_batch(
            vec![queued("fail-1", Some("g1")), queued("next-2", Some("g1"))],
            consumer.clone(),
        )
        .await
        .unwrap();
    eventually("both nacks", || consumer.nacked.lock().unwrap().len() == 2).await;

    let head = manager.flight_recorder().for_message("fail-1");
    let decision = head
        .iter()
        .find(|e| e.kind == EventKind::GroupDecision)
        .expect("a group decision for the head");
    let detail = decision.facts.detail.as_deref().unwrap();
    assert!(detail.starts_with("RETURN_GROUP"), "{detail}");
    assert!(detail.contains("1 buffered sibling"), "{detail}");
    assert_eq!(decision.group(), Some("g1"));
    assert_eq!(
        kinds(&manager, "next-2"),
        vec![EventKind::Routed, EventKind::Nacked],
        "the sibling was never dispatched"
    );
}

// ── What is message X doing now ────────────────────────────────────────────

#[tokio::test]
async fn the_message_lookup_says_where_a_message_is() {
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let manager = manager(4).await;
    let consumer = Arc::new(Consumer::default());
    // hang-1 holds the group's worker; next-2 waits behind it.
    manager
        .route_batch(
            vec![queued("hang-1", Some("g1")), queued("next-2", Some("g1"))],
            consumer.clone(),
        )
        .await
        .unwrap();
    eventually("hang-1 in a worker", || {
        !manager.mediating_snapshot().is_empty()
    })
    .await;

    let warnings = Arc::new(fc_router::WarningService::noop());
    let health = Arc::new(fc_router::HealthService::new(
        fc_router::HealthServiceConfig::default(),
        warnings.clone(),
    ));
    let app = fc_router::api::create_router(
        Arc::new(NoPublisher),
        manager.clone(),
        warnings,
        health,
        manager.circuit_breaker_registry().clone(),
    );
    let get = |path: &str| {
        let app = app.clone();
        let path = path.to_string();
        async move {
            let resp = app
                .oneshot(axum::http::Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let bytes = resp.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
        }
    };

    let head = get("/diagnostics/messages/hang-1").await;
    assert_eq!(head["status"], "MEDIATING", "{head}");
    assert_eq!(head["mediating"]["poolCode"], "P1");
    assert_eq!(head["tracker"]["messageGroup"], "g1");

    let behind = get("/diagnostics/messages/next-2").await;
    assert_eq!(behind["status"], "BUFFERED", "{behind}");
    assert_eq!(behind["buffered"]["group"], "g1");
    assert_eq!(behind["buffered"]["position"], 0);
    assert_eq!(behind["history"][0]["kind"], "ROUTED");

    let group = get("/diagnostics/groups/g1").await;
    assert_eq!(group["pools"][0]["working"], true, "{group}");
    assert_eq!(group["pools"][0]["inWorker"]["attempts"], 0);
    assert_eq!(group["pools"][0]["buffered"][0][0], "next-2");

    let events = get("/diagnostics/events?kind=routed&group=g1").await;
    assert_eq!(events.as_array().unwrap().len(), 2, "{events}");

    // The in-flight detail now carries last-seen and the real attempts.
    let detail = get("/monitoring/in-flight-messages/detail?messageId=hang-1").await;
    assert_eq!(detail["status"], "MEDIATING");
    assert!(detail["lastSeenAt"].is_string(), "{detail}");
    // The check answers flat, as Go and Java do (and the SDKs read).
    let check = get("/monitoring/in-flight-messages/check?messageId=hang-1").await;
    assert_eq!(check["poolCode"], "P1", "{check}");
    assert_eq!(check["queueId"], "q1");
}

struct NoPublisher;

#[async_trait]
impl fc_queue::QueuePublisher for NoPublisher {
    fn identifier(&self) -> &str {
        "none"
    }
    async fn publish(&self, _: Message) -> fc_queue::Result<String> {
        Ok(String::new())
    }
    async fn publish_batch(&self, m: Vec<Message>) -> fc_queue::Result<Vec<String>> {
        Ok(m.iter().map(|_| String::new()).collect())
    }
}

// ── Shutdown ───────────────────────────────────────────────────────────────

/// A delivery still in a worker when the drain budget runs out is handed
/// back to the broker (with a short delay, so this process is gone before
/// another router takes it), not left invisible for its whole visibility
/// timeout.
#[tokio::test]
async fn shutdown_hands_back_deliveries_still_in_hand() {
    let manager = manager(4).await;
    let consumer = Arc::new(Consumer::default());
    manager
        .add_consumer(consumer.clone() as Arc<dyn QueueConsumer>)
        .await;
    manager
        .route_batch(vec![queued("hang-9", None)], consumer.clone())
        .await
        .unwrap();
    eventually("hang-9 in a worker", || {
        !manager.mediating_snapshot().is_empty()
    })
    .await;

    manager
        .shutdown_with_timeout(Duration::from_millis(100))
        .await;

    assert_eq!(
        consumer.nacked.lock().unwrap().clone(),
        vec![("rh-hang-9".to_string(), Some(10))]
    );
    assert!(kinds(&manager, "hang-9").contains(&EventKind::ReleasedAtShutdown));
}

// ── Reconfigure ────────────────────────────────────────────────────────────

/// A concurrency decrease that cannot be applied (every slot is busy past
/// the 60 s wait) does not stop the rest of the reconfigure — a new pool is
/// still created — and the reload reports failure so the config is retried;
/// the stored config keeps the concurrency the pool really runs at.
#[tokio::test(start_paused = true)]
async fn a_reconfigure_applies_the_rest_and_retries_what_timed_out() {
    let manager = manager(2).await;
    let consumer = Arc::new(Consumer::default());
    manager
        .route_batch(
            vec![queued("hang-a", None), queued("hang-b", None)],
            consumer.clone(),
        )
        .await
        .unwrap();
    eventually("both slots busy", || {
        manager.mediating_snapshot().len() == 2
    })
    .await;

    let done = Arc::new(AtomicBool::new(false));
    let result = manager
        .reload_config(RouterConfig {
            processing_pools: vec![
                PoolConfig {
                    code: "P1".to_string(),
                    concurrency: 1,
                    rate_limit_per_minute: None,
                },
                PoolConfig {
                    code: "P2".to_string(),
                    concurrency: 3,
                    rate_limit_per_minute: None,
                },
            ],
            queues: vec![],
        })
        .await;
    done.store(true, Ordering::SeqCst);

    let err = result.expect_err("the timed-out decrease fails the reload");
    assert!(
        err.to_string()
            .contains("pool P1: concurrency 1 not applied (still 2)"),
        "{err}"
    );
    let pools: Vec<_> = manager
        .get_pool_stats()
        .into_iter()
        .map(|p| (p.pool_code, p.concurrency))
        .collect();
    assert!(pools.contains(&("P2".to_string(), 3)), "{pools:?}");
    assert!(pools.contains(&("P1".to_string(), 2)), "{pools:?}");
}
