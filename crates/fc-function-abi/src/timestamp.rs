use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// An instant on the UTC time-line, with Java `java.time.Instant`'s range and
/// precision: whole seconds from the epoch plus nanoseconds (always
/// non-negative, so `-0.5 s` is `(-1, 500_000_000)`).
///
/// Used for [`crate::Schedule`]'s timestamps, parsed with
/// [`Timestamp::parse`] (RFC 3339, which is what the platform sends). A small
/// type of its own rather than `chrono::DateTime`, so the guest API does not
/// depend on chrono's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp {
    epoch_second: i64,
    nano: u32,
}

/// `Instant.MIN` / `Instant.MAX` epoch seconds.
const MIN_SECOND: i64 = -31_557_014_167_219_200;
const MAX_SECOND: i64 = 31_556_889_864_403_199;

impl Timestamp {
    /// Seconds from 1970-01-01T00:00:00Z.
    pub fn epoch_second(&self) -> i64 {
        self.epoch_second
    }

    /// Nanoseconds within the second, `0..1_000_000_000`.
    pub fn nano(&self) -> u32 {
        self.nano
    }

    /// Builds a timestamp, `None` outside `Instant`'s range or when
    /// `nano >= 1_000_000_000`.
    pub fn from_epoch(epoch_second: i64, nano: u32) -> Option<Self> {
        ((MIN_SECOND..=MAX_SECOND).contains(&epoch_second) && nano < 1_000_000_000)
            .then_some(Self { epoch_second, nano })
    }

    /// The same instant as a `SystemTime`, `None` if the platform's
    /// `SystemTime` cannot represent it.
    pub fn to_system_time(&self) -> Option<SystemTime> {
        let nanos = Duration::from_nanos(u64::from(self.nano));
        if self.epoch_second >= 0 {
            UNIX_EPOCH
                .checked_add(Duration::from_secs(self.epoch_second as u64))?
                .checked_add(nanos)
        } else {
            UNIX_EPOCH
                .checked_sub(Duration::from_secs(self.epoch_second.unsigned_abs()))?
                .checked_add(nanos)
        }
    }

    /// An RFC 3339 timestamp (`chrono::DateTime::parse_from_rfc3339`):
    /// `2026-09-19T00:00:01.5Z`, `2026-09-19T00:00:00+01:00`; `None` for
    /// anything else. A leap second (`23:59:60`) reads as `23:59:59`, as
    /// Java's `Instant.parse` reads it.
    pub fn parse(text: &str) -> Option<Self> {
        let t = chrono::DateTime::parse_from_rfc3339(text).ok()?;
        // chrono carries a leap second as a nanosecond count past 1e9.
        Self::from_epoch(t.timestamp(), t.timestamp_subsec_nanos() % 1_000_000_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Option<(i64, u32)> {
        Timestamp::parse(s).map(|t| (t.epoch_second(), t.nano()))
    }

    #[test]
    fn parses_the_common_shapes() {
        assert_eq!(ts("1970-01-01T00:00:00Z"), Some((0, 0)));
        assert_eq!(
            ts("2026-09-19T00:00:01.500000Z"),
            Some((1_789_776_001, 500_000_000))
        );
        assert_eq!(
            ts("1969-12-31T23:59:59.999999999Z"),
            Some((-1, 999_999_999))
        );
        assert_eq!(ts("2026-09-19T00:00:00+01:00"), Some((1_789_772_400, 0)));
        assert_eq!(ts("not-a-time"), None);
    }

    #[test]
    fn a_leap_second_reads_as_the_second_before() {
        assert_eq!(
            ts("2026-09-19T23:59:60.5Z"),
            Some((1_789_862_399, 500_000_000))
        );
    }

    #[test]
    fn system_time_conversion() {
        let t = Timestamp::parse("1969-12-31T23:59:59.5Z").unwrap();
        assert_eq!(
            t.to_system_time(),
            Some(UNIX_EPOCH - Duration::from_millis(500))
        );
    }
}
