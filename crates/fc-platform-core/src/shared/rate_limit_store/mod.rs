//! Distributed rate-limit store.
//!
//! Used by the OAuth/auth-edge middleware to count requests across the
//! entire cluster — unlike `rate_limit_middleware` (per-instance,
//! in-memory `governor`) which only sees the requests landing on one node.
//! A coordinated attacker hitting all replicas would slip past the
//! in-memory limiter; this store catches them.
//!
//! Two backends ship with one trait:
//!
//! * **Redis** — preferred when `FC_REDIS_URL` is set and the URL is
//!   reachable at startup. Uses fixed-window counters via `INCR + EXPIRE`.
//! * **Postgres** — fallback when Redis is absent/unreachable. Appends a
//!   row per attempt to `iam_rate_limit_events` (migration 030) and counts
//!   rows in the window via index-only seeks. Slower at peak QPS but
//!   removes the Redis operational requirement entirely.
//!
//! The factory `build_rate_limit_store` picks one at startup and logs the
//! choice; the rest of the system depends only on `Arc<dyn
//! RateLimitStore>` and is indifferent to which backend won.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use thiserror::Error;
use tracing::{info, warn};

pub mod middleware;
pub mod postgres;
pub mod redis;

pub use middleware::{
    distributed_rate_limit_per_email, distributed_rate_limit_per_ip, enforce_distributed,
    DistributedEmailLimitState, DistributedIpLimitState,
};
pub use postgres::PostgresRateLimitStore;
pub use redis::RedisRateLimitStore;

/// One of a fixed set of limiter "buckets" — each bucket has its own
/// (policy, key shape) pair and rows/keys are scoped under it so they
/// don't collide. `&'static str` so the bucket name appears verbatim in
/// SQL/Redis keys (no per-call allocation) and the set is reviewable in
/// one grep.
#[derive(Debug, Clone, Copy)]
pub struct Bucket(pub &'static str);

impl Bucket {
    pub const OAUTH_TOKEN_IP: Self = Bucket("oauth_token_ip");
    pub const OAUTH_TOKEN_CLIENT: Self = Bucket("oauth_token_client");
    pub const OAUTH_AUTHORIZE_IP: Self = Bucket("oauth_authorize_ip");
    pub const OAUTH_AUTHORIZE_CLIENT: Self = Bucket("oauth_authorize_client");
    pub const OAUTH_INTROSPECT_IP: Self = Bucket("oauth_introspect_ip");
    pub const OAUTH_REVOKE_IP: Self = Bucket("oauth_revoke_ip");
    pub const PASSWORD_RESET_IP: Self = Bucket("password_reset_ip");
    pub const PASSWORD_RESET_EMAIL: Self = Bucket("password_reset_email");
    pub const CHECK_DOMAIN_IP: Self = Bucket("check_domain_ip");
    /// `/auth/password-setup/request` spends the password-reset policies
    /// under its own buckets, so it and the reset route never spend each
    /// other's budget (Java dbe3ad9c).
    pub const PASSWORD_SETUP_IP: Self = Bucket("password_setup_ip");
    pub const PASSWORD_SETUP_EMAIL: Self = Bucket("password_setup_email");
    /// `/auth/2fa/challenge/email`: the same shared limits, its own buckets
    /// (Java owner ruling 7, 2026-09-25).
    pub const TWO_FACTOR_EMAIL_IP: Self = Bucket("two_factor_email_ip");
    pub const TWO_FACTOR_EMAIL: Self = Bucket("two_factor_email");

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

/// Policy applied to a single (bucket, key) pair: at most `limit` events
/// in any rolling/fixed `window`. Constructed up front from env config
/// (`RateLimitPolicyConfig::from_env`) so the middleware never re-parses
/// per request.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitPolicy {
    pub window: Duration,
    pub limit: u32,
}

impl RateLimitPolicy {
    pub const fn new(window: Duration, limit: u32) -> Self {
        Self { window, limit }
    }
}

/// Default policies for each bucket, loaded once at startup. Each
/// entry is overridable via env so ops can tighten or loosen without a
/// redeploy. Defaults are deliberately generous — the in-memory
/// governor already catches obvious bursts; this is the long-tail
/// cluster-wide ceiling.
#[derive(Debug, Clone)]
pub struct RateLimitPolicies {
    pub oauth_token_ip: RateLimitPolicy,
    pub oauth_token_client: RateLimitPolicy,
    pub oauth_authorize_ip: RateLimitPolicy,
    pub oauth_authorize_client: RateLimitPolicy,
    pub password_reset_ip: RateLimitPolicy,
    pub password_reset_email: RateLimitPolicy,
}

impl RateLimitPolicies {
    /// Read every knob from env. Format for each variable is the
    /// numeric request budget in the matching window — paired
    /// constants on this struct document the window.
    pub fn from_env() -> Self {
        Self {
            oauth_token_ip: RateLimitPolicy::new(
                Duration::from_secs(60),
                parse_env_u32("FC_RL_OAUTH_TOKEN_IP_PER_MIN", 600),
            ),
            oauth_token_client: RateLimitPolicy::new(
                Duration::from_secs(60),
                parse_env_u32("FC_RL_OAUTH_TOKEN_CLIENT_PER_MIN", 300),
            ),
            oauth_authorize_ip: RateLimitPolicy::new(
                Duration::from_secs(60),
                parse_env_u32("FC_RL_OAUTH_AUTHORIZE_IP_PER_MIN", 600),
            ),
            oauth_authorize_client: RateLimitPolicy::new(
                Duration::from_secs(60),
                parse_env_u32("FC_RL_OAUTH_AUTHORIZE_CLIENT_PER_MIN", 300),
            ),
            password_reset_ip: RateLimitPolicy::new(
                Duration::from_secs(3600),
                parse_env_u32("FC_RL_PASSWORD_RESET_IP_PER_HOUR", 20),
            ),
            password_reset_email: RateLimitPolicy::new(
                Duration::from_secs(3600),
                parse_env_u32("FC_RL_PASSWORD_RESET_EMAIL_PER_HOUR", 5),
            ),
        }
    }

    /// Longest window across all configured policies — used by the
    /// background prune task to know how far back to keep history.
    pub fn max_window(&self) -> Duration {
        [
            self.oauth_token_ip.window,
            self.oauth_token_client.window,
            self.oauth_authorize_ip.window,
            self.oauth_authorize_client.window,
            self.password_reset_ip.window,
            self.password_reset_email.window,
        ]
        .into_iter()
        .max()
        .unwrap_or(Duration::from_secs(3600))
    }
}

/// Spend one request of an IP bucket and an address bucket, the budget of
/// an unauthenticated route that mails an address its caller names. `false`
/// when either is over budget: the caller then does nothing and answers
/// exactly as it would have. The IP is checked first, so an IP already over
/// budget doesn't also spend the address's. Fails open on a backend error,
/// like the middleware.
pub async fn within_mail_budget(
    store: &dyn RateLimitStore,
    buckets: (Bucket, Bucket),
    policies: (RateLimitPolicy, RateLimitPolicy),
    ip: Option<&str>,
    email: &str,
) -> bool {
    let (ip_bucket, email_bucket) = buckets;
    let (ip_policy, email_policy) = policies;
    if let Some(ip) = ip.filter(|ip| !ip.is_empty()) {
        if let Ok(RateLimitDecision::Reject { .. }) =
            store.check_and_record(ip_bucket, ip, ip_policy).await
        {
            return false;
        }
    }
    let key = email.trim().to_lowercase();
    if key.is_empty() {
        return true;
    }
    !matches!(
        store
            .check_and_record(email_bucket, &key, email_policy)
            .await,
        Ok(RateLimitDecision::Reject { .. })
    )
}

fn parse_env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitDecision {
    Allow,
    /// Caller is over the limit. `retry_after_secs` is a worst-case
    /// estimate (≤ window) of when the bucket will have room again.
    Reject {
        retry_after_secs: u32,
    },
}

#[derive(Debug, Error)]
pub enum RateLimitError {
    #[error("rate limit backend error: {0}")]
    Backend(String),
}

/// The store contract.
///
/// `check_and_record` is intentionally a single call — splitting count
/// and record into two round trips leaves a race window an attacker can
/// exploit to bypass the limit under burst load. Both backends implement
/// it atomically (Redis via `INCR`, Postgres via `INSERT … RETURNING
/// (SELECT COUNT(*))`).
#[async_trait]
pub trait RateLimitStore: Send + Sync {
    async fn check_and_record(
        &self,
        bucket: Bucket,
        key: &str,
        policy: RateLimitPolicy,
    ) -> Result<RateLimitDecision, RateLimitError>;

    /// Best-effort housekeeping: drop event rows older than the maximum
    /// window used by any registered policy. Redis is a no-op (TTLs
    /// auto-expire); Postgres runs a `DELETE … WHERE occurred_at < $1`.
    /// Called by the background prune task; safe to ignore failures.
    async fn prune(&self, older_than: Duration) -> Result<u64, RateLimitError> {
        let _ = older_than;
        Ok(0)
    }
}

/// No-op store that always allows. Convenient for tests and for
/// environments that want the in-memory governor only (set
/// `FC_RATE_LIMIT_DISABLE=1`). Production callers should use
/// `build_rate_limit_store` to get a real backend.
pub struct NoopRateLimitStore;

#[async_trait]
impl RateLimitStore for NoopRateLimitStore {
    async fn check_and_record(
        &self,
        _bucket: Bucket,
        _key: &str,
        _policy: RateLimitPolicy,
    ) -> Result<RateLimitDecision, RateLimitError> {
        Ok(RateLimitDecision::Allow)
    }
}

/// Build the store the rest of the platform will use.
///
/// Picks Redis when `FC_REDIS_URL` is set AND the URL is reachable
/// (round-trip `PING` succeeds within a short timeout). Falls back to
/// Postgres on any failure. The chosen backend is logged at startup so
/// ops can confirm which one ended up active.
pub async fn build_rate_limit_store(pool: PgPool) -> Arc<dyn RateLimitStore> {
    if std::env::var("FC_RATE_LIMIT_DISABLE").ok().as_deref() == Some("1") {
        info!("Distributed rate-limit store: DISABLED (FC_RATE_LIMIT_DISABLE=1)");
        return Arc::new(NoopRateLimitStore);
    }

    if let Ok(url) = std::env::var("FC_REDIS_URL") {
        match RedisRateLimitStore::connect(&url).await {
            Ok(store) => {
                info!(
                    redis_url = %redact_url(&url),
                    "Distributed rate-limit store: Redis"
                );
                return Arc::new(store);
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "FC_REDIS_URL set but Redis unreachable; falling back to Postgres rate-limit store",
                );
            }
        }
    } else {
        info!("FC_REDIS_URL not set; using Postgres rate-limit store");
    }
    Arc::new(PostgresRateLimitStore::new(pool))
}

/// Strip credentials from a redis:// URL before logging. The full URL
/// can contain `redis://user:password@host:6379` and that lands in logs
/// unless we mask it.
fn redact_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let (scheme, rest) = url.split_at(scheme_end + 3);
        if let Some(at) = rest.rfind('@') {
            return format!("{}***@{}", scheme, &rest[at + 1..]);
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_strips_credentials() {
        assert_eq!(
            redact_url("redis://user:pass@host:6379"),
            "redis://***@host:6379"
        );
        assert_eq!(
            redact_url("redis://:secret@host:6379/0"),
            "redis://***@host:6379/0"
        );
        // No creds → unchanged
        assert_eq!(redact_url("redis://host:6379"), "redis://host:6379");
        // No scheme → unchanged
        assert_eq!(redact_url("host:6379"), "host:6379");
    }
}
