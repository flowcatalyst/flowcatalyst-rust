//! The WASM runtime (plan §5 H4): `runtime: wasm` functions as WASI 0.2
//! components exporting `wasi:http/incoming-handler`, on plain wasmtime
//! (the design `docs/function-runner-density.md` §5-§8 chose), plus the
//! optional `flowcatalyst:function` interfaces (`wit/flowcatalyst-function`).
//!
//! | Piece | Where |
//! |---|---|
//! | one [`Engine`], pooling allocator, epoch ticker, `.cwasm` fingerprint | [`engine`] |
//! | load-time checks and their refusal codes | [`inspect`] |
//! | the `.cwasm` cache and its invariant | [`cwasm`] |
//! | a store's WASI, `wasi:http` and `flowcatalyst:function` host side | `guest` |
//! | guest log lines on `fn.<address>` | [`output`] |
//! | the invoker: instance per request, deadline, outcomes | `function` |
//!
//! **Where guests run.** Not on the listener's tokio workers: every guest
//! runs on its own runtime of `FC_FN_MAX_EXECUTING` worker threads (cores
//! minus one by default), so at most that many guests execute at any moment
//! and the listener always has a core to accept, route and answer on. A
//! guest yields its thread at every epoch tick (1 ms), so the guests share
//! those threads round-robin; one waiting on I/O (outbound HTTP, an emit,
//! a sleep) holds no thread at all. This is on top of the listener's own
//! permits (host-wide and per function, which bound how many invocations
//! are in flight at all).
//!
//! **Load refusals** (the heartbeat's `LOAD:<code>`): `WASM_INVALID`,
//! `WASM_CORE_MODULE_UNSUPPORTED`, `WASM_IMPORT_NOT_ALLOWED`,
//! `WASM_ENTRYPOINT_NOT_EXPORTED`, `WASM_MEMORY_OVER_CAP`.

pub mod cwasm;
pub mod engine;
mod function;
mod guest;
pub mod inspect;
pub mod output;

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use wasmtime::component::Linker;
use wasmtime::Engine;
use wasmtime_wasi_http::p2::bindings::ProxyPre;

pub use cwasm::Source as CompileSource;
pub use engine::EngineSettings;
pub use function::WasmFunction;
pub use guest::INVALID_EVENT_DATA_NOT_JSON;
pub use inspect::{
    WASM_CORE_MODULE_UNSUPPORTED, WASM_ENTRYPOINT_NOT_EXPORTED, WASM_IMPORT_NOT_ALLOWED,
    WASM_INVALID, WASM_MEMORY_OVER_CAP,
};

use crate::loader::{FunctionLoader, LoadOutcome, LoadRequest};
use crate::manifest::DEFAULT_WASM_MEMORY_MB;
use guest::{Emitter, FunctionShared, GuestState};
use inspect::Refusal;
use output::GuestLogger;

/// Everything the runtime is configured with.
#[derive(Debug, Clone)]
pub struct WasmSettings {
    pub engine: EngineSettings,
    /// `FC_FN_MAX_EXECUTING`: guests executing at once, host-wide.
    pub max_executing: usize,
    /// The artifact cache root (`FC_FN_CACHE_DIR`); `.cwasm` files go in
    /// its `cwasm/` directory.
    pub cache_dir: PathBuf,
}

impl WasmSettings {
    /// From the host's environment: the pool holds one instance per
    /// in-flight invocation the listener can admit (`FC_FN_MAX_CONCURRENCY`),
    /// plus headroom.
    pub fn from_env(env: &crate::env::HostEnv) -> Self {
        Self {
            engine: EngineSettings {
                max_instances: u32::try_from(env.max_concurrency.max(1))
                    .unwrap_or(u32::MAX)
                    .saturating_add(16),
                ..EngineSettings::default()
            },
            max_executing: env.max_executing,
            cache_dir: env.cache_dir.clone(),
        }
    }
}

/// The runtime every WASM function on this host shares.
pub struct WasmRuntime {
    engine: Engine,
    linker: Linker<GuestState>,
    cwasm: cwasm::CwasmCache,
    guests: GuestRuntime,
    /// The runtime the host itself runs on, for control-plane calls made on
    /// a guest's behalf.
    host_runtime: Option<tokio::runtime::Handle>,
    _ticker: engine::EpochTicker,
}

impl WasmRuntime {
    /// Builds the engine, the linker and the guest runtime, and starts the
    /// epoch ticker. Call it inside the host's tokio runtime.
    pub fn new(settings: WasmSettings) -> Result<Arc<Self>, String> {
        let engine = engine::engine(&settings.engine)
            .map_err(|e| format!("the wasm engine did not start: {e:#}"))?;
        let linker =
            guest::linker(&engine).map_err(|e| format!("the wasm linker did not build: {e:#}"))?;
        let fingerprint = engine::fingerprint(&engine);
        let guests = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(settings.max_executing.max(1))
            .thread_name("fn-guest")
            .enable_all()
            .build()
            .map_err(|e| format!("the guest runtime did not start: {e}"))?;
        let ticker = engine::EpochTicker::start(&engine)
            .map_err(|e| format!("the epoch ticker did not start: {e}"))?;
        tracing::info!(
            max_executing = settings.max_executing.max(1),
            max_instances = settings.engine.max_instances,
            engine = %fingerprint,
            "wasm runtime started"
        );
        Ok(Arc::new(Self {
            cwasm: cwasm::CwasmCache::new(&settings.cache_dir, &fingerprint),
            engine,
            linker,
            guests: GuestRuntime(Some(guests)),
            host_runtime: tokio::runtime::Handle::try_current().ok(),
            _ticker: ticker,
        }))
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Where this engine's `.cwasm` files live.
    pub fn cwasm_dir(&self) -> &Path {
        self.cwasm.dir()
    }

    fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.guests.spawn(future)
    }

    /// Checks, compiles (or loads the `.cwasm`) and pre-links one artifact.
    /// Blocking: CPU and file I/O. Also returns the component's declared
    /// maximum memory, when every memory declares one.
    fn prepare(
        &self,
        artifact: &Path,
        digest_hex: &str,
        entrypoint: &str,
        cap_bytes: u64,
    ) -> Result<(ProxyPre<GuestState>, CompileSource, Option<u64>), Refusal> {
        let bytes = std::fs::read(artifact).map_err(|e| {
            Refusal::new(
                WASM_INVALID,
                format!("unreadable: {}: {e}", artifact.display()),
            )
        })?;
        let accepted = inspect::check(&bytes, entrypoint, cap_bytes)?;
        let (component, source) = self
            .cwasm
            .load(&self.engine, digest_hex, &bytes)
            .map_err(|why| Refusal::new(WASM_INVALID, why))?;
        let pre = self
            .linker
            .instantiate_pre(&component)
            .map_err(|e| Refusal::new(WASM_IMPORT_NOT_ALLOWED, format!("{e:#}")))?;
        let pre = ProxyPre::new(pre)
            .map_err(|e| Refusal::new(WASM_ENTRYPOINT_NOT_EXPORTED, format!("{e:#}")))?;
        Ok((pre, source, accepted.declared_max_memory))
    }
}

/// The guests' own tokio runtime, shut down in the background when the
/// last function lets go of it (dropping a runtime inside another one
/// would panic).
struct GuestRuntime(Option<tokio::runtime::Runtime>);

impl GuestRuntime {
    fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.0
            .as_ref()
            .expect("the guest runtime lives as long as the WasmRuntime")
            .spawn(future)
    }
}

impl Drop for GuestRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

/// The [`FunctionLoader`] for `runtime: wasm`.
pub struct WasmLoader {
    runtime: Arc<WasmRuntime>,
}

impl WasmLoader {
    pub fn new(runtime: Arc<WasmRuntime>) -> Self {
        Self { runtime }
    }

    pub fn runtime(&self) -> &Arc<WasmRuntime> {
        &self.runtime
    }
}

#[async_trait]
impl FunctionLoader for WasmLoader {
    async fn load(&self, request: LoadRequest<'_>) -> LoadOutcome {
        let entry = request.entry;
        let memory_mb = entry
            .manifest
            .limits
            .wasm_memory_mb
            .unwrap_or(DEFAULT_WASM_MEMORY_MB)
            .max(1) as u64;
        let cap_bytes = (memory_mb << 20).min(engine::MAX_MEMORY_BYTES as u64);
        let prepared = {
            let runtime = self.runtime.clone();
            let artifact = request.artifact.to_owned();
            let digest = entry.digest.hex().to_owned();
            let entrypoint = entry.manifest.entrypoint.clone();
            tokio::task::spawn_blocking(move || {
                runtime.prepare(&artifact, &digest, &entrypoint, cap_bytes)
            })
            .await
        };
        let (pre, source, declared_max) = match prepared {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(refusal)) => {
                return LoadOutcome::Refused {
                    reason: refusal.reason.to_owned(),
                    detail: refusal.detail,
                }
            }
            Err(e) => {
                return LoadOutcome::Refused {
                    reason: WASM_INVALID.to_owned(),
                    detail: format!("the load panicked: {e}"),
                }
            }
        };
        tracing::debug!(address = %entry.address, version = entry.version, source = ?source, "wasm function prepared");
        let raw = &entry.manifest.raw;
        let declared = |key: &str| -> Vec<String> {
            match raw.get(key) {
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|s| !crate::java::is_blank(s))
                    .map(str::to_owned)
                    .collect(),
                _ => Vec::new(),
            }
        };
        let pick = |values: &std::collections::BTreeMap<String, String>, keys: Vec<String>| {
            keys.into_iter()
                .filter_map(|k| values.get(&k).map(|v| (k, v.clone())))
                .collect::<HashMap<_, _>>()
        };
        let mut secrets = pick(&entry.secrets, declared("secrets"));
        secrets.retain(|_, v| !v.is_empty());
        let shared = Arc::new(FunctionShared {
            address: entry.address.clone(),
            version: entry.version,
            logger: GuestLogger::for_address(&entry.address.render()),
            config: pick(&entry.config, declared("config")),
            secrets,
            memory_limit: declared_max.map_or(cap_bytes, |max| max.min(cap_bytes)) as usize,
            response_cap: cap_bytes as usize,
            emitter: Emitter {
                control_plane: request.control_plane.clone(),
                host_id: request.host_id.to_owned(),
                host_runtime: self.runtime.host_runtime.clone(),
            },
        });
        LoadOutcome::Loaded(Arc::new(WasmFunction::new(
            self.runtime.clone(),
            pre,
            shared,
        )))
    }
}
