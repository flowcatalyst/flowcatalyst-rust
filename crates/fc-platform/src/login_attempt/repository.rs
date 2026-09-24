//! LoginAttempt Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{AttemptType, LoginAttempt, LoginOutcome};
use crate::shared::error::Result;

#[derive(sqlx::FromRow)]
struct LoginAttemptRow {
    id: String,
    attempt_type: String,
    outcome: String,
    failure_reason: Option<String>,
    identifier: Option<String>,
    principal_id: Option<String>,
    ip_address: Option<String>,
    user_agent: Option<String>,
    attempted_at: DateTime<Utc>,
}

impl From<LoginAttemptRow> for LoginAttempt {
    fn from(r: LoginAttemptRow) -> Self {
        Self {
            id: r.id,
            attempt_type: AttemptType::from_str(&r.attempt_type),
            outcome: LoginOutcome::from_str(&r.outcome),
            failure_reason: r.failure_reason,
            identifier: r.identifier,
            principal_id: r.principal_id,
            ip_address: r.ip_address,
            user_agent: r.user_agent,
            attempted_at: r.attempted_at,
        }
    }
}

/// Filters for listing login attempts; `None` means "don't filter".
/// Dates are RFC 3339 strings.
#[derive(Debug, Default, Clone, Copy)]
pub struct LoginAttemptFilter<'a> {
    pub attempt_type: Option<&'a str>,
    pub outcome: Option<&'a str>,
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
        cursor: Option<&crate::shared::api_common::DecodedCursor>,
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
            qb.push("attempt_type = ").push_bind(at.to_string());
        }
        if let Some(o) = filter.outcome {
            push_where(&mut qb, &mut has_where);
            qb.push("outcome = ").push_bind(o.to_string());
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
        Ok(rows.into_iter().map(LoginAttempt::from).collect())
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
