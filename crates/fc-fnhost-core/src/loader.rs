//! The pluggable runtime seam (Java `fnhost/load/{FunctionLoader,
//! LoadOutcome,LoadedFunction}.java`).
//!
//! A [`FunctionLoader`] turns one fetched, verified artifact into a running
//! [`FunctionInstance`], selected by the manifest's `runtime`. The Rust host
//! registers none yet: the engine is being chosen by the density spike
//! (plan §5 F0) and lands in H4. A runtime with no registered loader (for
//! example `jvm`, which stays on JVM hosts) is reported `FAILED` with
//! `RUNTIME_UNSUPPORTED` and never fetched or loaded.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fc_function_abi::FunctionAddress;
use tokio::sync::Notify;

use crate::desired::Entry;
use crate::invoke::Invoker;

/// The heartbeat error for a runtime with no loader on this host.
pub const RUNTIME_UNSUPPORTED: &str = "RUNTIME_UNSUPPORTED";

/// How long [`LoadedFunction::close`] waits for in-flight calls.
pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// One loaded version, as its runtime sees it. The listeners call it
/// through [`Invoker`]; the reconciler only ever closes it.
#[async_trait]
pub trait FunctionInstance: Invoker + Send + Sync + 'static {
    /// Releases everything the runtime holds for this version. Called once,
    /// after every in-flight call has drained or the drain timed out.
    async fn close(&self);

    /// For the runtime's own invoke path to reach its concrete type.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// The result of one load attempt. A refusal is routine, not an error.
pub enum LoadOutcome {
    Loaded(Arc<dyn FunctionInstance>),
    /// Recorded as `LOAD:<reason>` (Java's `Refused`: `WASM_INVALID`,
    /// `WASM_IMPORT_NOT_ALLOWED`, `INIT_FAILED`, …).
    Refused {
        reason: String,
        detail: String,
    },
    /// Recorded verbatim (Java's `ContextLoadException` codes, e.g.
    /// `DB_UNSUPPORTED`).
    Failed {
        code: String,
        detail: String,
    },
}

impl std::fmt::Debug for LoadOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadOutcome::Loaded(_) => f.write_str("Loaded"),
            LoadOutcome::Refused { reason, detail } => write!(f, "Refused({reason}: {detail})"),
            LoadOutcome::Failed { code, detail } => write!(f, "Failed({code}: {detail})"),
        }
    }
}

/// What a loader is given: the artifact (a verified file in the cache) and
/// the whole desired-state entry (manifest, config, secrets, owner).
pub struct LoadRequest<'a> {
    pub artifact: &'a Path,
    pub entry: &'a Entry,
}

#[async_trait]
pub trait FunctionLoader: Send + Sync + 'static {
    async fn load(&self, request: LoadRequest<'_>) -> LoadOutcome;
}

/// The registered loaders, keyed by lower-case manifest runtime.
#[derive(Clone, Default)]
pub struct Loaders {
    by_runtime: HashMap<String, Arc<dyn FunctionLoader>>,
}

impl Loaders {
    /// No runtimes: every entry is `RUNTIME_UNSUPPORTED`.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn with(mut self, runtime: &str, loader: Arc<dyn FunctionLoader>) -> Self {
        self.by_runtime.insert(runtime.to_ascii_lowercase(), loader);
        self
    }

    pub fn get(&self, runtime: &str) -> Option<&Arc<dyn FunctionLoader>> {
        self.by_runtime.get(runtime)
    }

    pub fn supports(&self, runtime: &str) -> bool {
        self.by_runtime.contains_key(runtime)
    }
}

/// One loaded version: its instance plus the bookkeeping to unload it
/// safely (in-flight calls drain before the instance closes).
pub struct LoadedFunction {
    address: FunctionAddress,
    version: i32,
    instance: Arc<dyn FunctionInstance>,
    in_flight: AtomicUsize,
    drained: Notify,
    closed: AtomicBool,
}

impl std::fmt::Debug for LoadedFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LoadedFunction[{}@{}]", self.address, self.version)
    }
}

/// An in-flight call; releasing it (drop) lets a pending close proceed.
pub struct InFlight {
    function: Arc<LoadedFunction>,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if self.function.in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.function.drained.notify_waiters();
        }
    }
}

impl LoadedFunction {
    pub fn new(
        address: FunctionAddress,
        version: i32,
        instance: Arc<dyn FunctionInstance>,
    ) -> Arc<Self> {
        Arc::new(Self {
            address,
            version,
            instance,
            in_flight: AtomicUsize::new(0),
            drained: Notify::new(),
            closed: AtomicBool::new(false),
        })
    }

    pub fn address(&self) -> &FunctionAddress {
        &self.address
    }

    pub fn version(&self) -> i32 {
        self.version
    }

    pub fn instance(&self) -> &Arc<dyn FunctionInstance> {
        &self.instance
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Marks one call in flight; `None` once the function is closing.
    pub fn retain(self: &Arc<Self>) -> Option<InFlight> {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        if self.is_closed() {
            if self.in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
                self.drained.notify_waiters();
            }
            return None;
        }
        Some(InFlight {
            function: self.clone(),
        })
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    /// Waits (bounded) for every in-flight call, then closes the instance.
    /// Idempotent.
    pub async fn close(&self) {
        self.close_within(DEFAULT_DRAIN_TIMEOUT).await;
    }

    pub async fn close_within(&self, drain_timeout: Duration) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let drained = tokio::time::timeout(drain_timeout, async {
            loop {
                let notified = self.drained.notified();
                if self.in_flight.load(Ordering::Acquire) == 0 {
                    return;
                }
                notified.await;
            }
        })
        .await;
        if drained.is_err() {
            tracing::warn!(
                address = %self.address,
                version = self.version,
                count = self.in_flight(),
                "closing loaded function before every invocation released: forcing close"
            );
        }
        self.instance.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe(AtomicBool);

    #[async_trait]
    impl crate::invoke::Invoker for Probe {
        async fn invoke(
            &self,
            _context: crate::invoke::InvocationContext,
        ) -> Result<fc_function_abi::Response, crate::invoke::InvokeError> {
            Ok(fc_function_abi::Response::ack())
        }
    }

    #[async_trait]
    impl FunctionInstance for Probe {
        async fn close(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[tokio::test]
    async fn close_waits_for_in_flight_calls() {
        let probe = Arc::new(Probe(AtomicBool::new(false)));
        let function =
            LoadedFunction::new(FunctionAddress::parse("a.b.c").unwrap(), 1, probe.clone());
        let call = function.retain().unwrap();
        let closing = {
            let function = function.clone();
            tokio::spawn(async move { function.close().await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !probe.0.load(Ordering::SeqCst),
            "closed while a call was in flight"
        );
        assert!(
            function.retain().is_none(),
            "a closing function accepts no new calls"
        );
        drop(call);
        closing.await.unwrap();
        assert!(probe.0.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn close_is_bounded_and_idempotent() {
        let probe = Arc::new(Probe(AtomicBool::new(false)));
        let function =
            LoadedFunction::new(FunctionAddress::parse("a.b.c").unwrap(), 1, probe.clone());
        let _stuck = function.retain().unwrap();
        function.close_within(Duration::from_millis(10)).await;
        assert!(probe.0.load(Ordering::SeqCst));
        function.close_within(Duration::from_millis(10)).await;
    }
}
