//! Runner for the vendored mediation conformance corpus,
//! `conformance/mediation-outcomes.json` (copied from `flowcatalyst-javalin`;
//! see `conformance/PROVENANCE.md`).
//!
//! Read `conformance/README.md` first. The corpus states what a router
//! **should** do with a mediation response. Where Go does something else and
//! the corpus calls that a defect, the corpus wins (owner decision #29), and
//! the row is listed in `docs/parity/router-deviations-from-go.md`.
//!
//! # Black-box, end to end
//!
//! Every case runs through the router's real delivery path, not a call into
//! the classifier:
//!
//! 1. A local HTTP/1.1 server answers every request with the case's
//!    `given` status, body and headers. It is a raw TCP server rather than
//!    wiremock so it can send a bare `1xx` status line and hang up, which is
//!    what `unexpected-status-1xx` means on the wire.
//! 2. A real [`HttpMediator`] (dev preset, its real in-call retry burst) with
//!    its own breaker registry and warning service.
//! 3. A real [`ProcessPool`]. Two messages are submitted to one ordered
//!    group (`NEXT_ON_ERROR`): the case's *head*, and a *sibling* queued
//!    behind it. The sibling makes the group half of `disposition`
//!    observable: a message retried in place keeps its position, and a
//!    message returned to the broker takes its group with it.
//! 4. A recording broker callback, so the runner sees exactly what reached
//!    the broker: ack, nack (with delay), or nothing yet.
//!
//! The mediator is wrapped in a pass-through recorder that snapshots the
//! breaker counters and the warning store around each call, so `breaker` and
//! `warning` are measured for the head's own delivery only.
//!
//! # `disposition`
//!
//! Read from what the broker saw for the head:
//!
//! | broker saw | disposition |
//! |---|---|
//! | ack, outcome `Success` | `DELIVERED` |
//! | ack, any other outcome | `REJECTED` or `UNDELIVERABLE` (see below) |
//! | nack | `RETURN_TO_BROKER` |
//! | nothing, and the pool still holds it | `RETRY_IN_PLACE` |
//!
//! `REJECTED` and `UNDELIVERABLE` are both "acknowledged away"; the broker
//! cannot tell them apart and neither can Go's `Disposition`. Go's runner
//! (`internal/router/mediation_conformance_test.go`, `assertDisposition`)
//! folds them into one bucket, and so does this one.
//!
//! The broker callback answers `honours_delayed_return() == false`. That
//! matches how both other runners read the column: Java asserts the outcome's
//! own `disposition()`, and Go calls `DispositionOf(…, false)`. The corpus
//! pins the outcome's classification, not the owner's later R1 hand-back
//! (2026-09-17), which returns a delay-bearing deferral to a broker that can
//! hold it. [`deferral_naming_a_delay_goes_back_to_a_broker_that_honours_it`]
//! pins R1 separately.
//!
//! # A case that cannot run as written
//!
//! `unsupported-mediation-type` needs a message whose mediation type is not
//! HTTP. Owner rulings X-06 and X-10 make `fc_common::MediationType` a closed
//! enum with no catch-all, so such a message is refused when it is parsed,
//! before it reaches a pool. The runner checks that refusal instead and
//! reports the case as `RULED`. See the deviations doc for the warning
//! this leaves to the consumer.
//!
//! Any other mismatch fails the test, naming the case id and the field.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::net::TcpListener as StdTcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fc_common::{
    BatchMessage, DispatchMode, EnhancedPoolMetrics, MediationOutcome, MediationResult,
    MediationType, Message, MessageCallback, PoolConfig, WarningCategory, WarningSeverity,
};
use fc_router::{
    breaker_key, CircuitBreakerConfig, CircuitBreakerRegistry, CircuitBreakerState, HttpMediator,
    HttpMediatorConfig, Mediator, ProcessPool, WarningService, WarningServiceConfig,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ---------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

#[derive(Debug, Clone, Deserialize)]
struct Case {
    id: String,
    given: Given,
    expect: Expect,
}

#[derive(Debug, Clone, Deserialize)]
struct Given {
    kind: String,
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expect {
    outcome: String,
    status_code: u16,
    #[serde(default)]
    delay_seconds: Option<u32>,
    #[serde(default)]
    flush_group: Option<bool>,
    #[serde(default)]
    http_call_made: Option<bool>,
    warning: String,
    disposition: String,
    breaker: String,
    metric: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The vendored copy, unless `FC_CONFORMANCE_CORPUS` names another.
fn corpus_path() -> PathBuf {
    match std::env::var("FC_CONFORMANCE_CORPUS") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => repo_root().join("conformance/mediation-outcomes.json"),
    }
}

fn load_raw() -> String {
    let path = corpus_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read conformance corpus {}: {e}", path.display()))
}

fn load_corpus() -> Corpus {
    let corpus: Corpus = serde_json::from_str(&load_raw()).expect("corpus parses");
    // A corpus that silently shrinks to nothing would pass, loudly wrong.
    assert!(!corpus.cases.is_empty(), "conformance corpus has no cases");
    corpus
}

// ---------------------------------------------------------------------
// Corpus self-check (Java's `divergencesAreArgued` + Go's
// `corpus_selfcheck.go`)
// ---------------------------------------------------------------------

const KINDS: &[&str] = &[
    "response",
    "unreachableTarget",
    "malformedTargetUrl",
    "unsupportedMediationType",
    "breakerOpen",
];
const OUTCOMES: &[&str] = &[
    "Success",
    "Deferred",
    "ErrorConfig",
    "ErrorProcess",
    "ErrorConnection",
    "RateLimited",
    "CircuitOpen",
];
const DISPOSITIONS: &[&str] = &[
    "DELIVERED",
    "RETRY_IN_PLACE",
    "RETURN_TO_BROKER",
    "REJECTED",
    "UNDELIVERABLE",
];
const BREAKERS: &[&str] = &["success", "failure", "neither", "none"];
const METRICS: &[&str] = &["success", "failure", "transient", "rateLimited", "none"];
const WARNINGS: &[&str] = &["none", "ERROR", "CRITICAL"];

#[test]
fn corpus_is_well_formed() {
    let raw = load_raw();
    let root: Value = serde_json::from_str(&raw).expect("corpus is JSON");
    let cases = root["cases"]
        .as_array()
        .expect("corpus has a `cases` array");
    assert!(!cases.is_empty(), "corpus has no cases");

    let mut problems = Vec::new();
    let mut ids = HashSet::new();
    for (i, case) in cases.iter().enumerate() {
        let id = case["id"].as_str().unwrap_or("").to_string();
        let at = if id.is_empty() {
            format!("case[{i}]")
        } else {
            format!("case {id:?}")
        };
        if id.is_empty() {
            problems.push(format!("{at}: missing id"));
        } else if !ids.insert(id.clone()) {
            problems.push(format!("{at}: duplicate id"));
        }

        match case["given"]["kind"].as_str() {
            Some(k) if KINDS.contains(&k) => {}
            other => problems.push(format!("{at}: unknown given.kind {other:?}")),
        }
        if case["given"]["kind"] == "response" && case["given"]["status"].as_u64().is_none() {
            problems.push(format!("{at}: a response case needs given.status"));
        }

        let expect = &case["expect"];
        for (field, allowed) in [
            ("outcome", OUTCOMES),
            ("disposition", DISPOSITIONS),
            ("breaker", BREAKERS),
            ("metric", METRICS),
            ("warning", WARNINGS),
        ] {
            match expect[field].as_str() {
                Some(v) if allowed.contains(&v) => {}
                other => problems.push(format!("{at}: expect.{field} is {other:?}")),
            }
        }
        if expect["statusCode"].as_u64().is_none() {
            problems.push(format!("{at}: expect.statusCode missing"));
        }

        // An unargued divergence has, in practice, picked a side without
        // saying why — the bias the corpus exists to avoid.
        if let Some(d) = case.get("divergence") {
            match (d["correct"].as_str(), d["basis"].as_str()) {
                (Some("java" | "go" | "both"), Some(basis)) if !basis.is_empty() => {}
                _ => problems.push(format!(
                    "{at}: divergence needs `correct` (java/go/both) and a `basis`"
                )),
            }
        }
    }
    assert!(
        problems.is_empty(),
        "corpus problems:\n{}",
        problems.join("\n")
    );

    // Drift notice only: the vendored copy is what this repo asserts, and a
    // newer Java corpus must be adopted deliberately (PROVENANCE.md).
    let sibling = repo_root().join("../flowcatalyst-javalin/conformance/mediation-outcomes.json");
    if let Ok(theirs) = std::fs::read_to_string(&sibling) {
        if theirs != raw {
            eprintln!(
                "NOTICE: {} differs from the corpus this run used ({}). \
                 Re-vendor it deliberately; see conformance/PROVENANCE.md.",
                sibling.display(),
                corpus_path().display()
            );
        }
    }
}

// ---------------------------------------------------------------------
// The target: a raw HTTP/1.1 server answering every request with `given`
// ---------------------------------------------------------------------

struct TargetServer {
    url: String,
    /// `messageId` of every request that reached the server, in order.
    requests: Arc<parking_lot::Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TargetServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TargetServer {
    async fn start(given: Given) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind target");
        let addr = listener.local_addr().expect("target addr");
        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let seen = requests.clone();
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let given = given.clone();
                let seen = seen.clone();
                tokio::spawn(async move { serve(socket, &given, &seen).await });
            }
        });
        Self {
            url: format!("http://{addr}/hook"),
            requests,
            task,
        }
    }

    fn calls_for(&self, message_id: &str) -> usize {
        self.requests
            .lock()
            .iter()
            .filter(|id| id.as_str() == message_id)
            .count()
    }
}

async fn serve(mut socket: TcpStream, given: &Given, seen: &parking_lot::Mutex<Vec<String>>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let content_length = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body_end = buf.len().min(header_end + content_length);
    let message_id = serde_json::from_slice::<Value>(&buf[header_end..body_end])
        .ok()
        .and_then(|v| v["messageId"].as_str().map(str::to_string))
        .unwrap_or_default();
    seen.lock().push(message_id);

    let (status, body, headers) = if given.kind == "response" {
        (
            given.status.unwrap_or(200),
            given.body.clone().unwrap_or_default(),
            given.headers.clone(),
        )
    } else {
        // Only `breakerOpen` points at this server, and it must never be
        // called; a 200 here would surface as an unexpected call.
        (200, String::new(), HashMap::new())
    };

    if (100..200).contains(&status) {
        // A bare informational status line and nothing after it — what a
        // target misbehaving this way sends on the wire.
        let _ = socket
            .write_all(format!("HTTP/1.1 {status} Informational\r\n\r\n").as_bytes())
            .await;
        let _ = socket.shutdown().await;
        return;
    }

    let mut response = format!(
        "HTTP/1.1 {status} Conformance\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in &headers {
        response.push_str(&format!("{k}: {v}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(&body);
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

// ---------------------------------------------------------------------
// Recorders: mediator pass-through and broker callback
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Delivery {
    message_id: String,
    outcome: MediationOutcome,
    /// (successes, failures) delta on this target's breaker across the call.
    breaker_delta: (u64, u64),
    /// Warnings raised during the call.
    warnings: Vec<(WarningSeverity, WarningCategory)>,
    finished_at: Instant,
}

struct RecordingMediator {
    inner: HttpMediator,
    breakers: Arc<CircuitBreakerRegistry>,
    warnings: Arc<WarningService>,
    deliveries: parking_lot::Mutex<Vec<Delivery>>,
}

impl RecordingMediator {
    fn breaker_counts(&self, key: &str) -> (u64, u64) {
        self.breakers
            .get_stats(key)
            .map(|s| (s.successful_calls, s.failed_calls))
            .unwrap_or((0, 0))
    }

    fn deliveries_of(&self, message_id: &str) -> Vec<Delivery> {
        self.deliveries
            .lock()
            .iter()
            .filter(|d| d.message_id == message_id)
            .cloned()
            .collect()
    }
}

#[async_trait]
impl Mediator for RecordingMediator {
    async fn mediate(&self, message: &Message) -> MediationOutcome {
        let key = breaker_key(&message.mediation_target);
        let before = self.breaker_counts(&key);
        let warned_before: HashSet<String> = self
            .warnings
            .get_all_warnings()
            .into_iter()
            .map(|w| w.id)
            .collect();

        let outcome = self.inner.mediate(message).await;

        let after = self.breaker_counts(&key);
        let warnings = self
            .warnings
            .get_all_warnings()
            .into_iter()
            .filter(|w| !warned_before.contains(&w.id))
            .map(|w| (w.severity, w.category))
            .collect();
        self.deliveries.lock().push(Delivery {
            message_id: message.id.clone(),
            outcome: outcome.clone(),
            breaker_delta: (
                after.0.saturating_sub(before.0),
                after.1.saturating_sub(before.1),
            ),
            warnings,
            finished_at: Instant::now(),
        });
        outcome
    }

    fn circuit_breaker_registry(&self) -> Option<&Arc<CircuitBreakerRegistry>> {
        Some(&self.breakers)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum BrokerEvent {
    Ack,
    Nack(Option<u32>),
    /// The callback was dropped without an ack or nack.
    Dropped,
}

type BrokerLog = Arc<parking_lot::Mutex<Vec<(String, BrokerEvent)>>>;

struct RecordingCallback {
    message_id: String,
    log: BrokerLog,
    honours_delayed_return: bool,
    settled: AtomicBool,
}

impl RecordingCallback {
    fn record(&self, event: BrokerEvent) {
        self.settled.store(true, Ordering::SeqCst);
        self.log.lock().push((self.message_id.clone(), event));
    }
}

#[async_trait]
impl MessageCallback for RecordingCallback {
    async fn ack(&self) {
        self.record(BrokerEvent::Ack);
    }

    async fn nack(&self, delay_seconds: Option<u32>) {
        self.record(BrokerEvent::Nack(delay_seconds));
    }

    fn honours_delayed_return(&self) -> bool {
        self.honours_delayed_return
    }
}

impl Drop for RecordingCallback {
    fn drop(&mut self) {
        if !self.settled.load(Ordering::SeqCst) {
            self.log
                .lock()
                .push((self.message_id.clone(), BrokerEvent::Dropped));
        }
    }
}

// ---------------------------------------------------------------------
// One case, end to end
// ---------------------------------------------------------------------

const HEAD: &str = "conformance-head";
const SIBLING: &str = "conformance-sibling";
const GROUP: &str = "conformance-group";

/// How long after the head's first delivery the runner waits for the group
/// to settle. Every in-place retry the pool schedules waits at least 5s
/// (the deferred curve's floor; 429s floor at their Retry-After), so a
/// message still held after this long has been kept, not forgotten.
const SETTLE_WINDOW: Duration = Duration::from_secs(3);

fn message(id: &str, target: &str) -> Message {
    Message {
        id: id.to_string(),
        pool_code: "CONFORMANCE".to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: target.to_string(),
        message_group_id: Some(GROUP.to_string()),
        high_priority: false,
        dispatch_mode: DispatchMode::NextOnError,
        dispatch_mode_specified: true,
    }
}

fn batch(id: &str, target: &str, log: &BrokerLog, honours: bool) -> BatchMessage {
    BatchMessage {
        message: message(id, target),
        receipt_handle: format!("receipt-{id}"),
        broker_message_id: Some(format!("broker-{id}")),
        queue_identifier: "conformance-queue".to_string(),
        batch_id: Some(Arc::from("conformance-batch")),
        callback: Box::new(RecordingCallback {
            message_id: id.to_string(),
            log: log.clone(),
            honours_delayed_return: honours,
            settled: AtomicBool::new(false),
        }),
    }
}

/// A free port, released again: nothing listens there when the mediator
/// dials it.
fn unreachable_target() -> String {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    format!("http://127.0.0.1:{port}/hook")
}

/// Everything the runner saw for one case.
struct Observation {
    head: Vec<Delivery>,
    sibling: Vec<Delivery>,
    head_events: Vec<BrokerEvent>,
    sibling_events: Vec<BrokerEvent>,
    head_calls: usize,
    sibling_calls: usize,
    /// Messages the pool still holds (buffered, or waiting out a retry).
    pool_holds: u32,
    metrics: EnhancedPoolMetrics,
}

async fn observe(given: &Given, honours_delayed_return: bool) -> Observation {
    let server = TargetServer::start(given.clone()).await;
    let target = match given.kind.as_str() {
        "unreachableTarget" => unreachable_target(),
        // A target with no host. `http:///no-host` is what the Go and Java
        // runners use, but the WHATWG parser Rust's `url` crate implements
        // reads it as host `no-host`; `http://` is the no-host URL here.
        "malformedTargetUrl" => "http://".to_string(),
        _ => server.url.clone(),
    };

    let breakers = Arc::new(CircuitBreakerRegistry::new(CircuitBreakerConfig::default()));
    let warnings = Arc::new(WarningService::new(WarningServiceConfig::default()));

    if given.kind == "breakerOpen" {
        let key = breaker_key(&target);
        for _ in 0..1000 {
            if breakers.get_state(&key) == Some(CircuitBreakerState::Open) {
                break;
            }
            breakers.allow_request(&key);
            breakers.record_failure(&key);
        }
        assert_eq!(
            breakers.get_state(&key),
            Some(CircuitBreakerState::Open),
            "runner setup: the breaker must be open before delivery"
        );
    }

    let mediator = Arc::new(RecordingMediator {
        inner: HttpMediator::with_config(HttpMediatorConfig::dev())
            .with_warning_service(warnings.clone())
            .with_circuit_breakers(breakers.clone()),
        breakers,
        warnings,
        deliveries: parking_lot::Mutex::new(Vec::new()),
    });
    let pool = ProcessPool::new(
        PoolConfig {
            code: "CONFORMANCE".to_string(),
            concurrency: 2,
            rate_limit_per_minute: None,
        },
        mediator.clone(),
    );
    pool.start().await;

    let log: BrokerLog = Arc::new(parking_lot::Mutex::new(Vec::new()));
    pool.submit(batch(HEAD, &target, &log, honours_delayed_return))
        .await
        .unwrap();
    pool.submit(batch(SIBLING, &target, &log, honours_delayed_return))
        .await
        .unwrap();

    let events_of = |id: &str| -> Vec<BrokerEvent> {
        log.lock()
            .iter()
            .filter(|(m, _)| m == id)
            .map(|(_, e)| e.clone())
            .collect()
    };

    // The head's first delivery. Covers the mediator's full in-call burst
    // (three attempts, 1s + 2s apart) against an unreachable target.
    let deadline = Instant::now() + Duration::from_secs(30);
    let first_done = loop {
        if let Some(d) = mediator.deliveries_of(HEAD).first() {
            break d.finished_at;
        }
        if !events_of(HEAD).is_empty() || Instant::now() > deadline {
            // Settled without a delivery, or nothing happened at all; both
            // show up as mismatches below.
            break Instant::now();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    // Then let the group settle: both messages resolved at the broker, or
    // the window closes with something still held.
    loop {
        let settled = !events_of(HEAD).is_empty() && !events_of(SIBLING).is_empty();
        if settled || first_done.elapsed() >= SETTLE_WINDOW {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let observation = Observation {
        head: mediator.deliveries_of(HEAD),
        sibling: mediator.deliveries_of(SIBLING),
        head_events: events_of(HEAD),
        sibling_events: events_of(SIBLING),
        head_calls: server.calls_for(HEAD),
        sibling_calls: server.calls_for(SIBLING),
        pool_holds: pool.queue_size(),
        metrics: pool.get_enhanced_metrics(),
    };

    // Hand back whatever is still held so no task outlives the case.
    pool.release_remainder().await;
    pool.shutdown().await;
    observation
}

// ---------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------

fn outcome_name(result: MediationResult) -> &'static str {
    match result {
        MediationResult::Success => "Success",
        MediationResult::ErrorConfig => "ErrorConfig",
        MediationResult::ErrorProcess => "ErrorProcess",
        MediationResult::ErrorConnection => "ErrorConnection",
        MediationResult::RateLimited => "RateLimited",
        MediationResult::Deferred => "Deferred",
        MediationResult::CircuitOpen => "CircuitOpen",
    }
}

/// REJECTED and UNDELIVERABLE both mean "acknowledged away"; see the module
/// doc for why they share a bucket.
const ACKED_AWAY: &str = "REJECTED|UNDELIVERABLE";

fn disposition_bucket(corpus: &str) -> &str {
    match corpus {
        "REJECTED" | "UNDELIVERABLE" => ACKED_AWAY,
        other => other,
    }
}

/// The head's fate, read from the broker.
fn observed_disposition(obs: &Observation) -> String {
    let delivered = obs
        .head
        .first()
        .is_some_and(|d| d.outcome.result == MediationResult::Success);
    match obs.head_events.as_slice() {
        [] if obs.pool_holds > 0 => "RETRY_IN_PLACE".to_string(),
        [] => "LOST (no broker action, and the pool no longer holds it)".to_string(),
        [BrokerEvent::Ack] if delivered => "DELIVERED".to_string(),
        [BrokerEvent::Ack] => ACKED_AWAY.to_string(),
        [BrokerEvent::Nack(_)] => "RETURN_TO_BROKER".to_string(),
        other => format!("UNEXPECTED broker events {other:?}"),
    }
}

/// What the sibling queued behind the head must have seen, given the head's
/// disposition (NEXT_ON_ERROR). `None` when it is as required.
fn group_violation(expected: &str, flush_group: bool, obs: &Observation) -> Option<String> {
    let mediated = !obs.sibling.is_empty();
    let events = &obs.sibling_events;
    let ok = match expected {
        // Delivered: the group moves on, unless the target flushed it, in
        // which case the sibling is acked without being delivered.
        "DELIVERED" => events == &[BrokerEvent::Ack] && mediated != flush_group,
        // Acknowledged away under NEXT_ON_ERROR: the group moves on.
        ACKED_AWAY => events.len() == 1 && mediated,
        // Handed back: the whole group goes with it, untried.
        "RETURN_TO_BROKER" => matches!(events.as_slice(), [BrokerEvent::Nack(_)]) && !mediated,
        // Kept in place: nothing behind it may overtake it.
        "RETRY_IN_PLACE" => events.is_empty() && !mediated && obs.sibling_calls == 0,
        _ => true,
    };
    (!ok).then(|| {
        format!(
            "the sibling behind the head saw {events:?}, mediated {} time(s), \
             {} request(s) at the target",
            obs.sibling.len(),
            obs.sibling_calls
        )
    })
}

/// Which pool metric classes moved. The sibling can only add the same class
/// as the head (or none), so the set is the head's.
fn metric_classes(m: &EnhancedPoolMetrics) -> BTreeSet<&'static str> {
    let mut set = BTreeSet::new();
    if m.total_success > 0 {
        set.insert("success");
    }
    if m.total_failure > 0 {
        set.insert("failure");
    }
    if m.total_rate_limited > 0 {
        set.insert("rateLimited");
    }
    // A transient result is a non-success sample that is not a failure.
    if m.last_5_min.failure_count > m.total_failure {
        set.insert("transient");
    }
    set
}

fn check(field: &str, expected: impl ToString, actual: impl ToString, out: &mut Vec<String>) {
    let (e, a) = (expected.to_string(), actual.to_string());
    if e != a {
        out.push(format!("{field}: expected {e}, got {a}"));
    }
}

async fn run_case(case: &Case) -> Vec<String> {
    let obs = observe(&case.given, false).await;
    let expect = &case.expect;
    let mut out = Vec::new();

    let Some(head) = obs.head.first() else {
        // No delivery at all: only the breaker-open short circuit and the
        // pre-flight rejections skip HTTP, and both still go through the
        // mediator. Nothing further can be checked.
        out.push(format!(
            "the head never reached the mediator (broker saw {:?})",
            obs.head_events
        ));
        return out;
    };
    let outcome = &head.outcome;

    check(
        "outcome",
        &expect.outcome,
        outcome_name(outcome.result),
        &mut out,
    );
    check(
        "statusCode",
        expect.status_code,
        outcome.status_code.unwrap_or(0),
        &mut out,
    );
    if let Some(delay) = expect.delay_seconds {
        check(
            "delaySeconds",
            delay,
            outcome.delay_seconds.unwrap_or(0),
            &mut out,
        );
    }
    if let Some(flush) = expect.flush_group {
        check("flushGroup", flush, outcome.flush_group, &mut out);
    }

    let breaker = match head.breaker_delta {
        (1, 0) => "success",
        (0, 1) => "failure",
        (0, 0) if expect.breaker == "none" => "none",
        (0, 0) => "neither",
        _ => "more than one breaker record",
    };
    check("breaker", &expect.breaker, breaker, &mut out);

    let warning = match head.warnings.as_slice() {
        [] => "none".to_string(),
        [(WarningSeverity::Error, WarningCategory::Configuration)] => "ERROR".to_string(),
        [(WarningSeverity::Critical, WarningCategory::Configuration)] => "CRITICAL".to_string(),
        other => format!("{other:?}"),
    };
    check("warning", &expect.warning, warning, &mut out);

    if let Some(made) = expect.http_call_made {
        check("httpCallMade", made, obs.head_calls > 0, &mut out);
    }

    let classes = metric_classes(&obs.metrics);
    let metric = match classes.len() {
        0 => "none".to_string(),
        1 => classes.iter().next().unwrap().to_string(),
        _ => format!("{classes:?}"),
    };
    check("metric", &expect.metric, metric, &mut out);

    let expected = disposition_bucket(&expect.disposition);
    let observed = observed_disposition(&obs);
    if observed != expected {
        out.push(format!(
            "disposition: expected {} ({expected}), got {observed} (broker saw {:?}; {} delivery attempt(s))",
            expect.disposition,
            obs.head_events,
            obs.head.len()
        ));
    } else if let Some(v) = group_violation(expected, outcome.flush_group, &obs) {
        out.push(format!("disposition ({}): {v}", expect.disposition));
    }
    out
}

/// `unsupported-mediation-type`, as the ruling allows it to be run: a wire
/// message naming a non-HTTP mediation type never becomes a `Message`, so it
/// cannot reach a pool, a mediator or a breaker (X-06/X-10: closed enums,
/// no catch-all).
fn run_unsupported_mediation_type() -> Vec<String> {
    let wire = serde_json::json!({
        "id": HEAD,
        "poolCode": "CONFORMANCE",
        "mediationType": "SQS",
        "mediationTarget": "http://127.0.0.1:9/hook",
        "messageGroupId": GROUP,
        "dispatchMode": "NEXT_ON_ERROR",
    });
    match serde_json::from_value::<Message>(wire) {
        Err(_) => Vec::new(),
        Ok(m) => vec![format!(
            "a non-HTTP mediation type parsed as {:?}; it must be refused at the boundary",
            m.mediation_type
        )],
    }
}

/// Cases the owner's rulings stop from running as written, and why. The
/// runner checks the ruled behaviour in their place.
fn ruled(case_id: &str) -> Option<&'static str> {
    match case_id {
        "unsupported-mediation-type" => Some(
            "X-06/X-10: MediationType is a closed enum, so a non-HTTP type is refused \
             when the message is parsed and never reaches the mediator",
        ),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mediation_conformance() {
    let corpus = load_corpus();

    let results = futures::future::join_all(corpus.cases.iter().map(|case| async move {
        let mismatches = if case.given.kind == "unsupportedMediationType" {
            run_unsupported_mediation_type()
        } else {
            run_case(case).await
        };
        (case, mismatches)
    }))
    .await;

    let mut report = Vec::new();
    let mut failures = Vec::new();
    let (mut passed, mut ruled_count) = (0, 0);
    for (case, mismatches) in &results {
        if mismatches.is_empty() {
            match ruled(&case.id) {
                Some(reason) => {
                    ruled_count += 1;
                    report.push(format!("RULED {:<46} {reason}", case.id));
                }
                None => {
                    passed += 1;
                    report.push(format!("PASS  {}", case.id));
                }
            }
        } else {
            report.push(format!("FAIL  {}", case.id));
            for m in mismatches {
                report.push(format!("        - {m}"));
                failures.push(format!("{}: {m}", case.id));
            }
        }
    }

    eprintln!(
        "\n=== Mediation conformance ({}) ===",
        corpus_path().display()
    );
    for line in &report {
        eprintln!("{line}");
    }
    eprintln!(
        "\n{} cases: {passed} pass, {ruled_count} ruled, {} fail\n",
        results.len(),
        results.len() - passed - ruled_count
    );

    assert!(
        failures.is_empty(),
        "mediation conformance failures — check conformance/README.md \
         ('When a row fails') before changing anything:\n{}",
        failures.join("\n")
    );
}

/// Owner ruling R1 (2026-09-17, Go `d879b23`): a deferral that names a delay
/// goes straight back to a broker that can hold it, with exactly that delay,
/// and takes its group with it. The corpus row
/// `deferred-ack-false-with-delay` pins the outcome's own classification
/// (retry in place — what happens when the broker cannot hold a delay);
/// this pins the hand-back.
#[tokio::test]
async fn deferral_naming_a_delay_goes_back_to_a_broker_that_honours_it() {
    let given = Given {
        kind: "response".to_string(),
        status: Some(200),
        body: Some(r#"{"ack":false,"delaySeconds":15}"#.to_string()),
        headers: HashMap::new(),
    };
    let obs = observe(&given, true).await;

    assert_eq!(obs.head.len(), 1, "one delivery, no in-memory retry");
    assert_eq!(
        obs.head_events,
        vec![BrokerEvent::Nack(Some(15))],
        "handed back on its first occurrence with exactly the delay named"
    );
    assert!(
        matches!(obs.sibling_events.as_slice(), [BrokerEvent::Nack(_)]),
        "the group goes back with its head: {:?}",
        obs.sibling_events
    );
    assert!(
        obs.sibling.is_empty(),
        "the sibling must not overtake the head"
    );
}
