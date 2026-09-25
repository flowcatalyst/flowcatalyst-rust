//! The reconciler's decisions (Java `ReconcilerTest` R1-R13 where they apply
//! to this slice), against a scripted control plane, store and loader.

mod support;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use fc_fnhost_core::artifact::ArtifactError;
use fc_fnhost_core::clock::{ManualClock, SharedClock};
use fc_fnhost_core::heartbeat::HostState;
use fc_fnhost_core::loader::Loaders;
use fc_fnhost_core::reconciler::{Readiness, Reconciler, IDLE_UNLOAD};
use fc_fnhost_core::registry::FunctionRegistry;
use fc_fnhost_core::signature::{SignatureVerifier, Signatures};
use fc_function_abi::FunctionAddress;
use serde_json::json;
use support::fakes::{self, Answer, FakeControlPlane, FakeLoader, FakeStore};

struct Rig {
    control: Arc<FakeControlPlane>,
    store: Arc<FakeStore>,
    loader: Arc<FakeLoader>,
    registry: Arc<FunctionRegistry>,
    reconciler: Arc<Reconciler>,
    clock: ManualClock,
}

fn rig_with(signatures: Signatures, max_loaded: usize) -> Rig {
    let clock = ManualClock::new(Utc.with_ymd_and_hms(2026, 9, 24, 12, 0, 0).unwrap());
    let shared: SharedClock = Arc::new(clock.clone());
    let control = FakeControlPlane::new();
    let store = FakeStore::new();
    let loader = FakeLoader::new();
    let registry = Arc::new(FunctionRegistry::new(max_loaded, shared));
    *loader.registry.lock() = Some(registry.clone());
    let reconciler = Arc::new(Reconciler::new(
        "default",
        "host-1",
        control.clone(),
        store.clone(),
        signatures,
        Loaders::none().with("wasm", loader.clone()),
        registry.clone(),
    ));
    Rig {
        control,
        store,
        loader,
        registry,
        reconciler,
        clock,
    }
}

fn rig() -> Rig {
    rig_with(Signatures::Off, 200)
}

fn addr(raw: &str) -> FunctionAddress {
    FunctionAddress::parse(raw).unwrap()
}

impl Rig {
    async fn reconcile(&self) {
        self.reconciler.reconcile_once(self.clock.now_value()).await;
    }

    fn serve(&self, functions: Vec<serde_json::Value>) {
        self.control
            .serve(Answer::Document(fakes::document(functions)));
    }

    fn states(&self) -> Vec<(String, i32, String)> {
        fakes::states(&self.control.last_heartbeat())
    }

    fn serving(&self, address: &str) -> Option<i32> {
        self.registry.peek(&addr(address)).map(|f| f.version())
    }
}

trait NowValue {
    fn now_value(&self) -> chrono::DateTime<Utc>;
}
impl NowValue for ManualClock {
    fn now_value(&self) -> chrono::DateTime<Utc> {
        fc_fnhost_core::clock::Clock::now(self)
    }
}

fn st(address: &str, version: i32, state: &str) -> (String, i32, String) {
    (address.to_owned(), version, state.to_owned())
}

// ── R1: warm, lazy, candidate ────────────────────────────────────────────

#[tokio::test]
async fn warm_live_loads_lazy_live_waits_candidate_never_loads() {
    let r = rig();
    r.serve(vec![
        fakes::entry("app.svc.warm", 1, "live", "warm"),
        fakes::entry("app.svc.lazy", 1, "live", "lazy"),
        fakes::entry("app.svc.warm", 2, "candidate", "warm"),
    ]);
    r.reconcile().await;
    assert_eq!(r.loader.loads(), ["load app.svc.warm@1"]);
    assert_eq!(
        r.states(),
        [
            st("app.svc.warm", 1, "LOADED"),
            st("app.svc.lazy", 1, "REGISTERED"),
            st("app.svc.warm", 2, "REGISTERED"),
        ]
    );
    assert!(r.reconciler.is_lazily_routed(&addr("app.svc.lazy")));
    assert!(
        !r.reconciler.is_lazily_routed(&addr("app.svc.warm")),
        "a candidate is never routed"
    );
}

#[tokio::test]
async fn ensure_loaded_loads_a_lazy_function_once_under_concurrent_first_calls() {
    let r = rig();
    *r.loader.delay.lock() = Some(Duration::from_millis(30));
    r.serve(vec![fakes::entry("app.svc.lazy", 1, "live", "lazy")]);
    r.reconcile().await;
    let a = addr("app.svc.lazy");
    let (one, two) = tokio::join!(
        r.reconciler.ensure_loaded(&a),
        r.reconciler.ensure_loaded(&a)
    );
    assert_eq!(one.unwrap().version(), 1);
    assert_eq!(two.unwrap().version(), 1);
    assert_eq!(r.loader.loads(), ["load app.svc.lazy@1"]);
    r.reconcile().await;
    assert_eq!(r.states(), [st("app.svc.lazy", 1, "LOADED")]);
}

#[tokio::test]
async fn ensure_loaded_serves_live_not_a_candidate() {
    let r = rig();
    r.serve(vec![
        fakes::entry("app.svc.fn", 1, "live", "lazy"),
        fakes::entry("app.svc.fn", 2, "candidate", "lazy"),
    ]);
    r.reconcile().await;
    assert_eq!(
        r.reconciler
            .ensure_loaded(&addr("app.svc.fn"))
            .await
            .unwrap()
            .version(),
        1
    );
    assert!(r
        .reconciler
        .ensure_loaded(&addr("app.svc.none"))
        .await
        .is_none());
}

// ── R2: new before old ───────────────────────────────────────────────────

#[tokio::test]
async fn a_promote_registers_the_new_version_before_closing_the_old() {
    let r = rig();
    r.serve(vec![fakes::entry("app.svc.fn", 1, "live", "warm")]);
    r.reconcile().await;
    r.serve(vec![
        fakes::entry("app.svc.fn", 2, "live", "warm"),
        fakes::entry("app.svc.fn", 1, "alias", "warm"),
    ]);
    r.reconcile().await;
    assert_eq!(
        r.loader.journal(),
        [
            "load app.svc.fn@1",
            "load app.svc.fn@2",
            "close app.svc.fn@1"
        ]
    );
    let v1 = r.loader.instance("app.svc.fn@1");
    assert_eq!(
        *v1.served_at_close.lock(),
        Some(Some(2)),
        "v2 was serving when v1 closed"
    );
    assert_eq!(r.serving("app.svc.fn"), Some(2));
}

// ── R3: failures leave the old version serving and are retried ───────────

#[tokio::test]
async fn a_failed_prepare_leaves_the_old_version_serving_and_is_retried() {
    let r = rig();
    r.serve(vec![fakes::entry("app.svc.fn", 1, "live", "warm")]);
    r.reconcile().await;
    r.store.fail(
        "mem://app.svc.fn/2",
        ArtifactError::DigestMismatch {
            expected: fakes::digest_for("app.svc.fn", 2),
            actual: fakes::digest_for("x", 0),
        },
    );
    r.serve(vec![fakes::entry("app.svc.fn", 2, "live", "warm")]);
    r.reconcile().await;
    assert_eq!(r.serving("app.svc.fn"), Some(1));
    assert_eq!(
        r.states(),
        [st("app.svc.fn", 2, "FAILED:ARTIFACT:DigestMismatch")]
    );
    assert!(!r
        .loader
        .instance("app.svc.fn@1")
        .closed
        .load(Ordering::SeqCst));

    r.store.heal("mem://app.svc.fn/2");
    r.reconcile().await;
    assert_eq!(r.serving("app.svc.fn"), Some(2));
    assert_eq!(r.states(), [st("app.svc.fn", 2, "LOADED")]);
}

#[tokio::test]
async fn a_refused_load_leaves_the_old_version_serving_and_is_retried() {
    let r = rig();
    r.serve(vec![fakes::entry("app.svc.fn", 1, "live", "warm")]);
    r.reconcile().await;
    r.loader.refuse("app.svc.fn@2", "WASM_INVALID");
    r.serve(vec![fakes::entry("app.svc.fn", 2, "live", "warm")]);
    r.reconcile().await;
    assert_eq!(r.serving("app.svc.fn"), Some(1));
    assert_eq!(
        r.states(),
        [st("app.svc.fn", 2, "FAILED:LOAD:WASM_INVALID")]
    );
    r.loader.allow("app.svc.fn@2");
    r.reconcile().await;
    assert_eq!(r.serving("app.svc.fn"), Some(2));
}

/// Owner ruling 12 (Java 8a130505): a pinned version is `Preparing` while
/// desired but neither prepared nor refused, `Refused` once a failure is
/// recorded (preparing or loading), and loads once prepared.
#[tokio::test]
async fn a_pinned_version_is_preparing_until_it_is_prepared_or_refused() {
    use fc_fnhost_core::reconciler::PinnedLoad;

    let r = rig();
    let doc = fakes::document(vec![
        fakes::entry("app.svc.good", 2, "candidate", "lazy"),
        fakes::entry("app.svc.bad", 9, "candidate", "lazy"),
        fakes::entry("app.svc.odd", 3, "candidate", "lazy"),
    ]);
    let good = doc.entry_for(&addr("app.svc.good"), 2).unwrap().clone();
    let bad = doc.entry_for(&addr("app.svc.bad"), 9).unwrap().clone();
    let odd = doc.entry_for(&addr("app.svc.odd"), 3).unwrap().clone();

    for entry in [&good, &bad] {
        assert!(r.reconciler.is_preparing(entry));
        assert!(matches!(
            r.reconciler.load_pinned(entry).await,
            PinnedLoad::Preparing
        ));
    }

    r.store.fail(
        "mem://app.svc.bad/9",
        ArtifactError::DigestMismatch {
            expected: fakes::digest_for("app.svc.bad", 9),
            actual: fakes::digest_for("x", 0),
        },
    );
    r.loader.refuse("app.svc.odd@3", "WASM_INVALID");
    r.control.serve(Answer::Document(doc));
    r.reconcile().await;

    assert!(!r.reconciler.is_preparing(&good), "prepared");
    assert!(matches!(
        r.reconciler.load_pinned(&good).await,
        PinnedLoad::Loaded(f) if f.version() == 2
    ));
    assert!(!r.reconciler.is_preparing(&bad), "refused: digest mismatch");
    assert!(matches!(
        r.reconciler.load_pinned(&bad).await,
        PinnedLoad::Refused
    ));
    assert!(
        matches!(r.reconciler.load_pinned(&odd).await, PinnedLoad::Refused),
        "prepared, but the loader refuses it"
    );
}

#[tokio::test]
async fn a_full_registry_is_a_failed_entry_not_a_crash() {
    let r = rig_with(Signatures::Off, 1);
    r.serve(vec![
        fakes::entry("app.svc.one", 1, "live", "warm"),
        fakes::entry("app.svc.two", 1, "live", "warm"),
    ]);
    r.reconcile().await;
    assert_eq!(
        r.states(),
        [
            st("app.svc.one", 1, "LOADED"),
            st("app.svc.two", 1, "FAILED:LOAD:REGISTRY_FULL")
        ]
    );
    assert!(
        r.loader
            .instance("app.svc.two@1")
            .closed
            .load(Ordering::SeqCst),
        "never registered, so closed"
    );
}

// ── runtimes without a loader ────────────────────────────────────────────

#[tokio::test]
async fn a_runtime_without_a_loader_is_runtime_unsupported_and_never_fetched() {
    let r = rig();
    let mut jvm = fakes::entry("app.svc.jar", 1, "live", "warm");
    jvm["manifest"] = json!({"runtime": "JVM", "entrypoint": "com.example.Fn"});
    r.serve(vec![jvm, fakes::entry("app.svc.wasm", 1, "live", "warm")]);
    r.reconcile().await;
    assert_eq!(
        r.states(),
        [
            st("app.svc.jar", 1, "FAILED:RUNTIME_UNSUPPORTED"),
            st("app.svc.wasm", 1, "LOADED")
        ]
    );
    assert_eq!(*r.store.fetches.lock(), ["mem://app.svc.wasm/1"]);
    assert_eq!(r.loader.loads(), ["load app.svc.wasm@1"]);
}

// ── R4/R5: signatures ────────────────────────────────────────────────────

mod signatures {
    use super::*;
    use support::sigstore::{self as ts, LeafSpec};

    fn when() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 3, 19, 17, 27, 26).unwrap()
    }

    fn signed(
        eco: &ts::Ecosystem,
        address: &str,
        version: i32,
        signer: (&str, &str),
    ) -> serde_json::Value {
        let mut e = fakes::entry(address, version, "live", "warm");
        let digest = fakes::digest_for(address, version).bytes();
        e["signatureBundle"] = json!(ts::valid_bundle_json(eco, digest, when(), 7));
        e["signer"] = json!({"issuer": signer.0, "subject": signer.1});
        e
    }

    fn required(eco: &ts::Ecosystem) -> Signatures {
        let from = when() - chrono::Duration::days(1);
        Signatures::Required(Arc::new(SignatureVerifier::new(
            eco.trust_root_for(from, None, from, None),
        )))
    }

    const ISSUER: &str = "https://example.test/issuer";
    const SUBJECT: &str = "https://example.test/workflow.yml";

    #[tokio::test]
    async fn required_loads_only_a_verified_bundle_from_the_recorded_signer() {
        let eco = ts::build(&LeafSpec::valid(
            when() - chrono::Duration::minutes(5),
            when() + chrono::Duration::minutes(5),
        ));
        let r = rig_with(required(&eco), 200);
        let unsigned = fakes::entry("app.svc.unsigned", 1, "live", "warm");
        let mut no_signer = signed(&eco, "app.svc.nosigner", 1, (ISSUER, SUBJECT));
        no_signer.as_object_mut().unwrap().remove("signer");
        let mut bad = signed(&eco, "app.svc.bad", 1, (ISSUER, SUBJECT));
        // a bundle for a different artifact: the digest check fails
        bad["signatureBundle"] = json!(ts::valid_bundle_json(&eco, [9; 32], when(), 7));
        let other_subject = signed(
            &eco,
            "app.svc.other",
            1,
            (ISSUER, "https://example.test/someone-else.yml"),
        );
        let other_issuer = signed(
            &eco,
            "app.svc.issuer",
            1,
            ("https://other.example/issuer", SUBJECT),
        );
        let good = signed(&eco, "app.svc.good", 1, (ISSUER, SUBJECT));
        r.serve(vec![
            unsigned,
            no_signer,
            bad,
            other_subject,
            other_issuer,
            good,
        ]);
        r.reconcile().await;
        assert_eq!(
            r.states(),
            [
                st("app.svc.unsigned", 1, "FAILED:UNSIGNED"),
                st("app.svc.nosigner", 1, "FAILED:UNSIGNED"),
                st("app.svc.bad", 1, "FAILED:SIGNATURE:DIGEST_MISMATCH"),
                st("app.svc.other", 1, "FAILED:SIGNER_MISMATCH"),
                st("app.svc.issuer", 1, "FAILED:SIGNER_MISMATCH"),
                st("app.svc.good", 1, "LOADED"),
            ]
        );
        assert_eq!(r.loader.loads(), ["load app.svc.good@1"]);
    }

    #[tokio::test]
    async fn a_bad_signature_is_reported_with_its_reason() {
        let eco = ts::build(&LeafSpec::valid(
            when() - chrono::Duration::minutes(5),
            when() + chrono::Duration::minutes(5),
        ));
        let r = rig_with(required(&eco), 200);
        let mut e = signed(&eco, "app.svc.fn", 1, (ISSUER, SUBJECT));
        let digest = fakes::digest_for("app.svc.fn", 1).bytes();
        e["signatureBundle"] = json!(ts::bundle_json(
            &eco,
            digest,
            when(),
            7,
            ts::BundleOptions {
                sign_over_digest: Some([3; 32]),
                ..Default::default()
            }
        ));
        r.serve(vec![e]);
        r.reconcile().await;
        assert_eq!(
            r.states(),
            [st("app.svc.fn", 1, "FAILED:SIGNATURE:BAD_SIGNATURE")]
        );
    }

    #[tokio::test]
    async fn off_loads_an_unsigned_entry() {
        let r = rig();
        r.serve(vec![fakes::entry("app.svc.unsigned", 1, "live", "warm")]);
        r.reconcile().await;
        assert_eq!(r.states(), [st("app.svc.unsigned", 1, "LOADED")]);
    }
}

// ── R6: outages and not-modified ─────────────────────────────────────────

#[tokio::test]
async fn an_outage_unloads_nothing_and_still_heartbeats() {
    let r = rig();
    r.serve(vec![fakes::entry("app.svc.fn", 1, "live", "warm")]);
    r.reconcile().await;
    let beats = r.control.heartbeat_count();
    r.control.serve(Answer::Down);
    r.clock.advance(IDLE_UNLOAD * 3);
    r.reconcile().await;
    assert_eq!(r.serving("app.svc.fn"), Some(1));
    assert_eq!(r.control.heartbeat_count(), beats + 1);
    assert_eq!(r.states(), [st("app.svc.fn", 1, "LOADED")]);
    assert_eq!(
        r.reconciler.readiness(true, true),
        Readiness::Ready,
        "stays ready through a later outage"
    );
}

#[tokio::test]
async fn a_first_outage_sends_no_heartbeat_because_nothing_is_known() {
    let r = rig();
    r.control.serve(Answer::Down);
    r.reconcile().await;
    assert_eq!(r.control.heartbeat_count(), 0);
    assert_eq!(
        r.reconciler.readiness(true, true),
        Readiness::PlatformUnreachable
    );
}

#[tokio::test]
async fn not_modified_sends_the_etag_reloads_nothing_and_heartbeats() {
    let r = rig();
    r.serve(vec![fakes::entry("app.svc.fn", 1, "live", "warm")]);
    r.reconcile().await;
    r.control.serve(Answer::NotModified);
    r.reconcile().await;
    assert_eq!(
        *r.control.etags_sent.lock(),
        [None, Some("\"etag-0\"".to_owned())]
    );
    assert_eq!(r.loader.loads(), ["load app.svc.fn@1"]);
    assert_eq!(r.store.fetch_count(), 1);
    assert_eq!(r.control.heartbeat_count(), 2);
}

// ── R7: unloading ────────────────────────────────────────────────────────

/// Owner decision 5: the host derives what to unload from what it holds
/// and what the document names, with no `unload` list at all. An address
/// gone from the document closes. One whose live version moved on to a
/// version that cannot be fetched keeps serving the old one (new before
/// old), where a platform `unload` list naming v1 used to close it.
#[tokio::test]
async fn what_is_no_longer_live_closes_without_an_unload_list() {
    let r = rig();
    r.serve(vec![
        fakes::entry("app.svc.moved", 1, "live", "warm"),
        fakes::entry("app.svc.gone", 1, "live", "warm"),
        fakes::entry("app.svc.kept", 1, "live", "warm"),
    ]);
    r.reconcile().await;
    r.store
        .fail("mem://app.svc.moved/2", ArtifactError::NotFound);
    r.serve(vec![
        fakes::entry("app.svc.moved", 2, "live", "warm"),
        fakes::entry("app.svc.kept", 1, "live", "warm"),
    ]);
    r.reconcile().await;
    for (instance, closed) in [
        ("app.svc.moved@1", false),
        ("app.svc.gone@1", true),
        ("app.svc.kept@1", false),
    ] {
        assert_eq!(
            r.loader.instance(instance).closed.load(Ordering::SeqCst),
            closed,
            "{instance}"
        );
    }
    assert_eq!(r.serving("app.svc.gone"), None);
    assert_eq!(r.serving("app.svc.moved"), Some(1));
    assert!(!r.reconciler.has_load_lock(&addr("app.svc.gone")));
    assert_eq!(
        r.states(),
        [
            st("app.svc.moved", 2, "FAILED:ARTIFACT:NotFound"),
            st("app.svc.kept", 1, "LOADED")
        ]
    );
}

/// The document's `unload` list is not acted on: naming the version that
/// is still live closes nothing (the platform keeps sending the list for a
/// release, for JVM hosts).
#[tokio::test]
async fn the_documents_unload_list_is_ignored() {
    let r = rig();
    r.serve(vec![fakes::entry("app.svc.fn", 1, "live", "warm")]);
    r.reconcile().await;
    r.control
        .serve(Answer::Document(fakes::document_json(json!({
            "functions": [fakes::entry("app.svc.fn", 1, "live", "warm")],
            "unload": [{"address": "app.svc.fn", "version": 1}]
        }))));
    r.reconcile().await;
    assert_eq!(r.serving("app.svc.fn"), Some(1));
    assert!(!r
        .loader
        .instance("app.svc.fn@1")
        .closed
        .load(Ordering::SeqCst));
    assert_eq!(r.states(), [st("app.svc.fn", 1, "LOADED")]);
}

/// A version the document stops naming (a candidate that was retired, say)
/// is forgotten: its failure is dropped and its prepared artifact too, so
/// naming it again fetches it again.
#[tokio::test]
async fn a_version_the_document_stops_naming_is_forgotten() {
    let r = rig();
    r.store.fail("mem://app.svc.fn/2", ArtifactError::NotFound);
    r.serve(vec![
        fakes::entry("app.svc.fn", 1, "live", "warm"),
        fakes::entry("app.svc.fn", 2, "candidate", "lazy"),
        fakes::entry("app.svc.fn", 3, "candidate", "lazy"),
    ]);
    r.reconcile().await;
    assert!(r.reconciler.failure(&addr("app.svc.fn"), 2).is_some());
    let fetches = r.store.fetch_count();
    r.serve(vec![fakes::entry("app.svc.fn", 1, "live", "warm")]);
    r.reconcile().await;
    assert_eq!(r.reconciler.failure(&addr("app.svc.fn"), 2), None);
    assert_eq!(r.serving("app.svc.fn"), Some(1), "live is untouched");
    r.serve(vec![
        fakes::entry("app.svc.fn", 1, "live", "warm"),
        fakes::entry("app.svc.fn", 3, "candidate", "lazy"),
    ]);
    r.reconcile().await;
    assert_eq!(
        r.store.fetch_count(),
        fetches + 1,
        "version 3 was forgotten, so it is fetched again"
    );
}

#[tokio::test]
async fn a_resident_lazy_function_is_replaced_when_live_moves_on() {
    let r = rig();
    r.serve(vec![fakes::entry("app.svc.fn", 1, "live", "lazy")]);
    r.reconcile().await;
    r.reconciler
        .ensure_loaded(&addr("app.svc.fn"))
        .await
        .unwrap();
    r.serve(vec![fakes::entry("app.svc.fn", 2, "live", "lazy")]);
    r.reconcile().await;
    assert_eq!(r.serving("app.svc.fn"), Some(2));
    assert_eq!(
        r.loader.journal(),
        [
            "load app.svc.fn@1",
            "load app.svc.fn@2",
            "close app.svc.fn@1"
        ]
    );
}

#[tokio::test]
async fn an_idle_lazy_function_closes_after_an_hour_keeps_its_route_and_reloads() {
    let r = rig();
    r.serve(vec![
        fakes::entry("app.svc.fn", 1, "live", "lazy"),
        fakes::entry("app.svc.warm", 1, "live", "warm"),
    ]);
    r.reconcile().await;
    r.reconciler
        .ensure_loaded(&addr("app.svc.fn"))
        .await
        .unwrap();
    r.clock.advance(IDLE_UNLOAD - chrono::Duration::seconds(1));
    r.reconcile().await;
    assert_eq!(r.serving("app.svc.fn"), Some(1), "not idle long enough");
    r.clock.advance(chrono::Duration::seconds(2));
    r.reconcile().await;
    assert_eq!(r.serving("app.svc.fn"), None);
    assert_eq!(
        r.serving("app.svc.warm"),
        Some(1),
        "warm functions never idle out"
    );
    assert!(r.reconciler.is_lazily_routed(&addr("app.svc.fn")));
    assert_eq!(
        r.states(),
        [
            st("app.svc.fn", 1, "REGISTERED"),
            st("app.svc.warm", 1, "LOADED")
        ]
    );
    assert_eq!(
        r.reconciler
            .ensure_loaded(&addr("app.svc.fn"))
            .await
            .unwrap()
            .version(),
        1
    );
    assert_eq!(r.loader.loads().len(), 3);
}

// ── R8: an unreadable entry is reported and protects its address ─────────

#[tokio::test]
async fn an_unreadable_entry_is_reported_and_never_unloads_its_address() {
    let r = rig();
    r.serve(vec![
        fakes::entry("app.svc.fn", 1, "live", "warm"),
        fakes::entry("app.svc.other", 1, "live", "warm"),
    ]);
    r.reconcile().await;
    let mut unreadable = fakes::entry("app.svc.fn", 2, "live", "warm");
    unreadable["digest"] = json!("md5:nope");
    r.serve(vec![
        unreadable,
        fakes::entry("app.svc.other", 2, "live", "warm"),
    ]);
    r.reconcile().await;
    assert_eq!(
        r.serving("app.svc.fn"),
        Some(1),
        "a parse failure is not evidence the function is gone"
    );
    assert_eq!(
        r.serving("app.svc.other"),
        Some(2),
        "the rest of the document applies"
    );
    let states = r.states();
    assert_eq!(states[0], st("app.svc.other", 2, "LOADED"));
    assert_eq!(states[1].0, "app.svc.fn");
    assert!(
        states[1]
            .2
            .starts_with("FAILED:UNREADABLE:validation: DIGEST_INVALID"),
        "{states:?}"
    );
}

// ── settings fingerprint ─────────────────────────────────────────────────

#[tokio::test]
async fn a_settings_change_reloads_the_same_version_new_before_old() {
    let r = rig();
    let mut e = fakes::entry("app.svc.fn", 1, "live", "warm");
    e["config"] = json!({"greeting": "hello"});
    r.serve(vec![e.clone()]);
    r.reconcile().await;
    r.serve(vec![e.clone()]);
    r.reconcile().await;
    assert_eq!(
        r.loader.loads().len(),
        1,
        "unchanged settings do not reload"
    );
    e["secrets"] = json!({"token": "s3cr3t"});
    r.serve(vec![e]);
    r.reconcile().await;
    assert_eq!(
        r.loader.journal(),
        [
            "load app.svc.fn@1",
            "load app.svc.fn@1",
            "close app.svc.fn@1"
        ]
    );
    assert_eq!(r.serving("app.svc.fn"), Some(1));
}

// ── webhook signing secrets ──────────────────────────────────────────────

#[tokio::test]
async fn a_webhook_endpoint_without_a_secret_is_failed_but_still_loaded() {
    let r = rig();
    let mut e = fakes::entry("app.svc.fn", 1, "live", "warm");
    e["manifest"]["endpoints"] = json!([{"path": "/events", "auth": "webhook"}]);
    r.serve(vec![e.clone()]);
    r.reconcile().await;
    assert_eq!(
        r.states(),
        [st("app.svc.fn", 1, "FAILED:NO_SIGNING_SECRET")]
    );
    assert_eq!(r.serving("app.svc.fn"), Some(1));
    e["webhookSigningSecret"] = json!("whsec_1");
    r.serve(vec![e]);
    r.reconcile().await;
    assert_eq!(r.states(), [st("app.svc.fn", 1, "LOADED")]);
}

#[tokio::test]
async fn a_rotated_webhook_secret_stays_accepted_until_the_second_reconcile() {
    let r = rig();
    let a = addr("app.svc.fn");
    let mut e = fakes::entry("app.svc.fn", 1, "live", "warm");
    e["webhookSigningSecret"] = json!("old");
    r.serve(vec![e.clone()]);
    r.reconcile().await;
    e["webhookSigningSecret"] = json!("new");
    r.serve(vec![e]);
    r.reconcile().await; // the change lands on reconcile 2
    assert_eq!(
        r.reconciler.current_webhook_secret(&a).as_deref(),
        Some("new")
    );
    assert_eq!(
        r.reconciler.previous_webhook_secret(&a).as_deref(),
        Some("old")
    );
    r.control.serve(Answer::NotModified);
    r.reconcile().await; // 3
    assert_eq!(
        r.reconciler.previous_webhook_secret(&a).as_deref(),
        Some("old")
    );
    r.reconcile().await; // 4
    assert_eq!(r.reconciler.previous_webhook_secret(&a), None);
}

// ── readiness and draining ───────────────────────────────────────────────

#[tokio::test]
async fn readiness_follows_javas_precedence() {
    let r = rig();
    assert_eq!(r.reconciler.readiness(true, true), Readiness::Starting);
    r.serve(vec![]);
    r.reconcile().await;
    assert_eq!(r.reconciler.readiness(true, true), Readiness::Ready);
    assert_eq!(
        r.reconciler.readiness(false, false),
        Readiness::ListenerDown
    );
    assert_eq!(
        r.reconciler.readiness(true, false),
        Readiness::ReconcilerDown
    );
    r.reconciler.drain();
    assert_eq!(r.reconciler.readiness(false, false), Readiness::Draining);
    r.reconcile().await;
    assert_eq!(r.control.last_heartbeat().state, HostState::Draining);
}
