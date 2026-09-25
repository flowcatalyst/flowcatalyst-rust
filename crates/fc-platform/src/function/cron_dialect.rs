//! A manifest schedule's cron, as the Rust scheduler must store it to fire
//! at the instants Java's scheduler fires it.
//!
//! **What Java accepts and runs.** Java has one cron reader,
//! `scheduledjob/cron/CronExpression` (a port of robfig/cron): exactly six
//! fields `second minute hour day-of-month month day-of-week`, day of week
//! `0-6` with `0` = Sunday (or `sun`..`sat`), and robfig's day rule: when
//! either day field is a bare `*`/`?`, a day must match both; when both are
//! restricted, **either** one. Its publish check and its scheduler use that
//! same reader, so a five-field cron (the manifest schema's own example) is
//! refused at publish with `CRON_INVALID`. The code wins: six fields.
//!
//! **What the Rust scheduler runs.** The poller evaluates stored crons with
//! the `cron` crate, which reads the same six fields but numbers the days of
//! the week `1-7` from Sunday and always requires both day fields to match.
//! So `0 0 9 * * 1-5` is Monday to Friday to Java and Sunday to Thursday to
//! the crate, `... * * 0` is Sunday to Java and an error to the crate, and
//! `0 0 0 13 * 5` is "the 13th or any Friday" to Java and "Friday the 13th"
//! to the crate.
//!
//! **The adaptation, at the wiring boundary only.** Promote stores what
//! [`scheduler_crons`] returns: the manifest's text unchanged when both
//! dialects read it the same way (day of week `*`/`?` and plain numeric
//! fields: the usual case), otherwise an equivalent rendering from Java's
//! own bit sets that both dialects read the same way: explicit numeric lists,
//! days of the week by name, and robfig's either-day rule as two crons (the
//! day-of-month one and the day-of-week one), which the poller unions. The
//! scheduled-job domain itself is untouched: jobs made through its own API
//! keep the crate's reading.

use super::schedule_check::{parse_java_cron, JavaCron, STAR_BIT};

/// Field indexes.
const DOM: usize = 3;
const DOW: usize = 5;

/// `(min, max)` per field.
const RANGES: [(u32, u32); 6] = [(0, 59), (0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];

/// Day-of-week names, Sunday first: the crate and Java both read them.
const DAY_NAMES: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

/// The crons a scheduled job stores for the Java cron `text`, or Java's
/// parse error. One entry, or two for robfig's either-day rule.
pub fn scheduler_crons(text: &str) -> Result<Vec<String>, (&'static str, String)> {
    let cron = parse_java_cron(text)?;
    if reads_the_same(&cron) {
        return Ok(vec![cron.fields.join(" ")]);
    }
    Ok(render(&cron))
}

/// Day of week a bare wildcard, and every other field plain numbers,
/// ranges and steps on a wildcard or a range: both dialects then select the
/// same values and, with a wildcard day of week, the same days.
fn reads_the_same(cron: &JavaCron) -> bool {
    let dow = cron.fields[DOW].as_str();
    if dow != "*" && dow != "?" {
        return false;
    }
    cron.fields[..DOW]
        .iter()
        .enumerate()
        .all(|(i, f)| (i == DOM && f == "?") || plain(f))
}

/// `*`, `N` or `N-M`, each optionally `/S` (but never `N/S`, which Java
/// reads as `N-max/S`), comma-separated; digits only, no signs or names.
fn plain(field: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    field.split(',').all(|item| {
        let (range, step) = match item.split_once('/') {
            Some((r, s)) => (r, Some(s)),
            None => (item, None),
        };
        if step.is_some_and(|s| !digits(s)) {
            return false;
        }
        match range.split_once('-') {
            Some((lo, hi)) => digits(lo) && digits(hi),
            None => range == "*" || (digits(range) && step.is_none()),
        }
    })
}

fn render(cron: &JavaCron) -> Vec<String> {
    let field = |i: usize| -> String {
        let (min, max) = RANGES[i];
        let bits = cron.bits[i] & !STAR_BIT;
        let values: Vec<u32> = (min..=max).filter(|v| bits & (1 << v) != 0).collect();
        if values.len() == (max - min + 1) as usize {
            return "*".to_string();
        }
        if i == DOW {
            return values
                .iter()
                .map(|v| DAY_NAMES[*v as usize])
                .collect::<Vec<_>>()
                .join(",");
        }
        values
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let [second, minute, hour, dom, month, dow] = std::array::from_fn(field);
    let line = |dom: &str, dow: &str| format!("{second} {minute} {hour} {dom} {month} {dow}");
    let either_day = cron.bits[DOM] & STAR_BIT == 0 && cron.bits[DOW] & STAR_BIT == 0;
    if !either_day || dom == "*" || dow == "*" {
        // Both day fields must match (or one of them matches every day,
        // which makes the either-day rule every day too).
        if either_day {
            return vec![line("*", "*")];
        }
        return vec![line(&dom, &dow)];
    }
    vec![line(&dom, "*"), line("*", &dow)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crons(text: &str) -> Vec<String> {
        scheduler_crons(text).unwrap()
    }

    #[test]
    fn plain_crons_are_stored_as_written() {
        for text in [
            "0 0 * * * *",
            "*/15 * * * * *",
            "0 */5 9-17 * * *",
            "0 0 0 1,15 * ?",
            "0 0 12 ? * *",
            "0 0 0 1-7/2 */3 *",
        ] {
            assert_eq!(crons(text), vec![text.to_string()], "{text}");
        }
        assert_eq!(crons("  0 0 * * * *  "), vec!["0 0 * * * *"]);
    }

    #[test]
    fn days_of_the_week_are_named() {
        assert_eq!(
            crons("0 0 9 * * 1-5"),
            vec!["0 0 9 * * MON,TUE,WED,THU,FRI"]
        );
        assert_eq!(
            crons("0 0 9 * * mon-fri"),
            vec!["0 0 9 * * MON,TUE,WED,THU,FRI"]
        );
        assert_eq!(crons("0 0 12 ? * 0"), vec!["0 0 12 * * SUN"]);
        assert_eq!(crons("0 0 12 ? * SUN,sat"), vec!["0 0 12 * * SUN,SAT"]);
        assert_eq!(crons("0 0 1 * * 0-6"), vec!["0 0 1 * * *"]);
    }

    #[test]
    fn either_day_matches_become_two_crons() {
        assert_eq!(crons("0 0 0 13 * 5"), vec!["0 0 0 13 * *", "0 0 0 * * FRI"]);
        assert_eq!(
            crons("0 30 8 1-7 * mon"),
            vec!["0 30 8 1,2,3,4,5,6,7 * *", "0 30 8 * * MON"]
        );
        // Restricted but whole: the either-day rule is every day.
        assert_eq!(crons("0 0 12 1-31 * mon"), vec!["0 0 12 * * *"]);
        // A stepped wildcard is a restriction in Java, not "any day".
        assert_eq!(
            crons("0 0 0 */2 * 1"),
            vec![
                "0 0 0 1,3,5,7,9,11,13,15,17,19,21,23,25,27,29,31 * *",
                "0 0 0 * * MON"
            ]
        );
    }

    #[test]
    fn what_the_crate_reads_differently_is_rendered_from_javas_bits() {
        // N/S is N-max/S in Java.
        assert_eq!(crons("5/20 0 0 * * *"), vec!["5,25,45 0 0 * * *"]);
        // Signs and month names.
        assert_eq!(crons("+5 0 0 * * *"), vec!["5 0 0 * * *"]);
        assert_eq!(crons("0 0 0 1 JAN,jul *"), vec!["0 0 0 1 1,7 *"]);
    }

    #[test]
    fn javas_errors_pass_through() {
        assert_eq!(
            scheduler_crons("0 * * * *").unwrap_err().0,
            "CRON_INVALID_SHAPE"
        );
        assert_eq!(
            scheduler_crons("0 0 0 * * 7").unwrap_err().0,
            "INVALID_CRON"
        );
    }

    /// Every rendering is also valid Java, and parses in the scheduler.
    #[test]
    fn every_rendering_parses_in_both_dialects() {
        use std::str::FromStr;
        for text in [
            "0 0 9 * * 1-5",
            "0 0 0 13 * 5",
            "5/20 0 0 * * *",
            "0 0 0 1 JAN,jul *",
            "0 0 12 ? * 0",
            "0 0 0 */2 * 1",
        ] {
            for c in crons(text) {
                parse_java_cron(&c).unwrap_or_else(|e| panic!("{c}: {e:?}"));
                cron::Schedule::from_str(&c).unwrap_or_else(|e| panic!("{c}: {e}"));
            }
        }
    }
}
