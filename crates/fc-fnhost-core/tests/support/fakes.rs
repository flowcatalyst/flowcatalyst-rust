//! Scripted fakes for reconciler tests: a control plane, an artifact store
//! and a loader whose instances record what happened to them.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fc_fnhost_core::artifact::{ArtifactError, ArtifactStore, Fetched};
use fc_fnhost_core::control_plane::{
    ControlPlane, ControlPlaneError, EmitRequest, Fetched as DesiredFetched,
};
use fc_fnhost_core::desired::DesiredDocument;
use fc_fnhost_core::digest::Digest;
use fc_fnhost_core::heartbeat::{HeartbeatReport, LoadState};
use fc_fnhost_core::invoke::{InvocationContext, InvokeError, Invoker};
use fc_fnhost_core::loader::{FunctionInstance, FunctionLoader, LoadOutcome, LoadRequest};
use fc_fnhost_core::registry::FunctionRegistry;
use fc_function_abi::{EventEmitError, FunctionAddress, Response};
use parking_lot::Mutex;
use serde_json::{json, Value};

/// A desired-state entry, as the platform would send it.
pub fn entry(address: &str, version: i32, role: &str, mode: &str) -> Value {
    json!({
        "address": address,
        "functionId": format!("fnc_{}", address.replace('.', "_")),
        "versionId": format!("fnv_{}_{version}", address.replace('.', "_")),
        "version": version,
        "role": role,
        "mode": mode,
        "digest": digest_for(address, version).value(),
        "artifactRef": format!("mem://{address}/{version}"),
        "manifest": {"runtime": "wasm", "entrypoint": "handle"},
        "applicationId": "app_1"
    })
}

/// A distinct, valid digest per (address, version).
pub fn digest_for(address: &str, version: i32) -> Digest {
    Digest::from_sha256(&support_sha(format!("{address}@{version}").as_bytes()))
}

fn support_sha(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes).into()
}

pub fn document(functions: Vec<Value>) -> DesiredDocument {
    DesiredDocument::parse(&json!({ "functions": functions }).to_string()).unwrap()
}

pub fn document_json(body: Value) -> DesiredDocument {
    DesiredDocument::parse(&body.to_string()).unwrap()
}

/// What the next `desired_state` call answers.
#[derive(Clone)]
pub enum Answer {
    Document(DesiredDocument),
    NotModified,
    Down,
}

#[derive(Default)]
pub struct FakeControlPlane {
    answer: Mutex<Option<Answer>>,
    etag_counter: AtomicUsize,
    pub fetches: AtomicUsize,
    pub etags_sent: Mutex<Vec<Option<String>>>,
    pub heartbeats: Mutex<Vec<HeartbeatReport>>,
    /// When set, `desired_state` waits here (loop tests).
    pub gate: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
    pub panic_next: AtomicBool,
    /// Every emit, in order.
    pub emits: Mutex<Vec<EmitRequest>>,
    /// When set, every emit is refused with this.
    pub emit_refusal: Mutex<Option<EventEmitError>>,
}

impl FakeControlPlane {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn serve(&self, answer: Answer) {
        *self.answer.lock() = Some(answer);
    }

    pub fn last_heartbeat(&self) -> HeartbeatReport {
        self.heartbeats
            .lock()
            .last()
            .cloned()
            .expect("a heartbeat was sent")
    }

    pub fn heartbeat_count(&self) -> usize {
        self.heartbeats.lock().len()
    }
}

#[async_trait]
impl ControlPlane for FakeControlPlane {
    async fn desired_state(
        &self,
        _pool: &str,
        known_etag: Option<&str>,
    ) -> Result<DesiredFetched, ControlPlaneError> {
        if self.panic_next.swap(false, Ordering::SeqCst) {
            panic!("scripted control-plane panic");
        }
        let gate = self.gate.lock().clone();
        if let Some(gate) = gate {
            gate.acquire().await.unwrap().forget();
        }
        self.fetches.fetch_add(1, Ordering::SeqCst);
        self.etags_sent.lock().push(known_etag.map(str::to_owned));
        match self.answer.lock().clone() {
            Some(Answer::Document(document)) => {
                let n = self.etag_counter.fetch_add(1, Ordering::SeqCst);
                Ok(DesiredFetched::Changed {
                    etag: format!("\"etag-{n}\""),
                    document,
                })
            }
            Some(Answer::NotModified) => Ok(DesiredFetched::NotModified),
            Some(Answer::Down) | None => Err(ControlPlaneError::unavailable("scripted outage")),
        }
    }

    async fn heartbeat(&self, report: &HeartbeatReport) -> Result<(), ControlPlaneError> {
        self.heartbeats.lock().push(report.clone());
        Ok(())
    }

    async fn emit(&self, request: &EmitRequest) -> Result<(), EventEmitError> {
        self.emits.lock().push(request.clone());
        match self.emit_refusal.lock().clone() {
            Some(refusal) => Err(refusal),
            None => Ok(()),
        }
    }
}

/// Serves `mem://` references from a map; unknown references are fetched
/// successfully to a placeholder path unless scripted otherwise.
#[derive(Default)]
pub struct FakeStore {
    pub failures: Mutex<HashMap<String, ArtifactError>>,
    pub fetches: Mutex<Vec<String>>,
    /// References whose fetch waits until released: a version mid-prepare.
    pub held: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl FakeStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn fail(&self, artifact_ref: &str, error: ArtifactError) {
        self.failures.lock().insert(artifact_ref.to_owned(), error);
    }

    pub fn heal(&self, artifact_ref: &str) {
        self.failures.lock().remove(artifact_ref);
    }

    pub fn fetch_count(&self) -> usize {
        self.fetches.lock().len()
    }

    /// Fetches of `artifact_ref` wait until [`FakeStore::release`].
    #[allow(dead_code)]
    pub fn hold(&self, artifact_ref: &str) {
        self.held.lock().insert(
            artifact_ref.to_owned(),
            Arc::new(tokio::sync::Semaphore::new(0)),
        );
    }

    #[allow(dead_code)]
    pub fn release(&self, artifact_ref: &str) {
        if let Some(gate) = self.held.lock().remove(artifact_ref) {
            gate.close(); // every waiting fetch proceeds
        }
    }
}

#[async_trait]
impl ArtifactStore for FakeStore {
    async fn fetch(
        &self,
        artifact_ref: &str,
        expected: &Digest,
        _version_id: Option<&str>,
    ) -> Result<Fetched, ArtifactError> {
        self.fetches.lock().push(artifact_ref.to_owned());
        let gate = self.held.lock().get(artifact_ref).cloned();
        if let Some(gate) = gate {
            let _ = gate.acquire().await;
        }
        if let Some(error) = self.failures.lock().get(artifact_ref) {
            return Err(error.clone());
        }
        Ok(Fetched {
            file: PathBuf::from(format!("/cache/sha256/{}", expected.hex())),
            bytes: 1,
        })
    }
}

/// Everything the fake runtime saw, in order: `load a.b.c@2`, `close a.b.c@1`.
pub type Journal = Arc<Mutex<Vec<String>>>;

pub struct FakeInstance {
    pub label: String,
    pub closed: AtomicBool,
    journal: Journal,
    /// On close, the version the registry serves for this address, proving
    /// the replacement was registered first.
    registry: Option<Arc<FunctionRegistry>>,
    address: FunctionAddress,
    pub served_at_close: Mutex<Option<Option<i32>>>,
}

#[async_trait]
impl Invoker for FakeInstance {
    async fn invoke(&self, _context: InvocationContext) -> Result<Response, InvokeError> {
        Ok(Response::ack())
    }
}

#[async_trait]
impl FunctionInstance for FakeInstance {
    async fn close(&self) {
        if let Some(registry) = &self.registry {
            *self.served_at_close.lock() = Some(registry.peek(&self.address).map(|f| f.version()));
        }
        self.closed.store(true, Ordering::SeqCst);
        self.journal.lock().push(format!("close {}", self.label));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Default)]
pub struct FakeLoader {
    pub journal: Journal,
    pub refusals: Mutex<HashMap<String, (String, String)>>,
    pub instances: Mutex<Vec<Arc<FakeInstance>>>,
    pub registry: Mutex<Option<Arc<FunctionRegistry>>>,
    pub delay: Mutex<Option<Duration>>,
    pub loaded_config: Mutex<Vec<std::collections::BTreeMap<String, String>>>,
}

impl FakeLoader {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn refuse(&self, label: &str, reason: &str) {
        self.refusals
            .lock()
            .insert(label.to_owned(), (reason.to_owned(), "scripted".to_owned()));
    }

    pub fn allow(&self, label: &str) {
        self.refusals.lock().remove(label);
    }

    pub fn loads(&self) -> Vec<String> {
        self.journal
            .lock()
            .iter()
            .filter(|l| l.starts_with("load "))
            .cloned()
            .collect()
    }

    pub fn journal(&self) -> Vec<String> {
        self.journal.lock().clone()
    }

    pub fn instance(&self, label: &str) -> Arc<FakeInstance> {
        self.instances
            .lock()
            .iter()
            .rev()
            .find(|i| i.label == label)
            .cloned()
            .unwrap_or_else(|| panic!("{label} was never loaded"))
    }
}

#[async_trait]
impl FunctionLoader for FakeLoader {
    async fn load(&self, request: LoadRequest<'_>) -> LoadOutcome {
        let label = format!("{}@{}", request.entry.address, request.entry.version);
        let delay = *self.delay.lock();
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        if let Some((reason, detail)) = self.refusals.lock().get(&label).cloned() {
            self.journal.lock().push(format!("refuse {label}"));
            return LoadOutcome::Refused { reason, detail };
        }
        self.journal.lock().push(format!("load {label}"));
        self.loaded_config.lock().push(request.entry.config.clone());
        let instance = Arc::new(FakeInstance {
            label,
            closed: AtomicBool::new(false),
            journal: self.journal.clone(),
            registry: self.registry.lock().clone(),
            address: request.entry.address.clone(),
            served_at_close: Mutex::new(None),
        });
        self.instances.lock().push(instance.clone());
        LoadOutcome::Loaded(instance)
    }
}

/// `(address, version) → state` of a heartbeat, for compact assertions.
pub fn states(report: &HeartbeatReport) -> Vec<(String, i32, String)> {
    report
        .loaded
        .iter()
        .map(|e| {
            let state = match &e.state {
                LoadState::Registered => "REGISTERED".to_owned(),
                LoadState::Loaded => "LOADED".to_owned(),
                LoadState::Failed(error) => format!("FAILED:{error}"),
            };
            (e.address.render(), e.version, state)
        })
        .collect()
}
