//! The function listeners (Java `fnhost/http/FnHttpServer.java` and its
//! helpers; spec `function-host-listener.md`, `function-public-routes.md`
//! §3-§4, `function-zones-and-aliases.md` §4).
//!
//! Two entries share one pipeline, permits, pinned versions and
//! authenticators:
//! - **private** (`FC_FN_PORT`, default 8080): `/functions/{address}[:{version}]/{path}`;
//! - **public** (`FC_FN_PUBLIC_PORT`, default 8081, `off` disables it):
//!   `Host` → public route → function, with alias-prefixed hostnames; no
//!   by-address and no versioned access.
//!
//! Both speak HTTP/1.1 and cleartext HTTP/2 (prior knowledge). The HTTP
//! contract (paths, statuses, error codes and bodies, headers) is Java's.
//! Guests are reached only through [`crate::invoke::Invoker`].
//!
//! | Module | Java source |
//! |---|---|
//! | [`pipeline`] | `FnHttpServer` (the 10-step pipeline, versioned calls, the public entry) |
//! | `route_path` | `RoutePath` |
//! | [`permits`] | `Permits` |
//! | [`pinned`] | `PinnedVersions` |
//! | [`webhook`] | `WebhookVerifier`, `router/wire/WebhookSigner` |
//! | [`jwks`], [`bearer`] | `JwksKeySource`, `BearerAuthenticator`, `shared/auth/JwtVerifier`, `TokenClaims` |
//! | `cors` | `CorsPolicy` |
//! | [`public_routes`] | `route/PublicRouteTable`, `TrustedProxies.isIpLiteral` |
//! | `answer` | `HttpAnswer`, `ErrorBody`, `outcomeFor` |

mod answer;
pub mod bearer;
mod cors;
pub mod jwks;
pub mod permits;
pub mod pinned;
pub mod pipeline;
pub mod public_routes;
mod route_path;
pub mod webhook;

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use hyper::service::service_fn;
use parking_lot::{Mutex, RwLock};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::clock::{SharedClock, SystemClock};
use crate::control_plane::CONNECT_TIMEOUT;
use crate::desired::DesiredDocument;
use crate::env::{HostEnv, PublicPort, TrustedProxies};
use crate::host::Listener;
use crate::metrics::{FnMetrics, ListenerEntry};
use crate::reconciler::Reconciler;

use self::bearer::BearerAuthenticator;
use self::jwks::JwksKeySource;
use self::permits::Permits;
use self::pinned::PinnedVersions;
use self::public_routes::PublicRouteTable;

/// What the listeners need beyond the reconciler.
#[derive(Clone)]
pub struct ListenerConfig {
    /// The interface both listeners bind (Java binds `0.0.0.0`).
    pub bind: IpAddr,
    /// `FC_FN_PORT`; 0 binds an ephemeral port.
    pub port: u16,
    /// `FC_FN_PUBLIC_PORT`; `None` starts no public listener.
    pub public_port: Option<u16>,
    /// `FC_FN_MAX_CONCURRENCY`, the host-wide permit ceiling.
    pub max_concurrency: i32,
    /// Where `/.well-known/openid-configuration` lives (`FC_FN_PLATFORM_URL`).
    pub platform_url: String,
    /// `FC_FN_TRUSTED_PROXIES`.
    pub trusted_proxies: TrustedProxies,
    pub clock: SharedClock,
}

impl ListenerConfig {
    pub fn from_env(env: &HostEnv) -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: env.port,
            public_port: match env.public_port {
                PublicPort::Port(port) => Some(port),
                PublicPort::Disabled => None,
            },
            max_concurrency: env.max_concurrency,
            platform_url: env.platform_url.clone(),
            trusted_proxies: env.trusted_proxies.clone(),
            clock: Arc::new(SystemClock),
        }
    }
}

/// State every request shares.
pub(crate) struct Shared {
    reconciler: Arc<Reconciler>,
    metrics: Arc<FnMetrics>,
    permits: Arc<Permits>,
    pinned: Arc<PinnedVersions>,
    bearer: BearerAuthenticator,
    clock: SharedClock,
    trusted_proxies: TrustedProxies,
    draining: Arc<AtomicBool>,
    /// The public route table, rebuilt when the document changes.
    routes: RwLock<Option<(Arc<DesiredDocument>, Arc<PublicRouteTable>)>>,
}

impl Shared {
    fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    fn route_table(&self, document: &Arc<DesiredDocument>) -> Arc<PublicRouteTable> {
        if let Some((cached_for, table)) = &*self.routes.read() {
            if Arc::ptr_eq(cached_for, document) {
                return table.clone();
            }
        }
        let table = Arc::new(PublicRouteTable::of(&document.public_routes));
        *self.routes.write() = Some((document.clone(), table.clone()));
        table
    }
}

struct Server {
    stop: watch::Sender<Option<Duration>>,
    task: tokio::task::JoinHandle<()>,
    serving: Arc<AtomicBool>,
}

struct Running {
    shared: Arc<Shared>,
    servers: Vec<Server>,
}

/// Both listeners, plugged into [`crate::host::FnHost`] as its
/// [`Listener`].
pub struct FnListener {
    config: ListenerConfig,
    draining: Arc<AtomicBool>,
    port: OnceLock<u16>,
    public_port: OnceLock<u16>,
    running: Mutex<Option<Running>>,
}

impl FnListener {
    pub fn new(config: ListenerConfig) -> Self {
        Self {
            config,
            draining: Arc::new(AtomicBool::new(false)),
            port: OnceLock::new(),
            public_port: OnceLock::new(),
            running: Mutex::new(None),
        }
    }

    pub fn from_env(env: &HostEnv) -> Self {
        Self::new(ListenerConfig::from_env(env))
    }

    /// The shared permits, once started (tests, diagnostics).
    pub fn permits(&self) -> Option<Arc<Permits>> {
        self.running
            .lock()
            .as_ref()
            .map(|r| r.shared.permits.clone())
    }

    /// The pinned versions, once started (tests, diagnostics).
    pub fn pinned_versions(&self) -> Option<Arc<PinnedVersions>> {
        self.running
            .lock()
            .as_ref()
            .map(|r| r.shared.pinned.clone())
    }

    /// The JWKS source, once started (tests, diagnostics).
    pub fn key_source(&self) -> Option<Arc<JwksKeySource>> {
        self.running
            .lock()
            .as_ref()
            .map(|r| r.shared.bearer.key_source().clone())
    }
}

#[async_trait]
impl Listener for FnListener {
    async fn start(
        &self,
        reconciler: Arc<Reconciler>,
        metrics: Arc<FnMetrics>,
    ) -> std::io::Result<()> {
        let config = &self.config;
        let max_concurrency = usize::try_from(config.max_concurrency)
            .ok()
            .filter(|n| *n >= 1)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "FC_FN_MAX_CONCURRENCY must be at least 1: {}",
                        config.max_concurrency
                    ),
                )
            })?;
        let permits = Arc::new(Permits::new(max_concurrency));
        metrics.permits_ready(permits.clone());
        let pinned = Arc::new(PinnedVersions::new(reconciler.clone()));
        {
            // A pinned version is closed once its version leaves desired
            // state: swept after every reconcile.
            let pinned = Arc::downgrade(&pinned);
            reconciler.add_post_reconcile_listener(Arc::new(move || {
                if let Some(pinned) = pinned.upgrade() {
                    pinned.sweep();
                }
            }));
        }
        // Java's `HttpClient.newHttpClient()` never follows redirects.
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(std::io::Error::other)?;
        let keys = Arc::new(JwksKeySource::new(
            http,
            config.platform_url.clone(),
            config.clock.clone(),
        ));
        let shared = Arc::new(Shared {
            reconciler,
            metrics,
            permits,
            pinned,
            bearer: BearerAuthenticator::new(keys, config.clock.clone()),
            clock: config.clock.clone(),
            trusted_proxies: config.trusted_proxies.clone(),
            draining: self.draining.clone(),
            routes: RwLock::new(None),
        });

        let private = bind(config.bind, config.port)?;
        let public = config
            .public_port
            .map(|port| bind(config.bind, port))
            .transpose()?;
        let _ = self.port.set(private.local_addr()?.port());
        let mut servers = vec![serve(private, shared.clone(), ListenerEntry::Private)?];
        if let Some(public) = public {
            let _ = self.public_port.set(public.local_addr()?.port());
            servers.push(serve(public, shared.clone(), ListenerEntry::Public)?);
        }
        *self.running.lock() = Some(Running { shared, servers });
        Ok(())
    }

    fn port(&self) -> Option<u16> {
        self.port.get().copied()
    }

    fn public_port(&self) -> Option<u16> {
        self.public_port.get().copied()
    }

    fn is_serving(&self) -> bool {
        self.running
            .lock()
            .as_ref()
            .is_some_and(|r| r.servers.iter().all(|s| s.serving.load(Ordering::SeqCst)))
    }

    /// New requests get `503 DRAINING`; in-flight ones finish. One-way.
    async fn drain(&self) {
        self.draining.store(true, Ordering::SeqCst);
    }

    /// Stops accepting, waits up to `timeout` for in-flight requests, then
    /// drops whatever is left and closes the pinned versions.
    async fn close(&self, timeout: Duration) {
        self.draining.store(true, Ordering::SeqCst);
        let Some(running) = self.running.lock().take() else {
            return;
        };
        for server in &running.servers {
            let _ = server.stop.send(Some(timeout));
        }
        for server in running.servers {
            if tokio::time::timeout(timeout + Duration::from_secs(5), server.task)
                .await
                .is_err()
            {
                tracing::warn!("closing the function host listener did not complete cleanly within the drain timeout");
            }
        }
        running.shared.pinned.close().await;
    }
}

fn bind(ip: IpAddr, port: u16) -> std::io::Result<std::net::TcpListener> {
    let listener = std::net::TcpListener::bind(SocketAddr::new(ip, port))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// The accept loop for one entry. On stop: stop accepting, let open
/// connections finish in-flight requests (bounded), then drop them.
fn serve(
    listener: std::net::TcpListener,
    shared: Arc<Shared>,
    entry: ListenerEntry,
) -> std::io::Result<Server> {
    let listener = TcpListener::from_std(listener)?;
    let (stop, mut stopped) = watch::channel::<Option<Duration>>(None);
    let serving = Arc::new(AtomicBool::new(true));
    let alive = serving.clone();
    let timeouts = listener_timeouts();
    let task = tokio::spawn(async move {
        let _serving = ServingFlag(alive);
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (stream, peer) = match accepted {
                        Ok(accepted) => accepted,
                        Err(e) => {
                            tracing::warn!(err = %e, entry = entry.wire_value(), "accepting a connection failed");
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            continue;
                        }
                    };
                    let _ = stream.set_nodelay(true);
                    let shared = shared.clone();
                    let timeouts = timeouts.clone();
                    let mut stop = stopped.clone();
                    connections.spawn(async move {
                        let service = service_fn(move |request| {
                            let shared = shared.clone();
                            async move {
                                let answer = pipeline::handle(shared, entry, peer, request).await;
                                Ok::<_, Infallible>(answer.into_response())
                            }
                        });
                        let stopping = async move {
                            let _ = stop.wait_for(Option::is_some).await;
                        };
                        if let Err(e) =
                            fc_http_listener::serve_connection(stream, service, &timeouts, stopping)
                                .await
                        {
                            tracing::debug!(err = %e, "connection ended with an error");
                        }
                    });
                }
                Some(_) = connections.join_next(), if !connections.is_empty() => {}
                _ = stopped.changed() => break,
            }
        }
        drop(listener);
        // Every connection was told to stop: in-flight requests finish,
        // idle keep-alive connections close at once.
        let timeout = stopped.borrow().unwrap_or(Duration::ZERO);
        if tokio::time::timeout(timeout, async {
            while connections.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            tracing::warn!(entry = entry.wire_value(), "in-flight requests did not finish within the drain timeout; closing their connections");
        }
        connections.abort_all();
    });
    Ok(Server {
        stop,
        task,
        serving,
    })
}

/// Both entries' timeouts (owner ruling 10, Java 3f176222): a keep-alive
/// connection closes 75 s after its last request, a request must be read
/// within 30 s (a body that isn't answers `408 REQUEST_TIMEOUT`), and
/// nothing fires while an invocation runs or its response streams.
fn listener_timeouts() -> fc_http_listener::ListenerTimeouts {
    fc_http_listener::ListenerTimeouts::new(
        r#"{"error":"REQUEST_TIMEOUT","message":"the request was not received in time"}"#,
    )
}

/// Cleared when the accept loop ends, however it ends (`LISTENER_DOWN`).
struct ServingFlag(Arc<AtomicBool>);

impl Drop for ServingFlag {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Header bytes as ISO-8859-1, one character per byte (Netty's reading).
pub(crate) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}
