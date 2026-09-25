//! One-off data migration `036_scheduled_job_cron_dialect`: rewrite the crons
//! of scheduled jobs the previous Rust poller evaluated, so they keep firing
//! at the same instants now that the poller reads Java/Go's dialect
//! ([`super::cron`], owner decision 1 of 2026-09-25).
//!
//! **The old dialect.** The `cron` crate: six fields plus an optional year,
//! day of week `1-7` with `1` = Sunday (or names), `@yearly`..`@hourly`, and
//! a day must always match both day fields.
//!
//! **The rewrite.** Each stored cron is read with the `cron` crate itself (so
//! nothing is reinterpreted), and:
//! - left as written when it does not parse there (it never fired), or when
//!   Go's reader already gives it the same meaning (the usual case: day of
//!   week `*`);
//! - otherwise rendered in Go's dialect from the crate's own value sets:
//!   numbers and ranges, days of the week by name (`MON-FRI`), the
//!   unrestricted day field as `*`, no year. The rendering reads the same in
//!   both dialects, so a Rust instance still running the old code during a
//!   rolling deploy fires it the same way too;
//! - **not expressible** in Go's dialect, and left as written with an error
//!   in the log, when both day fields are restricted (the crate needs both,
//!   robfig either: "Friday the 13th" has no robfig form, and no set of
//!   robfig crons unions to it) or the year field is restricted (robfig has
//!   none). A job with such a cron keeps all its crons unchanged.
//!
//! **Only Rust's rows.** Go, Java and the TypeScript platform always wrote
//! crons in the robfig/Vixie dialect, and the rows carry no marker of which
//! platform wrote them. So the rewrite runs only on a database no other
//! platform has migrated: when one of their migration trackers is present
//! (Go's `goose_db_version` / `_fc_migrations`, Java's
//! `flyway_schema_history`, TypeScript's `__drizzle_migrations`), every row
//! is left as written, since whoever evaluated them last read them in Go's
//! dialect, and the jobs the two dialects read differently are listed in the
//! log for an operator to check.
//!
//! **Once.** The migration is recorded in `_schema_migrations` in the same
//! transaction as the rewrite, under an advisory lock, and never runs again:
//! a cron written after it is already Go's dialect and must not be
//! translated a second time. A row is updated only while it still holds the
//! crons that were read.
//!
//! ## Why this writes directly, not through a use case
//!
//! Like `shared/secret_backfill.rs`, this is platform maintenance with no
//! executing principal: it changes how a schedule is spelled, not when it
//! fires, so it emits no domain event or audit row and leaves `updated_at`
//! and `version` alone.

use std::str::FromStr;

use cron::TimeUnitSpec;
use sqlx::PgPool;
use tracing::{error, info, warn};

use super::cron::CronSpec;

/// The `_schema_migrations` id. It follows `035_scheduled_jobs_application_id`
/// in `shared::database::run_migrations`, which runs it after the SQL ones.
pub const MIGRATION_ID: &str = "036_scheduled_job_cron_dialect";

/// Recorded as the migration's checksum.
const CHECKSUM_SOURCE: &str = "036_scheduled_job_cron_dialect: cron crate 0.15 -> robfig v3";

/// Tables another platform's migration runner creates.
const FOREIGN_TRACKERS: [&str; 4] = [
    "goose_db_version",
    "_fc_migrations",
    "flyway_schema_history",
    "__drizzle_migrations",
];

/// Serialises concurrent boots (any constant; "fc036").
const LOCK_KEY: i64 = 0x0066_6330_3336;

/// Days of the week by name, Sunday first: both dialects read them.
const DAY_NAMES: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

/// What the migration does with one cron the old poller read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Translation {
    /// The `cron` crate cannot read it: it never fired. Left as written.
    Unreadable,
    /// Go's reader gives it the same meaning. Left as written.
    Same,
    /// Go's spelling of the same schedule.
    Rewrite(String),
    /// No Go cron fires at the same instants; why.
    Inexpressible(String),
}

/// The old reading of a cron: each field's values, day of week `0-6` from
/// Sunday.
struct OldReading {
    fields: [Vec<u32>; 6],
    dom_all: bool,
    dow_all: bool,
}

const RANGES: [(u32, u32); 6] = [(0, 59), (0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];

fn values(spec: &impl TimeUnitSpec, shift: u32) -> Vec<u32> {
    spec.iter().map(|v| v - shift).collect()
}

fn read_old(expr: &str) -> Result<OldReading, Translation> {
    let old = cron::Schedule::from_str(expr).map_err(|_| Translation::Unreadable)?;
    if !old.years().is_all() {
        return Err(Translation::Inexpressible(
            "its year field is restricted, and Go's dialect has no year".into(),
        ));
    }
    Ok(OldReading {
        fields: [
            values(old.seconds(), 0),
            values(old.minutes(), 0),
            values(old.hours(), 0),
            values(old.days_of_month(), 0),
            values(old.months(), 0),
            values(old.days_of_week(), 1),
        ],
        dom_all: old.days_of_month().is_all(),
        dow_all: old.days_of_week().is_all(),
    })
}

fn bits(values: &[u32]) -> u64 {
    values.iter().fold(0, |b, v| b | 1 << v)
}

/// Whether Go's reading `new` fires exactly when the old reading did.
fn same_meaning(old: &OldReading, new: &CronSpec) -> bool {
    let time = [0, 1, 2, 4].map(|i| bits(&old.fields[i]));
    if new.time_bits() != time {
        return false;
    }
    let (dom, dow) = (bits(&old.fields[3]), bits(&old.fields[5]));
    (1..=31).all(|d| {
        (0..7).all(|w| {
            let was = dom & (1 << d) != 0 && dow & (1 << w) != 0;
            was == new.matches_day(d, w)
        })
    })
}

/// A field as numbers (or day names): `*` when it has every value, runs of
/// three or more as ranges.
fn render_field(i: usize, values: &[u32]) -> String {
    let (min, max) = RANGES[i];
    if values.len() == (max - min + 1) as usize {
        return "*".into();
    }
    let name = |v: u32| {
        if i == 5 {
            DAY_NAMES[v as usize].to_string()
        } else {
            v.to_string()
        }
    };
    let mut parts = Vec::new();
    let mut k = 0;
    while k < values.len() {
        let mut j = k;
        while j + 1 < values.len() && values[j + 1] == values[j] + 1 {
            j += 1;
        }
        if j - k >= 2 {
            parts.push(format!("{}-{}", name(values[k]), name(values[j])));
        } else {
            parts.extend(values[k..=j].iter().map(|v| name(*v)));
        }
        k = j + 1;
    }
    parts.join(",")
}

/// What the migration does with `expr`, a cron the old poller read.
pub fn translate(expr: &str) -> Translation {
    let old = match read_old(expr) {
        Ok(old) => old,
        Err(t) => return t,
    };
    if let Ok(new) = expr.parse::<CronSpec>() {
        if new.zone().is_none() && same_meaning(&old, &new) {
            return Translation::Same;
        }
    }
    if !old.dom_all && !old.dow_all {
        return Translation::Inexpressible(
            "both day fields are restricted: it fired only when both matched, and Go's \
             dialect fires when either does"
                .into(),
        );
    }
    let mut fields: [String; 6] = std::array::from_fn(|i| render_field(i, &old.fields[i]));
    if old.dom_all {
        fields[3] = "*".into();
    }
    if old.dow_all {
        fields[5] = "*".into();
    }
    let rendered = fields.join(" ");
    match rendered.parse::<CronSpec>() {
        Ok(new) if same_meaning(&old, &new) => Translation::Rewrite(rendered),
        _ => Translation::Inexpressible(format!("no equivalent found (tried '{rendered}')")),
    }
}

/// A job's crons after the migration, or why they stay as they are.
fn plan_job(crons: &[String]) -> Result<Vec<String>, Vec<String>> {
    let mut out = Vec::with_capacity(crons.len());
    let mut problems = Vec::new();
    for c in crons {
        match translate(c) {
            Translation::Unreadable | Translation::Same => out.push(c.clone()),
            Translation::Rewrite(new) => out.push(new),
            Translation::Inexpressible(why) => problems.push(format!("'{c}': {why}")),
        }
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// What the migration found and did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CronMigrationReport {
    /// It had already run: nothing was read.
    pub already_applied: bool,
    /// Another platform's migration tracker found; nothing was rewritten.
    pub foreign_tracker: Option<String>,
    /// Jobs rewritten: id, crons before, crons after.
    pub rewritten: Vec<(String, Vec<String>, Vec<String>)>,
    /// Jobs that would need a rewrite, left as written: id, why. With a
    /// foreign tracker, every job the two dialects read differently.
    pub left_as_written: Vec<(String, String)>,
}

/// Run the migration once (see the module docs). Called by
/// `shared::database::run_migrations` after the SQL migrations.
pub async fn run(pool: &PgPool) -> Result<CronMigrationReport, sqlx::Error> {
    let started = std::time::Instant::now();
    let mut report = CronMigrationReport::default();
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(LOCK_KEY)
        .execute(&mut *tx)
        .await?;
    let (applied,): (bool,) =
        sqlx::query_as("SELECT EXISTS (SELECT 1 FROM _schema_migrations WHERE migration_id = $1)")
            .bind(MIGRATION_ID)
            .fetch_one(&mut *tx)
            .await?;
    if applied {
        report.already_applied = true;
        return Ok(report);
    }

    report.foreign_tracker = sqlx::query_as::<_, (String,)>(
        "SELECT relname::text FROM pg_catalog.pg_class \
         WHERE relkind IN ('r', 'p') AND relname = ANY($1) ORDER BY relname LIMIT 1",
    )
    .bind(&FOREIGN_TRACKERS[..])
    .fetch_optional(&mut *tx)
    .await?
    .map(|(t,)| t);

    let jobs: Vec<(String, String, Vec<String>)> =
        sqlx::query_as("SELECT id, code, crons FROM msg_scheduled_jobs ORDER BY id")
            .fetch_all(&mut *tx)
            .await?;
    let (mut ids, mut before, mut after) = (Vec::new(), Vec::new(), Vec::new());
    for (id, code, crons) in jobs {
        let plan = plan_job(&crons);
        if plan.as_ref().is_ok_and(|new| *new == crons) {
            continue;
        }
        if let Some(tracker) = &report.foreign_tracker {
            warn!(
                job_id = %id, code = %code, crons = ?crons, tracker = %tracker,
                "Scheduled job cron reads differently in the old Rust dialect; left as \
                 written because another platform has migrated this database"
            );
            report.left_as_written.push((
                id,
                format!("another platform's tracker ({tracker}) is present"),
            ));
            continue;
        }
        match plan {
            Ok(new) => {
                ids.push(id.clone());
                before.push(serde_json::to_string(&crons).unwrap_or_default());
                after.push(serde_json::to_string(&new).unwrap_or_default());
                report.rewritten.push((id, crons, new));
            }
            Err(problems) => {
                let why = problems.join("; ");
                error!(
                    job_id = %id, code = %code, crons = ?crons, why = %why,
                    "Scheduled job cron has no equivalent in Go's dialect; left as written, \
                     it now fires when either day field matches: fix or pause it"
                );
                report.left_as_written.push((id, why));
            }
        }
    }

    if !ids.is_empty() {
        sqlx::query(
            "UPDATE msg_scheduled_jobs AS t \
             SET crons = ARRAY(SELECT jsonb_array_elements_text(v.new_crons::jsonb)) \
             FROM UNNEST($1::text[], $2::text[], $3::text[]) AS v(id, old_crons, new_crons) \
             WHERE t.id = v.id AND to_jsonb(t.crons) = v.old_crons::jsonb",
        )
        .bind(&ids)
        .bind(&before)
        .bind(&after)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO _schema_migrations (migration_id, duration_ms, checksum) \
         VALUES ($1, $2, $3)",
    )
    .bind(MIGRATION_ID)
    .bind(started.elapsed().as_millis() as i32)
    .bind(crate::shared::database::sha256_hex(CHECKSUM_SOURCE))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    info!(
        migration = MIGRATION_ID,
        rewritten = report.rewritten.len(),
        left_as_written = report.left_as_written.len(),
        foreign_tracker = ?report.foreign_tracker,
        "Migration applied"
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduled_job::cron::JobSchedule;
    use chrono::{DateTime, TimeZone, Utc};

    fn rewrite(expr: &str) -> String {
        match translate(expr) {
            Translation::Rewrite(new) => new,
            other => panic!("{expr}: {other:?}"),
        }
    }

    #[test]
    fn days_of_the_week_shift_to_sunday_zero_by_name() {
        assert_eq!(rewrite("0 0 9 * * 2-6"), "0 0 9 * * MON-FRI");
        assert_eq!(rewrite("0 0 9 * * 1"), "0 0 9 * * SUN");
        assert_eq!(rewrite("0 0 9 * * 7"), "0 0 9 * * SAT");
        assert_eq!(rewrite("0 0 9 * * 1,7"), "0 0 9 * * SUN,SAT");
        assert_eq!(rewrite("0 0 9 ? * 2,4,6"), "0 0 9 * * MON,WED,FRI");
        assert_eq!(rewrite("0 0 9 * * 1-7"), "0 0 9 * * *");
    }

    #[test]
    fn what_go_reads_the_same_is_left_as_written() {
        for same in [
            "0 0 * * * *",
            "*/15 * * * * *",
            "0 */5 9-17 * * *",
            "0 0 0 1,15 * ?",
            "0 0 12 ? * *",
            "0 0 9 * * MON-FRI",
            "0 0 9 * * Sun",
            "0 0 0 13 * *",
            "0 0 0 1 JAN,jul *",
        ] {
            assert_eq!(translate(same), Translation::Same, "{same}");
        }
    }

    #[test]
    fn what_go_cannot_read_is_rendered() {
        assert_eq!(rewrite("0 0 9 * * * *"), "0 0 9 * * *");
        assert_eq!(rewrite("0 0 9 * * * 1970-2100"), "0 0 9 * * *");
        assert_eq!(rewrite("@daily"), "0 0 0 * * *");
        assert_eq!(rewrite("@weekly"), "0 0 0 * * SUN");
        assert_eq!(rewrite("@hourly"), "0 0 * * * *");
        assert_eq!(rewrite("0 0 9 * * Thurs"), "0 0 9 * * THU");
        assert_eq!(rewrite("0 0 9 * * Mon-Thurs/2"), "0 0 9 * * MON,WED");
        assert_eq!(
            rewrite("0 0 0 1 February-November/3 *"),
            "0 0 0 1 2,5,8,11 *"
        );
    }

    #[test]
    fn two_restricted_day_fields_or_a_year_cannot_be_expressed() {
        assert!(matches!(
            translate("0 0 0 13 * 6"),
            Translation::Inexpressible(_)
        ));
        assert!(matches!(
            translate("0 0 9 1-7 * 2"),
            Translation::Inexpressible(_)
        ));
        assert!(matches!(
            translate("0 0 9 * * * 2027"),
            Translation::Inexpressible(_)
        ));
    }

    #[test]
    fn what_the_crate_never_read_is_left() {
        for never in ["0 * * * *", "0 0 9 * * 0", "not a cron", "0 0 9 * * 1-5 x"] {
            assert_eq!(translate(never), Translation::Unreadable, "{never}");
        }
    }

    #[test]
    fn a_job_with_an_inexpressible_cron_keeps_all_its_crons() {
        let crons = vec!["0 0 9 * * 2-6".to_string(), "0 0 0 13 * 6".to_string()];
        assert!(plan_job(&crons).is_err());
        let crons = vec!["0 0 9 * * 2-6".to_string(), "0 0 12 * * *".to_string()];
        assert_eq!(
            plan_job(&crons).unwrap(),
            vec!["0 0 9 * * MON-FRI", "0 0 12 * * *"]
        );
    }

    /// The rewritten crons fire, on the new poller, at the instants the old
    /// poller fired the originals: walked side by side for a year of fires
    /// or 400 steps, in zones away from their daylight-saving changes.
    #[test]
    fn rewritten_crons_fire_when_the_old_poller_fired_them() {
        let crons = [
            "0 0 9 * * 2-6",
            "0 30 8 * * 1",
            "0 0 18 * * 7",
            "0 0 9 * * 1,4,7",
            "0 15 12 ? * 3",
            "0 0 9 * * * *",
            "@daily",
            "@weekly",
            "@monthly",
            "0 0 9 * * Thurs",
            "0 0 9 * * Mon-Thurs/2",
            "0 0 0 1 February-November/3 *",
            "30 */20 6-8 * * 2-6",
            "0 0 9 * * 1-5",
            "0 0 0 13 * *",
            "0 0 9 * * MON-FRI",
        ];
        let zones = ["UTC", "Asia/Kolkata", "Asia/Tokyo"];
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        for expr in crons {
            let stored = match translate(expr) {
                Translation::Rewrite(new) => new,
                Translation::Same => expr.to_string(),
                other => panic!("{expr}: {other:?}"),
            };
            let old = cron::Schedule::from_str(expr).unwrap();
            for zone in zones {
                let tz: chrono_tz::Tz = zone.parse().unwrap();
                let new = JobSchedule::new(std::slice::from_ref(&stored), zone);
                let before: Vec<DateTime<Utc>> = old
                    .after(&start.with_timezone(&tz))
                    .take(400)
                    .map(|t| t.with_timezone(&Utc))
                    .collect();
                let mut t = start;
                let mut after = Vec::new();
                for _ in 0..before.len() {
                    let next = new.next_after(t).expect("fires");
                    after.push(next);
                    t = next;
                }
                assert_eq!(after, before, "{expr} (stored '{stored}') in {zone}");
            }
        }
    }

    #[test]
    fn fields_render_as_ranges_and_lists() {
        assert_eq!(render_field(1, &[0, 1, 2, 3, 10, 20, 21]), "0-3,10,20,21");
        assert_eq!(render_field(5, &[1, 2, 3, 4, 5]), "MON-FRI");
        assert_eq!(render_field(5, &[0, 6]), "SUN,SAT");
        assert_eq!(render_field(4, &(1..=12).collect::<Vec<_>>()), "*");
    }
}
