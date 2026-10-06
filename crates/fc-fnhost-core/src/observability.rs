//! The observability listener (Java `fnhost/http/FnObservability.java`,
//! spec `function-host-process.md` §2): `/health`, `/ready`, `/metrics` on
//! `FC_METRICS_PORT`, all interfaces. It runs on its own OS thread with its
//! own single-threaded runtime, so a saturated function listener can never
//! make the process look dead to a liveness probe. Handlers do no I/O: a
//! few atomic reads and an in-memory scrape.

use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use axum::extract::State;
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tokio::sync::oneshot;

use crate::metrics::{self, FnMetrics};
use crate::reconciler::{Readiness, Reconciler};
use axum::http::Uri;
use fc_common::diagnostics;
use fc_common::diagnostics::Exposition;
use std::fs;
use std::io;
use std::net::TcpListener;
use std::thread::Builder;
use tokio::net;
use tokio::runtime;
use tokio::runtime::Handle;
use tokio::task;
use tokio::time;

const NOT_FOUND_BODY: &str = r#"{"error":"NOT_FOUND","message":"not found"}"#;

/// The live reads `/health` and `/ready` answer from.
#[derive(Clone)]
pub struct Probes {
    pub reconciler: Arc<Reconciler>,
    pub metrics: Arc<FnMetrics>,
    /// Whether the function listener is bound (or there is none to bind).
    pub listener_bound: Arc<dyn Fn() -> bool + Send + Sync>,
    pub reconcile_loop_alive: Arc<dyn Fn() -> bool + Send + Sync>,
    /// True once start-up has returned; `/health` is 200 before that.
    pub startup_complete: Arc<dyn Fn() -> bool + Send + Sync>,
}

pub struct Observability {
    port: u16,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Observability {
    /// Binds `0.0.0.0:port` (0 = ephemeral) and returns once listening.
    /// Called from the host's runtime, whose tokio figures `/metrics` then
    /// reports (this listener runs on a runtime of its own, so it can still
    /// describe a main runtime that is stuck).
    pub fn start(port: u16, probes: Probes) -> io::Result<Self> {
        let state = ObsState {
            probes,
            main_runtime: Handle::try_current().ok(),
        };
        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let (tx, rx) = oneshot::channel::<()>();
        let thread = Builder::new()
            .name("fn-observability".into())
            .spawn(move || {
                let runtime = runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("the observability runtime builds");
                runtime.block_on(async move {
                    let listener = match net::TcpListener::from_std(listener) {
                        Ok(listener) => listener,
                        Err(e) => {
                            tracing::error!(err = %e, "observability listener could not start");
                            return;
                        }
                    };
                    let app = Router::new().fallback(handle).with_state(state);
                    let served = axum::serve(listener, app)
                        .with_graceful_shutdown(async {
                            let _ = rx.await;
                        })
                        .await;
                    if let Err(e) = served {
                        tracing::warn!(err = %e, "observability listener stopped");
                    }
                });
            })?;
        Ok(Self {
            port,
            shutdown: Some(tx),
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stops the listener and joins its thread (bounded).
    pub async fn close(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let joined = task::spawn_blocking(move || thread.join());
            if time::timeout(Duration::from_secs(10), joined)
                .await
                .is_err()
            {
                tracing::warn!("closing the observability listener did not complete cleanly");
            }
        }
    }
}

fn json(status: StatusCode, body: String) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

/// The listener's state: the probes, and the host's main runtime.
#[derive(Clone)]
struct ObsState {
    probes: Probes,
    main_runtime: Option<Handle>,
}

/// The host's series, then the main runtime's `tokio_runtime_*` and the
/// process's `process_*` series, before OpenMetrics' closing `# EOF`.
fn with_runtime_series(mut text: String, main_runtime: Option<&Handle>) -> String {
    let eof = text.rfind("# EOF").unwrap_or(text.len());
    let tail = text.split_off(eof);
    if main_runtime.is_some() {
        diagnostics::render_prometheus(&mut text, main_runtime, Exposition::OpenMetrics);
    }
    text.push_str(&tail);
    text
}

async fn handle(State(state): State<ObsState>, method: Method, uri: Uri) -> Response {
    let probes = &state.probes;
    if method != Method::GET {
        return json(StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned());
    }
    match uri.path() {
        "/health" => health(probes),
        "/ready" => ready(probes),
        "/metrics" => match probes.metrics.encode() {
            Ok(text) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, metrics::CONTENT_TYPE)],
                with_runtime_series(text, state.main_runtime.as_ref()),
            )
                .into_response(),
            Err(_) => {
                tracing::warn!("Prometheus scrape failed");
                json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    r#"{"error":"INTERNAL","message":"scrape failed"}"#.to_owned(),
                )
            }
        },
        _ => json(StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
    }
}

/// Liveness: 200 unconditionally until start-up completes; then 503
/// `LISTENER_DOWN` / `RECONCILER_DOWN` when either has failed. Never
/// draining- or outage-aware: those are the process still doing its job.
fn health(probes: &Probes) -> Response {
    let status = if !(probes.startup_complete)() {
        None
    } else if !(probes.listener_bound)() {
        Some("LISTENER_DOWN")
    } else if !(probes.reconcile_loop_alive)() {
        Some("RECONCILER_DOWN")
    } else {
        None
    };
    match status {
        None => json(StatusCode::OK, r#"{"status":"UP"}"#.to_owned()),
        Some(status) => json(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(r#"{{"status":"{status}"}}"#),
        ),
    }
}

/// Readiness, with Java's precedence. The `memory` object carries the
/// container limit when one is set (Java's JVM heap/metaspace/direct
/// figures have no meaning here and are omitted).
fn ready(probes: &Probes) -> Response {
    let readiness = probes
        .reconciler
        .readiness((probes.listener_bound)(), (probes.reconcile_loop_alive)());
    let (status, code) = match readiness {
        Readiness::Ready => ("UP", StatusCode::OK),
        other @ (Readiness::Starting
        | Readiness::PlatformUnreachable
        | Readiness::ListenerDown
        | Readiness::ReconcilerDown
        | Readiness::Draining) => (other.name(), StatusCode::SERVICE_UNAVAILABLE),
    };
    let memory = match memory_limit_bytes() {
        Some(limit) => format!(r#"{{"limitBytes":{limit}}}"#),
        None => "{}".to_owned(),
    };
    json(
        code,
        format!(r#"{{"status":"{status}","memory":{memory}}}"#),
    )
}

/// The cgroup memory limit (v2, then v1), with Java's acceptance rules: a
/// missing file, `max`, a non-numeric value or the v1 "unlimited" sentinel
/// (≥ 2^60) all mean no limit.
fn memory_limit_bytes() -> Option<u64> {
    [
        "/sys/fs/cgroup/memory.max",
        "/sys/fs/cgroup/memory/memory.limit_in_bytes",
    ]
    .iter()
    .find_map(|path| fs::read_to_string(path).ok())
    .and_then(|raw| parse_limit(&raw))
}

fn parse_limit(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "max" || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u64>().ok().filter(|v| *v < (1u64 << 60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Handle;

    /// The runtime series go before OpenMetrics' `# EOF`, in its counter
    /// naming.
    #[tokio::test]
    async fn runtime_series_precede_the_eof() {
        let handle = Handle::current();
        let out = with_runtime_series("fc_fn_loaded 1\n# EOF\n".to_string(), Some(&handle));
        assert!(out.ends_with("# EOF\n"), "{out}");
        assert!(out.contains("tokio_runtime_workers "));
        assert!(out.contains("# TYPE tokio_runtime_worker_park counter"));
        assert_eq!(out.matches("# EOF").count(), 1);
    }

    #[test]
    fn limit_parsing_follows_java() {
        assert_eq!(parse_limit("536870912\n"), Some(536_870_912));
        assert_eq!(parse_limit("max"), None);
        assert_eq!(parse_limit("9223372036854771712"), None);
        assert_eq!(parse_limit("12a"), None);
    }
}
