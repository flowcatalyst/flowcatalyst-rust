//! The two schedule checks publish makes (Java
//! `FunctionTriggerSync.checkCron` / `checkTimezone`): Java's cron grammar
//! (`scheduledjob/cron/CronExpression.parse`, validation only) and Java's
//! `ZoneId.of`.
//!
//! **Beyond Java (owner decision 5):** a manifest cron may have 5 fields
//! (`minute hour day-of-month month day-of-week`) or Java's 6; with 5 the
//! seconds are `0`, and the stored cron (what the scheduler, which reads
//! six, evaluates) is the text with `0 ` in front. Every refusal has one
//! code, `CRON_INVALID` (Java splits `INVALID_CRON` / `CRON_INVALID_SHAPE`
//! internally and publishes `CRON_INVALID`).
//!
//! The grammar is ported with Java's field bit sets ([`JavaCron`]): publish
//! refuses what Java refuses, with the same messages, and promote stores the
//! cron as Java does. Evaluating a schedule is the scheduler's job; it reads
//! the same dialect (`scheduled_job::cron`).

use std::str::FromStr;

/// The six fields `second minute hour day-of-month month day-of-week`.
const FIELD_COUNT: usize = 6;

/// The one code every cron refusal carries.
pub const CRON_INVALID: &str = "CRON_INVALID";

struct Bounds {
    min: u32,
    max: u32,
    names: &'static [(&'static str, u32)],
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

/// Java's `\s`: space, tab, newline, vertical tab, form feed, carriage
/// return.
fn is_regex_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// Java's `CronExpression.STAR_BIT`: set on a day field written `*` or `?`
/// without a step over 1. When either day field has it, a day must match
/// both day fields; when neither has it, either one.
pub const STAR_BIT: u64 = 1 << 63;

/// A parsed Java cron expression (Java `CronExpression`): the text as the
/// scheduler stores it (stripped; a 5-field one with `0 ` seconds in
/// front), its six fields, and each field's bit set (bit `n` for value `n`,
/// plus [`STAR_BIT`] on the day fields). Day of week 0 is Sunday.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaCron {
    pub expression: String,
    pub fields: [String; FIELD_COUNT],
    pub bits: [u64; FIELD_COUNT],
}

/// Java `CronExpression.parse`, plus 5-field crons: `Ok` or `(code,
/// message)`, the code always [`CRON_INVALID`] (blank, descriptor,
/// per-expression zone, neither 5 nor 6 fields, a malformed field).
pub fn parse_cron(text: &str) -> Result<(), (&'static str, String)> {
    parse_java_cron(text).map(|_| ())
}

/// [`parse_cron`], keeping what it parsed.
pub fn parse_java_cron(text: &str) -> Result<JavaCron, (&'static str, String)> {
    if super::java_is_blank(text) {
        return Err((CRON_INVALID, "cron expressions cannot be empty".into()));
    }
    // Java's `strip()`: `Character.isWhitespace` at both ends.
    let expr = text.trim_matches(|c: char| super::java_is_blank(c.encode_utf8(&mut [0; 4])));
    if expr.starts_with('@') {
        return Err((
            CRON_INVALID,
            format!("cron expression '{expr}': descriptors are not supported"),
        ));
    }
    if expr.starts_with("TZ=") || expr.starts_with("CRON_TZ=") {
        return Err((
            CRON_INVALID,
            format!(
                "cron expression '{expr}': a per-expression time zone is not supported; \
                 use the job's timezone"
            ),
        ));
    }
    let mut fields: Vec<&str> = expr
        .split(is_regex_space)
        .filter(|f| !f.is_empty())
        .collect();
    let expression = match fields.len() {
        FIELD_COUNT => expr.to_string(),
        // No seconds field: they are 0, and the stored form says so.
        5 => {
            fields.insert(0, "0");
            format!("0 {expr}")
        }
        n => {
            return Err((
                CRON_INVALID,
                format!(
                    "cron expression must have 5 or 6 whitespace-separated fields \
                     ([sec] min hour dom mon dow), got {n}: '{expr}'"
                ),
            ))
        }
    };
    let bounds = [
        &SECONDS,
        &MINUTES,
        &HOURS,
        &DAYS_OF_MONTH,
        &MONTHS,
        &DAYS_OF_WEEK,
    ];
    let mut bits = [0u64; FIELD_COUNT];
    for (i, (field, bounds)) in fields.iter().zip(bounds).enumerate() {
        bits[i] = field_bits(field, bounds)
            .map_err(|why| (CRON_INVALID, format!("cron expression '{expr}': {why}")))?;
    }
    Ok(JavaCron {
        expression,
        fields: std::array::from_fn(|i| fields[i].to_string()),
        bits,
    })
}

/// A comma-separated list of ranges; an empty item is malformed.
fn field_bits(field: &str, bounds: &Bounds) -> Result<u64, String> {
    let mut bits = 0;
    for item in field.split(',') {
        if item.is_empty() {
            return Err(format!("empty list item in field: {field}"));
        }
        bits |= range_bits(item, bounds)?;
    }
    Ok(bits)
}

/// One `*|?|N|N-M|name[-name]` with an optional `/step`.
fn range_bits(expr: &str, r: &Bounds) -> Result<u64, String> {
    let range_and_step: Vec<&str> = expr.split('/').collect();
    if range_and_step.len() > 2 {
        return Err(format!("too many slashes: {expr}"));
    }
    let low_and_high: Vec<&str> = range_and_step[0].split('-').collect();
    let single_value = low_and_high.len() == 1;
    let mut extra = 0;
    let (start, mut end) = if low_and_high[0] == "*" || low_and_high[0] == "?" {
        extra = STAR_BIT;
        (r.min as i64, r.max as i64)
    } else {
        let start = int_or_name(low_and_high[0], r.names)?;
        let end = match low_and_high.len() {
            1 => start,
            2 => int_or_name(low_and_high[1], r.names)?,
            _ => return Err(format!("too many hyphens: {expr}")),
        };
        (start, end)
    };
    let mut step = 1;
    if range_and_step.len() == 2 {
        step = non_negative_int(range_and_step[1])?;
        if single_value {
            end = r.max as i64; // "N/step" means "N-max/step"
        }
        if step > 1 {
            extra = 0; // a stepped wildcard is a restriction, not "any day"
        }
    }
    if start < r.min as i64 {
        return Err(format!(
            "beginning of range ({start}) below minimum ({}): {expr}",
            r.min
        ));
    }
    if end > r.max as i64 {
        return Err(format!(
            "end of range ({end}) above maximum ({}): {expr}",
            r.max
        ));
    }
    if start > end {
        return Err(format!(
            "beginning of range ({start}) beyond end of range ({end}): {expr}"
        ));
    }
    if step == 0 {
        return Err(format!("step of range should be a positive number: {expr}"));
    }
    let mut bits = 0u64;
    let mut i = start;
    while i <= end {
        bits |= 1 << i;
        i += step;
    }
    Ok(bits | extra)
}

fn int_or_name(token: &str, names: &[(&str, u32)]) -> Result<i64, String> {
    let lower = token.to_lowercase();
    match names.iter().find(|(n, _)| *n == lower) {
        Some((_, v)) => Ok(*v as i64),
        None => non_negative_int(token),
    }
}

/// Go `Atoi` semantics as Java ports them: an optional sign and ASCII
/// digits that fit an `int`; negatives are rejected.
fn non_negative_int(token: &str) -> Result<i64, String> {
    let digits = token.strip_prefix(['+', '-']).unwrap_or(token);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("failed to parse int from {token}"));
    }
    let n: i32 = token
        .parse()
        .map_err(|_| format!("failed to parse int from {token}"))?;
    if n < 0 {
        return Err(format!("negative number ({n}) not allowed: {token}"));
    }
    Ok(n as i64)
}

/// Java `ZoneId.of`: `Z`, an offset (`+h`, `+hh`, `+hh:mm`, `+hhmm`,
/// `+hh:mm:ss`, `+hhmmss`, at most ±18:00), `UTC`/`GMT`/`UT` alone or
/// followed by an offset, or a tz database region id the scheduler can
/// evaluate ([`region_valid`]).
pub fn zone_id_valid(id: &str) -> bool {
    if id == "Z" || id.starts_with(['+', '-']) {
        return offset_valid(id);
    }
    for prefix in ["UTC", "GMT", "UT"] {
        if id == prefix {
            return true;
        }
        if let Some(rest) = id.strip_prefix(prefix) {
            if rest.starts_with(['+', '-']) {
                return offset_valid(rest);
            }
        }
    }
    region_valid(id)
}

/// The fixed offset, in seconds east of UTC, that a Java zone id which is
/// not a region stands for: `Z`, an offset, or `UTC`/`GMT`/`UT` alone or
/// followed by an offset. `None` for a region id (or an invalid one).
pub fn java_fixed_offset_seconds(id: &str) -> Option<i32> {
    if !zone_id_valid(id) {
        return None;
    }
    let offset = if id == "Z" || id.starts_with(['+', '-']) {
        id
    } else {
        let rest = ["UTC", "GMT", "UT"]
            .iter()
            .find_map(|prefix| id.strip_prefix(prefix))?;
        if rest.is_empty() {
            return Some(0);
        }
        rest
    };
    if offset == "Z" {
        return Some(0);
    }
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let (h, m, s) = offset_parts(&offset[1..])?;
    Some(sign * (h * 3600 + m * 60 + s) as i32)
}

/// Java `ZoneOffset.of`.
fn offset_valid(id: &str) -> bool {
    if id == "Z" {
        return true;
    }
    match id.strip_prefix(['+', '-']).and_then(offset_parts) {
        Some((h, m, s)) => h <= 18 && m <= 59 && s <= 59 && (h < 18 || (m == 0 && s == 0)),
        None => false,
    }
}

/// The hours, minutes and seconds of an offset after its sign.
fn offset_parts(body: &str) -> Option<(u32, u32, u32)> {
    if !body.is_ascii() {
        return None;
    }
    let num = |s: &str| -> Option<u32> {
        if s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    match body.len() {
        1 if body.bytes().all(|b| b.is_ascii_digit()) => Some((body.parse().ok()?, 0, 0)),
        2 => Some((num(body)?, 0, 0)),
        4 => Some((num(&body[0..2])?, num(&body[2..4])?, 0)),
        5 if &body[2..3] == ":" => Some((num(&body[0..2])?, num(&body[3..5])?, 0)),
        6 => Some((num(&body[0..2])?, num(&body[2..4])?, num(&body[4..6])?)),
        8 if &body[2..3] == ":" && &body[5..6] == ":" => {
            Some((num(&body[0..2])?, num(&body[3..5])?, num(&body[6..8])?))
        }
        _ => None,
    }
}

/// Region ids in chrono-tz's tz database that the JDK's leaves out: the
/// three that clash with its legacy short ids, and `ROC`.
const NOT_IN_JDK: [&str; 4] = ["EST", "HST", "MST", "ROC"];

/// Java `ZoneRegion.ofId(id, true)`: the id's shape, then a tz database
/// lookup, in chrono-tz's database without the ids the JDK's (25) leaves
/// out. The JDK also keeps the legacy `SystemV/*` ids, which chrono-tz's
/// database lacks: the scheduler could never evaluate them, so they are
/// refused (owner decision 2 of 2026-09-25) rather than accepted and never
/// fired.
fn region_valid(id: &str) -> bool {
    let mut chars = id.chars();
    let shaped = id.len() >= 2
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "~/._+-".contains(c));
    shaped && chrono_tz::Tz::from_str(id).is_ok() && !NOT_IN_JDK.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(text: &str) -> (&'static str, String) {
        parse_cron(text).unwrap_err()
    }

    /// Spot checks; `tests/function_publish_rules_golden_test.rs` holds the
    /// cases checked against Java itself.
    #[test]
    fn cron_grammar_follows_java() {
        for ok in [
            "0 0 * * * *",
            "*/15 * * * * ?",
            "0 30 9 * * mon-fri",
            "0 0 0 1 JAN,jul *",
            "0 0 12 ? * SUN",
            "5/10 0 0 * * *",
            "+5 0 0 * * *",
            "  0 0 * * * *  ",
        ] {
            assert!(parse_cron(ok).is_ok(), "{ok}");
        }
        assert_eq!(
            err("  "),
            ("CRON_INVALID", "cron expressions cannot be empty".into())
        );
        assert_eq!(
            err("@hourly"),
            (
                "CRON_INVALID",
                "cron expression '@hourly': descriptors are not supported".into()
            )
        );
        assert_eq!(err("TZ=UTC 0 * * * * *").0, "CRON_INVALID");
        for (text, n) in [("0 * * *", 4), ("0 0 * * * * *", 7)] {
            assert_eq!(
                err(text),
                (
                    "CRON_INVALID",
                    format!(
                        "cron expression must have 5 or 6 whitespace-separated fields \
                         ([sec] min hour dom mon dow), got {n}: '{text}'"
                    )
                )
            );
        }
        let cases = [
            ("60 * * * * *", "end of range (60) above maximum (59): 60"),
            ("0 0 0 0 * *", "beginning of range (0) below minimum (1): 0"),
            (
                "0 0 5-3 * * *",
                "beginning of range (5) beyond end of range (3): 5-3",
            ),
            (
                "0 0 0 * * 1/0",
                "step of range should be a positive number: 1/0",
            ),
            ("0,,1 * * * * *", "empty list item in field: 0,,1"),
            ("0/1/2 * * * * *", "too many slashes: 0/1/2"),
            ("1-2-3 * * * * *", "too many hyphens: 1-2-3"),
            ("x * * * * *", "failed to parse int from x"),
            ("0 0 0 * * 1/-1", "negative number (-1) not allowed: -1"),
        ];
        for (text, why) in cases {
            assert_eq!(
                err(text),
                ("CRON_INVALID", format!("cron expression '{text}': {why}")),
                "{text}"
            );
        }
    }

    /// Five fields are six with seconds 0, and are stored that way; six
    /// are stored as written (stripped).
    #[test]
    fn five_fields_mean_seconds_zero() {
        let five = parse_java_cron(" 30 9 * * 1-5 ").unwrap();
        let six = parse_java_cron("0 30 9 * * 1-5").unwrap();
        assert_eq!(five.expression, "0 30 9 * * 1-5");
        assert_eq!(five.fields, six.fields);
        assert_eq!(five.bits, six.bits);
        assert_eq!(
            parse_java_cron("0\t30 9 * * *").unwrap().expression,
            "0\t30 9 * * *"
        );
        assert_eq!(parse_java_cron("*/5 * * * *").unwrap().fields[0], "0");
        assert_eq!(
            err("61 * * * *"),
            (
                "CRON_INVALID",
                "cron expression '61 * * * *': end of range (61) above maximum (59): 61".into()
            ),
            "a 5-field cron's first field is the minute"
        );
    }

    #[test]
    fn zone_ids_follow_java() {
        for ok in [
            "UTC",
            "GMT",
            "UT",
            "Z",
            "Europe/Amsterdam",
            "America/New_York",
            "Etc/GMT+2",
            "+01:00",
            "-5",
            "+0130",
            "+18:00",
            "UTC+01:00",
            "GMT-5",
            "+01:30:15",
        ] {
            assert!(zone_id_valid(ok), "{ok}");
        }
        for bad in [
            "",
            "Mars/Olympus",
            "europe/amsterdam",
            "+19:00",
            "+18:30",
            "+1:00",
            "UTC+",
            "E",
            "Europe/Amsterdam ",
            "SystemV/EST5",
            "SystemV/PST8PDT",
        ] {
            assert!(!zone_id_valid(bad), "{bad:?}");
        }
    }
}
