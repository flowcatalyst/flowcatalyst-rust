//! Periodic housekeeping the platform runs alongside its HTTP server.
//!
//! [`AuthPurger`] is Go's auth purger (`StartPurger`,
//! internal/server/subsystems.go:500-599): every minute it drops the
//! short-lived auth rows that nothing else removes once they expire, and keeps
//! the `iam_login_attempts` quarterly partitions. Like Go's it runs wherever
//! the platform runs and is not leader-gated: every step is an idempotent
//! `DELETE … WHERE expires_at < NOW()` (or a `CREATE TABLE IF NOT EXISTS` /
//! bounded `DROP`), so instances running it side by side are harmless. A
//! failed step is logged and the sweep goes on.
//!
//! The rate-limit event prune stays on its own hourly task (see the binaries).

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Months, Utc};
use sqlx::PgPool;

use crate::auth::oidc_login_state_repository::OidcLoginStateRepository;
use crate::auth::oidc_payload_repository::OidcPayloadRepository;
use crate::login_attempt::repository::LoginAttemptRepository;
use crate::mfa::repository::MfaRepository;
use crate::password_reset::repository::PasswordResetTokenRepository;
use crate::portal::repository::PortalFlowRepository;
use crate::OAuthClientRepository;

/// How often the purger sweeps (Go: `time.NewTicker(time.Minute)`).
pub const PURGE_INTERVAL: Duration = Duration::from_secs(60);

/// How long `iam_login_attempts` partitions are kept (Go owner ruling X-03,
/// `loginAttemptsRetentionYears`).
pub const LOGIN_ATTEMPTS_RETENTION_YEARS: u32 = 3;

/// How long an expired password-reset or invite token is kept before it is
/// purged. Go defines the purge (`passwordreset.Repository.PurgeExpired`) but
/// never runs it, so its expired links answer "expired" indefinitely; a month
/// keeps that answer for any realistic late click while still clearing the
/// table.
pub const RESET_TOKEN_GRACE: chrono::Duration = chrono::Duration::days(30);

/// What one sweep removed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PurgeReport {
    /// `oauth_oidc_payloads`: authorization codes, refresh tokens, pending
    /// authorizations, WebAuthn ceremonies, every other payload type.
    pub oauth_payloads: u64,
    /// `oauth_oidc_login_states`: platform and portal OIDC login states.
    pub oidc_login_states: u64,
    /// `portal_login_flows`.
    pub portal_login_flows: u64,
    /// `iam_mfa_email_pins` and `iam_mfa_trusted_devices`.
    pub mfa: u64,
    /// `iam_password_reset_tokens` (reset and invite links), past the grace.
    pub reset_tokens: u64,
    /// `oauth_clients` rotation overlaps cleared.
    pub lapsed_previous_secrets: u64,
    /// `iam_login_attempts` partitions dropped.
    pub dropped_partitions: Vec<String>,
}

/// Go's auth purger. See the module docs.
pub struct AuthPurger {
    payloads: OidcPayloadRepository,
    login_states: OidcLoginStateRepository,
    portal_flows: PortalFlowRepository,
    mfa: MfaRepository,
    reset_tokens: PasswordResetTokenRepository,
    login_attempts: LoginAttemptRepository,
    /// The shared repository, so clearing a secret also clears its cache.
    oauth_clients: Arc<OAuthClientRepository>,
}

impl AuthPurger {
    pub fn new(pool: &PgPool, oauth_clients: Arc<OAuthClientRepository>) -> Self {
        Self {
            payloads: OidcPayloadRepository::new(pool),
            login_states: OidcLoginStateRepository::new(pool),
            portal_flows: PortalFlowRepository::new(pool),
            mfa: MfaRepository::new(pool),
            reset_tokens: PasswordResetTokenRepository::new(pool),
            login_attempts: LoginAttemptRepository::new(pool),
            oauth_clients,
        }
    }

    /// One sweep, at `now`. Each step runs whatever the others did.
    pub async fn run_once(&self, now: DateTime<Utc>) -> PurgeReport {
        fn counted(what: &str, result: crate::shared::error::Result<u64>) -> u64 {
            match result {
                Ok(n) => {
                    if n > 0 {
                        tracing::debug!(removed = n, "{what} purge");
                    }
                    n
                }
                Err(e) => {
                    tracing::warn!(error = %e, "{what} purge failed");
                    0
                }
            }
        }

        let mut report = PurgeReport {
            oauth_payloads: counted("oauth payload", self.payloads.purge_expired().await),
            oidc_login_states: counted(
                "oidc login state",
                self.login_states.delete_expired().await,
            ),
            portal_login_flows: counted(
                "portal login flow",
                self.portal_flows.purge_expired().await,
            ),
            mfa: counted(
                "2FA email PIN and trusted device",
                self.mfa.purge_expired().await,
            ),
            reset_tokens: counted(
                "password reset token",
                self.reset_tokens
                    .purge_expired_before(now - RESET_TOKEN_GRACE)
                    .await,
            ),
            lapsed_previous_secrets: counted(
                "lapsed oauth previous-secret",
                self.oauth_clients.purge_lapsed_previous_secrets().await,
            ),
            dropped_partitions: Vec::new(),
        };

        // The current quarter too, not just the next: a long gap between Go's
        // migration 049 and this start must not leave inserts with nowhere
        // to land but the DEFAULT partition (Go's comment, verbatim intent).
        for at in [now, now + Months::new(3)] {
            if let Err(e) = self.login_attempts.ensure_quarterly_partition(at).await {
                tracing::warn!(error = %e, "login-attempts partition ensure failed");
            }
        }
        match self
            .login_attempts
            .drop_partitions_older_than(now - Months::new(12 * LOGIN_ATTEMPTS_RETENTION_YEARS))
            .await
        {
            Ok(dropped) => {
                if !dropped.is_empty() {
                    tracing::debug!(partitions = ?dropped, "login-attempts old partitions dropped");
                }
                report.dropped_partitions = dropped;
            }
            Err(e) => tracing::warn!(error = %e, "login-attempts partition drop failed"),
        }
        report
    }

    /// Sweep every [`PURGE_INTERVAL`] for as long as the process runs.
    pub fn spawn(self) {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(PURGE_INTERVAL);
            tick.tick().await; // skip the immediate-fire tick, as Go's ticker
            tracing::info!("auth purger started");
            loop {
                tick.tick().await;
                self.run_once(Utc::now()).await;
            }
        });
    }
}

/// Start Go's auth purger over `pool` (see [`AuthPurger`]).
pub fn spawn_auth_purger(pool: &PgPool, oauth_clients: Arc<OAuthClientRepository>) {
    AuthPurger::new(pool, oauth_clients).spawn();
}
