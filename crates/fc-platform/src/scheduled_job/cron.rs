//! The scheduled-job cron dialect, read and evaluated as Go's scheduler does.
//!
//! Go's poller (`internal/platform/scheduledjob/cron.go`) reads every stored
//! cron with robfig/cron v3, `NewParser(Second | Minute | Hour | Dom | Month
//! | Dow)`, and walks it with `SpecSchedule.Next`; Java's
//! `scheduledjob/cron/CronExpression` is a port of the same code. This is a
//! port of both, so a cron stored by any of the three platforms fires at the
//! same instants on each:
//!
//! - exactly six fields, `second minute hour day-of-month month
//!   day-of-week`; day of week `0-6` with `0` = Sunday, or `sun`..`sat`;
//!   months `1-12` or `jan`..`dec`; `*`/`?`, `N`, `N-M`, `/step` (and `N/step`
//!   meaning `N-max/step`), comma lists (empty list items are dropped);
//! - an optional `TZ=<zone> ` / `CRON_TZ=<zone> ` prefix naming the zone the
//!   expression is evaluated in; no `@` descriptors;
//! - robfig's day rule: when either day field is a bare `*`/`?` (not stepped
//!   over 1), a day must match both; when both are restricted, **either**;
//! - `next`: whole seconds; months and days step on the local calendar,
//!   hours, minutes and seconds on the instant timeline, local wall times
//!   resolved as Go's `time.Date` resolves them, and nothing past five years.
//!
//! The job's zone ([`JobZone::resolve`]) is a tz database region, one of
//! Java's fixed-offset ids (a function manifest may name one), or, for any
//! other name, UTC, as Go's `LatestSlotInWindow` falls back to it.
//!
//! The previous reader (the `cron` crate: days of the week `1-7` from Sunday
//! and both day fields always required) survives only in the startup
//! migration that rewrites the crons it read (`cron_migration`).

use std::str::FromStr;

use chrono::{
    DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, Offset, TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;

/// robfig's `starBit`: set on a field written `*` or `?` without a step
/// over 1. Only the day fields' star bits matter (the day rule).
const STAR_BIT: u64 = 1 << 63;

/// How far `next` searches before giving up (robfig's `yearLimit`).
const YEAR_LIMIT: i32 = 5;

struct Bounds {
    min: u64,
    max: u64,
    names: &'static [(&'static str, u64)],
}

const SECONDS: Bounds = Bounds {
    min: 0,
    max: 59,
    names: &[],
};
const MINUTES: Bounds = SECONDS;
const HOURS: Bounds = Bounds {
    min: 0,
    max: 23,
    names: &[],
};
const DAYS_OF_MONTH: Bounds = Bounds {
    min: 1,
    max: 31,
    names: &[],
};
const MONTHS: Bounds = Bounds {
    min: 1,
    max: 12,
    names: &[
        ("jan", 1),
        ("feb", 2),
        ("mar", 3),
        ("apr", 4),
        ("may", 5),
        ("jun", 6),
        ("jul", 7),
        ("aug", 8),
        ("sep", 9),
        ("oct", 10),
        ("nov", 11),
        ("dec", 12),
    ],
};
const DAYS_OF_WEEK: Bounds = Bounds {
    min: 0,
    max: 6,
    names: &[
        ("sun", 0),
        ("mon", 1),
        ("tue", 2),
        ("wed", 3),
        ("thu", 4),
        ("fri", 5),
        ("sat", 6),
    ],
};

/// Why a cron expression does not parse (robfig's messages).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CronParseError(String);

/// A parsed cron expression (robfig `SpecSchedule`): a bit set per field
/// (bit `n` for value `n`, plus [`STAR_BIT`]), and the zone of a `TZ=`
/// prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSpec {
    second: u64,
    minute: u64,
    hour: u64,
    dom: u64,
    month: u64,
    dow: u64,
    zone: Option<Tz>,
}

impl FromStr for CronSpec {
    type Err = CronParseError;

    /// robfig `Parser.Parse` with `Second | Minute | Hour | Dom | Month |
    /// Dow`.
    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        if spec.is_empty() {
            return Err(CronParseError("empty spec string".into()));
        }
        let mut spec = spec;
        let mut zone = None;
        if spec.starts_with("TZ=") || spec.starts_with("CRON_TZ=") {
            let name_end = spec.find(' ');
            let eq = spec.find('=').unwrap_or(0);
            let name = &spec[eq + 1..name_end.unwrap_or(spec.len())];
            let tz = match (name_end, name) {
                (None, _) => None,              // Go slices out of range here: no schedule
                (Some(_), "") => Some(Tz::UTC), // `time.LoadLocation("")`
                (Some(_), name) => Tz::from_str(name).ok(),
            };
            let Some(tz) = tz else {
                return Err(CronParseError(format!("provided bad location {name}")));
            };
            zone = Some(tz);
            spec = spec[name_end.unwrap_or(spec.len())..].trim();
        }
        if spec.starts_with('@') {
            return Err(CronParseError(format!(
                "parser does not accept descriptors: {spec}"
            )));
        }
        let fields: Vec<&str> = spec.split_whitespace().collect();
        if fields.len() != 6 {
            return Err(CronParseError(format!(
                "expected exactly 6 fields, found {}: [{}]",
                fields.len(),
                fields.join(" ")
            )));
        }
        Ok(CronSpec {
            second: field_bits(fields[0], &SECONDS)?,
            minute: field_bits(fields[1], &MINUTES)?,
            hour: field_bits(fields[2], &HOURS)?,
            dom: field_bits(fields[3], &DAYS_OF_MONTH)?,
            month: field_bits(fields[4], &MONTHS)?,
            dow: field_bits(fields[5], &DAYS_OF_WEEK)?,
            zone,
        })
    }
}

/// robfig `getField`: comma-separated ranges, empty items dropped.
fn field_bits(field: &str, bounds: &Bounds) -> Result<u64, CronParseError> {
    field
        .split(',')
        .filter(|r| !r.is_empty())
        .try_fold(0, |bits, r| Ok(bits | range_bits(r, bounds)?))
}

/// robfig `getRange`: `*|?|N|N-M` (numbers or names), optionally `/step`.
fn range_bits(expr: &str, r: &Bounds) -> Result<u64, CronParseError> {
    let range_and_step: Vec<&str> = expr.split('/').collect();
    let low_and_high: Vec<&str> = range_and_step[0].split('-').collect();
    let single = low_and_high.len() == 1;
    let mut extra = 0;
    let (start, mut end) = if low_and_high[0] == "*" || low_and_high[0] == "?" {
        extra = STAR_BIT;
        (r.min, r.max)
    } else {
        let start = int_or_name(low_and_high[0], r.names)?;
        let end = match low_and_high.len() {
            1 => start,
            2 => int_or_name(low_and_high[1], r.names)?,
            _ => return Err(CronParseError(format!("too many hyphens: {expr}"))),
        };
        (start, end)
    };
    let step = match range_and_step.len() {
        1 => 1,
        2 => {
            let step = non_negative_int(range_and_step[1])?;
            if single {
                end = r.max; // "N/step" means "N-max/step"
            }
            if step > 1 {
                extra = 0;
            }
            step
        }
        _ => return Err(CronParseError(format!("too many slashes: {expr}"))),
    };
    if start < r.min {
        return Err(CronParseError(format!(
            "beginning of range ({start}) below minimum ({}): {expr}",
            r.min
        )));
    }
    if end > r.max {
        return Err(CronParseError(format!(
            "end of range ({end}) above maximum ({}): {expr}",
            r.max
        )));
    }
    if start > end {
        return Err(CronParseError(format!(
            "beginning of range ({start}) beyond end of range ({end}): {expr}"
        )));
    }
    if step == 0 {
        return Err(CronParseError(format!(
            "step of range should be a positive number: {expr}"
        )));
    }
    let mut bits = 0u64;
    let mut i = start;
    while i <= end {
        bits |= 1 << i;
        i = i.saturating_add(step);
    }
    Ok(bits | extra)
}

fn int_or_name(expr: &str, names: &[(&str, u64)]) -> Result<u64, CronParseError> {
    let lower = expr.to_lowercase();
    match names.iter().find(|(n, _)| *n == lower) {
        Some((_, v)) => Ok(*v),
        None => non_negative_int(expr),
    }
}

/// Go's `strconv.Atoi` (an optional sign, ASCII digits, 64 bits), then no
/// negatives.
fn non_negative_int(expr: &str) -> Result<u64, CronParseError> {
    let digits = expr.strip_prefix(['+', '-']).unwrap_or(expr);
    let n: i64 = if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        expr.parse().ok()
    } else {
        None
    }
    .ok_or_else(|| CronParseError(format!("failed to parse int from {expr}")))?;
    if n < 0 {
        return Err(CronParseError(format!(
            "negative number ({n}) not allowed: {expr}"
        )));
    }
    Ok(n as u64)
}

fn bit(bits: u64, n: u32) -> bool {
    bits & (1 << n) != 0
}

impl CronSpec {
    /// The zone a `TZ=` prefix names, which overrides the job's.
    pub fn zone(&self) -> Option<Tz> {
        self.zone
    }

    /// robfig `dayMatches`.
    fn day_matches(&self, local: &NaiveDateTime) -> bool {
        let dom = bit(self.dom, local.day());
        let dow = bit(self.dow, local.weekday().num_days_from_sunday());
        if self.dom & STAR_BIT != 0 || self.dow & STAR_BIT != 0 {
            dom && dow
        } else {
            dom || dow
        }
    }

    /// Whether any instant can match: a field with no values (a list of
    /// empty items, like `,`) never does. robfig would search five years to
    /// find that out.
    fn can_match(&self) -> bool {
        let values = |bits: u64| bits & !STAR_BIT != 0;
        values(self.second)
            && values(self.minute)
            && values(self.hour)
            && values(self.month)
            && (values(self.dom) || values(self.dow))
    }

    /// robfig `SpecSchedule.Next`: the first instant strictly after `after`
    /// that matches, evaluated in the `TZ=` zone if there is one, else in
    /// `zone`; `None` if none falls within five years.
    pub fn next(&self, after: DateTime<Utc>, zone: JobZone) -> Option<DateTime<Utc>> {
        if !self.can_match() {
            return None;
        }
        let z = self.zone.map(JobZone::Region).unwrap_or(zone);
        // The upcoming whole second.
        let mut t = after.timestamp() + 1;
        let mut added = false;
        let year_limit = z.local(t).year() + YEAR_LIMIT;

        'wrap: loop {
            if z.local(t).year() > year_limit {
                return None;
            }

            while !bit(self.month, z.local(t).month()) {
                if !added {
                    added = true;
                    let l = z.local(t);
                    t = z.date(l.year(), l.month() as i64, 1, 0, 0, 0);
                }
                t = z.add_date(t, 1, 0);
                if z.local(t).month() == 1 {
                    continue 'wrap;
                }
            }

            while !self.day_matches(&z.local(t)) {
                if !added {
                    added = true;
                    let l = z.local(t);
                    t = z.date(l.year(), l.month() as i64, l.day() as i64, 0, 0, 0);
                }
                t = z.add_date(t, 0, 1);
                // A DST change at midnight leaves the hour at 23 or 1.
                let hour = z.local(t).hour() as i64;
                if hour != 0 {
                    if hour > 12 {
                        t += (24 - hour) * 3600;
                    } else {
                        t -= hour * 3600;
                    }
                }
                if z.local(t).day() == 1 {
                    continue 'wrap;
                }
            }

            while !bit(self.hour, z.local(t).hour()) {
                if !added {
                    added = true;
                    let l = z.local(t);
                    t = z.date(
                        l.year(),
                        l.month() as i64,
                        l.day() as i64,
                        l.hour() as i64,
                        0,
                        0,
                    );
                }
                t += 3600;
                if z.local(t).hour() == 0 {
                    continue 'wrap;
                }
            }

            while !bit(self.minute, z.local(t).minute()) {
                if !added {
                    added = true;
                    t -= t.rem_euclid(60); // Truncate(time.Minute), on the instant
                }
                t += 60;
                if z.local(t).minute() == 0 {
                    continue 'wrap;
                }
            }

            while !bit(self.second, z.local(t).second()) {
                added = true;
                t += 1;
                if z.local(t).second() == 0 {
                    continue 'wrap;
                }
            }

            return DateTime::from_timestamp(t, 0);
        }
    }
}

/// The zone a job's crons are evaluated in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobZone {
    Region(Tz),
    Fixed(FixedOffset),
}

impl JobZone {
    /// A tz database region, else one of Java's fixed-offset zone ids (`Z`,
    /// `+05:30`, `UTC+1`, `GMT-5`, ...; `function::schedule_check`), else
    /// `None`.
    pub fn parse(name: &str) -> Option<JobZone> {
        if let Ok(tz) = Tz::from_str(name) {
            return Some(JobZone::Region(tz));
        }
        crate::function::schedule_check::java_fixed_offset_seconds(name)
            .and_then(FixedOffset::east_opt)
            .map(JobZone::Fixed)
    }

    /// [`JobZone::parse`], or UTC for a name it can't read: Go's
    /// `LatestSlotInWindow` falls back to UTC when `time.LoadLocation`
    /// fails.
    pub fn resolve(name: &str) -> JobZone {
        Self::parse(name).unwrap_or(JobZone::Region(Tz::UTC))
    }

    /// Seconds east of UTC at the instant `unix`.
    fn offset_at(self, unix: i64) -> i64 {
        let at = DateTime::from_timestamp(unix, 0)
            .unwrap_or_default()
            .naive_utc();
        match self {
            JobZone::Region(tz) => tz.offset_from_utc_datetime(&at).fix().local_minus_utc() as i64,
            JobZone::Fixed(offset) => offset.local_minus_utc() as i64,
        }
    }

    /// The local wall time at the instant `unix`.
    fn local(self, unix: i64) -> NaiveDateTime {
        DateTime::from_timestamp(unix + self.offset_at(unix), 0)
            .unwrap_or_default()
            .naive_utc()
    }

    /// Go's `time.Date`: out-of-range months and days normalise into the
    /// next ones, and a wall time is placed on the timeline with the offset
    /// in effect at that wall time read as UTC, corrected once if the
    /// result falls outside that offset's period (so a time in a gap or an
    /// overlap resolves as Go resolves it).
    fn date(self, year: i32, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
        let m0 = month - 1;
        let year = year as i64 + m0.div_euclid(12);
        let month = (m0.rem_euclid(12) + 1) as u32;
        let first = NaiveDate::from_ymd_opt(year as i32, month, 1)
            .unwrap_or_default()
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default()
            .and_utc()
            .timestamp();
        let unix = first + (day - 1) * 86_400 + hour * 3600 + minute * 60 + second;
        let offset = self.offset_at(unix);
        let corrected = self.offset_at(unix - offset);
        unix - corrected
    }

    /// Go's `Time.AddDate(0, months, days)`, on the local calendar.
    fn add_date(self, unix: i64, months: i64, days: i64) -> i64 {
        let l = self.local(unix);
        self.date(
            l.year(),
            l.month() as i64 + months,
            l.day() as i64 + days,
            l.hour() as i64,
            l.minute() as i64,
            l.second() as i64,
        )
    }
}

/// What the poller evaluates for one job: the crons that parse, in the
/// job's zone. Go's `LatestSlotInWindow` skips a cron that does not parse
/// and reads an unknown zone as UTC; [`JobSchedule::problems`] says which.
pub struct JobSchedule {
    specs: Vec<CronSpec>,
    zone: JobZone,
    problems: Vec<String>,
}

impl JobSchedule {
    pub fn new(crons: &[String], tz_name: &str) -> Self {
        let mut problems = Vec::new();
        let zone = JobZone::parse(tz_name).unwrap_or_else(|| {
            problems.push(format!("unknown timezone '{tz_name}', evaluated in UTC"));
            JobZone::Region(Tz::UTC)
        });
        let specs = crons
            .iter()
            .filter_map(|c| match c.parse::<CronSpec>() {
                Ok(spec) => Some(spec),
                Err(e) => {
                    problems.push(format!("cron '{c}' skipped: {e}"));
                    None
                }
            })
            .collect();
        JobSchedule {
            specs,
            zone,
            problems,
        }
    }

    /// Crons skipped and a zone read as UTC; empty when everything reads.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// The first slot of any cron strictly after `after`.
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.specs
            .iter()
            .filter_map(|s| s.next(after, self.zone))
            .min()
    }

    /// Go's `LatestSlotInWindow`: the latest slot of any cron in the
    /// half-open window `(after, up_to]`, walking each forward from `after`.
    pub fn latest_in_window(
        &self,
        after: DateTime<Utc>,
        up_to: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        if after >= up_to {
            return None;
        }
        let mut best: Option<DateTime<Utc>> = None;
        for spec in &self.specs {
            let mut slot = spec.next(after, self.zone);
            while let Some(s) = slot.filter(|s| *s <= up_to) {
                best = best.max(Some(s));
                slot = spec.next(s, self.zone);
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn walk(cron: &str, zone: &str, from: &str, n: usize) -> Vec<String> {
        let schedule = JobSchedule::new(&[cron.to_string()], zone);
        let mut t = at(from);
        let mut out = Vec::new();
        for _ in 0..n {
            let Some(next) = schedule.next_after(t) else {
                break;
            };
            out.push(next.format("%Y-%m-%dT%H:%M:%SZ").to_string());
            t = next;
        }
        out
    }

    #[test]
    fn days_of_the_week_count_from_sunday_zero() {
        // 2026-03-07 is a Saturday.
        assert_eq!(
            walk("0 0 9 * * 1-5", "UTC", "2026-03-07T12:00:00Z", 3),
            [
                "2026-03-09T09:00:00Z",
                "2026-03-10T09:00:00Z",
                "2026-03-11T09:00:00Z"
            ]
        );
        assert_eq!(
            walk("0 0 12 ? * 0", "UTC", "2026-03-07T12:00:00Z", 1),
            ["2026-03-08T12:00:00Z"]
        );
        assert!("0 0 0 * * 7".parse::<CronSpec>().is_err());
    }

    #[test]
    fn either_day_matches_when_both_are_restricted() {
        // The 13th, or any Friday.
        assert_eq!(
            walk("0 0 0 13 * 5", "UTC", "2026-03-07T12:00:00Z", 3),
            [
                "2026-03-13T00:00:00Z",
                "2026-03-20T00:00:00Z",
                "2026-03-27T00:00:00Z"
            ]
        );
        // A bare wildcard day field: both must match.
        assert_eq!(
            walk("0 0 0 13 * *", "UTC", "2026-03-07T12:00:00Z", 2),
            ["2026-03-13T00:00:00Z", "2026-04-13T00:00:00Z"]
        );
        // A stepped wildcard is a restriction: odd days or Mondays.
        assert_eq!(
            walk("0 0 0 */2 * 1", "UTC", "2026-03-07T12:00:00Z", 3),
            [
                "2026-03-09T00:00:00Z",
                "2026-03-11T00:00:00Z",
                "2026-03-13T00:00:00Z"
            ]
        );
    }

    #[test]
    fn grammar_follows_robfig() {
        for ok in [
            "0 0 * * * *",
            "*/15 * * * * ?",
            "0 30 9 * * mon-fri",
            "0 0 0 1 JAN,jul *",
            "+5 0 0 * * *",
            "0,,1 * * * * *",
            "TZ=Europe/Amsterdam 0 0 9 * * *",
            "CRON_TZ=UTC 0 0 9 * * *",
            "0 0 0 * * 1/99999999999",
        ] {
            assert!(ok.parse::<CronSpec>().is_ok(), "{ok}");
        }
        for bad in [
            "",
            "0 * * * *",
            "0 0 * * * * *",
            "@daily",
            "60 * * * * *",
            "0 0 0 0 * *",
            "0 0 5-3 * * *",
            "0 0 0 * * 1/0",
            "0/1/2 * * * * *",
            "1-2-3 * * * * *",
            "x * * * * *",
            "0 0 0 * * 1/-1",
            "TZ=Mars/Olympus 0 0 9 * * *",
            "TZ=UTC",
        ] {
            assert!(bad.parse::<CronSpec>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_tz_prefix_overrides_the_jobs_zone() {
        assert_eq!(
            walk(
                "TZ=America/New_York 0 0 9 * * *",
                "Asia/Kolkata",
                "2026-06-01T00:00:00Z",
                1
            ),
            ["2026-06-01T13:00:00Z"]
        );
    }

    #[test]
    fn an_unknown_zone_is_utc_and_an_unreadable_cron_is_skipped() {
        let s = JobSchedule::new(
            &["0 * * * *".to_string(), "0 0 9 * * *".to_string()],
            "SystemV/EST5",
        );
        assert_eq!(s.problems().len(), 2);
        assert_eq!(
            s.next_after(at("2026-06-01T00:00:00Z")),
            Some(at("2026-06-01T09:00:00Z"))
        );
    }

    #[test]
    fn a_field_with_no_values_never_matches() {
        let s = JobSchedule::new(&[", * * * * *".to_string()], "UTC");
        assert!(s.problems().is_empty());
        assert_eq!(s.next_after(at("2026-06-01T00:00:00Z")), None);
    }

    #[test]
    fn a_dst_gap_is_skipped_and_an_overlap_fires_twice() {
        // New York springs forward at 02:00 on 2026-03-08: 02:30 does not
        // exist that day.
        assert_eq!(
            walk(
                "0 30 2 * * *",
                "America/New_York",
                "2026-03-07T12:00:00Z",
                2
            ),
            ["2026-03-09T06:30:00Z", "2026-03-10T06:30:00Z"]
        );
        // ... and falls back at 02:00 on 2026-11-01: 01:30 happens twice.
        assert_eq!(
            walk(
                "0 30 1 * * *",
                "America/New_York",
                "2026-10-31T12:00:00Z",
                3
            ),
            [
                "2026-11-01T05:30:00Z",
                "2026-11-01T06:30:00Z",
                "2026-11-02T06:30:00Z"
            ]
        );
    }

    #[test]
    fn nothing_past_five_years() {
        let s = JobSchedule::new(&["0 0 0 30 2 *".to_string()], "UTC");
        assert_eq!(s.next_after(at("2026-06-01T00:00:00Z")), None);
    }
}
