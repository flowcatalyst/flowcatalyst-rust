//! Java 5afabe52: a refused version's detail is logged when the refusal is
//! new or changes, not on every cycle (or first call) that finds it still
//! refused; and a first-call load of a refused version is not recompiled on
//! every call within a cycle. Its own test binary: a thread-local
//! subscriber shares callsite interest with any test running beside it.

mod support;

use std::sync::Arc;

use chrono::Utc;
use fc_fnhost_core::clock::{ManualClock, SharedClock};
use fc_fnhost_core::loader::Loaders;
use fc_fnhost_core::reconciler::{PinnedLoad, Reconciler};
use fc_fnhost_core::registry::FunctionRegistry;
use fc_fnhost_core::signature::Signatures;
use fc_function_abi::FunctionAddress;
use support::fakes::{self, Answer, FakeControlPlane, FakeLoader, FakeStore};

struct CaptureWriter(Arc<parking_lot::Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Lines captured on this thread, and the guard keeping the capture on.
fn capture() -> (
    Arc<parking_lot::Mutex<Vec<u8>>>,
    tracing::subscriber::DefaultGuard,
) {
    use tracing_subscriber::layer::SubscriberExt;
    let lines = Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
    let writer = {
        let lines = lines.clone();
        move || CaptureWriter(lines.clone())
    };
    let subscriber =
        tracing_subscriber::registry().with(fc_fnhost_core::logging::SlogJsonLayer::new(writer));
    (lines, tracing::subscriber::set_default(subscriber))
}

fn count(lines: &parking_lot::Mutex<Vec<u8>>, message: &str) -> usize {
    String::from_utf8_lossy(&lines.lock())
        .lines()
        .filter(|l| l.contains(message))
        .count()
}

struct Rig {
    control: Arc<FakeControlPlane>,
    loader: Arc<FakeLoader>,
    reconciler: Arc<Reconciler>,
}

fn rig() -> Rig {
    let clock = ManualClock::new(Utc::now());
    let shared: SharedClock = Arc::new(clock);
    let control = FakeControlPlane::new();
    let loader = FakeLoader::new();
    let registry = Arc::new(FunctionRegistry::new(200, shared));
    let reconciler = Arc::new(Reconciler::new(
        "default",
        "host-1",
        control.clone(),
        FakeStore::new(),
        Signatures::Off,
        Loaders::none().with("wasm", loader.clone()),
        registry,
    ));
    Rig {
        control,
        loader,
        reconciler,
    }
}

impl Rig {
    async fn reconcile(&self) {
        self.reconciler.reconcile_once(Utc::now()).await;
    }

    fn attempts(&self, label: &str) -> usize {
        let wanted = format!("refuse {label}");
        self.loader
            .journal()
            .iter()
            .filter(|l| **l == wanted)
            .count()
    }
}

fn addr(raw: &str) -> FunctionAddress {
    FunctionAddress::parse(raw).unwrap()
}

#[tokio::test]
async fn a_warm_refusal_is_retried_every_cycle_but_logged_once() {
    let (lines, _guard) = capture();
    let r = rig();
    r.loader.refuse("app.svc.fn@1", "WASM_INVALID");
    r.control
        .serve(Answer::Document(fakes::document(vec![fakes::entry(
            "app.svc.fn",
            1,
            "live",
            "warm",
        )])));
    r.reconcile().await;
    r.reconcile().await;
    r.reconcile().await;
    assert_eq!(r.attempts("app.svc.fn@1"), 3, "retried every cycle");
    assert_eq!(count(&lines, "function version refused to load"), 1);
    assert!(
        String::from_utf8_lossy(&lines.lock()).contains("scripted"),
        "the loader's detail reaches the log"
    );

    // A changed refusal is logged again.
    r.loader.refuse("app.svc.fn@1", "WASM_IMPORT_NOT_ALLOWED");
    r.reconcile().await;
    assert_eq!(count(&lines, "function version refused to load"), 2);
}

#[tokio::test]
async fn a_lazy_refusal_is_tried_once_per_cycle_not_per_call() {
    let (lines, _guard) = capture();
    let r = rig();
    r.loader.refuse("app.svc.lazy@1", "WASM_INVALID");
    r.control
        .serve(Answer::Document(fakes::document(vec![fakes::entry(
            "app.svc.lazy",
            1,
            "live",
            "lazy",
        )])));
    r.reconcile().await;
    let a = addr("app.svc.lazy");
    for _ in 0..3 {
        assert!(r.reconciler.ensure_loaded(&a).await.is_none());
    }
    assert_eq!(r.attempts("app.svc.lazy@1"), 1, "not recompiled per call");

    r.reconcile().await;
    assert!(r.reconciler.ensure_loaded(&a).await.is_none());
    assert!(r.reconciler.ensure_loaded(&a).await.is_none());
    assert_eq!(
        r.attempts("app.svc.lazy@1"),
        2,
        "tried again once the next cycle"
    );
    assert_eq!(count(&lines, "function version refused to load"), 1);

    // Fixed: the next cycle's first call loads it.
    r.loader.allow("app.svc.lazy@1");
    r.reconcile().await;
    assert_eq!(r.reconciler.ensure_loaded(&a).await.unwrap().version(), 1);
}

#[tokio::test]
async fn a_pinned_refusal_is_tried_once_per_cycle_and_logged_once() {
    let (lines, _guard) = capture();
    let r = rig();
    r.loader.refuse("app.svc.fn@2", "WASM_INVALID");
    let document = fakes::document(vec![
        fakes::entry("app.svc.fn", 1, "live", "lazy"),
        fakes::entry("app.svc.fn", 2, "candidate", "lazy"),
    ]);
    let candidate = document.entry_for(&addr("app.svc.fn"), 2).unwrap().clone();
    r.control.serve(Answer::Document(document));
    r.reconcile().await;
    for _ in 0..3 {
        assert!(matches!(
            r.reconciler.load_pinned(&candidate).await,
            PinnedLoad::Refused
        ));
    }
    assert_eq!(r.attempts("app.svc.fn@2"), 1);
    r.reconcile().await;
    assert!(matches!(
        r.reconciler.load_pinned(&candidate).await,
        PinnedLoad::Refused
    ));
    assert_eq!(r.attempts("app.svc.fn@2"), 2);
    assert_eq!(count(&lines, "pinned function version refused to load"), 1);
}
