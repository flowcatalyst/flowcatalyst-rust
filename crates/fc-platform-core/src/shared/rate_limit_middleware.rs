//! Per-IP token-bucket rate limit middleware.
//!
//! Layered onto auth and token endpoints to catch broad scanning and the
//! "single attacker probing many emails" case (the per-(email, IP) backoff
//! protects each pair, but doesn't see total volume from one IP). This is
//! defence in depth — it complements but does not replace the per-account
//! backoff in `auth::login_backoff`.
//!
//! Keyed on the same IP-extraction logic the application uses elsewhere
//! (`X-Forwarded-For` first, falling back to `X-Real-IP`). Operate behind a
//! trusted proxy in production — see `shared::middleware::ClientIp`.

use std::env;
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use governor::{
    clock::{Clock, DefaultClock, QuantaInstant},
    middleware::NoOpMiddleware,
    state::keyed::DefaultKeyedStateStore,
    Quota, RateLimiter,
};

use crate::shared::api_common::ApiError;

type IpRateLimiter = RateLimiter<
    String,
    DefaultKeyedStateStore<String>,
    DefaultClock,
    NoOpMiddleware<QuantaInstant>,
>;

/// Per-IP rate limit configuration. Loaded from env at startup.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub per_minute: NonZeroU32,
    pub burst: NonZeroU32,
}

impl RateLimitConfig {
    /// Reads `FC_AUTH_IP_RATE_PER_MIN` (default 60) and
    /// `FC_AUTH_IP_BURST` (default 30).
    pub fn auth_default_from_env() -> Self {
        let per_min = parse_nz("FC_AUTH_IP_RATE_PER_MIN", 60);
        let burst = parse_nz("FC_AUTH_IP_BURST", 30);
        Self {
            per_minute: per_min,
            burst,
        }
    }

    /// Reads `FC_OAUTH_TOKEN_IP_RATE_PER_MIN` (default 120) and
    /// `FC_OAUTH_TOKEN_IP_BURST` (default 60).
    pub fn oauth_token_default_from_env() -> Self {
        let per_min = parse_nz("FC_OAUTH_TOKEN_IP_RATE_PER_MIN", 120);
        let burst = parse_nz("FC_OAUTH_TOKEN_IP_BURST", 60);
        Self {
            per_minute: per_min,
            burst,
        }
    }

    /// Per-`client_id` quota at `/oauth/token`. Reads
    /// `FC_OAUTH_TOKEN_CLIENT_RATE_PER_MIN` (default 60) and
    /// `FC_OAUTH_TOKEN_CLIENT_BURST` (default 30).
    pub fn oauth_token_per_client_from_env() -> Self {
        let per_min = parse_nz("FC_OAUTH_TOKEN_CLIENT_RATE_PER_MIN", 60);
        let burst = parse_nz("FC_OAUTH_TOKEN_CLIENT_BURST", 30);
        Self {
            per_minute: per_min,
            burst,
        }
    }
}

fn parse_nz(name: &str, default: u32) -> NonZeroU32 {
    let v: u32 = env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default);
    NonZeroU32::new(v.max(1)).expect("max(1) is always non-zero")
}

#[derive(Clone)]
pub struct IpRateLimiterState(pub Arc<IpRateLimiter>);

impl IpRateLimiterState {
    pub fn new(cfg: &RateLimitConfig) -> Self {
        let quota = Quota::per_minute(cfg.per_minute).allow_burst(cfg.burst);
        Self(Arc::new(RateLimiter::keyed(quota)))
    }

    /// Inline check for use inside handlers (e.g. per-`client_id` limiting at
    /// `/oauth/token`). Returns the seconds until the next allowed request
    /// when rejected.
    pub fn check(&self, key: &str) -> Result<(), u32> {
        match self.0.check_key(&key.to_string()) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                let wait = not_until.wait_time_from(DefaultClock::default().now());
                Err(wait.as_secs().max(1) as u32)
            }
        }
    }
}

fn extract_ip(headers: &HeaderMap) -> Option<String> {
    // Single source of truth — same trusted-hop logic as the ClientIp
    // extractor used elsewhere. Reads from the right of the X-Forwarded-For
    // chain so attacker-supplied prefix values don't shift our key.
    crate::shared::middleware::extract_trusted_client_ip(headers)
}

/// Axum middleware: reject with 429 + `Retry-After` when the source IP has
/// exhausted its bucket. Requests with no resolvable IP (no proxy header)
/// pass through — those need to be controlled by the rate limiter at the
/// trusted-proxy / load-balancer layer.
pub async fn rate_limit_per_ip(
    State(state): State<IpRateLimiterState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(ip) = extract_ip(request.headers()) else {
        return next.run(request).await;
    };

    match state.0.check_key(&ip) {
        Ok(()) => next.run(request).await,
        Err(not_until) => {
            // governor returns the cell at which the request would be allowed.
            let wait = not_until.wait_time_from(DefaultClock::default().now());
            let secs = wait.as_secs().max(1);
            let body = ApiError::new(
                "TOO_MANY_REQUESTS",
                "rate limit exceeded for this IP; back off and retry",
            );
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, secs.to_string())],
                Json(body),
            )
                .into_response()
        }
    }
}
