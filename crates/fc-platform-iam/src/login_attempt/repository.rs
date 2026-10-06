//! LoginAttempt Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::PrincipalId;
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{AttemptType, LoginAttempt, LoginOutcome};
use fc_platform_core::shared::api_common::DecodedCursor;
use fc_platform_core::shared::enum_str::decode;
use fc_platform_core::shared::error::{PlatformError, Result};

#[derive(sqlx::FromRow)]
struct LoginAttemptRow {
    id: String,
    attempt_type: String,
    outcome: String,
    failure_reason: Option<String>,
    identifier: Option<String>,
    principal_id: Option<PrincipalId>,
    ip_address: Option<String>,
    user_agent: Option<String>,
    attempted_at: DateTime<Utc>,
}

impl TryFrom<LoginAttemptRow> for LoginAttempt {
    type Error = PlatformError;
    fn try_from(r: LoginAttemptRow) -> Result<Self> {
        let attempt_type = decode(&r.attempt_type, "iam_login_attempts", "attempt_type", &r.id)?;
        let outcome = decode(&r.outcome, "iam_login_attempts", "outcome", &r.id)?;
        Ok(Self {
            id: r.id,
            attempt_type,
            outcome,
            failure_reason: r.failure_reason,
            identifier: r.identifier,
            principal_id: r.principal_id,
            ip_address: r.ip_address,
            user_agent: r.user_agent,
            attempted_at: r.attempted_at,
        })
    }
}

/// Filters for listing login attempts; `None` means "don't filter".
/// Dates are RFC 3339 strings.
#[derive(Debug, Default, Clone, Copy)]
pub struct LoginAttemptFilter<'a> {
    pub attempt_type: Option<AttemptType>,
    pub outcome: Option<LoginOutcome>,
    pub identifier: Option<&'a str>,
    pub principal_id: Option<&'a str>,
    pub date_from: Option<&'a str>,
    pub date_to: Option<&'a str>,
}

pub struct LoginAttemptRepository {
    pool: PgPool,
}

impl LoginAttemptRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create(&self, attempt: &LoginAttempt) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO iam_login_attempts
                (id, attempt_type, outcome, failure_reason, identifier,
                 principal_id, ip_address, user_agent, attempted_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        )
        .bind(&attempt.id)
        .bind(attempt.attempt_type.as_str())
        .bind(attempt.outcome.as_str())
        .bind(&attempt.failure_reason)
        .bind(&attempt.identifier)
        .bind(&attempt.principal_id)
        .bind(&attempt.ip_address)
        .bind(&attempt.user_agent)
        .bind(attempt.attempted_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Cursor-paginated listing. Orders by `(attempted_at, id) DESC` so the
    /// keyset comparison is well-defined. Returns `fetch_limit` rows so the
    /// caller can detect `hasMore`.
    pub async fn find_with_cursor(
        &self,
        filter: &LoginAttemptFilter<'_>,
        cursor: Option<&DecodedCursor>,
        fetch_limit: i64,
    ) -> Result<Vec<LoginAttempt>> {
        // Unparseable dates silently drop that condition.
        let date_from_parsed = filter
            .date_from
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let date_to_parsed = filter
            .date_to
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM iam_login_attempts");
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(at) = filter.attempt_type {
            push_where(&mut qb, &mut has_where);
            qb.push("attempt_type = ").push_bind(at.as_str());
        }
        if let Some(o) = filter.outcome {
            push_where(&mut qb, &mut has_where);
            qb.push("outcome = ").push_bind(o.as_str());
        }
        if let Some(ident) = filter.identifier {
            push_where(&mut qb, &mut has_where);
            qb.push("identifier = ").push_bind(ident.to_string());
        }
        if let Some(pid) = filter.principal_id {
            push_where(&mut qb, &mut has_where);
            qb.push("principal_id = ").push_bind(pid.to_string());
        }
        if let Some(dt) = date_from_parsed {
            push_where(&mut qb, &mut has_where);
            qb.push("attempted_at >= ").push_bind(dt);
        }
        if let Some(dt) = date_to_parsed {
            push_where(&mut qb, &mut has_where);
            qb.push("attempted_at <= ").push_bind(dt);
        }
        if let Some(c) = cursor {
            push_where(&mut qb, &mut has_where);
            qb.push("(attempted_at, id) < (")
                .push_bind(c.created_at)
                .push(", ")
                .push_bind(c.id.clone())
                .push(")");
        }
        qb.push(" ORDER BY attempted_at DESC, id DESC LIMIT ")
            .push_bind(fetch_limit);
        let rows: Vec<LoginAttemptRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        rows.into_iter().map(LoginAttempt::try_from).collect()
    }

    /// The `limit` most recent attempts for an identifier, newest first: the
    /// user's own sign-in history (Go `FindRecentByIdentifier`).
    pub async fn find_recent_by_identifier(
        &self,
        identifier: &str,
        limit: i64,
    ) -> Result<Vec<LoginAttempt>> {
        let rows = sqlx::query_as::<_, LoginAttemptRow>(
            "SELECT * FROM iam_login_attempts WHERE identifier = $1 \
             ORDER BY attempted_at DESC, id DESC LIMIT $2",
        )
        .bind(identifier)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(LoginAttempt::try_from).collect()
    }

    /// Last successful login attempt timestamp for an identifier. Used by the
    /// backoff helper to compute "failures since the last good login".
    pub async fn last_success_at(&self, identifier: &str) -> Result<Option<DateTime<Utc>>> {
        // MAX(...) over zero rows returns NULL — query_as decodes the single
        // aggregated row, then we treat the NULL as None.
        let (ts,): (Option<DateTime<Utc>>,) = sqlx::query_as(
            "SELECT MAX(attempted_at) FROM iam_login_attempts
             WHERE identifier = $1 AND outcome = 'SUCCESS'",
        )
        .bind(identifier)
        .fetch_one(&self.pool)
        .await?;
        Ok(ts)
    }

    /// Count failures and most-recent failure timestamp for `(identifier, ip)`
    /// strictly after `since`. Returns `(0, None)` when none exist.
    pub async fn failure_stats_by_identifier_ip_since(
        &self,
        identifier: &str,
        ip: &str,
        since: DateTime<Utc>,
    ) -> Result<(i64, Option<DateTime<Utc>>)> {
        let row: (i64, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT COUNT(*), MAX(attempted_at) FROM iam_login_attempts
             WHERE identifier = $1 AND ip_address = $2 AND outcome = 'FAILURE'
               AND attempted_at > $3",
        )
        .bind(identifier)
        .bind(ip)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Count failures across all IPs for `identifier` strictly after `since`.
    /// Used for the per-email global ceiling.
    pub async fn failure_count_by_identifier_since(
        &self,
        identifier: &str,
        since: DateTime<Utc>,
    ) -> Result<i64> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_login_attempts
             WHERE identifier = $1 AND outcome = 'FAILURE' AND attempted_at > $2",
        )
        .bind(identifier)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok(count.0)
    }
}

// ─── Partition maintenance (Go owner ruling X-03) ───────────────────────────
//
// Go's migration 049 range-partitions `iam_login_attempts` by quarter on
// `attempted_at`, with a DEFAULT partition. From then on Go's housekeeping
// purger (StartPurger, internal/server/subsystems.go) keeps the current and
// next quarter's partitions in place and drops partitions wholly older than
// the retention — never a row DELETE (loginattempt/loginattempt.go:358-509).
// Rust runs on the database Go migrated, so it keeps them the same way. On a
// table that is not partitioned (a Rust-only database) both are no-ops.

/// Every quarterly partition's name starts with this; the DEFAULT partition's
/// doesn't, so neither method touches it.
const PARTITION_PREFIX: &str = "iam_login_attempts_";

/// The first instant of `at`'s calendar quarter (UTC).
#[expect(
    clippy::expect_used,
    reason = "a first-of-quarter date built from a valid month is in range"
)]
fn quarter_start(at: DateTime<Utc>) -> chrono::NaiveDate {
    use chrono::Datelike;
    let month = ((at.month() - 1) / 3) * 3 + 1;
    chrono::NaiveDate::from_ymd_opt(at.year(), month, 1).expect("a quarter's first day")
}

/// `iam_login_attempts_YYYY_qN` for the quarter starting at `start`.
fn quarter_partition_name(start: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    format!(
        "{PARTITION_PREFIX}{:04}_q{}",
        start.year(),
        (start.month() - 1) / 3 + 1
    )
}

/// The exclusive end (next quarter's start) of a `…_YYYY_qN` partition;
/// `None` for any other name, such as the DEFAULT partition.
fn quarter_partition_end(name: &str) -> Option<chrono::NaiveDate> {
    let (year, quarter) = name.strip_prefix(PARTITION_PREFIX)?.split_once("_q")?;
    let year: i32 = year.parse().ok()?;
    let quarter: u32 = quarter.parse().ok()?;
    if !(1..=4).contains(&quarter) {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year, (quarter - 1) * 3 + 1, 1)?
        .checked_add_months(chrono::Months::new(3))
}

/// A double-quoted identifier.
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

impl LoginAttemptRepository {
    /// Whether `iam_login_attempts` is a partitioned table (Go
    /// `isPartitioned`); `false` when it is a plain table or absent.
    async fn is_partitioned(&self) -> Result<bool> {
        let relkind: Option<(String,)> = sqlx::query_as(
            "SELECT relkind::text FROM pg_class WHERE relname = 'iam_login_attempts'",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(relkind.is_some_and(|(k,)| k == "p"))
    }

    /// Create the partition covering `at`'s calendar quarter unless it
    /// exists (Go `EnsureQuarterlyPartition`). A no-op on an unpartitioned
    /// table.
    #[expect(
        clippy::expect_used,
        reason = "adding three months to a valid date stays within chrono's range"
    )]
    pub async fn ensure_quarterly_partition(&self, at: DateTime<Utc>) -> Result<()> {
        if !self.is_partitioned().await? {
            return Ok(());
        }
        let start = quarter_start(at);
        let end = start
            .checked_add_months(chrono::Months::new(3))
            .expect("the next quarter");
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS {} PARTITION OF iam_login_attempts FOR VALUES FROM ('{}') TO ('{}')",
            quote_ident(&quarter_partition_name(start)),
            start.format("%Y-%m-%d"),
            end.format("%Y-%m-%d"),
        );
        sqlx::query(&sql).execute(&self.pool).await?;
        Ok(())
    }

    /// Drop every quarterly partition whose whole range ends on or before
    /// `cutoff` (Go `DropPartitionsOlderThan`), returning their names. The
    /// DEFAULT partition and any other child are left alone. A schema-level
    /// drop, never a row DELETE; a no-op on an unpartitioned table.
    pub async fn drop_partitions_older_than(&self, cutoff: DateTime<Utc>) -> Result<Vec<String>> {
        if !self.is_partitioned().await? {
            return Ok(Vec::new());
        }
        let children: Vec<(String,)> = sqlx::query_as(
            "SELECT child.relname::text FROM pg_inherits i \
             JOIN pg_class parent ON i.inhparent = parent.oid \
             JOIN pg_class child ON i.inhrelid = child.oid \
             WHERE parent.relname = 'iam_login_attempts'",
        )
        .fetch_all(&self.pool)
        .await?;
        let cutoff = cutoff.date_naive();
        let mut dropped = Vec::new();
        for (name,) in children {
            let Some(end) = quarter_partition_end(&name) else {
                continue;
            };
            if end <= cutoff {
                sqlx::query(&format!("DROP TABLE IF EXISTS {}", quote_ident(&name)))
                    .execute(&self.pool)
                    .await?;
                dropped.push(name);
            }
        }
        Ok(dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn quarterly_partition_names_and_ranges_match_go() {
        let at = Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap();
        let start = quarter_start(at);
        assert_eq!(start.to_string(), "2026-07-01");
        assert_eq!(quarter_partition_name(start), "iam_login_attempts_2026_q3");
        assert_eq!(
            quarter_partition_end("iam_login_attempts_2026_q3").map(|d| d.to_string()),
            Some("2026-10-01".to_string())
        );
        assert_eq!(
            quarter_partition_end("iam_login_attempts_2026_q4").map(|d| d.to_string()),
            Some("2027-01-01".to_string())
        );
        for other in [
            "iam_login_attempts_default",
            "iam_login_attempts_2026_q5",
            "iam_login_attempts_x_q1",
            "other_2026_q1",
        ] {
            assert_eq!(quarter_partition_end(other), None, "{other}");
        }
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    }
}
