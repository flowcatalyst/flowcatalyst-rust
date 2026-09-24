//! Assembles and runs the host process (Java `fnhost/FnHost.java` and
//! `FnHostMain.java`).
//!
//! Start-up order, as Java (`FnHost.java:131-215`): the observability
//! listener binds first (so `/ready` can answer `STARTING`); the first
//! reconcile runs, and nothing it throws stops start-up; the reconcile loop
//! starts; then the function listener binds (after the first reconcile, so
//! the host never 404s a function it simply has not heard of yet).
//!
//! Shutdown: drain (heartbeats say `DRAINING`), stop accepting, wait for
//! in-flight calls up to `FC_DRAIN_TIMEOUT_SECONDS`, stop the loop, close
//! every loaded function, and close the observability listener last.

use std::future::Future;
use std::io::Write;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::FutureExt;
use reqwest::Client;

use crate::artifact::{
    ArtifactCache, ArtifactStore, ArtifactStores, FileSource, OciSource, PlatformSource,
    RegistryCredentials, DEFAULT_MAX_BYTES,
};
use crate::clock::{SharedClock, SystemClock};
use crate::control_plane::{ControlPlane, HttpControlPlane, CONNECT_TIMEOUT};
use crate::env::{EnvReader, HostEnv};
use crate::loader::Loaders;
use crate::metrics::FnMetrics;
use crate::observability::{Observability, Probes};
use crate::reconcile_loop::ReconcileLoop;
use crate::reconciler::Reconciler;
use crate::registry::FunctionRegistry;
use crate::token::TokenSource;

/// The function listeners' hook ([`crate::listener::FnListener`] is the
/// private `:8080` and public `:8081` listeners behind it). A host with no
/// listener is fine: it reconciles and heartbeats, and counts as bound.
#[async_trait]
pub trait Listener: Send + Sync {
    /// Binds and starts serving; returns once listening.
    async fn start(
        &self,
        reconciler: Arc<Reconciler>,
        metrics: Arc<FnMetrics>,
    ) -> std::io::Result<()>;
    /// The bound private port, once started.
    fn port(&self) -> Option<u16>;
    /// Whether every socket is still accepting; `/ready` reports
    /// `LISTENER_DOWN` otherwise.
    fn is_serving(&self) -> bool {
        true
    }
    /// Stops accepting new calls.
    async fn drain(&self);
    /// Waits up to `timeout` for in-flight calls, then closes.
    async fn close(&self, timeout: Duration);
    /// The bound public port, when there is a public listener.
    fn public_port(&self) -> Option<u16> {
        None
    }
}

/// Everything [`FnHost`] needs, built by [`FnHost::new`] from the
/// environment, or by hand in tests (a fake control plane, a test loader).
pub struct HostParts {
    pub control_plane: Arc<dyn ControlPlane>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub loaders: Loaders,
    pub listener: Option<Arc<dyn Listener>>,
    pub clock: SharedClock,
    /// The reconcile interval; [`crate::reconcile_loop::INTERVAL`] in production.
    pub interval: Duration,
}

pub struct FnHost {
    env: HostEnv,
    reconciler: Arc<Reconciler>,
    metrics: Arc<FnMetrics>,
    reconcile_loop: Arc<ReconcileLoop>,
    listener: Option<Arc<dyn Listener>>,
    listener_bound: Arc<AtomicBool>,
    startup_complete: Arc<AtomicBool>,
    observability: Option<Observability>,
    closed: bool,
}

impl FnHost {
    /// The production assembly: token source, the `file`/`oci`/`platform`
    /// stores over one cache, the HTTP control plane.
    pub fn new(
        env: HostEnv,
        loaders: Loaders,
        listener: Option<Arc<dyn Listener>>,
    ) -> std::io::Result<Self> {
        let clock: SharedClock = Arc::new(SystemClock);
        let control_client = HttpControlPlane::default_client();
        let token_source = Arc::new(TokenSource::new(
            control_client.clone(),
            env.platform_url.clone(),
            env.client_id.clone(),
            env.client_secret.clone(),
            clock.clone(),
        ));
        let artifact_client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("a plain reqwest client builds");
        let cache = ArtifactCache::new(env.cache_dir.clone(), DEFAULT_MAX_BYTES)?;
        let artifacts = ArtifactStores::new(cache)
            .with_source("file", Arc::new(FileSource))
            .with_source("oci", Arc::new(OciSource::new(RegistryCredentials::none())))
            .with_source(
                "platform",
                Arc::new(PlatformSource::new(
                    artifact_client,
                    env.platform_url.clone(),
                    token_source.clone(),
                )),
            );
        let control_plane =
            HttpControlPlane::new(control_client, env.platform_url.clone(), token_source);
        Ok(Self::with_parts(
            env,
            HostParts {
                control_plane: Arc::new(control_plane),
                artifacts: Arc::new(artifacts),
                loaders,
                listener,
                clock,
                interval: crate::reconcile_loop::INTERVAL,
            },
        ))
    }

    pub fn with_parts(env: HostEnv, parts: HostParts) -> Self {
        let registry = Arc::new(FunctionRegistry::new(env.max_loaded, parts.clock.clone()));
        let reconciler = Arc::new(Reconciler::new(
            env.pool.clone(),
            env.host_id.clone(),
            parts.control_plane,
            parts.artifacts,
            env.signatures.clone(),
            parts.loaders,
            registry.clone(),
        ));
        let metrics = Arc::new(FnMetrics::new(registry));
        reconciler.set_observer(metrics.clone());
        {
            let metrics = metrics.clone();
            let weak = Arc::downgrade(&reconciler);
            reconciler.add_post_reconcile_listener(Arc::new(move || {
                if let Some(reconciler) = weak.upgrade() {
                    metrics.sweep_dead_addresses(&reconciler.desired_addresses());
                }
            }));
        }
        let reconcile_loop = Arc::new(ReconcileLoop::with_interval(
            reconciler.clone(),
            parts.clock,
            parts.interval,
        ));
        Self {
            env,
            reconciler,
            metrics,
            reconcile_loop,
            listener: parts.listener,
            listener_bound: Arc::new(AtomicBool::new(false)),
            startup_complete: Arc::new(AtomicBool::new(false)),
            observability: None,
            closed: false,
        }
    }

    pub fn reconciler(&self) -> &Arc<Reconciler> {
        &self.reconciler
    }

    pub fn metrics(&self) -> &Arc<FnMetrics> {
        &self.metrics
    }

    pub fn env(&self) -> &HostEnv {
        &self.env
    }

    /// The observability listener's bound port, once started.
    pub fn metrics_port(&self) -> Option<u16> {
        self.observability.as_ref().map(Observability::port)
    }

    /// The function listener's bound port, if there is one.
    pub fn port(&self) -> Option<u16> {
        self.listener.as_ref().and_then(|l| l.port())
    }

    /// The public listener's bound port, if there is one.
    pub fn public_port(&self) -> Option<u16> {
        self.listener.as_ref().and_then(|l| l.public_port())
    }

    /// Asks the loop for a reconcile now (coalesced).
    pub fn trigger_reconcile(&self) {
        self.reconcile_loop.trigger();
    }

    pub async fn start(&mut self) -> std::io::Result<()> {
        let loop_for_probe = self.reconcile_loop.clone();
        let bound = self.listener_bound.clone();
        let listener_for_probe = self.listener.clone();
        let started = self.startup_complete.clone();
        self.observability = Some(Observability::start(
            self.env.metrics_port,
            Probes {
                reconciler: self.reconciler.clone(),
                metrics: self.metrics.clone(),
                listener_bound: Arc::new(move || {
                    bound.load(Ordering::SeqCst)
                        && listener_for_probe.as_ref().is_none_or(|l| l.is_serving())
                }),
                reconcile_loop_alive: Arc::new(move || loop_for_probe.is_alive()),
                startup_complete: Arc::new(move || started.load(Ordering::SeqCst)),
            },
        )?);

        // Nothing the first reconcile does may stop start-up: a host that
        // serves whatever it managed to load beats one that never binds.
        let first =
            AssertUnwindSafe(self.reconciler.reconcile_once(chrono::Utc::now())).catch_unwind();
        if first.await.is_err() {
            tracing::error!("first reconcile failed during startup; continuing to bind the function listener regardless");
        }
        self.reconcile_loop.start();
        if let Some(listener) = &self.listener {
            listener
                .start(self.reconciler.clone(), self.metrics.clone())
                .await?;
        }
        self.listener_bound.store(true, Ordering::SeqCst);
        self.startup_complete.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Drains and shuts down. Idempotent.
    pub async fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.reconciler.drain();
        // Tell the platform right away rather than at the next 15 s cycle.
        let _ =
            tokio::time::timeout(Duration::from_secs(10), self.reconciler.heartbeat_now()).await;
        if let Some(listener) = &self.listener {
            listener.drain().await;
            listener
                .close(Duration::from_secs(self.env.drain_timeout_seconds))
                .await;
        }
        self.reconcile_loop.close().await;
        self.reconciler.close_all().await;
        if let Some(observability) = self.observability.take() {
            observability.close().await;
        }
    }
}

/// The process: logging, environment (exit 2 with one line naming every
/// bad variable), start, then run until `shutdown` resolves, or exit right
/// after start with `FC_EXIT_AFTER_START`. Returns the exit code.
/// `loaders` builds the runtimes and `listener` the function listener, both
/// from the loaded environment; a runtime that cannot start exits 1.
pub async fn run(
    env_reader: EnvReader,
    err: &mut (dyn Write + Send),
    loaders: impl FnOnce(&HostEnv) -> Result<Loaders, String>,
    listener: impl FnOnce(&HostEnv) -> Option<Arc<dyn Listener>>,
    shutdown: impl Future<Output = ()>,
) -> i32 {
    crate::logging::init(&env_reader);
    let env = match HostEnv::load(&env_reader) {
        Ok(env) => env,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 2;
        }
    };
    let exit_after_start = env.exit_after_start;
    let loaders = match loaders(&env) {
        Ok(loaders) => loaders,
        Err(e) => {
            let _ = writeln!(err, "cannot start the function runtime: {e}");
            return 1;
        }
    };
    let listener = listener(&env);
    let mut host = match FnHost::new(env, loaders, listener) {
        Ok(host) => host,
        Err(e) => {
            let _ = writeln!(err, "cannot create the artifact cache directory: {e}");
            return 1;
        }
    };
    if let Err(e) = host.start().await {
        tracing::error!(err = %e, "function host failed to start");
        host.close().await;
        return 1;
    }
    tracing::info!(
        pool = %host.env().pool,
        host_id = %host.env().host_id,
        port = host.port(),
        public_port = host.public_port(),
        metrics_port = host.metrics_port(),
        "function host started"
    );
    if exit_after_start {
        host.close().await;
        return 0;
    }
    shutdown.await;
    tracing::info!("shutdown signal received");
    host.close().await;
    0
}

/// SIGTERM or Ctrl-C. The handlers are installed when this is called (not
/// when the future is first polled), so a signal during start-up is not
/// lost. Call it inside the runtime.
pub fn shutdown_signal() -> impl Future<Output = ()> {
    #[cfg(unix)]
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing the SIGTERM handler");
    async move {
        #[cfg(unix)]
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}
