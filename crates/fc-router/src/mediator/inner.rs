//! `MediatorInner` plus its background machinery.
//!
//! The mediator's state is held behind `Arc<MediatorInner>`. The sweep
//! task spawned by `spawn_sweep_task` holds a [`Weak`] — without it the
//! task would keep `MediatorInner` alive for the lifetime of the runtime,
//! preventing the mediator from being dropped (and the host pool /
//! connections from being released) when its owners go away.

use std::sync::{Arc, Weak};

use reqwest::Client;
use tracing::debug;

use crate::circuit_breaker_registry::CircuitBreakerRegistry;
use crate::http_pool::{HostKey, HostPoolRegistry};
use crate::warning::WarningService;

use super::{HttpMediatorConfig, HttpVersion};

/// State shared by the mediator's public methods and the background sweep.
pub(super) struct MediatorInner {
    pub(super) config: HttpMediatorConfig,
    pub(super) host_pools: HostPoolRegistry,
    pub(super) warning_service: Arc<WarningService>,
    /// Per-endpoint circuit breaker registry this mediator consults before
    /// every delivery and records into after — see `HttpMediator::mediate`.
    /// Defaults to a private registry (mirrors `warning_service`'s noop
    /// default) unless `HttpMediator::with_circuit_breakers` wires in a
    /// shared one, which is what every production pool does via
    /// `QueueManager`'s `MediatorFactory`.
    pub(super) breakers: Arc<CircuitBreakerRegistry>,
}

/// Build a closure that produces fresh `reqwest::Client`s for new per-host
/// slots. Each invocation yields an independent client (and therefore an
/// independent hyper connection pool), which is required by `http_pool`
/// to give each slot its own HTTP/2 connection.
///
/// Item 3 (owner ruling 2026-09-07; Go and Java both already do this):
/// deployed mode (`HttpVersion::Http2`) needs a DIFFERENT `reqwest`
/// configuration per target scheme, which a single shared closure with no
/// per-call input couldn't express — `https://` wants ALPN negotiation
/// (`http2_prior_knowledge()` NOT set: TLS + ALPN pick h2 or h1 with the
/// server, same as before this fix), but a plain `http://` target has no
/// ALPN to negotiate over, so without `http2_prior_knowledge()` reqwest
/// silently spoke HTTP/1.1 to every cleartext target regardless of
/// `http_version` — which is exactly the defect this fixes (was: every
/// mediation to this rig's plaintext sink went out as HTTP/1.1). Setting
/// `http2_prior_knowledge()` for `http://` targets makes the client send
/// the HTTP/2 connection preface immediately with no h1 fallback: an
/// HTTP/1.1-only cleartext target now FAILS the delivery (the preface is
/// nonsense to an h1-only server, so the connection/request errors out
/// and `mediate_once` classifies it as a connection error, subject to the
/// normal retry burst) rather than silently downgrading — deployed mode
/// must know its targets speak h2c, not guess. Dev mode
/// (`HttpVersion::Http1`) is unaffected: `.http1_only()` still wins
/// outright regardless of scheme, so a developer's plaintext target never
/// needs to speak h2c.
pub(super) fn make_client_builder(
    config: &HttpMediatorConfig,
) -> Arc<dyn Fn(&HostKey) -> Client + Send + Sync> {
    let timeout = config.timeout;
    let connect_timeout = config.connect_timeout;
    let http_version = config.http_version;
    Arc::new(move |host_key: &HostKey| {
        let mut builder = Client::builder()
            .timeout(timeout)
            .connect_timeout(connect_timeout)
            .pool_max_idle_per_host(10)
            // Ledger R-05 / A-06: never follow a redirect. reqwest's
            // default follows up to 10 and, for 301/302/303, rewrites the
            // POST into a bodyless GET — so the redirect target would
            // receive nothing and the router would record a false
            // success. Disabling it hands the 3xx back to `response`,
            // which classifies it as a permanent configuration error.
            .redirect(reqwest::redirect::Policy::none());
        match http_version {
            HttpVersion::Http1 => {
                builder = builder.http1_only();
            }
            HttpVersion::Http2 => {
                if host_key.scheme.eq_ignore_ascii_case("http") {
                    // Cleartext target, deployed mode: prior-knowledge h2c
                    // — see this function's doc comment.
                    builder = builder.http2_prior_knowledge();
                }
                // https:// — ALPN negotiation; do NOT use
                // http2_prior_knowledge() here, it would skip ALPN
                // entirely and break a target that only negotiates h1.
            }
        }
        builder.build().expect("Failed to build HTTP client")
    })
}

/// Spawn the background sweep task that prunes idle host-pool slots.
///
/// **Owns:** an interval timer plus a `Weak<MediatorInner>` so the host
/// pools can be reached during a sweep without keeping the mediator alive.
/// **Exits:** when the strong count on `MediatorInner` drops to zero
/// (i.e. when the last `HttpMediator` is dropped). The Weak fails to
/// upgrade on the next tick and the loop breaks.
/// **Joined by:** nobody — the task is intentionally detached. Drop is
/// the lifecycle signal.
///
/// No-ops outside a tokio runtime (some test paths build a mediator just
/// to inspect its API without a runtime in scope).
pub(super) fn spawn_sweep_task(inner: &Arc<MediatorInner>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        debug!("HttpMediator built outside tokio runtime; host-pool sweep task not spawned");
        return;
    };
    let interval = inner.config.host_pool_sizing.sweep_interval;
    let weak: Weak<MediatorInner> = Arc::downgrade(inner);
    handle.spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip the immediate first tick — the registry is empty at startup.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let Some(inner) = weak.upgrade() else {
                debug!("HttpMediator dropped; host-pool sweep task exiting");
                break;
            };
            inner.host_pools.sweep_all();
        }
    });
}
