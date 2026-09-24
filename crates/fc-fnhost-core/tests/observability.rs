//! `/health` and `/ready` over HTTP (Java `FnObservabilityHealthReadyTest`,
//! P1/P1b): bodies, status codes and precedence.

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fc_fnhost_core::clock::SystemClock;
use fc_fnhost_core::loader::Loaders;
use fc_fnhost_core::metrics::FnMetrics;
use fc_fnhost_core::observability::{Observability, Probes};
use fc_fnhost_core::reconciler::Reconciler;
use fc_fnhost_core::registry::FunctionRegistry;
use fc_fnhost_core::signature::Signatures;
use support::fakes::{Answer, FakeControlPlane, FakeStore};

struct Flags {
    listener: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
}

async fn get(port: u16, path: &str) -> (u16, serde_json::Value) {
    let response = reqwest::get(format!("http://127.0.0.1:{port}{path}"))
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap())
}

#[tokio::test]
async fn health_and_ready_follow_javas_rules() {
    let control = FakeControlPlane::new();
    let registry = Arc::new(FunctionRegistry::new(10, Arc::new(SystemClock)));
    let reconciler = Arc::new(Reconciler::new(
        "default",
        "h",
        control.clone(),
        FakeStore::new(),
        Signatures::Off,
        Loaders::none(),
        registry.clone(),
    ));
    let flags = Flags {
        listener: Arc::new(AtomicBool::new(false)),
        alive: Arc::new(AtomicBool::new(false)),
        started: Arc::new(AtomicBool::new(false)),
    };
    let (l, a, s) = (
        flags.listener.clone(),
        flags.alive.clone(),
        flags.started.clone(),
    );
    let observability = Observability::start(
        0,
        Probes {
            reconciler: reconciler.clone(),
            metrics: Arc::new(FnMetrics::new(registry)),
            listener_bound: Arc::new(move || l.load(Ordering::SeqCst)),
            reconcile_loop_alive: Arc::new(move || a.load(Ordering::SeqCst)),
            startup_complete: Arc::new(move || s.load(Ordering::SeqCst)),
        },
    )
    .unwrap();
    let port = observability.port();

    // before start-up completes: health is UP whatever else is true; ready is STARTING
    assert_eq!(
        get(port, "/health").await,
        (200, serde_json::json!({"status": "UP"}))
    );
    let (code, body) = get(port, "/ready").await;
    assert_eq!((code, body["status"].as_str().unwrap()), (503, "STARTING"));

    control.serve(Answer::Down);
    reconciler.reconcile_once(chrono::Utc::now()).await;
    assert_eq!(
        get(port, "/ready").await.1["status"],
        "PLATFORM_UNREACHABLE"
    );

    control.serve(Answer::NotModified);
    reconciler.reconcile_once(chrono::Utc::now()).await;
    assert_eq!(get(port, "/ready").await.1["status"], "LISTENER_DOWN");

    flags.started.store(true, Ordering::SeqCst);
    assert_eq!(
        get(port, "/health").await,
        (503, serde_json::json!({"status": "LISTENER_DOWN"}))
    );
    flags.listener.store(true, Ordering::SeqCst);
    assert_eq!(
        get(port, "/health").await,
        (503, serde_json::json!({"status": "RECONCILER_DOWN"}))
    );
    assert_eq!(get(port, "/ready").await.1["status"], "RECONCILER_DOWN");
    flags.alive.store(true, Ordering::SeqCst);
    assert_eq!(get(port, "/health").await.0, 200);
    let (code, body) = get(port, "/ready").await;
    assert_eq!((code, body["status"].as_str().unwrap()), (200, "UP"));

    control.serve(Answer::Down);
    reconciler.reconcile_once(chrono::Utc::now()).await;
    assert_eq!(
        get(port, "/ready").await.0,
        200,
        "a later outage keeps serving and stays ready"
    );

    reconciler.drain();
    let (code, body) = get(port, "/ready").await;
    assert_eq!((code, body["status"].as_str().unwrap()), (503, "DRAINING"));
    assert_eq!(
        get(port, "/health").await.0,
        200,
        "draining is not a liveness failure"
    );
    assert_eq!(
        get(port, "/nope").await,
        (
            404,
            serde_json::json!({"error": "NOT_FOUND", "message": "not found"})
        )
    );
    observability.close().await;
}
