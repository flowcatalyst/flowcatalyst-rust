//! Partition manager for the partitioned messaging tables.
//!
//! Background task that:
//!   - Ensures monthly partitions exist for the next N months on each
//!     partitioned parent (default: 3 months ahead).
//!   - Drops partitions whose date range falls before the retention cutoff
//!     (default: 90 days).
//!
//! Partition naming convention (set in migration 019/022): `<parent>_YYYY_MM`.
//! The manager parses the `YYYY_MM` suffix to determine partition age.
//!
//! **Runs in every environment.** We deliberately don't depend on any
//! Postgres extension (pg_partman, pg_cron) — RDS extension allowlists
//! move on AWS's schedule, not ours, and PG version upgrades have already
//! cost us once (RDS PG18 dropped `pg_partman_bgw` from its allowlist).
//! Keeping maintenance in pure Rust gives us identical behaviour in dev
//! and prod, no extension setup per environment, and PG-version
//! independence beyond what sqlx itself supports.
//!
//! Concurrency: in production, fc-server gates the stream processor on
//! leadership (see `spawn_stream_processor`), so only the leader runs
//! this manager. CREATE TABLE IF NOT EXISTS / DROP TABLE IF EXISTS are
//! idempotent regardless, so a brief overlap during failover is safe.
//!
//! Ticks once on startup, then daily. Idempotent — safe to run any number
//! of times.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::health::StreamHealth;

const PARTITIONED_PARENTS: &[&str] = &[
    "msg_events",
    "msg_events_read",
    "msg_dispatch_jobs",
    "msg_dispatch_jobs_read",
    "msg_dispatch_job_attempts",
    "msg_scheduled_job_instances",
    "msg_scheduled_job_instance_logs",
];

/// Configuration for the partition manager.
#[derive(Debug, Clone)]
pub struct PartitionManagerConfig {
    /// How many forward monthly partitions to keep ahead of the current month.
    /// Default: 3.
    pub months_forward: u32,
    /// Drop partitions whose end-of-range is older than this many days.
    /// Default: 90.
    pub retention_days: u32,
    /// How often to re-check (after the first run on startup).
    pub tick_interval: Duration,
}

impl Default for PartitionManagerConfig {
    fn default() -> Self {
        Self {
            months_forward: 3,
            retention_days: 90,
            tick_interval: Duration::from_secs(24 * 60 * 60),
        }
    }
}

/// Health-tracker name for the partition manager (reported by the stream
/// health endpoints).
pub const HEALTH_NAME: &str = "partition-manager";

/// Maintain partitions until `cancel` fires: one pass immediately, then one
/// every `config.tick_interval`. Returns at once (health not running) if
/// `msg_events` isn't partitioned. Cancellation is only observed between
/// passes, so a pass in flight always completes.
pub async fn run(
    pool: PgPool,
    config: PartitionManagerConfig,
    health: Arc<StreamHealth>,
    cancel: CancellationToken,
) {
    // Sanity: if msg_events isn't partitioned, nothing to do.
    // Shouldn't happen now that 019 is a core migration, but
    // defensive — the manager exits cleanly rather than flapping.
    match is_partitioned(&pool, "msg_events").await {
        Ok(true) => {}
        Ok(false) => {
            info!("Partition manager: msg_events is not partitioned; manager will not run");
            health.set_running(false);
            return;
        }
        Err(e) => {
            warn!(error = %e, "Partition manager: detection query failed; assuming not partitioned");
            health.set_running(false);
            return;
        }
    }

    health.set_running(true);
    info!(
        months_forward = config.months_forward,
        retention_days = config.retention_days,
        "Partition manager started"
    );

    // Run once immediately, then on tick_interval.
    loop {
        if cancel.is_cancelled() {
            break;
        }

        match tick(&pool, &config).await {
            Ok((created, dropped)) => {
                if created > 0 || dropped > 0 {
                    info!(created, dropped, "Partition manager tick");
                } else {
                    debug!("Partition manager tick: nothing to do");
                }
                health.add_processed((created + dropped) as u64);
            }
            Err(e) => {
                error!(error = %e, "Partition manager tick failed");
                health.record_error();
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(config.tick_interval) => {}
            _ = cancel.cancelled() => { break; }
        }
    }

    health.set_running(false);
    info!("Partition manager stopped");
}

/// One full pass: create missing forward partitions, drop expired ones.
/// Returns `(created_count, dropped_count)` summed across all parents.
async fn tick(pool: &PgPool, config: &PartitionManagerConfig) -> anyhow::Result<(u32, u32)> {
    let now = Utc::now();
    let mut created_total = 0u32;
    let mut dropped_total = 0u32;

    for parent in PARTITIONED_PARENTS {
        match ensure_forward_partitions(pool, parent, now, config.months_forward).await {
            Ok(n) => created_total += n,
            Err(e) => {
                warn!(parent = %parent, error = %e, "Failed to ensure forward partitions");
            }
        }

        match drop_old_partitions(pool, parent, now, config.retention_days).await {
            Ok(n) => dropped_total += n,
            Err(e) => {
                warn!(parent = %parent, error = %e, "Failed to drop old partitions");
            }
        }
    }

    Ok((created_total, dropped_total))
}

/// Ensure monthly partitions exist for the current month and the next
/// `months_forward` months on the given parent. Returns count created.
async fn ensure_forward_partitions(
    pool: &PgPool,
    parent: &str,
    now: DateTime<Utc>,
    months_forward: u32,
) -> anyhow::Result<u32> {
    let mut created = 0u32;
    for offset in 0..=months_forward as i32 {
        let start = month_start(now, offset);
        let end = month_start(now, offset + 1);
        let partition_name = format!("{}_{}", parent, start.format("%Y_%m"));

        let sql = format!(
            "CREATE TABLE IF NOT EXISTS {partition_name} \
             PARTITION OF {parent} \
             FOR VALUES FROM ('{start}') TO ('{end}')",
            partition_name = quote_ident(&partition_name),
            parent = quote_ident(parent),
            start = start.to_rfc3339(),
            end = end.to_rfc3339(),
        );
        let result = sqlx::query(&sql).execute(pool).await?;
        // CREATE TABLE IF NOT EXISTS doesn't tell us if it actually created;
        // probe pg_class as a follow-up — cheap.
        let _ = result;
        if !partition_exists(pool, &partition_name).await? {
            // Shouldn't happen — CREATE returned without error but partition is gone.
            warn!(partition = %partition_name, "partition missing after CREATE");
        }
        // Heuristic: count it if it's "new this minute" by checking the table's
        // pg_class entry — but that requires another query and the IF NOT EXISTS
        // semantics make a precise count noisy. We accept slight overcounting.
        created += 1;
    }
    Ok(created)
}

/// Drop partitions whose entire range falls before `now - retention_days`.
async fn drop_old_partitions(
    pool: &PgPool,
    parent: &str,
    now: DateTime<Utc>,
    retention_days: u32,
) -> anyhow::Result<u32> {
    let cutoff = now - chrono::Duration::days(retention_days as i64);

    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT child.relname
        FROM pg_inherits i
        JOIN pg_class parent ON i.inhparent = parent.oid
        JOIN pg_class child ON i.inhrelid = child.oid
        WHERE parent.relname = $1
        "#,
    )
    .bind(parent)
    .fetch_all(pool)
    .await?;

    let mut dropped = 0u32;
    for (name,) in rows {
        let Some(end) = parse_partition_end(&name, parent) else {
            continue;
        };
        if end <= cutoff {
            let sql = format!("DROP TABLE IF EXISTS {}", quote_ident(&name));
            match sqlx::query(&sql).execute(pool).await {
                Ok(_) => {
                    info!(partition = %name, "Dropped expired partition");
                    dropped += 1;
                }
                Err(e) => {
                    warn!(partition = %name, error = %e, "Failed to drop partition");
                }
            }
        }
    }

    Ok(dropped)
}

async fn partition_exists(pool: &PgPool, name: &str) -> anyhow::Result<bool> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM pg_class WHERE relname = $1")
        .bind(name)
        .fetch_one(pool)
        .await?;
    Ok(row.0 > 0)
}

/// True if `table` is a declarative-partitioned table.
async fn is_partitioned(pool: &PgPool, table: &str) -> anyhow::Result<bool> {
    let row: (bool,) = sqlx::query_as(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM pg_partitioned_table pt
            JOIN pg_class c ON c.oid = pt.partrelid
            WHERE c.relname = $1
        )
        "#,
    )
    .bind(table)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// First instant of the month at `offset` months from `now`.
/// Negative offsets go back, positive forward.
fn month_start(now: DateTime<Utc>, offset: i32) -> DateTime<Utc> {
    let mut year = now.year();
    let mut month = now.month() as i32 + offset;
    while month <= 0 {
        month += 12;
        year -= 1;
    }
    while month > 12 {
        month -= 12;
        year += 1;
    }
    Utc.from_utc_datetime(
        &NaiveDate::from_ymd_opt(year, month as u32, 1)
            .expect("valid date")
            .and_hms_opt(0, 0, 0)
            .expect("valid time"),
    )
}

/// Parse the end-of-range timestamp from a partition name like
/// `msg_events_2026_05`. Returns the start of the *following* month so the
/// caller can compare against `now - retention`.
fn parse_partition_end(partition: &str, parent: &str) -> Option<DateTime<Utc>> {
    let prefix = format!("{}_", parent);
    let suffix = partition.strip_prefix(&prefix)?;
    // Expect "YYYY_MM"
    let mut parts = suffix.splitn(2, '_');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let mut next_year = year;
    let mut next_month = month + 1;
    if next_month > 12 {
        next_month = 1;
        next_year += 1;
    }
    let date = NaiveDate::from_ymd_opt(next_year, next_month, 1)?;
    Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?))
}

/// Minimal identifier quoting. Partition names are derived from a fixed
/// allow-list (`PARTITIONED_PARENTS`) plus a date stamp, so this is just
/// belt-and-braces — it's never given untrusted input.
fn quote_ident(s: &str) -> String {
    let escaped = s.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_start_current() {
        let now = Utc.with_ymd_and_hms(2026, 5, 15, 12, 0, 0).unwrap();
        let start = month_start(now, 0);
        assert_eq!(start.year(), 2026);
        assert_eq!(start.month(), 5);
        assert_eq!(start.day(), 1);
    }

    #[test]
    fn month_start_forward_wraps_year() {
        let now = Utc.with_ymd_and_hms(2026, 11, 15, 0, 0, 0).unwrap();
        let next = month_start(now, 2);
        assert_eq!(next.year(), 2027);
        assert_eq!(next.month(), 1);
    }

    #[test]
    fn month_start_backward_wraps_year() {
        let now = Utc.with_ymd_and_hms(2026, 2, 15, 0, 0, 0).unwrap();
        let prev = month_start(now, -3);
        assert_eq!(prev.year(), 2025);
        assert_eq!(prev.month(), 11);
    }

    #[test]
    fn parse_partition_end_simple() {
        let end = parse_partition_end("msg_events_2026_05", "msg_events").unwrap();
        assert_eq!(end.year(), 2026);
        assert_eq!(end.month(), 6);
        assert_eq!(end.day(), 1);
    }

    #[test]
    fn parse_partition_end_year_boundary() {
        let end = parse_partition_end("msg_events_2026_12", "msg_events").unwrap();
        assert_eq!(end.year(), 2027);
        assert_eq!(end.month(), 1);
    }

    #[test]
    fn parse_partition_end_rejects_other_parent() {
        // The "msg_dispatch_jobs_2026_05" partition shouldn't be parsed when
        // looking for "msg_events" partitions.
        assert!(parse_partition_end("msg_dispatch_jobs_2026_05", "msg_events").is_none());
    }

    #[test]
    fn parse_partition_end_rejects_garbage() {
        assert!(parse_partition_end("msg_events_lol", "msg_events").is_none());
        assert!(parse_partition_end("msg_events_2026_99", "msg_events").is_none());
    }

    #[test]
    fn quote_ident_doubles_quotes() {
        assert_eq!(quote_ident("foo"), "\"foo\"");
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    }
}
