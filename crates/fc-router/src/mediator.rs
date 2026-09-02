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
//! Circuit breaking is handled by the per-endpoint `CircuitBreakerRegistry`
//! in `ProcessPool`, not here.

mod inner;
mod response;
mod retry;
mod signing;

pub use retry::RetryPolicy;
pub use signing::{SIGNATURE_HEADER, TIMESTAMP_HEADER};

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fc_common::{MediationOutcome, MediationType, Message};
use tracing::{debug, error, info, warn};

use crate::http_pool::{HostKey, HostPoolSizing};
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
            timeout: Duration::from_secs(900), // 15 minutes — matches Java default.
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
        Self::build(config, Arc::new(WarningService::noop()))
    }

    fn build(config: HttpMediatorConfig, warning_service: Arc<WarningService>) -> Self {
        let builder = make_client_builder(&config);
        // Warm up the global rustls / native-certs init before any per-host
        // slot is built. The first reqwest::Client::build() in a process
        // pays a few hundred ms for native-certs loading; we don't want
        // that tax landing on the first mediation call.
        drop(builder());
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
        });

        spawn_sweep_task(&inner);

        Self { inner }
    }

    /// Attach the warning service. Rebuilds the inner state so the
    /// per-host pools created later report saturation to *this* service
    /// rather than the noop default.
    pub fn with_warning_service(self, warning_service: Arc<WarningService>) -> Self {
        Self::build(self.inner.config.clone(), warning_service)
    }

    /// Replace the warning service post-construction. Rebuilds the
    /// per-host pool registry; existing slots and their open connections
    /// are dropped, so prefer `with_warning_service` at construction time.
    pub fn set_warning_service(&mut self, warning_service: Arc<WarningService>) {
        *self = Self::build(self.inner.config.clone(), warning_service);
    }

    async fn mediate_once(&self, message: &Message) -> MediationOutcome {
        if message.mediation_type != MediationType::HTTP {
            return MediationOutcome::error_config(
                0,
                format!("Unsupported mediation type: {:?}", message.mediation_type),
            );
        }

        let host_key = match HostKey::from_url(&message.mediation_target) {
            Ok(k) => k,
            Err(e) => {
                warn!(
                    message_id = %message.id,
                    target = %message.mediation_target,
                    error = %e,
                    "Invalid mediation target URL"
                );
                return MediationOutcome::error_config(
                    0,
                    format!("Invalid mediation target URL: {}", e),
                );
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
            auth_token_preview = message.auth_token.as_ref().map(|t| if t.len() > 20 { format!("{}...", &t[..20]) } else { t.clone() }),
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
    async fn mediate(&self, message: &Message) -> MediationOutcome {
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
        retry::run(&message.id, &policy, || self.mediate_once(message)).await
    }
}

impl Default for HttpMediator {
    fn default() -> Self {
        Self::new()
    }
}

// Circuit breaker tests are in circuit_breaker_registry.rs.
