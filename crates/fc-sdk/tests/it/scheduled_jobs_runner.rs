//! Integration tests for `ScheduledJobRunner`.
//!
//! We focus on the dispatch-side logic that doesn't need a live platform:
//! envelope validation, handler routing, lock-contention skip behaviour,
//! and 202-then-spawn timing. The HTTP callback paths (log/complete) are
//! exercised against a mock issuer in a separate test that needs a network.

#![cfg(feature = "scheduled-jobs-runner")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fc_sdk::client::FlowCatalystClient;
use fc_sdk::lock::{LockProvider, MemoryLockProvider, NoOpLockProvider};
use fc_sdk::scheduled_jobs::{
    HandlerError, HandlerFuture, LogOptions, RunResult, ScheduledJobRunner,
};
use serde_json::json;

fn dummy_client() -> FlowCatalystClient {
    // Base URL never actually reached in tests that don't tracksCompletion
    // and skip the success path. Tests that DO trigger callbacks point at
    // a never-resolved port and use `tracks_completion: false`.
    FlowCatalystClient::new("http://127.0.0.1:1").with_token("test")
}

fn envelope(code: &str, tracks_completion: bool) -> serde_json::Value {
    json!({
        "jobId":          "job_x",
        "jobCode":        code,
        "instanceId":     "inst_x",
        "firedAt":        "2026-05-22T00:00:00Z",
        "triggerKind":    "CRON",
        "tracksCompletion": tracks_completion,
    })
}

#[tokio::test]
async fn unknown_code_returns_not_found() {
    let runner = ScheduledJobRunner::builder(dummy_client(), Arc::new(NoOpLockProvider))
        .handler("known", |_| {
            Box::pin(async { Ok::<_, HandlerError>(json!({})) }) as HandlerFuture
        })
        .build();
    let res = runner.process(envelope("ghost", false));
    assert!(matches!(res, RunResult::NotFound(_)), "got {res:?}");
}

#[tokio::test]
async fn malformed_envelope_returns_bad_request() {
    let runner = ScheduledJobRunner::builder(dummy_client(), Arc::new(NoOpLockProvider))
        .handler("k", |_| {
            Box::pin(async { Ok::<_, HandlerError>(json!({})) }) as HandlerFuture
        })
        .build();
    let res = runner.process(json!({ "not": "a real envelope" }));
    assert!(matches!(res, RunResult::BadRequest(_)), "got {res:?}");
}

#[tokio::test]
async fn handler_runs_in_background_and_process_returns_immediately() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    let runner = ScheduledJobRunner::builder(dummy_client(), Arc::new(NoOpLockProvider))
        .handler("rollup", move |_ctx| {
            let counter = counter_clone.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok::<_, HandlerError>(json!({ "done": true }))
            }) as HandlerFuture
        })
        .build();

    let res = runner.process(envelope("rollup", false));
    assert!(matches!(res, RunResult::Accepted), "got {res:?}");

    // Give the spawned task a moment to run.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn lock_contention_skips_handler() {
    let lock = Arc::new(MemoryLockProvider::new());
    // Pre-acquire the lock so the runner sees contention.
    let _holder = lock
        .acquire("scheduled-job:contention", Duration::from_secs(60))
        .await
        .expect("lock backend")
        .expect("lock acquired");

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let runner = ScheduledJobRunner::builder(dummy_client(), Arc::clone(&lock) as _)
        .handler("contention", move |_| {
            let counter = counter_clone.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok::<_, HandlerError>(json!({}))
            }) as HandlerFuture
        })
        .build();

    let res = runner.process(envelope("contention", false));
    assert!(matches!(res, RunResult::Accepted), "got {res:?}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "handler should not have run while another holder owned the lock",
    );
}

#[tokio::test]
async fn lock_released_so_subsequent_fires_succeed() {
    let lock: Arc<dyn fc_sdk::lock::LockProvider> = Arc::new(MemoryLockProvider::new());
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let runner = ScheduledJobRunner::builder(dummy_client(), Arc::clone(&lock))
        .handler("seq", move |_| {
            let counter = counter_clone.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok::<_, HandlerError>(json!({}))
            }) as HandlerFuture
        })
        .lock_ttl(Duration::from_millis(100))
        .build();

    runner.process(envelope("seq", false));
    tokio::time::sleep(Duration::from_millis(50)).await;
    runner.process(envelope("seq", false));
    tokio::time::sleep(Duration::from_millis(80)).await;

    assert_eq!(
        counter.load(Ordering::SeqCst),
        2,
        "both fires should have run when separated by lock release",
    );
}

#[tokio::test]
async fn registered_codes_lists_all_handlers() {
    let runner = ScheduledJobRunner::builder(dummy_client(), Arc::new(NoOpLockProvider))
        .handler("a", |_| {
            Box::pin(async { Ok::<_, HandlerError>(json!({})) }) as HandlerFuture
        })
        .handler("b", |_| {
            Box::pin(async { Ok::<_, HandlerError>(json!({})) }) as HandlerFuture
        })
        .build();
    let mut codes = runner.registered_codes();
    codes.sort();
    assert_eq!(codes, vec!["a", "b"]);
}

#[tokio::test]
async fn log_options_default_is_info_no_metadata() {
    let opts = LogOptions::default();
    // LogLevel is `Copy`; comparing values via formatting (no Eq derived).
    assert_eq!(format!("{:?}", opts.level), "Info");
    assert!(opts.metadata.is_none());
}
