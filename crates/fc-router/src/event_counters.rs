//! Event-time counters for the Prometheus surface (Go: `event_counters.go`).
//!
//! The counters the metrics contract defines that a snapshot of pool state
//! cannot give: `fc_messages_submitted_total`, `fc_messages_rejected_total`
//! by reason, the `result` label on `fc_messages_processed_total`, the
//! `fc_mediation_duration_seconds` histogram, and
//! `fc_consumer_polls_total` / `fc_consumer_errors_total`. Each is a plain
//! atomic bumped where the event happens and read by the scrape; nothing
//! here allocates per message.

use std::sync::atomic::{AtomicU64, Ordering};

use fc_common::MediationResult;

/// Why a pool handed a message back, or settled it, without delivering it.
/// The label value of `fc_messages_rejected_total` (Go: `RejectReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// The pool was full; the message went back to the broker.
    Capacity,
    /// The pool was stopping or draining.
    Stopped,
    /// Handed back because the target was unavailable (or the retry budget
    /// was spent), including the untried siblings of a released head.
    Released,
    /// An untried sibling ACKed away behind a head that failed terminally
    /// under BLOCK_ON_ERROR.
    Blocked,
    /// ACKed without delivery because its group is flushed.
    Suppressed,
}

impl RejectReason {
    const ALL: [Self; 5] = [
        Self::Capacity,
        Self::Stopped,
        Self::Released,
        Self::Blocked,
        Self::Suppressed,
    ];

    /// The metric label value.
    pub fn label(self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::Stopped => "stopped",
            Self::Released => "released",
            Self::Blocked => "blocked",
            Self::Suppressed => "suppressed",
        }
    }
}

/// `result` label values, in the order `processed` is indexed.
const RESULTS: [(MediationResult, &str); 7] = [
    (MediationResult::Success, "SUCCESS"),
    (MediationResult::ErrorConfig, "ERROR_CONFIG"),
    (MediationResult::ErrorProcess, "ERROR_PROCESS"),
    (MediationResult::ErrorConnection, "ERROR_CONNECTION"),
    (MediationResult::RateLimited, "RATE_LIMITED"),
    (MediationResult::CircuitOpen, "CIRCUIT_OPEN"),
    (MediationResult::Deferred, "DEFERRED"),
];

#[expect(
    clippy::expect_used,
    reason = "RESULTS lists every MediationResult variant"
)]
fn result_index(result: MediationResult) -> usize {
    RESULTS
        .iter()
        .position(|(r, _)| *r == result)
        .expect("RESULTS lists every MediationResult")
}

/// Upper bounds, in seconds, of `fc_mediation_duration_seconds` (Go:
/// `mediationBucketsSeconds`, the default client latency buckets; `+Inf` is
/// implicit in the exposition).
pub const MEDIATION_BUCKETS_SECONDS: [f64; 11] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// One pool's event-time counters.
#[derive(Debug, Default)]
pub struct PoolEventCounters {
    submitted: AtomicU64,
    /// Counts, by mediation result, exactly the outcomes the pool's
    /// success/failure totals count, so the per-result series sum to
    /// `fc_messages_processed_total{success}`.
    processed: [AtomicU64; RESULTS.len()],
    rejected: [AtomicU64; RejectReason::ALL.len()],
    /// Observations in each bucket (not cumulative; the snapshot
    /// accumulates). An observation above the last bound is in `count` only.
    duration_buckets: [AtomicU64; MEDIATION_BUCKETS_SECONDS.len()],
    duration_count: AtomicU64,
    duration_sum_micros: AtomicU64,
}

impl PoolEventCounters {
    /// A message was routed to the pool.
    pub fn submitted(&self) {
        self.submitted.fetch_add(1, Ordering::Relaxed);
    }

    /// `n` messages went back, or were settled, without being delivered.
    pub fn reject(&self, reason: RejectReason, n: usize) {
        if n > 0 {
            self.rejected[reason as usize].fetch_add(n as u64, Ordering::Relaxed);
        }
    }

    /// A delivery outcome the pool counts as a success or a failure.
    pub fn processed(&self, result: MediationResult) {
        self.processed[result_index(result)].fetch_add(1, Ordering::Relaxed);
    }

    /// One mediation took `duration_ms`.
    pub fn observe_duration_ms(&self, duration_ms: u64) {
        let seconds = duration_ms as f64 / 1000.0;
        if let Some(i) = MEDIATION_BUCKETS_SECONDS.iter().position(|b| seconds <= *b) {
            self.duration_buckets[i].fetch_add(1, Ordering::Relaxed);
        }
        self.duration_count.fetch_add(1, Ordering::Relaxed);
        self.duration_sum_micros
            .fetch_add(duration_ms.saturating_mul(1000), Ordering::Relaxed);
    }

    /// Zero everything (tests).
    pub fn reset(&self) {
        let zero = |a: &AtomicU64| a.store(0, Ordering::Relaxed);
        zero(&self.submitted);
        self.processed.iter().for_each(zero);
        self.rejected.iter().for_each(zero);
        self.duration_buckets.iter().for_each(zero);
        zero(&self.duration_count);
        zero(&self.duration_sum_micros);
    }

    /// The counters as a scrape reads them.
    pub fn snapshot(&self) -> PoolEventSnapshot {
        let nonzero = |name: &'static str, a: &AtomicU64| {
            let n = a.load(Ordering::Relaxed);
            (n > 0).then_some((name, n))
        };
        let mut cumulative = 0;
        let counts = self
            .duration_buckets
            .iter()
            .map(|b| {
                cumulative += b.load(Ordering::Relaxed);
                cumulative
            })
            .collect();
        PoolEventSnapshot {
            submitted: self.submitted.load(Ordering::Relaxed),
            processed: RESULTS
                .iter()
                .zip(&self.processed)
                .filter_map(|((_, name), a)| nonzero(name, a))
                .collect(),
            rejected: RejectReason::ALL
                .iter()
                .zip(&self.rejected)
                .filter_map(|(reason, a)| nonzero(reason.label(), a))
                .collect(),
            duration: DurationHistogram {
                counts,
                count: self.duration_count.load(Ordering::Relaxed),
                sum_seconds: self.duration_sum_micros.load(Ordering::Relaxed) as f64 / 1e6,
            },
        }
    }
}

/// A cumulative histogram of mediation durations. `counts[i]` is the number
/// of observations at or below `MEDIATION_BUCKETS_SECONDS[i]`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DurationHistogram {
    pub counts: Vec<u64>,
    pub count: u64,
    pub sum_seconds: f64,
}

/// One pool's event counters at a point in time. Only non-zero results and
/// reasons are present.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoolEventSnapshot {
    pub submitted: u64,
    pub processed: Vec<(&'static str, u64)>,
    pub rejected: Vec<(&'static str, u64)>,
    pub duration: DurationHistogram,
}

/// One queue's consumer-side counters. They live on the manager, keyed by
/// queue name, so a rebuilt consumer carries on counting instead of
/// resetting the series.
#[derive(Debug, Default)]
pub struct ConsumerEventCounters {
    polls: AtomicU64,
    poll_errors: AtomicU64,
}

impl ConsumerEventCounters {
    /// A broker poll returned.
    pub fn polled(&self) {
        self.polls.fetch_add(1, Ordering::Relaxed);
    }

    /// A broker poll failed, or did not return in time.
    pub fn poll_failed(&self) {
        self.poll_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ConsumerEventSnapshot {
        ConsumerEventSnapshot {
            polls: self.polls.load(Ordering::Relaxed),
            poll_errors: self.poll_errors.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConsumerEventSnapshot {
    pub polls: u64,
    pub poll_errors: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mediation_result_has_a_label() {
        for (result, _) in RESULTS {
            assert_eq!(RESULTS[result_index(result)].0, result);
        }
    }

    #[test]
    fn snapshot_lists_only_non_zero_results_and_reasons() {
        let c = PoolEventCounters::default();
        c.submitted();
        c.submitted();
        c.processed(MediationResult::Success);
        c.processed(MediationResult::ErrorConfig);
        c.processed(MediationResult::Success);
        c.reject(RejectReason::Capacity, 1);
        c.reject(RejectReason::Released, 3);
        c.reject(RejectReason::Blocked, 0);

        let s = c.snapshot();
        assert_eq!(s.submitted, 2);
        assert_eq!(s.processed, vec![("SUCCESS", 2), ("ERROR_CONFIG", 1)]);
        assert_eq!(s.rejected, vec![("capacity", 1), ("released", 3)]);
    }

    #[test]
    fn duration_histogram_is_cumulative_with_an_implicit_inf_bucket() {
        let c = PoolEventCounters::default();
        c.observe_duration_ms(3); // <= 0.005
        c.observe_duration_ms(5); // <= 0.005 (bounds are inclusive)
        c.observe_duration_ms(40); // <= 0.05
        c.observe_duration_ms(30_000); // above every bound: in `count` only

        let h = c.snapshot().duration;
        assert_eq!(h.counts.len(), MEDIATION_BUCKETS_SECONDS.len());
        assert_eq!(h.counts[0], 2);
        assert_eq!(h.counts[3], 3, "0.05s bucket includes the two below it");
        assert_eq!(*h.counts.last().unwrap(), 3);
        assert_eq!(h.count, 4);
        assert!((h.sum_seconds - 30.048).abs() < 1e-9);
    }

    #[test]
    fn reset_zeroes_everything() {
        let c = PoolEventCounters::default();
        c.submitted();
        c.reject(RejectReason::Stopped, 2);
        c.observe_duration_ms(10);
        c.reset();
        assert_eq!(
            c.snapshot(),
            PoolEventSnapshot {
                duration: DurationHistogram {
                    counts: vec![0; MEDIATION_BUCKETS_SECONDS.len()],
                    ..DurationHistogram::default()
                },
                ..PoolEventSnapshot::default()
            }
        );
    }
}
