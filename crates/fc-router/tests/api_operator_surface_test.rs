//! HTTP-level tests for the operator-surface-parity endpoints added
//! alongside Go's `internal/router/api` dashboard: in-flight detail,
//! force-ACK, the mediating view, blocked groups, and group-flush
//! suppressions. Each test pins the endpoint's JSON field names (matching
//! Go's `dto.go`) plus one behaviour, per the parity work's testing brief.
//!
//! No other test file in this crate exercises the axum router at the HTTP
//! layer yet (existing coverage is all at the `QueueManager`/`ProcessPool`
//! level) — this file's small harness (`build_app`/`body_json`) is new.

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use fc_common::{
    MediationOutcome, MediationType, Message, PoolConfig, QueuedMessage, RouterConfig,
};
use fc_queue::{QueueConsumer, QueuePublisher};
use fc_router::{
    api::create_router, HealthService, HealthServiceConfig, Mediator, QueueManager, WarningService,
    WarningServiceConfig,
};
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

// ---------------------------------------------------------------------
// Minimal mocks — this file's own, not shared with manager_tests.rs
// (integration test binaries don't share modules).
// ---------------------------------------------------------------------

struct NoOpPublisher;

#[async_trait]
impl QueuePublisher for NoOpPublisher {
    fn identifier(&self) -> &str {
        "noop"
    }
    async fn publish(&self, _message: Message) -> fc_queue::Result<String> {
        Ok("noop".to_string())
    }
    async fn publish_batch(&self, messages: Vec<Message>) -> fc_queue::Result<Vec<String>> {
        Ok(messages.iter().map(|_| "noop".to_string()).collect())
    }
}

/// A mediator with a configurable delay, so a test can observe a delivery
/// while it's actually in flight (same technique `pool_tests.rs` uses).
struct DelayMediator {
    delay: Duration,
    call_count: AtomicU32,
}

impl DelayMediator {
    fn new(delay: Duration) -> Self {
        Self {
            delay,
            call_count: AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl Mediator for DelayMediator {
    async fn mediate(&self, _message: &Message) -> MediationOutcome {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        MediationOutcome::success(200)
    }
}

struct MockQueueConsumer {
    identifier: String,
    messages: parking_lot::Mutex<Vec<QueuedMessage>>,
    acked: parking_lot::Mutex<Vec<String>>,
}

impl MockQueueConsumer {
    fn with_messages(identifier: &str, messages: Vec<QueuedMessage>) -> Self {
        Self {
            identifier: identifier.to_string(),
            messages: parking_lot::Mutex::new(messages),
            acked: parking_lot::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl QueueConsumer for MockQueueConsumer {
    fn identifier(&self) -> &str {
        &self.identifier
    }
    async fn poll(&self, _max_messages: u32) -> fc_queue::Result<Vec<QueuedMessage>> {
        Ok(std::mem::take(&mut *self.messages.lock()))
    }
    async fn ack(&self, receipt_handle: &str) -> fc_queue::Result<()> {
        self.acked.lock().push(receipt_handle.to_string());
        Ok(())
    }
    async fn nack(
        &self,
        _receipt_handle: &str,
        _delay_seconds: Option<u32>,
    ) -> fc_queue::Result<()> {
        Ok(())
    }
    async fn extend_visibility(
        &self,
        _receipt_handle: &str,
        _seconds: u32,
    ) -> fc_queue::Result<()> {
        Ok(())
    }
    fn is_healthy(&self) -> bool {
        true
    }
    async fn stop(&self) {}
}

fn test_message(id: &str, pool_code: &str, group_id: Option<&str>) -> Message {
    Message {
        id: id.to_string(),
        pool_code: pool_code.to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: "http://localhost:9/unused".to_string(),
        message_group_id: group_id.map(str::to_string),
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::NextOnError,
        dispatch_mode_specified: true,
    }
}

fn queued(msg: Message, queue_id: &str) -> QueuedMessage {
    QueuedMessage {
        receipt_handle: format!("receipt-{}", msg.id),
        broker_message_id: Some(format!("broker-{}", msg.id)),
        queue_identifier: queue_id.to_string(),
        message: msg,
    }
}

// ---------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------

async fn build_app(manager: Arc<QueueManager>) -> axum::Router {
    let publisher: Arc<dyn QueuePublisher> = Arc::new(NoOpPublisher);
    let warnings = Arc::new(WarningService::new(WarningServiceConfig::default()));
    let health = Arc::new(HealthService::new(
        HealthServiceConfig::default(),
        warnings.clone(),
    ));
    let breakers = manager.circuit_breaker_registry().clone();
    create_router(publisher, manager, warnings, health, breakers)
}

async fn get(app: &axum::Router, path: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response body must be JSON")
    };
    (status, body)
}

async fn post(app: &axum::Router, path: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response body must be JSON")
    };
    (status, body)
}

// ---------------------------------------------------------------------
// Mediating
// ---------------------------------------------------------------------

#[tokio::test]
async fn get_mediating_shape_and_reflects_in_flight_delivery() {
    let mediator = Arc::new(DelayMediator::new(Duration::from_millis(150)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone() as Arc<dyn Mediator>,
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "MED".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let app = build_app(manager.clone()).await;

    // Empty before anything is routed.
    let (status, body) = get(&app, "/monitoring/mediating").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!([]));

    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "med-queue",
        vec![queued(
            test_message("med-msg-1", "MED", Some("g1")),
            "med-queue",
        )],
    ));
    manager.add_consumer(consumer.clone()).await;
    let polled = consumer.poll(10).await.unwrap();
    manager.route_batch(polled, consumer).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    let (status, body) = get(&app, "/monitoring/mediating").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().expect("array body");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    // Field names must match Go's MediatingInfo exactly.
    assert_eq!(row["messageId"], "med-msg-1");
    assert_eq!(row["poolCode"], "MED");
    assert_eq!(row["group"], "g1");
    assert_eq!(row["queue"], "med-queue");
    assert_eq!(row["target"], "http://localhost:9/unused");
    assert_eq!(row["attempts"], 0);
    assert!(row["elapsedTimeMs"].as_u64().unwrap() < 150);

    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, body) = get(&app, "/monitoring/mediating").await;
    assert_eq!(
        body,
        serde_json::json!([]),
        "must clear once delivery finishes"
    );
}

// ---------------------------------------------------------------------
// In-flight detail
// ---------------------------------------------------------------------

#[tokio::test]
async fn get_in_flight_detail_shape_for_untracked_message() {
    let mediator = Arc::new(DelayMediator::new(Duration::from_millis(10)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator as Arc<dyn Mediator>,
    ));
    let app = build_app(manager).await;

    let (status, body) = get(
        &app,
        "/monitoring/in-flight-messages/detail?messageId=never-seen",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a miss is 200 inPipeline=false, not 404"
    );
    assert_eq!(body["messageId"], "never-seen");
    assert_eq!(body["inPipeline"], false);
    assert!(body["status"].is_null());
}

#[tokio::test]
async fn get_in_flight_detail_reports_mediating_status_while_in_flight() {
    let mediator = Arc::new(DelayMediator::new(Duration::from_millis(150)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone() as Arc<dyn Mediator>,
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "DETAIL".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let app = build_app(manager.clone()).await;

    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "detail-queue",
        vec![queued(
            test_message("detail-msg-1", "DETAIL", None),
            "detail-queue",
        )],
    ));
    manager.add_consumer(consumer.clone()).await;
    let polled = consumer.poll(10).await.unwrap();
    manager.route_batch(polled, consumer).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    let (status, body) = get(
        &app,
        "/monitoring/in-flight-messages/detail?messageId=detail-msg-1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["messageId"], "detail-msg-1");
    assert_eq!(body["inPipeline"], true);
    assert_eq!(body["status"], "MEDIATING");
    assert_eq!(body["poolCode"], "DETAIL");
    assert_eq!(body["queueId"], "detail-queue");
    assert_eq!(body["attempts"], 0);
    assert_eq!(body["mediationTarget"], "http://localhost:9/unused");
    assert!(body["mediatingElapsedMs"].as_u64().is_some());
    assert!(body["addedToInPipelineAt"].is_string());
}

// ---------------------------------------------------------------------
// Force-ACK
// ---------------------------------------------------------------------

#[tokio::test]
async fn force_ack_unknown_message_is_404() {
    let mediator = Arc::new(DelayMediator::new(Duration::from_millis(10)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator as Arc<dyn Mediator>,
    ));
    let app = build_app(manager).await;

    let (status, _) = post(&app, "/monitoring/in-flight-messages/never-seen/ack").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn force_ack_clears_tracker_entry_and_acks_broker() {
    let mediator = Arc::new(DelayMediator::new(Duration::from_millis(10)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator as Arc<dyn Mediator>,
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "ACKPOOL".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let app = build_app(manager.clone()).await;

    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "ack-queue",
        vec![queued(
            test_message("ack-msg-1", "ACKPOOL", None),
            "ack-queue",
        )],
    ));
    manager.add_consumer(consumer.clone()).await;
    let polled = consumer.poll(10).await.unwrap();
    manager.route_batch(polled, consumer.clone()).await.unwrap();

    let (status, body) = post(&app, "/monitoring/in-flight-messages/ack-msg-1/ack").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["messageId"], "ack-msg-1");
    assert_eq!(body["removed"], true);
    assert_eq!(body["brokerAcked"], true);
    assert_eq!(body["queueId"], "ack-queue");
    assert_eq!(body["poolCode"], "ACKPOOL");
    assert!(body["elapsedTimeMs"].as_u64().is_some());
    assert_eq!(consumer.acked.lock().len(), 1);

    // Second call: the entry is gone.
    let (status, _) = post(&app, "/monitoring/in-flight-messages/ack-msg-1/ack").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------
// Blocked groups
// ---------------------------------------------------------------------

#[tokio::test]
async fn get_blocked_groups_shape_and_reflects_a_gated_group() {
    let mediator = Arc::new(DelayMediator::new(Duration::from_millis(150)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator.clone() as Arc<dyn Mediator>,
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "GROUPED".to_string(),
                concurrency: 1,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let app = build_app(manager.clone()).await;

    let (status, body) = get(&app, "/monitoring/blocked-groups").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!([]));

    let consumer = Arc::new(MockQueueConsumer::with_messages(
        "grouped-queue",
        vec![
            queued(
                test_message("grp-msg-1", "GROUPED", Some("gA")),
                "grouped-queue",
            ),
            queued(
                test_message("grp-msg-2", "GROUPED", Some("gA")),
                "grouped-queue",
            ),
        ],
    ));
    manager.add_consumer(consumer.clone()).await;
    let polled = consumer.poll(10).await.unwrap();
    manager.route_batch(polled, consumer).await.unwrap();

    tokio::time::sleep(Duration::from_millis(30)).await;
    let (status, body) = get(&app, "/monitoring/blocked-groups").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["group"], "gA");
    assert_eq!(row["poolCode"], "GROUPED");
    assert_eq!(row["buffered"], 1);
    assert_eq!(row["working"], true);
    assert_eq!(row["suppressed"], false);

    // Pool filter: a different pool code returns nothing.
    let (_, body) = get(&app, "/monitoring/blocked-groups?poolCode=OTHER").await;
    assert_eq!(body, serde_json::json!([]));

    tokio::time::sleep(Duration::from_millis(350)).await;
    let (_, body) = get(&app, "/monitoring/blocked-groups").await;
    assert_eq!(
        body,
        serde_json::json!([]),
        "fully-drained group must not show up"
    );
}

// ---------------------------------------------------------------------
// Group flushes
// ---------------------------------------------------------------------

#[tokio::test]
async fn get_and_clear_group_flushes_shape_and_behaviour() {
    let mediator = Arc::new(DelayMediator::new(Duration::from_millis(10)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator as Arc<dyn Mediator>,
    ));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "FLUSHED".to_string(),
                concurrency: 5,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let app = build_app(manager.clone()).await;

    // Arm a suppression directly on the pool's registry (a target's
    // flushGroup response is out of this test's scope — see
    // manager_tests.rs's equivalent manager-level test for the same
    // rationale).
    let pool = manager.get_pool("FLUSHED").expect("pool exists");
    assert!(pool.group_flush_registry().flush("gF", Some(3600)));

    let (status, body) = get(&app, "/monitoring/group-flushes").await;
    assert_eq!(status, StatusCode::OK);
    // One row per pool — DEFAULT-POOL is always present (Go: Reconfigure
    // ensures it) — so pick the pool under test.
    let rows: Vec<Value> = body
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["poolCode"] == "FLUSHED")
        .cloned()
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["poolCode"], "FLUSHED");
    assert_eq!(rows[0]["activeCount"], 1);
    assert_eq!(rows[0]["totalFlushes"], 1);
    assert_eq!(rows[0]["totalSuppressed"], 0);
    let groups = rows[0]["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["group"], "gF");
    assert!(groups[0]["suppressedUntil"].is_string());

    // Clearing an unknown pool/group 404s.
    let (status, _) = post(
        &app,
        "/monitoring/group-flushes/FLUSHED/no-such-group/clear",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = post(&app, "/monitoring/group-flushes/FLUSHED/gF/clear").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["cleared"], true);
    assert_eq!(body["poolCode"], "FLUSHED");
    assert_eq!(body["group"], "gF");

    let (_, body) = get(&app, "/monitoring/group-flushes").await;
    let row = body
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["poolCode"] == "FLUSHED")
        .cloned()
        .unwrap();
    assert_eq!(row["activeCount"], 0);
    assert_eq!(row["groups"], serde_json::json!([]));

    // Already cleared: 404 again.
    let (status, _) = post(&app, "/monitoring/group-flushes/FLUSHED/gF/clear").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------
// Dashboard HTML: smoke-tests that the new tabs actually shipped
// ---------------------------------------------------------------------

#[tokio::test]
async fn dashboard_html_includes_the_new_operator_tabs() {
    let mediator = Arc::new(DelayMediator::new(Duration::from_millis(10)));
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        mediator as Arc<dyn Mediator>,
    ));
    let app = build_app(manager).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/monitoring/dashboard")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(bytes.to_vec()).unwrap();

    for marker in [
        "id=\"tabMediating\"",
        "id=\"tabBlockedGroups\"",
        "id=\"tabGroupFlushes\"",
        "id=\"contentMediating\"",
        "id=\"contentBlockedGroups\"",
        "id=\"contentGroupFlushes\"",
        "id=\"messageDetailAckBtn\"",
        "forceAckMessage",
        "/monitoring/mediating",
        "/monitoring/blocked-groups",
        "/monitoring/group-flushes",
        "clearGroupFlush",
    ] {
        assert!(
            html.contains(marker),
            "dashboard.html must contain {marker:?}"
        );
    }
}

// ---------------------------------------------------------------------
// PUT /monitoring/pools/{code} (Go: UpdatePool)
// ---------------------------------------------------------------------

async fn put_json(app: &axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response body must be JSON")
    };
    (status, body)
}

/// H11: the pool is updated IN PLACE (same instance, new settings); only
/// the fields sent change; the body echoes them as Go does. It used to
/// build a second pool and swap it in without draining the first.
#[tokio::test]
async fn put_pool_updates_in_place_and_echoes_the_request() {
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(Arc::new(
        DelayMediator::new(Duration::from_millis(1)),
    )));
    manager
        .apply_config(RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "P".to_string(),
                concurrency: 5,
                rate_limit_per_minute: Some(100),
            }],
            queues: vec![],
        })
        .await
        .unwrap();
    let before = manager.get_pool("P").unwrap();
    let app = build_app(manager.clone()).await;

    let (status, body) = put_json(
        &app,
        "/monitoring/pools/P",
        serde_json::json!({"concurrency": 8}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);
    assert_eq!(body["pool_code"], "P");
    assert_eq!(body["new_config"], serde_json::json!({"concurrency": 8}));

    let after = manager.get_pool("P").unwrap();
    assert!(
        Arc::ptr_eq(&before, &after),
        "updated in place, not replaced"
    );
    assert_eq!(after.concurrency(), 8);
    assert_eq!(
        after.rate_limit_per_minute(),
        Some(100),
        "an omitted field is left unchanged"
    );
    assert_eq!(manager.draining_pool_count(), 0);
}

/// Go answers 404 for an unknown pool; it never creates one.
#[tokio::test]
async fn put_unknown_pool_is_404_and_creates_nothing() {
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(Arc::new(
        DelayMediator::new(Duration::from_millis(1)),
    )));
    let app = build_app(manager.clone()).await;
    let (status, _) = put_json(
        &app,
        "/monitoring/pools/NOPE",
        serde_json::json!({"concurrency": 3}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(manager.get_pool("NOPE").is_none());
}
