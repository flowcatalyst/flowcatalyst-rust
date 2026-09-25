//! Desired state → fetch → verify → load → heartbeat, one cycle at a time
//! (Java `fnhost/reconcile/Reconciler.java`, spec
//! `function-host-reconciler.md` §1.2). [`ReconcileLoop`] calls
//! [`Reconciler::reconcile_once`] on a schedule; this type owns no task of
//! its own, which is what makes it directly testable.
//!
//! The pinned rules:
//! - a control-plane outage prepares, loads and unloads nothing (a platform
//!   outage never unloads a function), but still heartbeats what it knows;
//! - **new before old**: a promote registers the new version before the old
//!   one is closed, and a version that fails to prepare or load leaves the
//!   old one serving;
//! - an unreadable desired-state entry is reported `FAILED` and protects its
//!   address from unloading;
//! - **what to unload is derived here**, as the difference between what this
//!   host holds and what the document names (owner decision 5). The
//!   document's `unload` list is read but ignored: it is the platform's
//!   guess from the last heartbeats, which lags by a beat and churns the
//!   ETag, and it could close an old version whose replacement failed,
//!   against new-before-old. The platform keeps sending it for one release,
//!   for JVM hosts;
//! - failures are never cached across cycles: every failed entry is retried
//!   next cycle. Within a cycle, a version refused at load is not loaded
//!   again on first call (lazy or pinned) with the same settings: the
//!   refusal is a property of its digest-pinned artifact and manifest, and
//!   recompiling it per request would only burn CPU. Its detail is logged
//!   when the refusal is new or changes (Java 5afabe52).
//!
//! [`ReconcileLoop`]: crate::reconcile_loop::ReconcileLoop

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use fc_function_abi::FunctionAddress;
use futures::StreamExt;
use parking_lot::{Mutex, RwLock};

use crate::artifact::ArtifactStore;
use crate::control_plane::{ControlPlane, Fetched};
use crate::desired::{DesiredDocument, Entry, Mode, Role};
use crate::fingerprint::settings_fingerprint;
use crate::heartbeat::{HeartbeatReport, HostState, LoadState, LoadedEntry};
use crate::loader::{LoadOutcome, LoadRequest, LoadedFunction, Loaders, RUNTIME_UNSUPPORTED};
use crate::registry::FunctionRegistry;
use crate::signature::{SignatureVerifier, Signatures, Verification};

/// A lazy entry idle longer than this is closed but keeps its route. A
/// constant, not a knob.
pub const IDLE_UNLOAD: chrono::Duration = chrono::Duration::hours(1);

/// Entries are prepared at most this many at a time.
pub const MAX_CONCURRENT_PREPARES: usize = 4;

/// What `/ready` (and, after start-up, `/health`) report. Precedence, as
/// Java: `DRAINING` > `STARTING` > `PLATFORM_UNREACHABLE` > `LISTENER_DOWN`
/// > `RECONCILER_DOWN` > `READY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Starting,
    PlatformUnreachable,
    ListenerDown,
    ReconcilerDown,
    Ready,
    Draining,
}

impl Readiness {
    pub fn name(self) -> &'static str {
        match self {
            Readiness::Starting => "STARTING",
            Readiness::PlatformUnreachable => "PLATFORM_UNREACHABLE",
            Readiness::ListenerDown => "LISTENER_DOWN",
            Readiness::ReconcilerDown => "RECONCILER_DOWN",
            Readiness::Ready => "READY",
            Readiness::Draining => "DRAINING",
        }
    }
}

/// Who hears about load errors and reconcile outcomes (the metrics). Every
/// method defaults to a no-op.
pub trait ReconcileObserver: Send + Sync {
    /// A prepare/load failure reason: `ARTIFACT:…`, `SIGNATURE:…`,
    /// `SIGNER_MISMATCH`, `UNSIGNED`, `LOAD:…`, `RUNTIME_UNSUPPORTED`. Not
    /// called for `NO_SIGNING_SECRET` (an auth configuration state).
    fn load_error(&self, _reason: &str) {}

    /// One cycle's fetch outcome: `changed`, `not_modified` or `failed`;
    /// `success` for the first two.
    fn reconciled(&self, _outcome: &str, _success: bool, _now: DateTime<Utc>) {}
}

struct NoopObserver;
impl ReconcileObserver for NoopObserver {}

type Key = (FunctionAddress, i32);

struct Refusal {
    reason: String,
    cycle: u64,
    fingerprint: String,
}

/// What [`Reconciler::load_pinned`] found.
pub enum PinnedLoad {
    Loaded(Arc<LoadedFunction>),
    /// Desired, but not yet prepared and nothing failed: `503
    /// VERSION_NOT_READY` with `Retry-After`.
    Preparing,
    /// Refused for good (preparing or loading failed): `404
    /// VERSION_NOT_AVAILABLE`.
    Refused,
}

#[derive(Default)]
struct State {
    etag: Option<String>,
    document: Option<Arc<DesiredDocument>>,
    /// `versionId → artifact path` for every version fetched and verified.
    prepared: HashMap<String, PathBuf>,
    /// `(address, version) → reason`, retried every cycle.
    failures: HashMap<Key, String>,
    /// Load refusals (`LoadOutcome::Refused`): the reason, and the reconcile
    /// cycle and settings fingerprint it was found under. A refusal is a
    /// property of the digest-pinned artifact, its manifest and settings, so
    /// a first-call load (lazy or pinned) is not attempted again for the
    /// same key in the same cycle with the same settings; the next cycle
    /// retries it once, as failures always are.
    refusals: HashMap<Key, Refusal>,
    version_id_by_key: HashMap<Key, String>,
    /// `address → live entry routed lazily`.
    lazy_routes: HashMap<FunctionAddress, Entry>,
    /// The settings fingerprint the address's loaded version was built with.
    settings_fingerprint: HashMap<FunctionAddress, String>,
    current_secret: HashMap<FunctionAddress, String>,
    /// The secret a rotation displaced, and the reconcile number through
    /// which it is still accepted.
    previous_secret: HashMap<FunctionAddress, (String, u64)>,
}

pub struct Reconciler {
    pool: String,
    host_id: String,
    control_plane: Arc<dyn ControlPlane>,
    artifacts: Arc<dyn ArtifactStore>,
    signatures: Signatures,
    loaders: Loaders,
    registry: Arc<FunctionRegistry>,
    state: Mutex<State>,
    /// One lock per address, so concurrent loads of one address load once.
    load_locks: Mutex<HashMap<FunctionAddress, Arc<tokio::sync::Mutex<()>>>>,
    /// Serialises whole cycles (the loop, start-up and close all call in).
    cycle: tokio::sync::Mutex<()>,
    observer: RwLock<Arc<dyn ReconcileObserver>>,
    post_reconcile: RwLock<Vec<Arc<dyn Fn() + Send + Sync>>>,
    draining: AtomicBool,
    reconcile_attempted: AtomicBool,
    ever_succeeded: AtomicBool,
    reconcile_count: AtomicU64,
}

impl Reconciler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: impl Into<String>,
        host_id: impl Into<String>,
        control_plane: Arc<dyn ControlPlane>,
        artifacts: Arc<dyn ArtifactStore>,
        signatures: Signatures,
        loaders: Loaders,
        registry: Arc<FunctionRegistry>,
    ) -> Self {
        Self {
            pool: pool.into(),
            host_id: host_id.into(),
            control_plane,
            artifacts,
            signatures,
            loaders,
            registry,
            state: Mutex::new(State::default()),
            load_locks: Mutex::new(HashMap::new()),
            cycle: tokio::sync::Mutex::new(()),
            observer: RwLock::new(Arc::new(NoopObserver)),
            post_reconcile: RwLock::new(Vec::new()),
            draining: AtomicBool::new(false),
            reconcile_attempted: AtomicBool::new(false),
            ever_succeeded: AtomicBool::new(false),
            reconcile_count: AtomicU64::new(0),
        }
    }

    pub fn set_observer(&self, observer: Arc<dyn ReconcileObserver>) {
        *self.observer.write() = observer;
    }

    /// Runs after every cycle, whatever its outcome (the metrics' series
    /// sweep; H5's pinned-version sweep).
    pub fn add_post_reconcile_listener(&self, listener: Arc<dyn Fn() + Send + Sync>) {
        self.post_reconcile.write().push(listener);
    }

    fn observer(&self) -> Arc<dyn ReconcileObserver> {
        self.observer.read().clone()
    }

    pub fn registry(&self) -> &Arc<FunctionRegistry> {
        &self.registry
    }

    pub fn host_id(&self) -> &str {
        &self.host_id
    }

    pub fn pool(&self) -> &str {
        &self.pool
    }

    /// Every heartbeat from now on reports `DRAINING`. One-way.
    pub fn drain(&self) {
        self.draining.store(true, Ordering::SeqCst);
    }

    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    pub fn readiness(&self, listener_bound: bool, reconcile_loop_alive: bool) -> Readiness {
        if self.is_draining() {
            Readiness::Draining
        } else if !self.reconcile_attempted.load(Ordering::SeqCst) {
            Readiness::Starting
        } else if !self.ever_succeeded.load(Ordering::SeqCst) {
            Readiness::PlatformUnreachable
        } else if !listener_bound {
            Readiness::ListenerDown
        } else if !reconcile_loop_alive {
            Readiness::ReconcilerDown
        } else {
            Readiness::Ready
        }
    }

    pub fn reconcile_count(&self) -> u64 {
        self.reconcile_count.load(Ordering::SeqCst)
    }

    /// One full cycle: fetch; then prepare, load and unload (skipped on an
    /// outage); then always a heartbeat when a document is known.
    ///
    /// `NotModified` still runs prepare/load/unload against the cached
    /// document: prepare and load are no-ops when nothing changed, a failed
    /// entry gets its retry, and idle unloading is a decision about time.
    pub async fn reconcile_once(&self, now: DateTime<Utc>) {
        let _cycle = self.cycle.lock().await;
        let reconcile_number = self.reconcile_count.fetch_add(1, Ordering::SeqCst) + 1;
        let known_etag = self.state.lock().etag.clone();
        let observer = self.observer();
        let mut outage = false;
        match self
            .control_plane
            .desired_state(&self.pool, known_etag.as_deref())
            .await
        {
            Ok(Fetched::NotModified) => {
                self.ever_succeeded.store(true, Ordering::SeqCst);
                observer.reconciled("not_modified", true, now);
            }
            Ok(Fetched::Changed { etag, document }) => {
                let mut state = self.state.lock();
                update_secret_history(&mut state, &document, reconcile_number);
                state.etag = Some(etag);
                state.document = Some(Arc::new(document));
                drop(state);
                self.ever_succeeded.store(true, Ordering::SeqCst);
                observer.reconciled("changed", true, now);
            }
            Err(e) => {
                tracing::warn!(
                    host_id = %self.host_id,
                    pool = %self.pool,
                    err = %e,
                    "desired-state fetch failed; keeping what is already loaded"
                );
                outage = true;
                observer.reconciled("failed", false, now);
            }
        }
        // Set only once the fetch has resolved: a slow first fetch reads as STARTING.
        self.reconcile_attempted.store(true, Ordering::SeqCst);

        let document = self.state.lock().document.clone();
        if let Some(document) = &document {
            if !outage {
                self.prepare(document).await;
                self.load(document).await;
                self.check_signing_secrets(document);
                self.unload(document, now).await;
            }
            self.send_heartbeat(document).await;
        }
        let listeners = self.post_reconcile.read().clone();
        for listener in listeners {
            listener();
        }
    }

    // ── step 2: prepare ──────────────────────────────────────────────────

    async fn prepare(&self, document: &DesiredDocument) {
        let pending: Vec<&Entry> = {
            let state = self.state.lock();
            document
                .functions
                .iter()
                .filter(|e| !state.prepared.contains_key(&e.version_id))
                .collect()
        };
        futures::stream::iter(pending)
            .for_each_concurrent(MAX_CONCURRENT_PREPARES, |entry| self.prepare_one(entry))
            .await;
    }

    async fn prepare_one(&self, entry: &Entry) {
        let key: Key = (entry.address.clone(), entry.version);
        let outcome = self.fetch_and_verify(entry).await;
        let mut state = self.state.lock();
        match outcome {
            Ok(path) => {
                state.prepared.insert(entry.version_id.clone(), path);
                state
                    .version_id_by_key
                    .insert(key.clone(), entry.version_id.clone());
                state.failures.remove(&key);
            }
            Err(reason) => {
                let previous = state.failures.insert(key, reason.clone());
                drop(state);
                self.observer().load_error(&reason);
                // Retried every cycle; logged when new or changed (Java 5afabe52).
                if previous.as_deref() != Some(reason.as_str()) {
                    tracing::warn!(
                        address = %entry.address,
                        version = entry.version,
                        reason = %reason,
                        "failed to prepare a function version"
                    );
                }
            }
        }
    }

    /// A runtime this host has no loader for is never fetched: the artifact
    /// could never run here, so it fails fast as `RUNTIME_UNSUPPORTED`.
    async fn fetch_and_verify(&self, entry: &Entry) -> Result<PathBuf, String> {
        if !self.loaders.supports(entry.manifest.runtime.wire_value()) {
            return Err(RUNTIME_UNSUPPORTED.to_owned());
        }
        let fetched = self
            .artifacts
            .fetch(&entry.artifact_ref, &entry.digest, Some(&entry.version_id))
            .await
            .map_err(|e| format!("ARTIFACT:{}", e.simple_name()))?;
        match &self.signatures {
            Signatures::Off => {}
            Signatures::Required(verifier) => verify_signature(verifier, entry)?,
        }
        Ok(fetched.file)
    }

    // ── step 3: load ─────────────────────────────────────────────────────

    async fn load(&self, document: &DesiredDocument) {
        for entry in document.functions.iter().filter(|e| e.role == Role::Live) {
            let Some(path) = self.state.lock().prepared.get(&entry.version_id).cloned() else {
                continue; // still pending, or failed to prepare this cycle
            };
            match entry.mode {
                Mode::Warm => self.load_warm(entry, path).await,
                Mode::Lazy => self.load_lazy(entry, path).await,
            }
        }
    }

    fn load_lock(&self, address: &FunctionAddress) -> Arc<tokio::sync::Mutex<()>> {
        self.load_locks
            .lock()
            .entry(address.clone())
            .or_default()
            .clone()
    }

    fn is_current(&self, entry: &Entry) -> bool {
        self.registry.peek(&entry.address).is_some_and(|current| {
            current.version() == entry.version && !self.settings_changed(entry)
        })
    }

    async fn load_warm(&self, entry: &Entry, path: PathBuf) {
        let lock = self.load_lock(&entry.address);
        let _guard = lock.lock().await;
        if self.is_current(entry) {
            return;
        }
        let outcome = self.attempt_load(&path, entry).await;
        let loaded = matches!(outcome, LoadOutcome::Loaded(_));
        self.apply_load_outcome(outcome, entry, true).await;
        if loaded {
            self.state.lock().lazy_routes.remove(&entry.address); // warm now, not lazily routed
        }
    }

    /// A lazy function already resident must not keep serving an old
    /// version (or stale settings) until it idles out; otherwise the first
    /// invocation loads it ([`Reconciler::ensure_loaded`]).
    async fn load_lazy(&self, entry: &Entry, path: PathBuf) {
        self.state
            .lock()
            .lazy_routes
            .insert(entry.address.clone(), entry.clone());
        let lock = self.load_lock(&entry.address);
        let _guard = lock.lock().await;
        if self.registry.peek(&entry.address).is_none() || self.is_current(entry) {
            return;
        }
        let outcome = self.attempt_load(&path, entry).await;
        self.apply_load_outcome(outcome, entry, false).await;
    }

    async fn attempt_load(&self, path: &std::path::Path, entry: &Entry) -> LoadOutcome {
        match self.loaders.get(entry.manifest.runtime.wire_value()) {
            Some(loader) => {
                loader
                    .load(LoadRequest {
                        artifact: path,
                        entry,
                        control_plane: &self.control_plane,
                        host_id: &self.host_id,
                    })
                    .await
            }
            None => LoadOutcome::Failed {
                code: RUNTIME_UNSUPPORTED.to_owned(),
                detail: format!(
                    "no loader for runtime {}",
                    entry.manifest.runtime.wire_value()
                ),
            },
        }
    }

    fn settings_changed(&self, entry: &Entry) -> bool {
        let new = settings_fingerprint(&entry.config, &entry.secrets);
        self.state.lock().settings_fingerprint.get(&entry.address) != Some(&new)
    }

    /// Records `reason` for `entry`; whether it is new or changed, so the
    /// caller logs its detail once rather than on every retry (Java
    /// 5afabe52).
    fn record_failure(&self, entry: &Entry, reason: String) -> bool {
        let previous = self
            .state
            .lock()
            .failures
            .insert((entry.address.clone(), entry.version), reason.clone());
        self.observer().load_error(&reason);
        previous.as_deref() != Some(reason.as_str())
    }

    /// Notes a load refusal under the current cycle and settings; whether
    /// its reason is new or changed.
    fn note_refusal(&self, entry: &Entry, reason: &str) -> bool {
        let refusal = Refusal {
            reason: reason.to_owned(),
            cycle: self.reconcile_count(),
            fingerprint: settings_fingerprint(&entry.config, &entry.secrets),
        };
        self.state
            .lock()
            .refusals
            .insert((entry.address.clone(), entry.version), refusal)
            .is_none_or(|previous| previous.reason != reason)
    }

    /// Whether `entry` was refused at load in this cycle with these
    /// settings: a first-call load does not try again until the next cycle.
    fn refused_this_cycle(&self, entry: &Entry) -> bool {
        self.state
            .lock()
            .refusals
            .get(&(entry.address.clone(), entry.version))
            .is_some_and(|r| {
                r.cycle == self.reconcile_count()
                    && r.fingerprint == settings_fingerprint(&entry.config, &entry.secrets)
            })
    }

    fn clear_refusal(&self, entry: &Entry) {
        self.state
            .lock()
            .refusals
            .remove(&(entry.address.clone(), entry.version));
    }

    /// **New before old**: the displaced version is closed only after the
    /// new one is registered. Returns the registered function.
    async fn apply_load_outcome(
        &self,
        outcome: LoadOutcome,
        entry: &Entry,
        warm: bool,
    ) -> Option<Arc<LoadedFunction>> {
        match outcome {
            LoadOutcome::Loaded(instance) => {
                let function = LoadedFunction::new(entry.address.clone(), entry.version, instance);
                self.state.lock().settings_fingerprint.insert(
                    entry.address.clone(),
                    settings_fingerprint(&entry.config, &entry.secrets),
                );
                match self.registry.put(function.clone(), warm) {
                    Ok(displaced) => {
                        self.clear_refusal(entry);
                        self.state
                            .lock()
                            .failures
                            .remove(&(entry.address.clone(), entry.version));
                        for old in [displaced.previous, displaced.evicted]
                            .into_iter()
                            .flatten()
                        {
                            old.close().await;
                        }
                        Some(function)
                    }
                    Err(full) => {
                        // Capacity is transient: retried on the next call, but
                        // logged only when new.
                        if self.record_failure(entry, "LOAD:REGISTRY_FULL".to_owned()) {
                            tracing::warn!(
                                address = %entry.address,
                                version = entry.version,
                                err = %full,
                                "registry refused to load a function version: at capacity and every loaded entry is warm"
                            );
                        }
                        function.close().await; // never registered
                        None
                    }
                }
            }
            // The loader's detail reaches the log when a version's refusal is
            // new or changes, not on every cycle (or call) that finds it
            // still refused (Java 5afabe52).
            LoadOutcome::Refused { reason, detail } => {
                self.note_refusal(entry, &reason);
                if self.record_failure(entry, format!("LOAD:{reason}")) {
                    tracing::warn!(address = %entry.address, version = entry.version, reason = %reason, detail = %detail, "function version refused to load");
                }
                None
            }
            LoadOutcome::Failed { code, detail } => {
                if self.record_failure(entry, code.clone()) {
                    tracing::warn!(address = %entry.address, version = entry.version, reason = %code, detail = %detail, "failed to load a function version");
                }
                None
            }
        }
    }

    /// What the invoke path (H5) calls on first invocation of a lazy
    /// address: loads from the prepared artifact under the per-address lock,
    /// so two first callers load once. Returns whatever ends up registered.
    pub async fn ensure_loaded(&self, address: &FunctionAddress) -> Option<Arc<LoadedFunction>> {
        let current = self.registry.get(address); // a real access: about to be invoked
        let Some(route) = self.state.lock().lazy_routes.get(address).cloned() else {
            return current;
        };
        let matches = |f: &Option<Arc<LoadedFunction>>| {
            f.as_ref()
                .is_some_and(|f| f.version() == route.version && !self.settings_changed(&route))
        };
        if matches(&current) {
            return current;
        }
        let lock = self.load_lock(address);
        let _guard = lock.lock().await;
        let current = self.registry.get(address);
        if matches(&current) {
            return current;
        }
        let Some(path) = self.state.lock().prepared.get(&route.version_id).cloned() else {
            return current;
        };
        if self.refused_this_cycle(&route) {
            return current; // refused already this cycle: not recompiled per call
        }
        let outcome = self.attempt_load(&path, &route).await;
        match self.apply_load_outcome(outcome, &route, false).await {
            Some(loaded) => Some(loaded),
            None => current,
        }
    }

    /// Loads `entry`'s version fresh, outside the registry, for a versioned
    /// call pinning a candidate (H5). The caller owns the result. Says which
    /// case it hit when nothing loaded (owner ruling 12): the version is
    /// still being prepared, or it was refused.
    pub async fn load_pinned(&self, entry: &Entry) -> PinnedLoad {
        let path = {
            let state = self.state.lock();
            match state.prepared.get(&entry.version_id) {
                Some(path) => path.clone(),
                None if state
                    .failures
                    .contains_key(&(entry.address.clone(), entry.version)) =>
                {
                    return PinnedLoad::Refused
                }
                None => return PinnedLoad::Preparing,
            }
        };
        if self.refused_this_cycle(entry) {
            return PinnedLoad::Refused; // not recompiled per call
        }
        match self.attempt_load(&path, entry).await {
            LoadOutcome::Loaded(instance) => {
                self.clear_refusal(entry);
                PinnedLoad::Loaded(LoadedFunction::new(
                    entry.address.clone(),
                    entry.version,
                    instance,
                ))
            }
            LoadOutcome::Refused { reason, detail } => {
                if self.note_refusal(entry, &reason) {
                    tracing::warn!(address = %entry.address, version = entry.version, reason = %reason, detail = %detail, "pinned function version refused to load");
                }
                PinnedLoad::Refused
            }
            LoadOutcome::Failed { code, detail } => {
                tracing::warn!(address = %entry.address, version = entry.version, reason = %code, detail = %detail, "failed to load a pinned function version");
                PinnedLoad::Refused
            }
        }
    }

    /// Whether `entry`'s version is still being prepared: desired, not yet
    /// prepared, and no failure recorded for it (Java `isPreparing`, owner
    /// ruling 12). It can change between a call that loaded nothing and this
    /// one; the worst case is one 503 as preparation completes, which a
    /// retry resolves.
    pub fn is_preparing(&self, entry: &Entry) -> bool {
        let state = self.state.lock();
        !state.prepared.contains_key(&entry.version_id)
            && !state
                .failures
                .contains_key(&(entry.address.clone(), entry.version))
    }

    // ── webhook signing secrets ──────────────────────────────────────────

    /// A live entry with a `webhook` endpoint but no secret is `FAILED:
    /// NO_SIGNING_SECRET`: flagged after load so it never stops the entry
    /// loading and serving its other endpoints.
    fn check_signing_secrets(&self, document: &DesiredDocument) {
        let mut state = self.state.lock();
        for entry in document.functions.iter().filter(|e| e.role == Role::Live) {
            if !entry.manifest.has_webhook_endpoint() {
                continue;
            }
            let key: Key = (entry.address.clone(), entry.version);
            if entry.webhook_signing_secret.is_none() {
                state.failures.insert(key, "NO_SIGNING_SECRET".to_owned());
            } else if state.failures.get(&key).map(String::as_str) == Some("NO_SIGNING_SECRET") {
                state.failures.remove(&key);
            }
        }
    }

    pub fn current_webhook_secret(&self, address: &FunctionAddress) -> Option<String> {
        self.state.lock().current_secret.get(address).cloned()
    }

    /// The secret a rotation displaced, accepted until the second reconcile
    /// after the change.
    pub fn previous_webhook_secret(&self, address: &FunctionAddress) -> Option<String> {
        let mut state = self.state.lock();
        let (secret, valid_through) = state.previous_secret.get(address).cloned()?;
        if self.reconcile_count() > valid_through {
            state.previous_secret.remove(address);
            return None;
        }
        Some(secret)
    }

    // ── step 4: unload ───────────────────────────────────────────────────

    /// Step 4, derived from the document alone (its `unload` list is not
    /// consulted):
    /// - 4a: a loaded address that is no longer live at all (gone, disabled,
    ///   only a candidate now) is closed, unless an unreadable entry
    ///   protects it. An address whose live version moved on keeps serving
    ///   the old one until the new one loads (new before old, step 3), even
    ///   when the new one fails: that failure is what the heartbeat reports;
    /// - 4b: what was prepared or failed for a version the document no
    ///   longer names is forgotten, so the maps track the document;
    /// - 4c: an idle lazy function is closed but keeps its route.
    async fn unload(&self, document: &DesiredDocument, now: DateTime<Utc>) {
        let live: HashMap<FunctionAddress, i32> = document
            .functions
            .iter()
            .filter(|e| e.role == Role::Live)
            .map(|e| (e.address.clone(), e.version))
            .collect();
        // An entry the host could not even read never unloads a good version.
        let protected: HashSet<FunctionAddress> = document
            .unreadable
            .iter()
            .map(|u| u.address.clone())
            .collect();
        let named: HashSet<Key> = document
            .functions
            .iter()
            .map(|e| (e.address.clone(), e.version))
            .chain(
                document
                    .unreadable
                    .iter()
                    .map(|u| (u.address.clone(), u.version)),
            )
            .collect();
        let keep: HashSet<FunctionAddress> = live.keys().chain(protected.iter()).cloned().collect();

        // 4a: loaded, but the address is no longer live.
        for snapshot in self.registry.snapshot() {
            if !keep.contains(&snapshot.address) {
                self.close_and_remove(&snapshot.address).await;
            }
        }
        // 4b: prepared or failed versions the document no longer names.
        {
            let mut state = self.state.lock();
            let gone: Vec<Key> = state
                .version_id_by_key
                .keys()
                .filter(|key| !named.contains(*key))
                .cloned()
                .collect();
            for key in gone {
                if let Some(version_id) = state.version_id_by_key.remove(&key) {
                    state.prepared.remove(&version_id);
                }
            }
            state.failures.retain(|key, _| named.contains(key));
            state.refusals.retain(|key, _| named.contains(key));
            // Routes and per-address load locks go with the address, or the
            // maps only ever grow over the life of the process.
            state
                .lazy_routes
                .retain(|address, _| keep.contains(address));
        }
        self.load_locks
            .lock()
            .retain(|address, _| keep.contains(address));
        // 4c: idle lazy eviction: closed, but the route stays.
        let cutoff = now - IDLE_UNLOAD;
        for snapshot in self.registry.snapshot() {
            let routed = self
                .state
                .lock()
                .lazy_routes
                .contains_key(&snapshot.address);
            if !snapshot.warm && routed && snapshot.last_accessed < cutoff {
                self.close_and_remove(&snapshot.address).await;
            }
        }
    }

    async fn close_and_remove(&self, address: &FunctionAddress) {
        if let Some(removed) = self.registry.remove(address) {
            removed.close().await;
        }
    }

    /// Closes every loaded function (host shutdown).
    pub async fn close_all(&self) {
        for snapshot in self.registry.snapshot() {
            self.close_and_remove(&snapshot.address).await;
        }
    }

    // ── step 5: heartbeat ────────────────────────────────────────────────

    /// The heartbeat for `document`: loaded ⇒ `LOADED`; prepared ⇒
    /// `REGISTERED`; failed ⇒ `FAILED` + error; still fetching ⇒ absent;
    /// then every unreadable entry as `FAILED`.
    pub fn heartbeat_report(&self, document: &DesiredDocument) -> HeartbeatReport {
        let state = self.state.lock();
        let mut loaded = Vec::new();
        for entry in &document.functions {
            let key: Key = (entry.address.clone(), entry.version);
            let load_state = if let Some(reason) = state.failures.get(&key) {
                LoadState::Failed(reason.clone())
            } else if self
                .registry
                .peek(&entry.address)
                .is_some_and(|f| f.version() == entry.version)
            {
                LoadState::Loaded
            } else if state.prepared.contains_key(&entry.version_id) {
                LoadState::Registered
            } else {
                continue;
            };
            loaded.push(LoadedEntry {
                address: entry.address.clone(),
                version: entry.version,
                state: load_state,
            });
        }
        for unreadable in &document.unreadable {
            loaded.push(LoadedEntry {
                address: unreadable.address.clone(),
                version: unreadable.version,
                state: LoadState::Failed(unreadable.reason.clone()),
            });
        }
        HeartbeatReport {
            host_id: self.host_id.clone(),
            pool: self.pool.clone(),
            state: if self.is_draining() {
                HostState::Draining
            } else {
                HostState::Active
            },
            loaded,
            runtimes: self.loaders.runtimes(),
        }
    }

    async fn send_heartbeat(&self, document: &DesiredDocument) {
        let report = self.heartbeat_report(document);
        if let Err(e) = self.control_plane.heartbeat(&report).await {
            tracing::warn!(host_id = %self.host_id, err = %e, "heartbeat failed");
        }
    }

    /// A heartbeat for the last known document, outside a cycle: the host
    /// sends one `DRAINING` beat as it shuts down. Nothing when no document
    /// was ever received.
    pub async fn heartbeat_now(&self) {
        let _cycle = self.cycle.lock().await;
        let document = self.state.lock().document.clone();
        if let Some(document) = document {
            self.send_heartbeat(&document).await;
        }
    }

    // ── what the listener (H5) and metrics read ──────────────────────────

    pub fn document(&self) -> Option<Arc<DesiredDocument>> {
        self.state.lock().document.clone()
    }

    pub fn live_entry(&self, address: &FunctionAddress) -> Option<Entry> {
        self.document()?.live_entry(address).cloned()
    }

    pub fn entry_for(&self, address: &FunctionAddress, version: i32) -> Option<Entry> {
        self.document()?.entry_for(address, version).cloned()
    }

    pub fn entry_for_alias(&self, address: &FunctionAddress, alias: &str) -> Option<Entry> {
        self.document()?.entry_for_alias(address, alias).cloned()
    }

    /// Every address named anywhere in the current document.
    pub fn desired_addresses(&self) -> HashSet<FunctionAddress> {
        self.document()
            .map(|d| d.functions.iter().map(|e| e.address.clone()).collect())
            .unwrap_or_default()
    }

    /// The failure recorded for `(address, version)`, if any (tests, H5).
    pub fn failure(&self, address: &FunctionAddress, version: i32) -> Option<String> {
        self.state
            .lock()
            .failures
            .get(&(address.clone(), version))
            .cloned()
    }

    pub fn is_lazily_routed(&self, address: &FunctionAddress) -> bool {
        self.state.lock().lazy_routes.contains_key(address)
    }

    pub fn has_load_lock(&self, address: &FunctionAddress) -> bool {
        self.load_locks.lock().contains_key(address)
    }
}

/// `Required`: a bundle and a recorded signer must both be present, the
/// bundle must verify, and the extracted identity must equal the recorded
/// one exactly.
fn verify_signature(verifier: &SignatureVerifier, entry: &Entry) -> Result<(), String> {
    let bundle = match &entry.signature_bundle {
        Some(bundle) if !crate::java::is_blank(bundle) => bundle,
        _ => return Err("UNSIGNED".to_owned()),
    };
    let Some(recorded) = &entry.signer else {
        return Err("UNSIGNED".to_owned());
    };
    match verifier.verify(Some(bundle), &entry.digest) {
        Verification::Rejected { reason, .. } => Err(format!("SIGNATURE:{}", reason.name())),
        Verification::Verified { signer, .. } if &signer != recorded => {
            Err("SIGNER_MISMATCH".to_owned())
        }
        Verification::Verified { .. } => Ok(()),
    }
}

/// Records a rotation the moment a new document changes a live entry's
/// webhook secret: the displaced value stays accepted through the next
/// reconcile as well.
fn update_secret_history(state: &mut State, document: &DesiredDocument, reconcile_number: u64) {
    for entry in document.functions.iter().filter(|e| e.role == Role::Live) {
        let new = entry.webhook_signing_secret.clone();
        let old = state.current_secret.get(&entry.address).cloned();
        if old == new {
            continue;
        }
        if let Some(old) = old {
            state
                .previous_secret
                .insert(entry.address.clone(), (old, reconcile_number + 1));
        }
        match new {
            Some(secret) => {
                state.current_secret.insert(entry.address.clone(), secret);
            }
            None => {
                state.current_secret.remove(&entry.address);
            }
        }
    }
}
