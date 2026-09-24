//! The loop (Java `ReconcileLoopTest`, R10): coalescing triggers, a close
//! that cancels a run blocked in the control plane, and a panicking run
//! that does not end the loop.

mod support;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use fc_fnhost_core::clock::SystemClock;
use fc_fnhost_core::loader::Loaders;
use fc_fnhost_core::reconcile_loop::ReconcileLoop;
use fc_fnhost_core::reconciler::Reconciler;
use fc_fnhost_core::registry::FunctionRegistry;
use fc_fnhost_core::signature::Signatures;
use support::fakes::{Answer, FakeControlPlane, FakeStore};
use tokio::sync::Semaphore;

fn reconciler(control: Arc<FakeControlPlane>) -> Arc<Reconciler> {
    Arc::new(Reconciler::new(
        "default",
        "h",
        control,
        FakeStore::new(),
        Signatures::Off,
        Loaders::none(),
        Arc::new(FunctionRegistry::new(10, Arc::new(SystemClock))),
    ))
}

async fn wait_for(mut condition: impl FnMut() -> bool) {
    for _ in 0..500 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition never became true");
}

#[tokio::test]
async fn triggers_during_a_run_coalesce_into_exactly_one_more_run() {
    let control = FakeControlPlane::new();
    control.serve(Answer::NotModified);
    let gate = Arc::new(Semaphore::new(0));
    *control.gate.lock() = Some(gate.clone());
    let lp = ReconcileLoop::with_interval(
        reconciler(control.clone()),
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    );
    lp.start();
    // run 1 is now blocked in the control plane; trigger many times
    tokio::time::sleep(Duration::from_millis(50)).await;
    for _ in 0..10 {
        lp.trigger();
    }
    gate.add_permits(100);
    wait_for(|| control.fetches.load(Ordering::SeqCst) == 2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        control.fetches.load(Ordering::SeqCst),
        2,
        "ten triggers are one extra run"
    );
    lp.trigger();
    wait_for(|| control.fetches.load(Ordering::SeqCst) == 3).await;
    lp.close().await;
}

#[tokio::test]
async fn close_cancels_a_run_blocked_in_the_control_plane() {
    let control = FakeControlPlane::new();
    *control.gate.lock() = Some(Arc::new(Semaphore::new(0))); // never released
    let lp = ReconcileLoop::with_interval(
        reconciler(control.clone()),
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    );
    lp.start();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(lp.is_alive());
    let started = std::time::Instant::now();
    lp.close().await;
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!lp.is_alive());
}

#[tokio::test]
async fn a_panicking_run_does_not_end_the_loop() {
    let control = FakeControlPlane::new();
    control.serve(Answer::NotModified);
    control.panic_next.store(true, Ordering::SeqCst);
    let lp = ReconcileLoop::with_interval(
        reconciler(control.clone()),
        Arc::new(SystemClock),
        Duration::from_millis(20),
    );
    lp.start();
    wait_for(|| control.fetches.load(Ordering::SeqCst) >= 2).await;
    assert!(lp.is_alive());
    lp.close().await;
}

#[tokio::test]
async fn the_interval_is_measured_from_the_end_of_a_run() {
    let control = FakeControlPlane::new();
    control.serve(Answer::NotModified);
    let lp = ReconcileLoop::with_interval(
        reconciler(control.clone()),
        Arc::new(SystemClock),
        Duration::from_millis(300),
    );
    lp.start();
    wait_for(|| control.fetches.load(Ordering::SeqCst) == 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        control.fetches.load(Ordering::SeqCst),
        1,
        "no second run before the interval"
    );
    wait_for(|| control.fetches.load(Ordering::SeqCst) == 2).await;
    lp.close().await;
}
