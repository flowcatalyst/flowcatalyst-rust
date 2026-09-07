//! Item 5 (bench rig finding, 2026-09-07): the bench rig's `wait_health`
//! (`bench/router/run.sh`) polls `/health` and starts its drain-time clock
//! the moment the raw HTTP status is 200 — it never reads the JSON body.
//! Before this fix, `api::health::health_handler` returned an
//! *unconditional* 200 (only the body's `"status"` field varied between
//! "UP"/"DEGRADED"), and `QueueManager::start()` — which spawns every
//! consumer's poll task — runs as an independent tokio task with no
//! ordering against the HTTP listener (`bin/fc-router/src/main.rs`). So
//! `/health` could answer ready before a single consumer poll task existed,
//! making the rig's `drain_time_s` measure the router's boot time, not its
//! drain.
//!
//! `QueueManager::consumers_started()` (set inside `start()`, right after
//! every configured consumer's poll task is spawned — see
//! `manager/shutdown.rs`) is what `health_handler` now gates on. This test
//! pins the HTTP **status code**, not the body, because that's the field
//! the rig (and any operator health probe) actually acts on.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use fc_common::Message;
use fc_queue::QueuePublisher;
use fc_router::{
    api::create_router, HealthService, HealthServiceConfig, HttpMediatorConfig, QueueManager,
    WarningService, WarningServiceConfig,
};
use tower::ServiceExt;

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

async fn health_status(app: &axum::Router) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

/// Pins two things in one test (they're two ends of the same gate, and
/// splitting them risks the second one passing vacuously if the first
/// never actually observed "not started"):
///
/// 1. Before `QueueManager::start()` has run at all, `/health` must answer
///    503 — not the unconditional 200 it answered before this fix.
/// 2. Once `start()` has spawned every configured consumer's poll task
///    (here: zero consumers, so the spawn loop completes immediately),
///    `/health` must answer 200.
///
/// Mutant check (reverted `health_handler` to always return
/// `Json(SimpleHealthResponse { .. })` with no status-code gate, confirmed
/// by hand while implementing this fix, then restored): assertion 1 fails
/// immediately — the pre-start response is 200, not 503.
#[tokio::test]
async fn health_gates_on_consumers_started() {
    let manager = Arc::new(QueueManager::builder(HttpMediatorConfig::dev()).build());
    let publisher: Arc<dyn QueuePublisher> = Arc::new(NoOpPublisher);
    let warnings = Arc::new(WarningService::new(WarningServiceConfig::default()));
    let health = Arc::new(HealthService::new(
        HealthServiceConfig::default(),
        warnings.clone(),
    ));
    let breakers = manager.circuit_breaker_registry().clone();
    let app = create_router(publisher, manager.clone(), warnings, health, breakers);

    assert!(
        !manager.consumers_started(),
        "a freshly built manager must not read as started before start() runs"
    );
    assert_eq!(
        health_status(&app).await,
        StatusCode::SERVICE_UNAVAILABLE,
        "before QueueManager::start() has spawned any consumer poll task, \
         /health must not answer 200 — a caller that treats 200 as \"the \
         router is consuming\" would be lied to"
    );

    // Spawn start() the same way main.rs does: fire-and-forget, no
    // rendezvous with the HTTP listener. With zero configured consumers
    // the spawn loop is empty, so `consumers_started` flips essentially
    // immediately, then start() blocks forever on the in-pipeline reaper
    // (never returns) — exactly like production.
    let mgr = manager.clone();
    tokio::spawn(async move {
        let _ = mgr.start().await;
    });

    // Give the spawned task a scheduling opportunity to run past the flag
    // flip — tokio::spawn only schedules, it doesn't run synchronously.
    for _ in 0..50 {
        if manager.consumers_started() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert!(
        manager.consumers_started(),
        "start() must flip consumers_started once every consumer poll task is spawned"
    );
    assert_eq!(
        health_status(&app).await,
        StatusCode::OK,
        "once start() has spawned every consumer's poll task, /health must \
         read ready again"
    );
}
