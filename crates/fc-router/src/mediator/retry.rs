//! Retry loop around `mediate_once`.
//!
//! Success, config errors, and rate-limit responses bypass retries —
//! they should be terminal for the dispatcher, not consume the retry
//! budget here. Everything else (connection error, process error, 5xx)
//! retries up to `max_retries - 1` times with delays drawn from
//! `retry_delays` (falling back to the last configured delay, or 3s if
//! none were configured, once the list runs out).

use std::future::Future;
use std::time::Duration;

use fc_common::{MediationOutcome, MediationResult};
use tracing::debug;

/// The in-call HTTP retry schedule (ledger A-03).
///
/// This is the *in-call* half of the router's retry story: the bounded
/// loop in [`run`] wraps a single `HttpMediator::mediate_once` attempt,
/// entirely before the message is ever handed back to the broker.
/// [`retries`](Self::retries) names exactly which outcomes consume this
/// budget — `Success`, `ErrorConfig` and `RateLimited` are terminal here
/// and return on the first attempt.
///
/// **Not the whole picture.** `pool.rs` applies its *own*, separately
/// configured delay whenever a message NACKs back to the broker instead
/// of exhausting this schedule (see the `nack(delay_seconds)` calls
/// around `pool.rs`'s delivery loop, and the pool-side backoff curve for
/// deferred/blocked messages). A-03 calls for folding both into one
/// policy object with one observable schedule; that pool-side half is a
/// later lane's work — this type only collapses the mediator's half, and
/// is deliberately kept a pure, standalone value so that later collapse
/// has something concrete to absorb.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    /// Total attempts, including the first. `max_attempts: 1` never
    /// retries.
    pub max_attempts: u32,
    /// Delay before attempt `n+1`, indexed from the first retry
    /// (`delays[0]` is the wait after attempt 1 fails). When `attempts`
    /// exceeds `delays.len()`, the last configured delay repeats; an
    /// empty list falls back to a flat 3s.
    pub delays: Vec<Duration>,
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, delays: Vec<Duration>) -> Self {
        Self {
            max_attempts,
            delays,
        }
    }

    /// Which outcomes consume the in-call retry budget at all.
    ///
    /// `Success`, `ErrorConfig` and `RateLimited` are terminal for
    /// `mediate_once` and bypass [`run`]'s loop entirely — a config error
    /// or a healthy-but-throttling target isn't going to change its mind
    /// because we asked again a second later. `ErrorProcess` (5xx that
    /// means "target unavailable") and `ErrorConnection` (couldn't even
    /// complete the request) are the two outcomes this policy governs.
    pub fn retries(result: MediationResult) -> bool {
        matches!(
            result,
            MediationResult::ErrorProcess | MediationResult::ErrorConnection
        )
    }

    /// The delay to wait before the given 1-indexed retry attempt (i.e.
    /// `attempt = 1` is the wait after the first failure, before the
    /// second try).
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        self.delays
            .get(attempt as usize - 1)
            .copied()
            .or_else(|| self.delays.last().copied())
            .unwrap_or(Duration::from_secs(3))
    }
}

impl Default for RetryPolicy {
    /// The schedule this crate has always run: 3 attempts total, waiting
    /// 1s then 2s between them.
    fn default() -> Self {
        Self {
            max_attempts: 3,
            delays: vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(3),
            ],
        }
    }
}

pub(super) async fn run<F, Fut>(
    message_id: &str,
    policy: &RetryPolicy,
    mut mediate_once: F,
) -> MediationOutcome
where
    F: FnMut() -> Fut,
    Fut: Future<Output = MediationOutcome>,
{
    let mut attempts: u32 = 0;
    loop {
        let outcome = mediate_once().await;

        if !RetryPolicy::retries(outcome.result) {
            return outcome;
        }

        attempts += 1;
        if attempts >= policy.max_attempts {
            return outcome;
        }

        let delay = policy.delay_for_attempt(attempts);

        debug!(
            message_id = %message_id,
            attempt = attempts,
            delay_ms = delay.as_millis(),
            "Retrying mediation"
        );
        tokio::time::sleep(delay).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the exact schedule this crate has always run, per ledger
    /// A-03: 3 attempts, waiting 1s then 2s between them. Any change here
    /// is an observable behaviour change, not a refactor.
    #[test]
    fn default_schedule_is_3_attempts_1s_2s() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(
            policy.delays,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(3),
            ]
        );
        // Attempt 1 fails -> wait 1s before attempt 2.
        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(1));
        // Attempt 2 fails -> wait 2s before attempt 3.
        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(2));
    }

    /// The list only has 3 entries; a policy that (mis)configured more
    /// attempts than delays repeats the last one rather than panicking.
    #[test]
    fn delay_for_attempt_past_the_list_repeats_the_last_entry() {
        let policy = RetryPolicy::new(
            5,
            vec![Duration::from_millis(100), Duration::from_millis(200)],
        );
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(4), Duration::from_millis(200));
    }

    /// An empty delay list is a degenerate but legal policy: fall back to
    /// a flat 3s rather than panicking on the empty `Vec`.
    #[test]
    fn delay_for_attempt_with_no_configured_delays_falls_back_to_3s() {
        let policy = RetryPolicy::new(3, vec![]);
        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(3));
    }

    #[test]
    fn retries_classifies_by_outcome() {
        assert!(RetryPolicy::retries(MediationResult::ErrorProcess));
        assert!(RetryPolicy::retries(MediationResult::ErrorConnection));
        assert!(!RetryPolicy::retries(MediationResult::Success));
        assert!(!RetryPolicy::retries(MediationResult::ErrorConfig));
        assert!(!RetryPolicy::retries(MediationResult::RateLimited));
    }
}
