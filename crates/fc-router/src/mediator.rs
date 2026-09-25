//! HTTP-based message mediator.
//!
//! Public surface for HTTP delivery of mediation messages. Mirrors the
//! Java `HttpMediator`:
//!
//! - HTTP POST `{"messageId":"<id>"}` to `message.mediation_target`.
//! - Optional bearer token from `message.auth_token`.
//! - Optional HMAC-SHA256 webhook signing (see [`signing`]).
//! - Response classification: 2xx ack/no-ack, 4xx config errors, 429
//!   rate-limit, 5xx transient. See [`response`].
//! - Retry with exponential backoff for transient outcomes. See [`retry`].
//! - Per-host HTTP/2 connection pool that grows under load and shrinks
//!   when idle (the AWS ALB 128-stream cap). See [`crate::http_pool`].
//!
//! Circuit breaking: [`Mediator::mediate`] on `HttpMediator` consults and
//! records into the shared per-endpoint `CircuitBreakerRegistry`.

mod inner;
mod response;
mod retry;
mod signing;

pub use retry::RetryPolicy;
pub use signing::{SIGNATURE_HEADER, TIMESTAMP_HEADER};

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fc_common::{MediationOutcome, MediationType, Message, WarningCategory, WarningSeverity};
use tracing::{debug, error, info, warn};

use crate::circuit_breaker_registry::{breaker_key, CircuitBreakerRegistry};
use crate::http_pool::{HostKey, HostPoolSizing};
use crate::pool::breaker_effect;
use crate::warning::WarningService;

use inner::{make_client_builder, spawn_sweep_task, MediatorInner};
use signing::{sign_webhook, MediationPayload};

/// Trait for message mediation.
///
/// Currently has one production implementation ([`HttpMediator`]) plus
/// test mocks. The trait stays object-safe via `#[async_trait]` because
/// `Arc<dyn Mediator>` is used widely (per-pool injection).
#[async_trait]
pub trait Mediator: Send + Sync {
    async fn mediate(&self, message: &Message) -> MediationOutcome;

    /// The breaker registry this mediator consults/records into, when it
    /// has one. `None` by default so test mocks (which don't model a
    /// breaker at all) need no changes; [`HttpMediator`] overrides this to
    /// expose the real registry — used to pin that every pool's mediator
    /// really does share the manager's single registry (see
    /// `manager.rs`'s `pools_share_managers_circuit_breaker_registry`
    /// test) rather than each getting a private default.
    fn circuit_breaker_registry(&self) -> Option<&Arc<CircuitBreakerRegistry>> {
        None
    }
}

/// HTTP version to use for mediation requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpVersion {
    /// HTTP/1.1 — better for development/debugging.
    Http1,
    /// HTTP/2 — better for production (multiplexing, header compression).
    #[default]
    Http2,
}

/// Configuration for [`HttpMediator`].
#[derive(Debug, Clone)]
pub struct HttpMediatorConfig {
    /// Request timeout (Java default: 900s / 15 minutes).
    pub timeout: Duration,
    /// HTTP version to use.
    pub http_version: HttpVersion,
    pub max_retries: u32,
    pub retry_delays: Vec<Duration>,
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// Per-host connection-pool sizing. Controls when extra HTTP/2
    /// connections to a host are opened to stay under AWS ALB's
    /// 128-stream cap, and when idle connections are reaped.
    pub host_pool_sizing: HostPoolSizing,
}

impl Default for HttpMediatorConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(900), // 15 minutes
            http_version: HttpVersion::Http2,  // Production default.
            max_retries: 3,
            retry_delays: vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(3),
            ],
            connect_timeout: Duration::from_secs(30),
            host_pool_sizing: HostPoolSizing::default(),
        }
    }
}

impl HttpMediatorConfig {
    /// Config for development mode: HTTP/1.1, short timeouts.
    pub fn dev() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            http_version: HttpVersion::Http1,
            max_retries: 3,
            retry_delays: vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(3),
            ],
            connect_timeout: Duration::from_secs(10),
            // HTTP/1.1 doesn't multiplex; the per-host pool collapses to
            // a single reqwest::Client whose own connection pool handles
            // concurrent TCP connections.
            host_pool_sizing: HostPoolSizing::http1(),
        }
    }

    /// Config for production: HTTP/2, long timeout.
    pub fn production() -> Self {
        Self::default()
    }
}

/// HTTP-based message mediator.
///
/// Each mediator owns a per-host connection-pool registry that grows
/// extra HTTP/2 connections as demand crosses the high watermark and
/// shrinks them once they go quiet. See [`crate::http_pool`] for the
/// sizing model.
pub struct HttpMediator {
    inner: Arc<MediatorInner>,
}

impl HttpMediator {
    pub fn new() -> Self {
        Self::with_config(HttpMediatorConfig::default())
    }

    /// Build with the dev-mode preset (HTTP/1.1).
    pub fn dev() -> Self {
        Self::with_config(HttpMediatorConfig::dev())
    }

    /// Build with the production preset (HTTP/2).
    pub fn production() -> Self {
        Self::with_config(HttpMediatorConfig::production())
    }

    pub fn with_config(config: HttpMediatorConfig) -> Self {
        Self::build(
            config,
            Arc::new(WarningService::noop()),
            Arc::new(CircuitBreakerRegistry::default()),
        )
    }

    fn build(
        config: HttpMediatorConfig,
        warning_service: Arc<WarningService>,
        breakers: Arc<CircuitBreakerRegistry>,
    ) -> Self {
        let builder = make_client_builder(&config);
        // Warm up the global rustls / native-certs init before any per-host
        // slot is built. The first reqwest::Client::build() in a process
        // pays a few hundred ms for native-certs loading; we don't want
        // that tax landing on the first mediation call. Warms with an
        // https:// placeholder key — native-certs loading is TLS-specific,
        // so that's the scheme whose builder path actually exercises it
        // (item 3: `make_client_builder` is now scheme-aware, but this
        // warm-up build is never registered in any real host pool).
        let warmup_key = HostKey {
            scheme: "https".to_string(),
            host: "warmup.invalid".to_string(),
            port: 443,
        };
        drop(builder(&warmup_key));
        let host_pools = crate::http_pool::HostPoolRegistry::new(
            config.host_pool_sizing.clone(),
            builder,
            warning_service.clone(),
        );

        info!(
            timeout_secs = config.timeout.as_secs(),
            http_version = ?config.http_version,
            high_watermark = config.host_pool_sizing.streams_high_watermark,
            max_slots_per_host = config.host_pool_sizing.max_slots_per_host,
            "HttpMediator initialized"
        );

        match config.http_version {
            HttpVersion::Http1 => info!("HttpMediator configured for HTTP/1.1"),
            HttpVersion::Http2 => info!("HttpMediator configured for HTTP/2 (ALPN negotiation)"),
        }

        let inner = Arc::new(MediatorInner {
            config,
            host_pools,
            warning_service,
            breakers,
        });

        spawn_sweep_task(&inner);

        Self { inner }
    }

    /// Attach the warning service. Rebuilds the inner state so the
    /// per-host pools created later report saturation to *this* service
    /// rather than the noop default. Preserves whatever circuit breaker
    /// registry was already wired in (a private default unless
    /// `with_circuit_breakers` ran first).
    pub fn with_warning_service(self, warning_service: Arc<WarningService>) -> Self {
        Self::build(
            self.inner.config.clone(),
            warning_service,
            self.inner.breakers.clone(),
        )
    }

    /// Attach the shared circuit breaker registry every pool's mediator
    /// must record into (ledger: breaker admission/recording centralised
    /// here rather than at the pool call site — see [`Mediator::mediate`]'s
    /// impl on this type). Rebuilds the inner state the same way
    /// `with_warning_service` does; preserves whatever warning service was
    /// already wired in. Production wiring is `QueueManager`'s
    /// `MediatorFactory`, which calls this with the manager's single
    /// registry for every pool's mediator so a breaker tripped by one pool
    /// protects every other pool targeting the same endpoint.
    pub fn with_circuit_breakers(self, breakers: Arc<CircuitBreakerRegistry>) -> Self {
        Self::build(
            self.inner.config.clone(),
            self.inner.warning_service.clone(),
            breakers,
        )
    }

    async fn mediate_once(&self, message: &Message) -> MediationOutcome {
        // Ledger R-06/A-11: a pre-flight rejection — no network call was
        // made — must raise a CONFIGURATION warning (same class as a real
        // 400/404) and must not credit the breaker with a success for a
        // call that never happened. `MediationOutcome::pre_flight_rejected`
        // carries the `pre_flight` flag the pool needs to skip the breaker
        // entirely; see `pool.rs`'s `breaker_effect`.
        if message.mediation_type != MediationType::HTTP {
            let detail = format!("Unsupported mediation type: {:?}", message.mediation_type);
            warn!(message_id = %message.id, detail = %detail, "Pre-flight rejection");
            self.inner.warning_service.add_warning(
                WarningCategory::Configuration,
                WarningSeverity::Error,
                format!(
                    "{} for message {}: target {}",
                    detail, message.id, message.mediation_target
                ),
                "HttpMediator".to_string(),
            );
            return MediationOutcome::pre_flight_rejected(detail);
        }

        let host_key = match HostKey::from_url(&message.mediation_target) {
            Ok(k) => k,
            Err(e) => {
                let detail = format!("Invalid mediation target URL: {}", e);
                warn!(
                    message_id = %message.id,
                    target = %message.mediation_target,
                    error = %e,
                    "Invalid mediation target URL"
                );
                self.inner.warning_service.add_warning(
                    WarningCategory::Configuration,
                    WarningSeverity::Error,
                    format!(
                        "{} for message {}: target {}",
                        detail, message.id, message.mediation_target
                    ),
                    "HttpMediator".to_string(),
                );
                return MediationOutcome::pre_flight_rejected(detail);
            }
        };
        let slot = self.inner.host_pools.acquire(host_key);

        let payload = MediationPayload {
            message_id: &message.id,
        };

        debug!(
            message_id = %message.id,
            target = %message.mediation_target,
            has_auth_token = message.auth_token.is_some(),
            auth_token_preview = message.auth_token.as_deref().map(token_preview),
            "Mediating message"
        );

        let payload_json = serde_json::to_string(&payload).expect("Failed to serialize payload");

        let mut request = slot
            .client()
            .post(&message.mediation_target)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");

        if let Some(ref signing_secret) = message.signing_secret {
            let (signature, timestamp) = sign_webhook(&payload_json, signing_secret);
            request = request
                .header(SIGNATURE_HEADER, signature)
                .header(TIMESTAMP_HEADER, timestamp);
        }

        if let Some(token) = &message.auth_token {
            request = request.bearer_auth(token);
        }

        request = request.body(payload_json);

        match request.send().await {
            Ok(response) => {
                // Item 3: record the protocol this specific request actually
                // negotiated — `{:?}` on `http::Version` gives "HTTP/2.0",
                // "HTTP/1.1", etc., same label shape as the bench rig's own
                // `proto_counts`.
                crate::router_metrics::record_mediation_http_version(&format!(
                    "{:?}",
                    response.version()
                ));
                response::classify(response, message, &self.inner.warning_service).await
            }
            Err(e) => {
                if e.is_timeout() {
                    warn!(
                        message_id = %message.id,
                        error = %e,
                        "Request timeout"
                    );
                    MediationOutcome::error_connection("Request timeout".to_string())
                } else if e.is_connect() {
                    warn!(
                        message_id = %message.id,
                        error = %e,
                        "Connection error"
                    );
                    MediationOutcome::error_connection(format!("Connection error: {}", e))
                } else {
                    error!(
                        message_id = %message.id,
                        target = %message.mediation_target,
                        error = %e,
                        error_debug = ?e,
                        is_request = e.is_request(),
                        is_redirect = e.is_redirect(),
                        is_status = e.is_status(),
                        is_body = e.is_body(),
                        is_decode = e.is_decode(),
                        "Request failed"
                    );
                    MediationOutcome::error_connection(format!("Request failed: {}", e))
                }
            }
        }
    }
}

#[async_trait]
impl Mediator for HttpMediator {
    /// Circuit breaker admission and recording live HERE, in one place, so
    /// nothing that calls a mediator can forget to arm the breaker for a
    /// new call path (ledger: the Go port's `3c3ec7a` lesson — a
    /// forgotten-arm bug class killed by having exactly one call site).
    /// Moved verbatim from `pool.rs`'s two former call sites
    /// (`spawn_immediate_task` / the group-drain path), which no longer
    /// touch `CircuitBreakerRegistry` at all:
    ///
    /// - Checked BEFORE the retry burst, once per `mediate()` call — not
    ///   once per internal retry attempt — exactly as the pool checked it
    ///   once before calling `mediate` at all.
    /// - Not admitted: no network call is made and `breaker_effect` isn't
    ///   even consulted — see `MediationOutcome::circuit_open`'s doc for
    ///   why this returns its own outcome rather than reusing an existing
    ///   classification, and the known gap it carries forward unchanged.
    /// - Admitted: recorded via `breaker_effect` on the outcome the retry
    ///   burst finally settles on — pre-flight rejections record neither,
    ///   `Success`/`ErrorConfig` record a success, `ErrorProcess`/
    ///   `ErrorConnection` record a failure, `RateLimited`/`Deferred`
    ///   record neither. Exactly `pool.rs`'s old rule table, unchanged.
    async fn mediate(&self, message: &Message) -> MediationOutcome {
        let endpoint = breaker_key(&message.mediation_target);
        if !self.inner.breakers.allow_request(&endpoint) {
            debug!(
                message_id = %message.id,
                endpoint = %endpoint,
                "Endpoint circuit breaker open"
            );
            return MediationOutcome::circuit_open();
        }

        // `HttpMediatorConfig`'s `max_retries` / `retry_delays` fields stay
        // as they were — collapsing them into `RetryPolicy` here (rather
        // than renaming the config fields) keeps every existing config
        // call site compiling. `RetryPolicy` is the named, documented,
        // independently-tested schedule ledger A-03 asks for; this is just
        // where config's loose fields become that policy for the call.
        let policy = RetryPolicy::new(
            self.inner.config.max_retries,
            self.inner.config.retry_delays.clone(),
        );
        let outcome = retry::run(&message.id, &policy, || self.mediate_once(message)).await;

        match breaker_effect(&outcome) {
            Some(true) => self.inner.breakers.record_success(&endpoint),
            Some(false) => self.inner.breakers.record_failure(&endpoint),
            None => {}
        }

        outcome
    }

    fn circuit_breaker_registry(&self) -> Option<&Arc<CircuitBreakerRegistry>> {
        Some(&self.inner.breakers)
    }
}

impl Default for HttpMediator {
    fn default() -> Self {
        Self::new()
    }
}

/// The first 20 characters of a bearer token, for a debug log line. Cut on
/// a character boundary: slicing bytes (`&t[..20]`) panicked the delivery
/// task whenever byte 20 fell inside a multi-byte character.
fn token_preview(token: &str) -> String {
    match token.char_indices().nth(20) {
        Some((cut, _)) => format!("{}...", &token[..cut]),
        None => token.to_string(),
    }
}

#[cfg(test)]
mod token_preview_tests {
    use super::token_preview;

    #[test]
    fn short_tokens_are_shown_whole() {
        assert_eq!(token_preview("abc"), "abc");
        assert_eq!(token_preview(&"a".repeat(20)), "a".repeat(20));
    }

    #[test]
    fn long_tokens_are_cut_at_20_characters() {
        assert_eq!(
            token_preview(&"a".repeat(25)),
            format!("{}...", "a".repeat(20))
        );
    }

    #[test]
    fn a_multibyte_character_across_byte_20_does_not_panic() {
        // 19 ASCII bytes, then a 2-byte 'é' spanning bytes 19..21.
        let token = format!("{}é{}", "a".repeat(19), "b".repeat(10));
        assert_eq!(token_preview(&token), format!("{}é...", "a".repeat(19)));
    }
}

// Circuit breaker tests are in circuit_breaker_registry.rs.
