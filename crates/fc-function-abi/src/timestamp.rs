use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// An instant on the UTC time-line, with Java `java.time.Instant`'s range and
/// precision: whole seconds from the epoch plus nanoseconds (always
/// non-negative, so `-0.5 s` is `(-1, 500_000_000)`).
///
/// Used for [`crate::Schedule`]'s timestamps, parsed with
/// [`Timestamp::parse`] exactly as Java's `Instant.parse` would (Java
/// `Webhook.java` `parseInstant`). A dedicated type rather than
/// `chrono::DateTime`, because `Instant` spans ±1,000,000,000 years and
/// chrono does not.
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

    /// Java's `Instant.parse` (`DateTimeFormatter.ISO_INSTANT`, JDK 25), with
    /// its exact grammar and quirks, pinned by `tests/data/java-golden/webhook-schedule.tsv`:
    ///
    /// - `uuuu-MM-dd'T'HH:mm:ss[.fffffffff](Z|±HH:MM[:SS])`, case-insensitive
    ///   `T` and `Z`; seconds are required; `.` may be followed by 0-9 digits;
    /// - the year is four digits, or more than four with a mandatory sign
    ///   (`+12026`, `-0001`; `+2026` and `12026` are refused), up to
    ///   `Instant`'s range;
    /// - `24:00:00` (no fraction) is midnight of the next day, and `23:59:60`
    ///   is a leap second read as `23:59:59`;
    /// - offsets up to ±18:00, minutes and seconds 0-59.
    pub fn parse(text: &str) -> Option<Self> {
        let mut p = Cursor {
            s: text.as_bytes(),
            pos: 0,
        };
        let year = p.year()?;
        p.expect(b'-')?;
        let month = p.fixed(2)?;
        p.expect(b'-')?;
        let day = p.fixed(2)?;
        p.expect_ci(b'T')?;
        let hour = p.fixed(2)?;
        p.expect(b':')?;
        let minute = p.fixed(2)?;
        p.expect(b':')?;
        let mut second = p.fixed(2)?;
        let nano = p.fraction()?;
        let offset = p.offset()?;
        if p.pos != p.s.len() {
            return None;
        }

        if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
            return None;
        }
        let mut extra_days = 0;
        if hour == 24 && minute == 0 && second == 0 && nano == 0 {
            extra_days = 1;
        } else if hour == 23 && minute == 59 && second == 60 {
            second = 59;
        } else if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        let hour = if extra_days == 1 { 0 } else { hour };

        let days = days_from_civil(year, month, day) + extra_days;
        let local = days
            .checked_mul(86_400)?
            .checked_add(i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))?;
        Self::from_epoch(local.checked_sub(offset)?, nano)
    }
}

struct Cursor<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn expect(&mut self, b: u8) -> Option<()> {
        (self.peek()? == b).then(|| self.pos += 1)
    }

    fn expect_ci(&mut self, b: u8) -> Option<()> {
        (self.peek()?.eq_ignore_ascii_case(&b)).then(|| self.pos += 1)
    }

    fn digits(&mut self, max: usize) -> usize {
        let start = self.pos;
        while self.pos - start < max && self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&self, from: usize) -> i64 {
        self.s[from..self.pos]
            .iter()
            .fold(0, |acc, b| acc * 10 + i64::from(b - b'0'))
    }

    /// Exactly `n` digits.
    fn fixed(&mut self, n: usize) -> Option<u32> {
        let start = self.pos;
        (self.digits(n) == n).then(|| self.number(start) as u32)
    }

    /// `appendValue(YEAR, 4, 10, SignStyle.EXCEEDS_PAD)`.
    fn year(&mut self) -> Option<i64> {
        let sign = match self.peek()? {
            b'+' => Some(1),
            b'-' => Some(-1),
            _ => None,
        };
        if sign.is_some() {
            self.pos += 1;
        }
        let start = self.pos;
        let n = self.digits(10);
        let value = self.number(start);
        match (sign, n) {
            (None, 4) => Some(value),
            // Strict resolving refuses a negative zero.
            (Some(-1), 4..=10) if value != 0 => Some(-value),
            (Some(1), 5..=10) => Some(value),
            _ => None,
        }
    }

    /// `appendFraction(NANO_OF_SECOND, 0, 9, true)`: optional; a `.` may be
    /// followed by zero to nine digits.
    fn fraction(&mut self) -> Option<u32> {
        if self.peek() != Some(b'.') {
            return Some(0);
        }
        self.pos += 1;
        let start = self.pos;
        let n = self.digits(9);
        Some(self.number(start) as u32 * 10u32.pow(9 - n as u32))
    }

    /// `appendOffsetId()`: `Z`, or `±HH:MM` with optional `:SS`, at most 18
    /// hours. Returns the offset in seconds.
    fn offset(&mut self) -> Option<i64> {
        let sign = match self.peek()? {
            b'Z' | b'z' => {
                self.pos += 1;
                return Some(0);
            }
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        self.pos += 1;
        let hours = i64::from(self.fixed(2)?);
        self.expect(b':')?;
        let minutes = i64::from(self.fixed(2)?);
        let seconds = if self.peek() == Some(b':') {
            self.pos += 1;
            i64::from(self.fixed(2)?)
        } else {
            0
        };
        if minutes > 59 || seconds > 59 {
            return None;
        }
        let total = hours * 3600 + minutes * 60 + seconds;
        (total <= 18 * 3600).then_some(sign * total)
    }
}

fn is_leap(year: i64) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 in the proleptic Gregorian calendar (Howard
/// Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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
    fn range_is_instants() {
        assert_eq!(
            ts("+1000000000-12-31T23:59:59.999999999Z"),
            Some((MAX_SECOND, 999_999_999))
        );
        assert_eq!(ts("-1000000000-01-01T00:00:00Z"), Some((MIN_SECOND, 0)));
        assert_eq!(ts("+1000000001-01-01T00:00:00Z"), None);
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
