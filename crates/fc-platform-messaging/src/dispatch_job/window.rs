//! The time window of a dispatch-job list read (Go `dispatchjob/window.go`).
//!
//! `msg_dispatch_jobs_read` is partitioned by `created_at`, so a `created_at`
//! range is what lets a read touch a few partitions instead of all of them.
//! Every list-shaped read (the list, list-raw, filter-options, the
//! requeue-by-filter preview and run) therefore carries a window: the
//! caller's `since`/`until`, or, when the caller names neither, the last 24
//! hours. A window wider than the configured maximum is refused. Lookups keyed
//! by one job or one event (by id, by event, attempts, sign) are exempt: their
//! key already bounds them.

use std::env;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use utoipa::ToSchema;

use fc_platform_core::shared::error::PlatformError;

/// Applied to a read that names no time bound.
pub const DEFAULT_WINDOW_HOURS: i64 = 24;
/// The widest window a read may ask for unless `FC_DISPATCH_JOB_MAX_WINDOW_DAYS`
/// says otherwise.
pub const DEFAULT_MAX_WINDOW_DAYS: i64 = 31;
/// The environment variable that overrides [`DEFAULT_MAX_WINDOW_DAYS`].
pub const MAX_WINDOW_DAYS_ENV: &str = "FC_DISPATCH_JOB_MAX_WINDOW_DAYS";
/// The 400 code for a window beyond the maximum.
pub const WINDOW_TOO_LARGE_CODE: &str = "WINDOW_TOO_LARGE";

/// The configured maximum, in days: [`MAX_WINDOW_DAYS_ENV`], or the default
/// when it is unset, unparseable or not positive (Go `positiveOr`).
pub fn max_window_days_from_env() -> i64 {
    parse_max_window_days(env::var(MAX_WINDOW_DAYS_ENV).ok().as_deref())
}

fn parse_max_window_days(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_WINDOW_DAYS)
}

/// The `created_at` window a read was held to, echoed to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// True when the server supplied a bound the caller did not.
    pub defaulted: bool,
}

/// A request's `since`/`until` after [`enforce_window`]: `since` is always
/// set; `until` stays as the caller gave it (`None`: open at the top, the
/// reported "to" is then now).
#[derive(Debug, Clone, Copy)]
pub struct Enforced {
    pub since: DateTime<Utc>,
    pub until: Option<DateTime<Utc>>,
    pub window: Window,
}

/// Go `MaxWindow`: the widest window for `max_days` (non-positive: the default).
pub fn max_window(max_days: i64) -> Duration {
    Duration::days(if max_days < 1 {
        DEFAULT_MAX_WINDOW_DAYS
    } else {
        max_days
    })
}

/// Go `EnforceWindow`: holds a read to a `created_at` window and reports it.
///
/// - No since, no until: the last 24 hours (`since` = now - 24h; no upper bound
///   in SQL, the reported "to" is now).
/// - since only: since .. now. until only: until-24h .. until (`defaulted`
///   is true: the server supplied the missing bound).
/// - Both: as given; until before since is a 400 `INVALID_WINDOW`.
///
/// A window wider than `max_days` days (non-positive: the default 31) is a
/// 400 `WINDOW_TOO_LARGE`.
pub fn enforce_window(
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    max_days: i64,
) -> Result<Enforced, PlatformError> {
    let default = Duration::hours(DEFAULT_WINDOW_HOURS);
    let (from, to, defaulted) = match (since, until) {
        (None, None) => (now - default, now, true),
        (Some(s), None) => (s, now, false),
        (None, Some(u)) => (u - default, u, true),
        (Some(s), Some(u)) => {
            if u < s {
                return Err(PlatformError::bad_request_code(
                    "INVALID_WINDOW",
                    "until must not be before since",
                ));
            }
            (s, u, false)
        }
    };
    let max = max_window(max_days);
    if to - from > max {
        return Err(PlatformError::bad_request_code(
            WINDOW_TOO_LARGE_CODE,
            format!(
                "the time window is {}; at most {} days may be read at once (narrow since/until)",
                go_duration(to - from),
                max.num_days()
            ),
        ));
    }
    Ok(Enforced {
        since: from,
        until,
        window: Window {
            from,
            to,
            defaulted,
        },
    })
}

/// A duration rounded to the minute, spelled as Go's `Duration.String`
/// (`1488h0m0s`).
fn go_duration(d: Duration) -> String {
    let minutes = (d.num_seconds() + 30).div_euclid(60);
    let (h, m) = (minutes / 60, minutes % 60);
    if h > 0 {
        format!("{h}h{m}m0s")
    } else {
        format!("{m}m0s")
    }
}

/// Parses an RFC 3339 `since`/`until` value. `strict` (the filter-options and
/// the POST bodies) makes an unparseable one a 400 `INVALID_TIMESTAMP`; the
/// list ignores one and shows more rows (Go `toParams(strict)`). Blank is none.
pub fn parse_timestamp(
    name: &str,
    value: Option<&str>,
    strict: bool,
) -> Result<Option<DateTime<Utc>>, PlatformError> {
    match value.filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => match DateTime::parse_from_rfc3339(v) {
            Ok(t) => Ok(Some(t.with_timezone(&Utc))),
            Err(_) if strict => Err(PlatformError::bad_request_code(
                "INVALID_TIMESTAMP",
                format!("{name} must be an RFC3339 timestamp"),
            )),
            Err(_) => Ok(None),
        },
    }
}

/// The window as the bodies of filter-options, the preview and the run carry
/// it: `{from, to, defaulted}`.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowResponse {
    /// Start of the window (inclusive)
    pub from: String,
    /// End of the window; now when the request gave no until
    pub to: String,
    /// true when the server supplied a bound the request did not (no since/until: the last 24 hours)
    pub defaulted: bool,
}

impl From<Window> for WindowResponse {
    fn from(w: Window) -> Self {
        Self {
            from: w.from.to_rfc3339_opts(SecondsFormat::AutoSi, true),
            to: w.to.to_rfc3339_opts(SecondsFormat::AutoSi, true),
            defaulted: w.defaulted,
        }
    }
}

impl Window {
    /// The `X-Window-From` / `-To` / `-Defaulted` headers (names lower-cased, as
    /// they travel; values RFC 3339 UTC to the second).
    pub fn header_values(&self) -> [(&'static str, String); 3] {
        [
            (
                "x-window-from",
                self.from.to_rfc3339_opts(SecondsFormat::Secs, true),
            ),
            (
                "x-window-to",
                self.to.to_rfc3339_opts(SecondsFormat::Secs, true),
            ),
            ("x-window-defaulted", self.defaulted.to_string()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 19, 12, 0, 0).unwrap() + Duration::hours(h)
    }

    fn enforce(
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Result<Enforced, PlatformError> {
        enforce_window(since, until, at(0), 31)
    }

    #[test]
    fn no_bounds_is_the_last_24_hours_and_defaulted() {
        let e = enforce(None, None).unwrap();
        assert_eq!(e.since, at(-24));
        assert_eq!(e.until, None);
        assert_eq!(
            e.window,
            Window {
                from: at(-24),
                to: at(0),
                defaulted: true
            }
        );
    }

    #[test]
    fn since_only_runs_to_now_and_is_not_defaulted() {
        let e = enforce(Some(at(-48)), None).unwrap();
        assert_eq!((e.since, e.until), (at(-48), None));
        assert_eq!(
            e.window,
            Window {
                from: at(-48),
                to: at(0),
                defaulted: false
            }
        );
    }

    #[test]
    fn until_only_looks_24_hours_back_from_it_and_is_defaulted() {
        let e = enforce(None, Some(at(-10))).unwrap();
        assert_eq!((e.since, e.until), (at(-34), Some(at(-10))));
        assert_eq!(
            e.window,
            Window {
                from: at(-34),
                to: at(-10),
                defaulted: true
            }
        );
    }

    #[test]
    fn both_bounds_are_taken_as_given_and_until_before_since_is_invalid() {
        let e = enforce(Some(at(-5)), Some(at(-1))).unwrap();
        assert!(!e.window.defaulted);
        let err = enforce(Some(at(-1)), Some(at(-5))).unwrap_err();
        assert!(
            matches!(&err, PlatformError::Coded { code, .. } if code == "INVALID_WINDOW"),
            "{err:?}"
        );
    }

    #[test]
    fn a_window_wider_than_the_maximum_is_refused_and_exactly_the_maximum_is_not() {
        let day = 24;
        assert!(enforce(Some(at(-31 * day)), None).is_ok());
        let err = enforce(Some(at(-31 * day - 1)), None).unwrap_err();
        let PlatformError::Coded { code, message, .. } = &err else {
            panic!("{err:?}");
        };
        assert_eq!(code, WINDOW_TOO_LARGE_CODE);
        assert!(message.contains("745h0m0s"), "{message}");
        assert!(message.contains("at most 31 days"), "{message}");
        // The configured maximum applies; non-positive is the default.
        assert!(enforce_window(Some(at(-8 * day)), None, at(0), 7).is_err());
        assert!(enforce_window(Some(at(-7 * day)), None, at(0), 7).is_ok());
        assert!(enforce_window(Some(at(-31 * day)), None, at(0), 0).is_ok());
        assert!(enforce_window(Some(at(-32 * day)), None, at(0), -3).is_err());
    }

    #[test]
    fn the_maximum_comes_from_the_environment_or_the_default() {
        assert_eq!(parse_max_window_days(None), 31);
        assert_eq!(parse_max_window_days(Some("7")), 7);
        assert_eq!(parse_max_window_days(Some(" 14 ")), 14);
        for bad in ["0", "-2", "x", ""] {
            assert_eq!(parse_max_window_days(Some(bad)), 31, "{bad}");
        }
    }

    #[test]
    fn timestamps_are_ignored_on_the_list_and_refused_when_strict() {
        assert_eq!(parse_timestamp("since", Some(""), true).unwrap(), None);
        assert_eq!(parse_timestamp("since", None, true).unwrap(), None);
        assert_eq!(parse_timestamp("since", Some("nope"), false).unwrap(), None);
        let err = parse_timestamp("until", Some("nope"), true).unwrap_err();
        assert!(
            matches!(&err, PlatformError::Coded { code, message, .. }
                if code == "INVALID_TIMESTAMP" && message == "until must be an RFC3339 timestamp"),
            "{err:?}"
        );
        assert_eq!(
            parse_timestamp("since", Some("2026-10-19T14:00:00+02:00"), true).unwrap(),
            Some(at(0))
        );
    }

    #[test]
    fn the_headers_and_body_spell_the_window_in_rfc3339_utc() {
        let w = enforce(None, None).unwrap().window;
        let h = w.header_values();
        assert_eq!(h[0], ("x-window-from", "2026-10-18T12:00:00Z".to_string()));
        assert_eq!(h[1], ("x-window-to", "2026-10-19T12:00:00Z".to_string()));
        assert_eq!(h[2], ("x-window-defaulted", "true".to_string()));
        let b = WindowResponse::from(w);
        assert_eq!(b.from, "2026-10-18T12:00:00Z");
        assert!(b.defaulted);
    }
}
