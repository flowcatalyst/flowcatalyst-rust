//! The function host's JS runtime (owner decisions 6 and 27): `runtime:
//! js` functions, one ES module bundle each, run in V8 isolates through
//! deno_core, beside fc-fnhost-core's WASI components. It plugs into the
//! host through the same seam ([`fc_fnhost_core::loader::FunctionLoader`]
//! for `js`), so the listeners, permits, deadlines, reconciler and
//! heartbeat are the host's own, unchanged.
//!
//! | Piece | Where |
//! |---|---|
//! | V8's platform, the base snapshot, the worker threads (on the host's shared executing budget) | [`engine`] |
//! | V8's foreground tasks for short-lived isolates | [`platform`] |
//! | load: check the bundle by running it, keep its code cache | [`prepare`] |
//! | what a bundle may import (`flowcatalyst:function/*`) | [`modules`] |
//! | the host APIs, `console` and `fetch` (ops) | [`ops`] and `js/bootstrap.js` |
//! | one isolate per request: limits, entering, the call | [`isolate`] |
//! | the `ArrayBuffer` budget | [`allocator`] |
//! | the invoker: outcomes, deadline, watchdog | [`function`] |
//!
//! The API a function is written against is declared, for TypeScript, in
//! `types/flowcatalyst-function.d.ts`: a JS projection of
//! `wit/flowcatalyst-function` (config, secrets, log, events, invocation)
//! plus the web-platform subset a function sees as globals. There is no
//! Node API, no filesystem, no environment and no network beyond `fetch`
//! under the manifest's `httpAllow`.
//!
//! **Load refusals** (the heartbeat's `LOAD:<code>`): `JS_INVALID`,
//! `JS_IMPORT_NOT_ALLOWED`, `JS_ENTRYPOINT_NOT_EXPORTED`, `JS_INIT_FAILED`.

pub mod allocator;
pub mod engine;
mod function;
pub mod isolate;
pub mod modules;
pub mod ops;
pub mod platform;
pub mod prepare;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fc_fnhost_core::emit::Emitter;
use fc_fnhost_core::env::HostEnv;
use fc_fnhost_core::exec::ExecBudget;
use fc_fnhost_core::loader::{FunctionLoader, LoadOutcome, LoadRequest, Loaders};
use fc_fnhost_core::wasm::egress::HttpAllowlist;
use fc_fnhost_core::wasm::output::GuestLogger;
use fc_function_model::FunctionLimits;

pub use function::JsFunction;
pub use prepare::{JS_ENTRYPOINT_NOT_EXPORTED, JS_IMPORT_NOT_ALLOWED, JS_INIT_FAILED, JS_INVALID};

/// A `db[]` on a `js` function: the WASM runtime's load-failure code for a
/// database it cannot serve.
pub const DB_UNSUPPORTED: &str = "DB_UNSUPPORTED";

/// How long a bundle's top-level code may run at load.
pub const DEFAULT_INIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Everything the runtime is configured with.
#[derive(Debug, Clone)]
pub struct JsSettings {
    /// Worker threads (`FC_FN_MAX_EXECUTING`, so JS alone can use the
    /// whole budget).
    pub workers: usize,
    /// The executing permits every runtime on the host shares: at most
    /// `FC_FN_MAX_EXECUTING` guests, WASM and JS together, execute at once.
    pub budget: ExecBudget,
    /// How long a bundle's top-level code may run at load.
    pub init_timeout: Duration,
}

impl JsSettings {
    /// From the host's environment, on `budget`, the host's one executing
    /// budget (shared with its WASM runtime).
    pub fn from_env(env: &HostEnv, budget: &ExecBudget) -> Self {
        Self {
            workers: env.max_executing.max(1),
            budget: budget.clone(),
            init_timeout: DEFAULT_INIT_TIMEOUT,
        }
    }
}

/// The runtime every JS function on this host shares.
pub struct JsRuntime {
    workers: Arc<engine::Workers>,
    /// The base snapshot, where isolates are made from one.
    base: Option<&'static [u8]>,
    http: reqwest::Client,
    /// The host's own runtime, where outbound calls and emits run.
    host_runtime: Option<tokio::runtime::Handle>,
    init_timeout: Duration,
}

impl JsRuntime {
    /// Starts V8 (once per process) and the workers. Call it inside the
    /// host's tokio runtime.
    pub fn new(settings: JsSettings) -> Result<Arc<Self>, String> {
        let base = engine::init_v8()?;
        let workers = engine::Workers::start(settings.workers, settings.budget.clone())?;
        tracing::info!(
            workers = workers.len(),
            max_executing = settings.budget.limit(),
            v8 = deno_core::v8::VERSION_STRING,
            "js runtime started"
        );
        Ok(Arc::new(Self {
            workers: Arc::new(workers),
            base,
            http: ops::http_client()?,
            host_runtime: tokio::runtime::Handle::try_current().ok(),
            init_timeout: settings.init_timeout,
        }))
    }
}

/// The [`FunctionLoader`] for `runtime: js`.
pub struct JsLoader {
    runtime: Arc<JsRuntime>,
}

impl JsLoader {
    /// The manifest runtimes this loader serves.
    pub const RUNTIMES: [&'static str; 1] = ["js"];

    pub fn new(runtime: Arc<JsRuntime>) -> Self {
        Self { runtime }
    }

    /// `loaders` plus this loader for each of [`JsLoader::RUNTIMES`].
    pub fn register(self: Arc<Self>, mut loaders: Loaders) -> Loaders {
        for runtime in Self::RUNTIMES {
            loaders = loaders.with(runtime, self.clone());
        }
        loaders
    }
}

#[async_trait]
impl FunctionLoader for JsLoader {
    async fn load(&self, request: LoadRequest<'_>) -> LoadOutcome {
        let entry = request.entry;
        let manifest = &entry.manifest;
        // Database access (`db[]`) is WASM-only for now; a JS module
        // mirroring `flowcatalyst:function/db` slots in later. Refused, not
        // ignored, so the function never runs without what it declared.
        if !manifest.db.is_empty() {
            return LoadOutcome::Failed {
                code: DB_UNSUPPORTED.to_owned(),
                detail:
                    "the js runtime has no database access yet: db[] is for wasm and component \
                         functions"
                        .to_owned(),
            };
        }
        let memory_mb = manifest
            .limits
            .wasm_memory_mb
            .unwrap_or(FunctionLimits::DEFAULT_WASM_MEMORY_MB)
            .max(1) as usize;
        let cap_bytes = memory_mb << 20;
        let limits = isolate::Limits::of(cap_bytes);
        // The keys the manifest declares, as the WASM runtime keeps them.
        let pick = |values: &std::collections::BTreeMap<String, String>, keys: &[String]| {
            keys.iter()
                .filter_map(|k| values.get(k).map(|v| (k.clone(), v.clone())))
                .collect::<HashMap<_, _>>()
        };
        let mut secrets = pick(&entry.secrets, &manifest.secrets);
        secrets.retain(|_, v| !v.is_empty());
        let version = Arc::new(ops::VersionShared {
            address: entry.address.clone(),
            version: entry.version,
            logger: GuestLogger::for_address(&entry.address.render()),
            config: pick(&entry.config, &manifest.config),
            secrets,
            allow: Arc::new(HttpAllowlist::new(
                manifest.http_allow.iter().map(String::as_str),
            )),
            emitter: Emitter {
                control_plane: request.control_plane.clone(),
                host_id: request.host_id.to_owned(),
                host_runtime: self.runtime.host_runtime.clone(),
            },
            body_cap: cap_bytes,
            http: self.runtime.http.clone(),
            host_runtime: self.runtime.host_runtime.clone(),
        });
        let prepared = {
            let artifact = request.artifact.to_owned();
            let entrypoint = manifest.entrypoint.clone();
            let version = version.clone();
            let init_timeout = self.runtime.init_timeout;
            let base = self.runtime.base;
            tokio::task::spawn_blocking(move || {
                let bundle = std::fs::read(&artifact).map_err(|e| prepare::Refusal {
                    reason: JS_INVALID,
                    detail: format!("unreadable: {}: {e}", artifact.display()),
                })?;
                prepare::prepare(base, &bundle, &entrypoint, limits, version, init_timeout)
            })
            .await
        };
        let prepared = match prepared {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(refusal)) => {
                return LoadOutcome::Refused {
                    reason: refusal.reason.to_owned(),
                    detail: refusal.detail,
                }
            }
            Err(e) => {
                return LoadOutcome::Refused {
                    reason: JS_INIT_FAILED.to_owned(),
                    detail: format!("the load panicked: {e}"),
                }
            }
        };
        tracing::debug!(
            address = %entry.address,
            version = entry.version,
            held_bytes = prepared.code.held_bytes(),
            code_cache = prepared.code.code_cache.is_some(),
            took_ms = prepared.took.as_millis() as u64,
            "js function prepared"
        );
        LoadOutcome::Loaded(Arc::new(JsFunction::new(
            self.runtime.workers.clone(),
            self.runtime.base,
            version,
            prepared.code,
            limits,
        )))
    }
}

/// The runtimes the deployed host loads: fc-fnhost-core's WASI components
/// (`component`, `wasm`) and V8 isolates (`js`); `jvm` stays
/// `RUNTIME_UNSUPPORTED`. Shared by `fc-server`'s function-host role,
/// `fc-dev`'s in-process host and the end-to-end tests. Both runtimes
/// execute on one [`ExecBudget`] of `FC_FN_MAX_EXECUTING` permits.
pub fn loaders(env: &HostEnv) -> Result<Loaders, String> {
    let budget = ExecBudget::new(env.max_executing);
    let loaders = fc_fnhost_core::host::wasm_loaders_with(env, &budget)?;
    let js = JsRuntime::new(JsSettings::from_env(env, &budget))?;
    Ok(Arc::new(JsLoader::new(js)).register(loaders))
}
