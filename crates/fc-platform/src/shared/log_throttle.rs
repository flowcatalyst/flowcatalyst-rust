//! At most one log line per interval from one call site that can fire per
//! request (Java `shared/LogThrottle.java`): a failure on a hot path leaves
//! a line with its cause for the operator without writing one per call. The
//! admitted line carries how many were held back since the previous one.
//!
//! The function host has the same type (`fc_fnhost_core::log_throttle`);
//! the two crates share no dependency it would belong in.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub struct LogThrottle {
    interval: Duration,
    /// Nanoseconds since `epoch` before which nothing is admitted.
    next_allowed: AtomicU64,
    suppressed: AtomicU64,
    epoch: OnceLock<Instant>,
}

impl LogThrottle {
    pub const fn new(interval: Duration) -> Self {
        Self {
            interval,
            next_allowed: AtomicU64::new(0),
            suppressed: AtomicU64::new(0),
            epoch: OnceLock::new(),
        }
    }

    /// `Some(count suppressed since the last admitted line)` when the caller
    /// should log now; `None` (and counted) when it should not.
    pub fn admit(&self) -> Option<u64> {
        let epoch = *self.epoch.get_or_init(Instant::now);
        let now = epoch.elapsed().as_nanos() as u64;
        let next = self.next_allowed.load(Ordering::SeqCst);
        if now >= next
            && self
                .next_allowed
                .compare_exchange(
                    next,
                    now + self.interval.as_nanos() as u64,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
        {
            return Some(self.suppressed.swap(0, Ordering::SeqCst));
        }
        self.suppressed.fetch_add(1, Ordering::SeqCst);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_per_interval_carrying_what_was_held_back() {
        let throttle = LogThrottle::new(Duration::from_millis(50));
        assert_eq!(throttle.admit(), Some(0));
        assert_eq!(throttle.admit(), None);
        assert_eq!(throttle.admit(), None);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(throttle.admit(), Some(2));
    }
}
