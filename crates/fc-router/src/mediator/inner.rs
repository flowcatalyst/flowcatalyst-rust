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
use reqwest::redirect::Policy;
use tokio::runtime::Handle;
use tokio::time;
use tokio::time::MissedTickBehavior;

/// State shared by the mediator's public methods and the background sweep.
pub(super) struct MediatorInner {
    pub(super) config: HttpMediatorConfig,
    /// The in-call retry schedule, derived from `config` once.
    pub(super) retry_policy: super::RetryPolicy,
    pub(super) host_pools: HostPoolRegistry,
    pub(super) warning_service: Arc<WarningService>,
    /// Per-endpoint circuit breaker registry this mediator consults before
    /// every delivery and records into after — see `HttpMediator::mediate`.
    /// Defaults to a private registry (mirrors `warning_service`'s noop
    /// default) unless `HttpMediator::wired` supplies a
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
#[expect(
    clippy::expect_used,
    reason = "a reqwest client built from fixed options fails only if the TLS backend cannot initialise: a start-up failure, not a runtime one"
)]
pub(super) fn make_client_builder(
    config: &HttpMediatorConfig,
) -> Arc<dyn Fn(&HostKey) -> Client + Send + Sync> {
    let timeout = config.timeout;
    let connect_timeout = config.connect_timeout;
    let http_version = config.http_version;
    let ping_interval = config.http2_ping_interval;
    let ping_timeout = config.http2_ping_timeout;
    Arc::new(move |host_key: &HostKey| {
        // No delivery policy here (owner decision #56): router targets are
        // first-party (queue config, our own apps' pointers). The endpoint
        // that receives a pointer checks its own target when it delivers.
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
            .redirect(Policy::none());
        match http_version {
            HttpVersion::Http1 => {
                builder = builder.http1_only();
            }
            HttpVersion::Http2 => {
                // Connection health check (Go: ReadIdleTimeout/PingTimeout):
                // PING after `ping_interval` without a received frame, close
                // the connection if the ack misses `ping_timeout`. Idle
                // connections are pinged too, as Go's are.
                builder = builder
                    .http2_keep_alive_interval(ping_interval)
                    .http2_keep_alive_timeout(ping_timeout)
                    .http2_keep_alive_while_idle(true);
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
    let Ok(handle) = Handle::try_current() else {
        debug!("HttpMediator built outside tokio runtime; host-pool sweep task not spawned");
        return;
    };
    let interval = inner.config.host_pool_sizing.sweep_interval;
    let weak: Weak<MediatorInner> = Arc::downgrade(inner);
    handle.spawn(async move {
        let mut ticker = time::interval(interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
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

/// Go `TestHTTP2HealthCheckClosesAStalledConnection` (h2c half; the TLS half
/// is not ported, see docs/go-mirror): against a peer that completes the
/// HTTP/2 handshake and then goes silent, a delivery fails promptly after an
/// unanswered PING instead of running to its own deadline.
#[cfg(test)]
mod h2_ping_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const PREFACE_LEN: usize = 24;
    const FRAME_SETTINGS: u8 = 4;
    const FRAME_PING: u8 = 6;
    const FLAG_ACK: u8 = 1;

    /// Completes the connection preface and SETTINGS exchange, then swallows
    /// every frame: never answers a request, never acks a PING.
    async fn stalled_h2c_peer(pings: Arc<AtomicUsize>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let pings = Arc::clone(&pings);
                tokio::spawn(async move {
                    let mut preface = [0u8; PREFACE_LEN];
                    if sock.read_exact(&mut preface).await.is_err() {
                        return;
                    }
                    // Our (empty) SETTINGS.
                    if sock
                        .write_all(&[0, 0, 0, FRAME_SETTINGS, 0, 0, 0, 0, 0])
                        .await
                        .is_err()
                    {
                        return;
                    }
                    loop {
                        let mut head = [0u8; 9];
                        if sock.read_exact(&mut head).await.is_err() {
                            return;
                        }
                        let len = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
                        let mut payload = vec![0u8; len];
                        if sock.read_exact(&mut payload).await.is_err() {
                            return;
                        }
                        match (head[3], head[4] & FLAG_ACK != 0) {
                            (FRAME_SETTINGS, false) => {
                                if sock
                                    .write_all(&[0, 0, 0, FRAME_SETTINGS, FLAG_ACK, 0, 0, 0, 0])
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            (FRAME_PING, false) => {
                                pings.fetch_add(1, Ordering::SeqCst); // seen, never acked
                            }
                            _ => {} // HEADERS/DATA for the request: swallowed.
                        }
                    }
                });
            }
        });
        port
    }

    async fn deliver(
        config: &HttpMediatorConfig,
        port: u16,
        deadline: Duration,
    ) -> (Duration, bool) {
        let host = HostKey {
            scheme: "http".to_string(),
            host: "127.0.0.1".to_string(),
            port,
        };
        let client = make_client_builder(config)(&host);
        let start = Instant::now();
        let res = client
            .post(format!("http://127.0.0.1:{port}/deliver"))
            .timeout(deadline)
            .body(r#"{"messageId":"m"}"#)
            .send()
            .await;
        let err = res.expect_err("nothing answers; the request must fail");
        (start.elapsed(), err.is_timeout())
    }

    #[tokio::test]
    async fn health_check_closes_a_stalled_connection() {
        let pings = Arc::new(AtomicUsize::new(0));
        let port = stalled_h2c_peer(Arc::clone(&pings)).await;
        let config = HttpMediatorConfig::production()
            .with_http2_health_check(Duration::from_millis(200), Duration::from_millis(300));

        // (hyper reports the keep-alive expiry as a timed-out I/O error, so
        // `is_timeout` cannot tell it from the deadline; the elapsed time
        // against the 10 s deadline does.)
        let (took, _) = deliver(&config, port, Duration::from_secs(10)).await;
        assert!(
            took < Duration::from_secs(5),
            "the health check, not the 10 s request deadline, must end it; took {took:?}"
        );
        assert!(
            pings.load(Ordering::SeqCst) > 0,
            "the client must have pinged the silent connection"
        );
    }

    /// Control: with the default 30 s health check the same stalled peer runs
    /// the request to its own deadline, so the test above measures the health
    /// check and nothing else.
    #[tokio::test]
    async fn default_health_check_waits_for_the_request_deadline() {
        let pings = Arc::new(AtomicUsize::new(0));
        let port = stalled_h2c_peer(Arc::clone(&pings)).await;
        let config = HttpMediatorConfig::production();

        let (took, timed_out) = deliver(&config, port, Duration::from_millis(1500)).await;
        assert!(timed_out, "ran to its own deadline");
        assert!(took >= Duration::from_millis(1400), "{took:?}");
        assert_eq!(pings.load(Ordering::SeqCst), 0, "no ping inside 30 s");
    }
}
