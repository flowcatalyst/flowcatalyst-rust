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

use crate::http_pool::HostPoolRegistry;
use crate::warning::WarningService;

use super::{HttpMediatorConfig, HttpVersion};

/// State shared by the mediator's public methods and the background sweep.
pub(super) struct MediatorInner {
    pub(super) config: HttpMediatorConfig,
    pub(super) host_pools: HostPoolRegistry,
    pub(super) warning_service: Arc<WarningService>,
}

/// Build a closure that produces fresh `reqwest::Client`s for new per-host
/// slots. Each invocation yields an independent client (and therefore an
/// independent hyper connection pool), which is required by `http_pool`
/// to give each slot its own HTTP/2 connection.
pub(super) fn make_client_builder(
    config: &HttpMediatorConfig,
) -> Arc<dyn Fn() -> Client + Send + Sync> {
    let timeout = config.timeout;
    let connect_timeout = config.connect_timeout;
    let http_version = config.http_version;
    Arc::new(move || {
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
                // ALPN negotiation; do NOT use http2_prior_knowledge() for HTTPS.
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
