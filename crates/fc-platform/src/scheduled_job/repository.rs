//! ScheduledJob repository — PostgreSQL via SQLx.
//!
//! Owns the **only** write path for the ScheduledJob aggregate:
//! `impl Persist<ScheduledJob> for ScheduledJobRepository` is used by use cases
//! through the UnitOfWork. Direct write methods on this repository (e.g.
//! `mark_fired`) are reserved for the platform-infrastructure poller path —
//! analogous to dispatch-job delivery lifecycle. They do not emit domain events
//! by design (would be recursive — every cron tick would emit an event).
//!
//! Instance + log writes live in `instance_repository.rs` (also infrastructure).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{ScheduledJob, ScheduledJobStatus};
use crate::shared::enum_str::decode;
use crate::shared::error::{PlatformError, Result};
use crate::usecase::unit_of_work::HasId;

#[derive(sqlx::FromRow)]
struct ScheduledJobRow {
    id: String,
    client_id: Option<String>,
    application_id: Option<String>,
    code: String,
    name: String,
    description: Option<String>,
    status: String,
    crons: Vec<String>,
    timezone: String,
    payload: Option<serde_json::Value>,
    concurrent: bool,
    tracks_completion: bool,
    timeout_seconds: Option<i32>,
    delivery_max_attempts: i32,
    target_url: Option<String>,
    last_fired_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<String>,
    updated_by: Option<String>,
    version: i32,
}

impl TryFrom<ScheduledJobRow> for ScheduledJob {
    type Error = PlatformError;
    fn try_from(r: ScheduledJobRow) -> Result<Self> {
        let status = decode(&r.status, "msg_scheduled_jobs", "status", &r.id)?;
        Ok(Self {
            id: r.id,
            client_id: r.client_id,
            application_id: r.application_id,
            code: r.code,
            name: r.name,
            description: r.description,
            status,
            crons: r.crons,
            timezone: r.timezone,
            payload: r.payload,
            concurrent: r.concurrent,
            tracks_completion: r.tracks_completion,
            timeout_seconds: r.timeout_seconds,
            delivery_max_attempts: r.delivery_max_attempts,
            target_url: r.target_url,
            last_fired_at: r.last_fired_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
            created_by: r.created_by,
            updated_by: r.updated_by,
            version: r.version,
        })
    }
}

const SELECT_COLS: &str =
    "id, client_id, application_id, code, name, description, status, crons, timezone, \
                            payload, concurrent, tracks_completion, timeout_seconds, \
                            delivery_max_attempts, target_url, last_fired_at, created_at, \
                            updated_at, created_by, updated_by, version";

pub struct ScheduledJobRepository {
    pool: PgPool,
}

impl ScheduledJobRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    // ── Reads ────────────────────────────────────────────────────────────────

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ScheduledJob>> {
        let row = sqlx::query_as::<_, ScheduledJobRow>(&format!(
            "SELECT {SELECT_COLS} FROM msg_scheduled_jobs WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(ScheduledJob::try_from).transpose()
    }

    /// Every job named by `ids`; an id with no row is simply absent.
    pub async fn find_by_ids(&self, ids: &[String]) -> Result<Vec<ScheduledJob>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, ScheduledJobRow>(&format!(
            "SELECT {SELECT_COLS} FROM msg_scheduled_jobs WHERE id = ANY($1)"
        ))
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(ScheduledJob::try_from).collect()
    }

    /// Look up by `(client_id, code)`. Pass `client_id = None` for platform-scoped jobs.
    pub async fn find_by_code(
        &self,
        client_id: Option<&str>,
        code: &str,
    ) -> Result<Option<ScheduledJob>> {
        let row = match client_id {
            Some(cid) => {
                sqlx::query_as::<_, ScheduledJobRow>(&format!(
                    "SELECT {SELECT_COLS} FROM msg_scheduled_jobs \
                     WHERE client_id = $1 AND code = $2"
                ))
                .bind(cid)
                .bind(code)
                .fetch_optional(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, ScheduledJobRow>(&format!(
                    "SELECT {SELECT_COLS} FROM msg_scheduled_jobs \
                     WHERE client_id IS NULL AND code = $1"
                ))
                .bind(code)
                .fetch_optional(&self.pool)
                .await?
            }
        };
        row.map(ScheduledJob::try_from).transpose()
    }

    pub async fn find_all(&self) -> Result<Vec<ScheduledJob>> {
        let rows = sqlx::query_as::<_, ScheduledJobRow>(&format!(
            "SELECT {SELECT_COLS} FROM msg_scheduled_jobs ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(ScheduledJob::try_from).collect()
    }

    pub async fn find_by_client(&self, client_id: &str) -> Result<Vec<ScheduledJob>> {
        let rows = sqlx::query_as::<_, ScheduledJobRow>(&format!(
            "SELECT {SELECT_COLS} FROM msg_scheduled_jobs \
             WHERE client_id = $1 ORDER BY created_at DESC"
        ))
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(ScheduledJob::try_from).collect()
    }

    /// Combined-filter list with optional pagination. AND semantics across
    /// non-None filters. `client_id = Some(None)` selects platform-scoped jobs;
    /// `client_id = None` skips the filter entirely.
    pub async fn find_with_filters(
        &self,
        client_id: Option<Option<&str>>,
        status: Option<ScheduledJobStatus>,
        search: Option<&str>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<ScheduledJob>> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {SELECT_COLS} FROM msg_scheduled_jobs"));
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(cid) = client_id {
            push_where(&mut qb, &mut has_where);
            match cid {
                Some(c) => {
                    qb.push("client_id = ").push_bind(c.to_string());
                }
                None => {
                    qb.push("client_id IS NULL");
                }
            }
        }
        if let Some(s) = status {
            push_where(&mut qb, &mut has_where);
            qb.push("status = ").push_bind(s.as_str().to_string());
        }
        if let Some(term) = search {
            push_where(&mut qb, &mut has_where);
            let like = format!("%{}%", term);
            qb.push("(code ILIKE ")
                .push_bind(like.clone())
                .push(" OR name ILIKE ")
                .push_bind(like)
                .push(")");
        }

        qb.push(" ORDER BY created_at DESC");
        if let Some(l) = limit {
            qb.push(" LIMIT ").push_bind(l);
        }
        if let Some(o) = offset {
            qb.push(" OFFSET ").push_bind(o);
        }

        let rows: Vec<ScheduledJobRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        rows.into_iter().map(ScheduledJob::try_from).collect()
    }

    /// Total count for pagination, sharing the same filter shape as
    /// `find_with_filters`. SQL stays identical apart from SELECT COUNT(*).
    pub async fn count_with_filters(
        &self,
        client_id: Option<Option<&str>>,
        status: Option<ScheduledJobStatus>,
        search: Option<&str>,
    ) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM msg_scheduled_jobs");
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(cid) = client_id {
            push_where(&mut qb, &mut has_where);
            match cid {
                Some(c) => {
                    qb.push("client_id = ").push_bind(c.to_string());
                }
                None => {
                    qb.push("client_id IS NULL");
                }
            }
        }
        if let Some(s) = status {
            push_where(&mut qb, &mut has_where);
            qb.push("status = ").push_bind(s.as_str().to_string());
        }
        if let Some(term) = search {
            push_where(&mut qb, &mut has_where);
            let like = format!("%{}%", term);
            qb.push("(code ILIKE ")
                .push_bind(like.clone())
                .push(" OR name ILIKE ")
                .push_bind(like)
                .push(")");
        }

        let row: (i64,) = qb.build_query_as().fetch_one(&self.pool).await?;
        Ok(row.0)
    }

    /// The list as Go's `FindWithFilters` (scheduledjob/repository.go):
    /// `accessible = Some(ids)` scopes it to platform jobs plus those
    /// clients' jobs (a non-anchor caller), ordered by code.
    pub async fn find_with_filters_scoped(
        &self,
        client_id: Option<Option<&str>>,
        status: Option<ScheduledJobStatus>,
        search: Option<&str>,
        accessible: Option<&[String]>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<ScheduledJob>> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {SELECT_COLS} FROM msg_scheduled_jobs"));
        push_list_filters(&mut qb, client_id, status, search, accessible);
        qb.push(" ORDER BY code");
        if let Some(l) = limit {
            qb.push(" LIMIT ").push_bind(l);
        }
        if let Some(o) = offset {
            qb.push(" OFFSET ").push_bind(o);
        }
        let rows: Vec<ScheduledJobRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        rows.into_iter().map(ScheduledJob::try_from).collect()
    }

    /// The count for [`Self::find_with_filters_scoped`].
    pub async fn count_with_filters_scoped(
        &self,
        client_id: Option<Option<&str>>,
        status: Option<ScheduledJobStatus>,
        search: Option<&str>,
        accessible: Option<&[String]>,
    ) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM msg_scheduled_jobs");
        push_list_filters(&mut qb, client_id, status, search, accessible);
        let row: (i64,) = qb.build_query_as().fetch_one(&self.pool).await?;
        Ok(row.0)
    }

    // ── Poller-only writes (infrastructure exemption) ───────────────────────

    /// All ACTIVE jobs whose `last_fired_at` is older than the given cutoff
    /// (or NULL). Caller (poller) computes the next due slot per row from the
    /// `crons` array. Selecting all active jobs each tick is fine — the count
    /// is small (definitions, not firings).
    pub async fn find_active_for_polling(&self) -> Result<Vec<ScheduledJob>> {
        let rows = sqlx::query_as::<_, ScheduledJobRow>(&format!(
            "SELECT {SELECT_COLS} FROM msg_scheduled_jobs \
             WHERE status = 'ACTIVE' \
             ORDER BY last_fired_at NULLS FIRST"
        ))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(ScheduledJob::try_from).collect()
    }

    /// Mark a slot as fired. Idempotent against re-runs of the same poller
    /// tick. Only updates `last_fired_at` — does not touch any other field
    /// or bump `version`. Bypasses UoW intentionally (infrastructure path).
    pub async fn mark_fired(&self, id: &str, slot: DateTime<Utc>) -> Result<()> {
        sqlx::query(
            "UPDATE msg_scheduled_jobs \
             SET last_fired_at = GREATEST(last_fired_at, $2) \
             WHERE id = $1",
        )
        .bind(id)
        .bind(slot)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// ── Persist<ScheduledJob> ────────────────────────────────────────────────────

impl HasId for ScheduledJob {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl crate::usecase::Persist<ScheduledJob> for ScheduledJobRepository {
    async fn persist(&self, sj: &ScheduledJob, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        // last_fired_at is intentionally excluded from the UPDATE clause —
        // it is owned by the poller and updated via mark_fired().
        sqlx::query(
            "INSERT INTO msg_scheduled_jobs \
                (id, client_id, code, name, description, status, crons, timezone, \
                 payload, concurrent, tracks_completion, timeout_seconds, \
                 delivery_max_attempts, target_url, last_fired_at, created_at, \
                 updated_at, created_by, updated_by, version, application_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
                     $14, $15, $16, $17, $18, $19, $20, $21) \
             ON CONFLICT (id) DO UPDATE SET \
                code = EXCLUDED.code, \
                application_id = EXCLUDED.application_id, \
                name = EXCLUDED.name, \
                description = EXCLUDED.description, \
                status = EXCLUDED.status, \
                crons = EXCLUDED.crons, \
                timezone = EXCLUDED.timezone, \
                payload = EXCLUDED.payload, \
                concurrent = EXCLUDED.concurrent, \
                tracks_completion = EXCLUDED.tracks_completion, \
                timeout_seconds = EXCLUDED.timeout_seconds, \
                delivery_max_attempts = EXCLUDED.delivery_max_attempts, \
                target_url = EXCLUDED.target_url, \
                updated_at = EXCLUDED.updated_at, \
                updated_by = EXCLUDED.updated_by, \
                version = EXCLUDED.version",
        )
        .bind(&sj.id)
        .bind(&sj.client_id)
        .bind(&sj.code)
        .bind(&sj.name)
        .bind(&sj.description)
        .bind(sj.status.as_str())
        .bind(&sj.crons)
        .bind(&sj.timezone)
        .bind(&sj.payload)
        .bind(sj.concurrent)
        .bind(sj.tracks_completion)
        .bind(sj.timeout_seconds)
        .bind(sj.delivery_max_attempts)
        .bind(&sj.target_url)
        .bind(sj.last_fired_at)
        .bind(sj.created_at)
        .bind(sj.updated_at)
        .bind(&sj.created_by)
        .bind(&sj.updated_by)
        .bind(sj.version)
        .bind(&sj.application_id)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, sj: &ScheduledJob, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        // Instances + logs remain — they're history. Retention sweeps drop
        // partitions on age, not on parent-row existence.
        sqlx::query("DELETE FROM msg_scheduled_jobs WHERE id = $1")
            .bind(&sj.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

/// The WHERE clause shared by the scoped list and its count.
fn push_list_filters(
    qb: &mut QueryBuilder<Postgres>,
    client_id: Option<Option<&str>>,
    status: Option<ScheduledJobStatus>,
    search: Option<&str>,
    accessible: Option<&[String]>,
) {
    let mut has_where = false;
    let mut push_where = |qb: &mut QueryBuilder<Postgres>| {
        qb.push(if has_where { " AND " } else { " WHERE " });
        has_where = true;
    };
    if let Some(cid) = client_id {
        push_where(qb);
        match cid {
            Some(c) => {
                qb.push("client_id = ").push_bind(c.to_string());
            }
            None => {
                qb.push("client_id IS NULL");
            }
        }
    }
    if let Some(s) = status {
        push_where(qb);
        qb.push("status = ").push_bind(s.as_str().to_string());
    }
    if let Some(term) = search.filter(|t| !t.is_empty()) {
        push_where(qb);
        let like = format!("%{term}%");
        qb.push("(code ILIKE ")
            .push_bind(like.clone())
            .push(" OR name ILIKE ")
            .push_bind(like)
            .push(")");
    }
    if let Some(ids) = accessible {
        push_where(qb);
        qb.push("(client_id IS NULL OR client_id = ANY(")
            .push_bind(ids.to_vec())
            .push("))");
    }
}
