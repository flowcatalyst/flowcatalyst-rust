//! In-flight tracker entries and the rules for reaping them (Go:
//! `InFlightTracker`, `internal/router/inflight.go`).

use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};

use fc_common::InFlightMessage;

/// Go's `defaultAbsoluteMaxAgeFactor`: the hard ceiling on an entry's age is
/// this many times the idle bound (15m idle → 2h ceiling).
pub(crate) const ABSOLUTE_MAX_AGE_FACTOR: u32 = 8;

/// Go's `reapRetryGrace` (2 × the 15-minute mediation timeout): how long
/// after its last in-place retry attempt an entry still counts as a live
/// retry, exempt from the idle bound. A single attempt may legitimately run
/// for the whole mediation timeout and records no new attempt meanwhile.
pub(crate) const REAP_RETRY_GRACE: Duration = Duration::from_secs(30 * 60);

/// One message the router currently owns — routed and not yet acked or
/// nacked — plus what the reaper and callbacks need beyond the shared
/// [`InFlightMessage`] (which it derefs to).
#[derive(Debug, Clone)]
pub(crate) struct Tracked {
    pub(crate) msg: InFlightMessage,
    /// Refreshed at route time and on every broker redelivery of the
    /// message (the receipt-handle swap), so the idle bound reaps only an
    /// entry the broker has stopped redelivering (Go: `LastSeenAt`).
    pub(crate) last_seen: Instant,
    /// Identifies this admission of the message. The callback created with
    /// it acts on the entry only while the entry still carries it, so a
    /// callback from a copy whose entry was reaped and re-admitted can never
    /// ack, nack or clear the newer copy's entry.
    pub(crate) generation: u64,
    /// In-place retry attempts recorded by the pool (Go: `Attempts`).
    pub(crate) attempts: u32,
    /// When the last retry attempt was recorded (Go: `LastRetryAt`).
    pub(crate) last_retry_at: Option<Instant>,
}

impl Tracked {
    pub(crate) fn new(msg: InFlightMessage, generation: u64) -> Self {
        Self {
            msg,
            last_seen: Instant::now(),
            generation,
            attempts: 0,
            last_retry_at: None,
        }
    }

    /// A broker redelivery of this message was just seen: adopt its fresher
    /// receipt handle and refresh the idle clock.
    pub(crate) fn redelivered(&mut self, receipt_handle: &str) {
        if self.msg.receipt_handle != receipt_handle {
            self.msg.receipt_handle = receipt_handle.to_string();
        }
        self.last_seen = Instant::now();
    }

    /// Record an in-place retry attempt (Go: `MarkRetrying`).
    pub(crate) fn mark_retrying(&mut self) {
        self.attempts += 1;
        self.last_retry_at = Some(Instant::now());
    }

    /// A live in-place retry: an attempt was recorded within the grace.
    pub(crate) fn is_retrying(&self, now: Instant) -> bool {
        self.attempts > 0
            && self
                .last_retry_at
                .is_some_and(|t| now.duration_since(t) <= REAP_RETRY_GRACE)
    }

    /// Go's `Reap` rule for one entry, under two independent bounds:
    ///
    /// - past `ceiling` (measured from when it was first tracked) it is
    ///   reaped whatever else is true — the idle clock alone is circular,
    ///   since every redelivery of an orphaned entry refreshes it;
    /// - otherwise a live retry is exempt, and anything else is reaped once
    ///   the broker has not been seen redelivering it for `idle`.
    pub(crate) fn should_reap(&self, now: Instant, idle: Duration, ceiling: Duration) -> bool {
        if now.duration_since(self.msg.started_at) > ceiling {
            return true;
        }
        if self.is_retrying(now) {
            return false;
        }
        now.duration_since(self.last_seen) > idle
    }
}

impl From<InFlightMessage> for Tracked {
    fn from(msg: InFlightMessage) -> Self {
        Self::new(msg, 0)
    }
}

impl Deref for Tracked {
    type Target = InFlightMessage;
    fn deref(&self) -> &InFlightMessage {
        &self.msg
    }
}

impl DerefMut for Tracked {
    fn deref_mut(&mut self) -> &mut InFlightMessage {
        &mut self.msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_common::{DispatchMode, MediationType, Message};

    fn tracked(age: Duration, last_seen_ago: Duration) -> Tracked {
        let msg = Message {
            id: "m".to_string(),
            pool_code: "P".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://localhost/x".to_string(),
            message_group_id: None,
            high_priority: false,
            dispatch_mode: DispatchMode::Immediate,
            dispatch_mode_specified: true,
        };
        let mut t = Tracked::new(
            InFlightMessage::new(&msg, Some("b".into()), "q".into(), None, "rh".into()),
            1,
        );
        let now = Instant::now();
        t.msg.started_at = now - age;
        t.last_seen = now - last_seen_ago;
        t
    }

    const IDLE: Duration = Duration::from_secs(15 * 60);
    const CEILING: Duration = Duration::from_secs(2 * 60 * 60);

    /// H12: a slow delivery the broker keeps redelivering is NOT reaped at
    /// 15 minutes old — it ages on last-seen, not on when it started.
    #[test]
    fn ages_on_last_seen_not_started_at() {
        let t = tracked(Duration::from_secs(40 * 60), Duration::from_secs(60));
        assert!(!t.should_reap(Instant::now(), IDLE, CEILING));
        let idle = tracked(Duration::from_secs(40 * 60), Duration::from_secs(16 * 60));
        assert!(idle.should_reap(Instant::now(), IDLE, CEILING));
    }

    /// The absolute ceiling reaps even an entry whose idle clock keeps
    /// being refreshed by redeliveries (an orphan).
    #[test]
    fn absolute_ceiling_reaps_regardless() {
        let t = tracked(Duration::from_secs(3 * 60 * 60), Duration::from_secs(1));
        assert!(t.should_reap(Instant::now(), IDLE, CEILING));
    }

    /// A live in-place retry is exempt from the idle bound; a stale one is
    /// not.
    #[test]
    fn live_retry_is_exempt_until_the_grace_lapses() {
        let mut t = tracked(Duration::from_secs(50 * 60), Duration::from_secs(20 * 60));
        t.mark_retrying();
        assert!(!t.should_reap(Instant::now(), IDLE, CEILING));
        t.last_retry_at = Some(Instant::now() - REAP_RETRY_GRACE - Duration::from_secs(1));
        assert!(t.should_reap(Instant::now(), IDLE, CEILING));
    }

    #[test]
    fn redelivery_refreshes_last_seen_and_adopts_the_handle() {
        let mut t = tracked(Duration::from_secs(60), Duration::from_secs(600));
        t.redelivered("rh-2");
        assert_eq!(t.receipt_handle, "rh-2");
        assert!(t.last_seen.elapsed() < Duration::from_secs(1));
    }
}
