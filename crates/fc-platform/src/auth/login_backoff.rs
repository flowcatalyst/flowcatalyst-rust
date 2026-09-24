//! Layered backoff for login endpoints.
//!
//! Two checks run on each attempt before credentials are evaluated:
//!
//! 1. **Per-(identifier, IP) exponential backoff** — slows targeted brute
//!    force from a single source without locking out the legitimate user
//!    coming from a different IP. The first few failures are free, then
//!    each additional failure doubles the required delay up to a cap.
//!
//! 2. **Per-identifier global ceiling** — caps total failures across all
//!    IPs in a sliding window, catching distributed attacks (botnets).
//!    A high threshold so it never trips on normal usage.
//!
//! Federated principals never reach this code path — the email-domain gate
//! redirects them to their IdP before any credential check.
//!
//! See the discussion in conversation memory for the threat model and why
//! this layered design beats a hard lockout (lockout DoS vector).

use chrono::{Duration, Utc};
use std::env;
use tracing::warn;

use crate::login_attempt::entity::{AttemptType, LoginAttempt, LoginOutcome};
use crate::login_attempt::repository::LoginAttemptRepository;
use crate::shared::error::{PlatformError, Result};

/// Configuration knobs for the backoff/ceiling policy. All values are
/// env-overridable so ops can tune without a redeploy.
#[derive(Debug, Clone)]
pub struct BackoffPolicy {
    /// Failures allowed without any required delay.
    pub free_attempts: u32,
    /// Base delay in seconds applied at `free_attempts + 1`.
    pub base_delay_secs: u32,
    /// Cap on the per-(identifier, IP) backoff delay.
    pub max_delay_secs: u32,
    /// Window for the per-identifier global ceiling.
    pub global_window_secs: i64,
    /// Failures within `global_window_secs` (any IP) that trigger a hard
    /// lockout response.
    pub global_ceiling: i64,
    /// Lock duration when the global ceiling triggers.
    pub global_lock_secs: i64,
}

impl BackoffPolicy {
    pub fn from_env() -> Self {
        Self {
            free_attempts: parse_env("FC_LOGIN_BACKOFF_FREE_ATTEMPTS", 3),
            base_delay_secs: parse_env("FC_LOGIN_BACKOFF_BASE_SECS", 2),
            max_delay_secs: parse_env("FC_LOGIN_BACKOFF_MAX_SECS", 300),
            global_window_secs: parse_env_i64("FC_LOGIN_GLOBAL_WINDOW_SECS", 3600),
            global_ceiling: parse_env_i64("FC_LOGIN_GLOBAL_CEILING", 100),
            global_lock_secs: parse_env_i64("FC_LOGIN_GLOBAL_LOCK_SECS", 900),
        }
    }

    /// Required delay (seconds) given the count of failures since the last
    /// successful login from the same `(identifier, ip)` pair.
    ///
    /// Returns `0` when below `free_attempts`, otherwise grows exponentially
    /// from `base_delay_secs` and caps at `max_delay_secs`.
    pub fn compute_delay_secs(&self, failure_count: u32) -> u32 {
        if failure_count <= self.free_attempts {
            return 0;
        }
        let exponent = failure_count - self.free_attempts - 1;
        let scaled = (self.base_delay_secs as u64).saturating_mul(1u64 << exponent.min(31));
        scaled.min(self.max_delay_secs as u64) as u32
    }
}

fn parse_env(name: &str, default: u32) -> u32 {
    env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_env_i64(name: &str, default: i64) -> i64 {
    env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Outcome of the backoff/ceiling check. `Allow` lets the request proceed;
/// `Reject` carries the seconds the caller should wait before retrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffDecision {
    Allow,
    Reject {
        retry_after_secs: u32,
        reason: BackoffReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffReason {
    /// Per-(identifier, IP) exponential backoff.
    PairBackoff,
    /// Per-identifier global ceiling exceeded (sustained distributed failures).
    GlobalCeiling,
}

/// Run the backoff + global-ceiling check.
///
/// Federated users must be screened out before calling this — the gate is
/// only meaningful for principals we authenticate with a local credential.
///
/// `ip` is best-effort. When unavailable (e.g. local development with no
/// proxy), pass an empty string and only the global ceiling applies.
pub async fn check(
    repo: &LoginAttemptRepository,
    policy: &BackoffPolicy,
    identifier: &str,
    ip: Option<&str>,
) -> Result<BackoffDecision> {
    let now = Utc::now();
    // Window 1: failures since last success (used by the per-pair backoff).
    let last_success_cutoff = repo
        .last_success_at(identifier)
        .await?
        .unwrap_or_else(|| now - Duration::days(30));

    // Per-pair backoff. Skip when no IP is known (local dev / pre-LB setup).
    if let Some(ip) = ip {
        let (count, last_failure_at) = repo
            .failure_stats_by_identifier_ip_since(identifier, ip, last_success_cutoff)
            .await?;

        let count = count.max(0) as u32;
        let required = policy.compute_delay_secs(count);
        if required > 0 {
            let last = last_failure_at.unwrap_or(now);
            let elapsed = (now - last).num_seconds().max(0) as u32;
            if elapsed < required {
                return Ok(BackoffDecision::Reject {
                    retry_after_secs: required - elapsed,
                    reason: BackoffReason::PairBackoff,
                });
            }
        }
    }

    // Window 2: per-identifier global ceiling within `global_window_secs`.
    let global_cutoff = now - Duration::seconds(policy.global_window_secs);
    let cutoff = global_cutoff.max(last_success_cutoff);
    let global_count = repo
        .failure_count_by_identifier_since(identifier, cutoff)
        .await?;
    if global_count >= policy.global_ceiling {
        return Ok(BackoffDecision::Reject {
            retry_after_secs: policy.global_lock_secs.max(0) as u32,
            reason: BackoffReason::GlobalCeiling,
        });
    }

    Ok(BackoffDecision::Allow)
}

/// Record a user-login attempt (password or passkey). Fire-and-forget:
/// errors are logged but never affect the login flow.
pub async fn record_user_login_attempt(
    repo: &LoginAttemptRepository,
    identifier: Option<&str>,
    principal_id: Option<&str>,
    ip: Option<&str>,
    outcome: LoginOutcome,
    failure_reason: Option<&str>,
) {
    let attempt = LoginAttempt {
        identifier: identifier.map(String::from),
        principal_id: principal_id.map(String::from),
        ip_address: ip.map(String::from),
        failure_reason: failure_reason.map(String::from),
        ..LoginAttempt::new(AttemptType::UserLogin, outcome)
    };
    if let Err(e) = repo.create(&attempt).await {
        warn!(error = %e, "Failed to record login attempt (non-blocking)");
    }
}

/// Convert a `Reject` decision into the standard rejection error. Handlers
/// catch this, set `Retry-After` on the response, and return 429.
pub fn rejection_error(retry_after_secs: u32) -> PlatformError {
    PlatformError::TooManyRequests {
        retry_after_secs,
        message: "too many failed login attempts; try again later".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pol() -> BackoffPolicy {
        BackoffPolicy {
            free_attempts: 3,
            base_delay_secs: 2,
            max_delay_secs: 300,
            global_window_secs: 3600,
            global_ceiling: 100,
            global_lock_secs: 900,
        }
    }

    #[test]
    fn first_three_failures_are_free() {
        let p = pol();
        assert_eq!(p.compute_delay_secs(0), 0);
        assert_eq!(p.compute_delay_secs(1), 0);
        assert_eq!(p.compute_delay_secs(2), 0);
        assert_eq!(p.compute_delay_secs(3), 0);
    }

    #[test]
    fn fourth_failure_starts_curve() {
        let p = pol();
        assert_eq!(p.compute_delay_secs(4), 2);
        assert_eq!(p.compute_delay_secs(5), 4);
        assert_eq!(p.compute_delay_secs(6), 8);
        assert_eq!(p.compute_delay_secs(7), 16);
        assert_eq!(p.compute_delay_secs(8), 32);
        assert_eq!(p.compute_delay_secs(9), 64);
        assert_eq!(p.compute_delay_secs(10), 128);
        assert_eq!(p.compute_delay_secs(11), 256);
    }

    #[test]
    fn delay_caps_at_max() {
        let p = pol();
        assert_eq!(p.compute_delay_secs(12), 300);
        assert_eq!(p.compute_delay_secs(20), 300);
        assert_eq!(p.compute_delay_secs(100), 300);
        // Sanity: don't panic on absurd inputs.
        assert_eq!(p.compute_delay_secs(u32::MAX), 300);
    }

    #[test]
    fn aggressive_policy_curve() {
        let p = BackoffPolicy {
            free_attempts: 1,
            base_delay_secs: 5,
            max_delay_secs: 60,
            ..pol()
        };
        assert_eq!(p.compute_delay_secs(0), 0);
        assert_eq!(p.compute_delay_secs(1), 0);
        assert_eq!(p.compute_delay_secs(2), 5);
        assert_eq!(p.compute_delay_secs(3), 10);
        assert_eq!(p.compute_delay_secs(4), 20);
        assert_eq!(p.compute_delay_secs(5), 40);
        assert_eq!(p.compute_delay_secs(6), 60); // capped
    }
}
