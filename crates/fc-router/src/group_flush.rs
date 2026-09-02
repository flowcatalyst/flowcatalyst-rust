//! Per-message-group delivery suppression (ledger A-05, R-52, R-53).
//!
//! Port of Go's `internal/router/group_flush.go`. A target may answer 2xx
//! with `{"ack": true, "flushGroup": true}` (ledger A-05: any target may
//! do this, no per-pool opt-in) to ask the router to stop delivering the
//! rest of that message's group for a while — the target already owns
//! these records (the message-pointer pattern) and will re-drive them
//! itself. `GroupFlushRegistry` is the per-message-group sibling of the
//! per-endpoint `CircuitBreakerRegistry`: instead of gating whether a call
//! is attempted at all, it gates whether the group's remaining messages
//! are delivered at all.
//!
//! Semantics: a suppressed group's messages are ACKed without any HTTP
//! call, which spends no rate-limit token and holds no concurrency slot.
//! That is only safe because the TARGET asked for it — a target whose
//! messages carry the only copy of the payload must never set
//! `flushGroup`.
//!
//! Suppression is TTL-bounded rather than explicitly cleared, so it
//! self-heals with no resume protocol: once the window lapses the next
//! message of the group goes through as a probe. If the group is still
//! blocked the target simply flushes again; if it has recovered, delivery
//! resumes on its own. An operator can also lift a suppression early via
//! [`GroupFlushRegistry::clear`] (ledger R-52) — the pool wiring for that
//! is a later lane's work; this module only provides the primitive.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Default suppression window when a target sets `flushGroup` without a
/// `delaySeconds` (or with `delaySeconds: 0`).
pub const DEFAULT_FLUSH_TTL: Duration = Duration::from_secs(60);
/// Ceiling on a single suppression window, regardless of what the target
/// requests — a misbehaving or malicious target must not be able to
/// silence a group indefinitely.
pub const MAX_FLUSH_TTL: Duration = Duration::from_secs(5 * 60);

/// Live suppression set plus lifetime counters, as returned by
/// [`GroupFlushRegistry::stats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupFlushStats {
    /// Number of groups currently suppressed (their window has not lapsed).
    pub active: usize,
    /// Lifetime count of `flushGroup` responses that suppressed a group
    /// (each call to [`GroupFlushRegistry::flush`] that actually took
    /// effect — a re-flush that would have SHORTENED the window doesn't
    /// count again).
    pub flushes: u64,
    /// Lifetime count of messages ACKed without delivery because their
    /// group was suppressed.
    pub suppressed: u64,
}

/// Suppresses delivery for message groups a target has asked the router
/// to stop sending to. See the module doc for the full semantics.
///
/// Thread-safe; cheap to check on every dispatched message (a single
/// `parking_lot::Mutex` around a `HashMap`, no I/O).
pub struct GroupFlushRegistry {
    until: Mutex<HashMap<String, Instant>>,
    flushes: AtomicU64,
    suppressed: AtomicU64,
}

impl GroupFlushRegistry {
    /// Build an empty registry.
    pub fn new() -> Self {
        Self {
            until: Mutex::new(HashMap::new()),
            flushes: AtomicU64::new(0),
            suppressed: AtomicU64::new(0),
        }
    }

    /// Suppress `group` for `ttl_secs` seconds, clamped to
    /// `[DEFAULT_FLUSH_TTL, MAX_FLUSH_TTL]` — `None` or `Some(0)` means
    /// [`DEFAULT_FLUSH_TTL`], anything past [`MAX_FLUSH_TTL`] is capped.
    ///
    /// An empty group id is a no-op (an ungrouped message has no siblings
    /// to suppress) and returns `false`. Re-flushing a group that is
    /// already suppressed EXTENDS the window rather than shortening it —
    /// returns `false` without changing anything if the new expiry would
    /// be earlier than the current one, so a probe response landing
    /// mid-window can never pull the expiry in.
    pub fn flush(&self, group: &str, ttl_secs: Option<u32>) -> bool {
        if group.is_empty() {
            return false;
        }
        let mut ttl = match ttl_secs {
            Some(0) | None => DEFAULT_FLUSH_TTL,
            Some(secs) => Duration::from_secs(secs as u64),
        };
        if ttl > MAX_FLUSH_TTL {
            ttl = MAX_FLUSH_TTL;
        }
        let expiry = Instant::now() + ttl;

        let mut until = self.until.lock();
        if let Some(&current) = until.get(group) {
            if current > expiry {
                return false;
            }
        }
        until.insert(group.to_string(), expiry);
        self.flushes.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Whether `group` is currently suppressed. Evicts the entry as it
    /// expires (so the very next message probes the target) and counts
    /// each suppressed message toward the lifetime `suppressed` counter.
    /// An empty group id is never suppressed.
    pub fn suppressed(&self, group: &str) -> bool {
        if group.is_empty() {
            return false;
        }
        let mut until = self.until.lock();
        match until.get(group).copied() {
            None => false,
            Some(expiry) => {
                if Instant::now() >= expiry {
                    until.remove(group);
                    false
                } else {
                    self.suppressed.fetch_add(1, Ordering::Relaxed);
                    true
                }
            }
        }
    }

    /// When `group`'s suppression lapses, or `None` if it isn't currently
    /// suppressed. Unlike [`Self::suppressed`], this counts nothing and
    /// evicts nothing — the read-only view for an operator asking "why is
    /// this group quiet?" (ledger R-52).
    pub fn suppressed_until(&self, group: &str) -> Option<Instant> {
        let until = self.until.lock();
        match until.get(group).copied() {
            Some(expiry) if Instant::now() < expiry => Some(expiry),
            _ => None,
        }
    }

    /// Lift suppression for `group` immediately (operator override, ledger
    /// R-52). A no-op if the group wasn't suppressed.
    pub fn clear(&self, group: &str) {
        self.until.lock().remove(group);
    }

    /// The live suppression set plus lifetime counters.
    pub fn stats(&self) -> GroupFlushStats {
        let now = Instant::now();
        let until = self.until.lock();
        let active = until.values().filter(|&&expiry| now < expiry).count();
        GroupFlushStats {
            active,
            flushes: self.flushes.load(Ordering::Relaxed),
            suppressed: self.suppressed.load(Ordering::Relaxed),
        }
    }
}

impl Default for GroupFlushRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_group_is_never_suppressed() {
        let r = GroupFlushRegistry::new();
        assert!(!r.flush("", Some(60)));
        assert!(!r.suppressed(""));
    }

    #[test]
    fn flush_then_suppressed_is_true_until_ttl() {
        let r = GroupFlushRegistry::new();
        assert!(r.flush("g1", Some(3600))); // clamped to MAX_FLUSH_TTL, plenty of headroom
        assert!(r.suppressed("g1"));
        let stats = r.stats();
        assert_eq!(stats.active, 1);
        assert_eq!(stats.flushes, 1);
        assert_eq!(stats.suppressed, 1);
    }

    #[test]
    fn suppressed_expires_after_ttl() {
        // ttl_secs is whole seconds, so the shortest TTL a unit test can
        // exercise is 1s: flush for 1s and sleep past it.
        let r = GroupFlushRegistry::new();
        assert!(r.flush("g1", Some(1)));
        assert!(r.suppressed("g1"));
        std::thread::sleep(Duration::from_millis(1100));
        assert!(!r.suppressed("g1"), "suppression must lapse after its TTL");
        assert!(r.suppressed_until("g1").is_none());
    }

    #[test]
    fn clear_lifts_suppression_early() {
        let r = GroupFlushRegistry::new();
        assert!(r.flush("g1", Some(3600)));
        assert!(r.suppressed("g1"));
        r.clear("g1");
        assert!(!r.suppressed("g1"), "clear must lift suppression immediately");
    }

    #[test]
    fn reflush_extends_but_never_shortens() {
        let r = GroupFlushRegistry::new();
        assert!(r.flush("g1", Some(3600)));
        let long_expiry = r.suppressed_until("g1").unwrap();

        // A shorter re-flush must not pull the expiry in.
        assert!(!r.flush("g1", Some(1)));
        assert_eq!(r.suppressed_until("g1").unwrap(), long_expiry);

        // A longer one (still clamped) is accepted, but MAX_FLUSH_TTL caps
        // both at the same ceiling, so re-flushing at the ceiling again
        // extends the window forward from "now".
        std::thread::sleep(Duration::from_millis(10));
        assert!(r.flush("g1", Some(3600)));
        assert!(r.suppressed_until("g1").unwrap() >= long_expiry);
    }

    #[test]
    fn ttl_is_clamped_to_max() {
        let r = GroupFlushRegistry::new();
        let before = Instant::now();
        assert!(r.flush("g1", Some(3600))); // 1h requested
        let expiry = r.suppressed_until("g1").unwrap();
        assert!(
            expiry <= before + MAX_FLUSH_TTL + Duration::from_millis(50),
            "TTL must be clamped to MAX_FLUSH_TTL"
        );
    }

    #[test]
    fn none_or_zero_ttl_uses_default() {
        let r = GroupFlushRegistry::new();
        let before = Instant::now();
        assert!(r.flush("g1", None));
        let expiry = r.suppressed_until("g1").unwrap();
        assert!(expiry > before + Duration::from_secs(1));
        assert!(expiry <= before + DEFAULT_FLUSH_TTL + Duration::from_millis(50));
    }
}
