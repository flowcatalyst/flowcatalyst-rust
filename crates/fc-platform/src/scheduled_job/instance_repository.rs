//! Instance + log repository for the scheduled-job pipeline.
//!
//! **Bypasses UnitOfWork by design.** Instances are created on every cron tick
//! and lifecycle-transitioned during webhook delivery — wrapping these in UoW
//! would emit a domain event per firing and saturate the event log. This is
//! the same exemption that applies to dispatch-job delivery lifecycle and
//! outbox processing (see CLAUDE.md).
//!
//! Permission checks for the SDK callback paths (`/instances/:id/log`,
//! `/instances/:id/complete`) are enforced at the handler layer.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{
    CompletionStatus, InstanceStatus, ScheduledJobInstance, ScheduledJobInstanceLog, TriggerKind,
};
use crate::shared::enum_str::{decode, decode_opt};
use crate::shared::error::{PlatformError, Result};

#[derive(sqlx::FromRow)]
struct InstanceRow {
    id: String,
    scheduled_job_id: String,
    client_id: Option<String>,
    job_code: String,
    trigger_kind: String,
    scheduled_for: Option<DateTime<Utc>>,
    fired_at: DateTime<Utc>,
    delivered_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    status: String,
    delivery_attempts: i32,
    delivery_error: Option<String>,
    completion_status: Option<String>,
    completion_result: Option<serde_json::Value>,
    correlation_id: Option<String>,
    created_at: DateTime<Utc>,
}

impl TryFrom<InstanceRow> for ScheduledJobInstance {
    type Error = PlatformError;
    fn try_from(r: InstanceRow) -> Result<Self> {
        let trigger_kind = decode(
            &r.trigger_kind,
            "msg_scheduled_job_instances",
            "trigger_kind",
            &r.id,
        )?;
        let status = decode(&r.status, "msg_scheduled_job_instances", "status", &r.id)?;
        let completion_status = decode_opt(
            r.completion_status.as_deref(),
            "msg_scheduled_job_instances",
            "completion_status",
            &r.id,
        )?;
        Ok(Self {
            id: r.id,
            scheduled_job_id: r.scheduled_job_id,
            client_id: r.client_id,
            job_code: r.job_code,
            trigger_kind,
            scheduled_for: r.scheduled_for,
            fired_at: r.fired_at,
            delivered_at: r.delivered_at,
            completed_at: r.completed_at,
            status,
            delivery_attempts: r.delivery_attempts,
            delivery_error: r.delivery_error,
            completion_status,
            completion_result: r.completion_result,
            correlation_id: r.correlation_id,
            created_at: r.created_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct LogRow {
    id: String,
    instance_id: String,
    scheduled_job_id: Option<String>,
    client_id: Option<String>,
    level: String,
    message: String,
    metadata: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

impl TryFrom<LogRow> for ScheduledJobInstanceLog {
    type Error = PlatformError;
    fn try_from(r: LogRow) -> Result<Self> {
        let level = decode(&r.level, "msg_scheduled_job_instance_logs", "level", &r.id)?;
        Ok(Self {
            id: r.id,
            instance_id: r.instance_id,
            scheduled_job_id: r.scheduled_job_id,
            client_id: r.client_id,
            level,
            message: r.message,
            metadata: r.metadata,
            created_at: r.created_at,
        })
    }
}

const INSTANCE_COLS: &str = "id, scheduled_job_id, client_id, job_code, trigger_kind, \
                              scheduled_for, fired_at, delivered_at, completed_at, status, \
                              delivery_attempts, delivery_error, completion_status, \
                              completion_result, correlation_id, created_at";

const LOG_COLS: &str = "id, instance_id, scheduled_job_id, client_id, level, message, \
                         metadata, created_at";

/// Composite filter for paginated instance lists. None means "no constraint".
#[derive(Debug, Default, Clone)]
pub struct InstanceListFilters<'a> {
    pub scheduled_job_id: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub status: Option<InstanceStatus>,
    pub trigger_kind: Option<TriggerKind>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub struct ScheduledJobInstanceRepository {
    pool: PgPool,
}

impl ScheduledJobInstanceRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    // ── Instance writes (cron tick + delivery lifecycle) ────────────────────

    /// Insert a freshly-created instance row in QUEUED status. Returns the row
    /// as inserted (caller already populated the id/timestamps).
    pub async fn insert(&self, inst: &ScheduledJobInstance) -> Result<()> {
        sqlx::query(
            "INSERT INTO msg_scheduled_job_instances \
                (id, scheduled_job_id, client_id, job_code, trigger_kind, scheduled_for, \
                 fired_at, status, delivery_attempts, correlation_id, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&inst.id)
        .bind(&inst.scheduled_job_id)
        .bind(&inst.client_id)
        .bind(&inst.job_code)
        .bind(inst.trigger_kind.as_str())
        .bind(inst.scheduled_for)
        .bind(inst.fired_at)
        .bind(inst.status.as_str())
        .bind(inst.delivery_attempts)
        .bind(&inst.correlation_id)
        .bind(inst.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Mark an instance as IN_FLIGHT and bump delivery_attempts. Used by the
    /// dispatcher just before issuing the HTTP POST.
    pub async fn mark_in_flight(&self, id: &str, created_at: DateTime<Utc>) -> Result<()> {
        sqlx::query(
            "UPDATE msg_scheduled_job_instances \
             SET status = 'IN_FLIGHT', delivery_attempts = delivery_attempts + 1 \
             WHERE id = $1 AND created_at = $2",
        )
        .bind(id)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Successful 202 ACK from the SDK. Sets DELIVERED + delivered_at. If the
    /// job has tracks_completion = false, this is the terminal state.
    pub async fn mark_delivered(&self, id: &str, created_at: DateTime<Utc>) -> Result<()> {
        sqlx::query(
            "UPDATE msg_scheduled_job_instances \
             SET status = 'DELIVERED', delivered_at = NOW() \
             WHERE id = $1 AND created_at = $2",
        )
        .bind(id)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delivery failed (non-2xx, network, or attempts exhausted). Caller
    /// decides terminal vs. retryable based on `delivery_max_attempts`.
    pub async fn mark_delivery_failed(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        error: &str,
        terminal: bool,
    ) -> Result<()> {
        let status = if terminal {
            "DELIVERY_FAILED"
        } else {
            "QUEUED"
        };
        sqlx::query(
            "UPDATE msg_scheduled_job_instances \
             SET status = $3, delivery_error = $4 \
             WHERE id = $1 AND created_at = $2",
        )
        .bind(id)
        .bind(created_at)
        .bind(status)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// SDK-reported completion. Only meaningful when the job's
    /// `tracks_completion` is true; otherwise the instance is already
    /// terminal at DELIVERED and this is a stale callback.
    pub async fn record_completion(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        status: InstanceStatus,
        completion_status: Option<CompletionStatus>,
        result: Option<&serde_json::Value>,
    ) -> Result<()> {
        // Go's `MarkComplete`: the caller has resolved the instance status
        // and the completion outcome (see the complete handler).
        sqlx::query(
            "UPDATE msg_scheduled_job_instances \
             SET status = $3, completion_status = $4, completion_result = $5, \
                 completed_at = NOW() \
             WHERE id = $1 AND created_at = $2",
        )
        .bind(id)
        .bind(created_at)
        .bind(status.as_str())
        .bind(completion_status.map(|c| c.as_str()))
        .bind(result)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Instance reads ──────────────────────────────────────────────────────

    /// Fetch by id. Partitioned-table reads need either a created_at hint or
    /// a full scan across partitions; we accept the scan since UI lookups are
    /// rare and the partial index on (status) handles the hot path elsewhere.
    pub async fn find_by_id(&self, id: &str) -> Result<Option<ScheduledJobInstance>> {
        let row = sqlx::query_as::<_, InstanceRow>(&format!(
            "SELECT {INSTANCE_COLS} FROM msg_scheduled_job_instances WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(ScheduledJobInstance::try_from).transpose()
    }

    /// True if the job has any non-terminal instance — used by the UI as the
    /// "overlap detected" badge for `concurrent: false` jobs.
    /// Which of `jobs` (id, tracks-completion) have an active instance, in
    /// one query: QUEUED or IN_FLIGHT, or DELIVERED and not yet completed
    /// for a job that tracks completion (Go `HasActiveInstance`).
    pub async fn active_job_ids(
        &self,
        jobs: &[(String, bool)],
    ) -> Result<std::collections::HashSet<String>> {
        if jobs.is_empty() {
            return Ok(Default::default());
        }
        let ids: Vec<&str> = jobs.iter().map(|(id, _)| id.as_str()).collect();
        let tracks: Vec<bool> = jobs.iter().map(|(_, t)| *t).collect();
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT j.id FROM UNNEST($1::text[], $2::bool[]) AS j(id, tracks) \
             WHERE EXISTS ( \
                 SELECT 1 FROM msg_scheduled_job_instances i \
                 WHERE i.scheduled_job_id = j.id \
                   AND (i.status IN ('QUEUED', 'IN_FLIGHT') \
                        OR (i.status = 'DELIVERED' AND j.tracks AND i.completed_at IS NULL)))",
        )
        .bind(&ids)
        .bind(&tracks)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    pub async fn has_active_instance(&self, scheduled_job_id: &str) -> Result<bool> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM msg_scheduled_job_instances \
             WHERE scheduled_job_id = $1 \
               AND status IN ('QUEUED', 'IN_FLIGHT', 'DELIVERED')",
        )
        .bind(scheduled_job_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    /// Go's `HasActiveInstance`: a QUEUED or IN_FLIGHT instance, or a
    /// DELIVERED one still awaiting its completion callback when the job
    /// tracks completion.
    pub async fn has_active_instance_for(
        &self,
        scheduled_job_id: &str,
        tracks_completion: bool,
    ) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM msg_scheduled_job_instances \
             WHERE scheduled_job_id = $1 \
               AND (status IN ('QUEUED', 'IN_FLIGHT') \
                    OR (status = 'DELIVERED' AND $2 AND completed_at IS NULL)))",
        )
        .bind(scheduled_job_id)
        .bind(tracks_completion)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    /// [`Self::has_active_instance_for`] for many jobs at once: the ids of
    /// `job_ids` with an active instance (`tracking` names the jobs that
    /// track completion).
    pub async fn jobs_with_active_instances(
        &self,
        job_ids: &[String],
        tracking: &[String],
    ) -> Result<std::collections::HashSet<String>> {
        if job_ids.is_empty() {
            return Ok(Default::default());
        }
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT scheduled_job_id FROM msg_scheduled_job_instances \
             WHERE scheduled_job_id = ANY($1) \
               AND (status IN ('QUEUED', 'IN_FLIGHT') \
                    OR (status = 'DELIVERED' AND completed_at IS NULL \
                        AND scheduled_job_id = ANY($2)))",
        )
        .bind(job_ids)
        .bind(tracking)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    pub async fn list(&self, f: &InstanceListFilters<'_>) -> Result<Vec<ScheduledJobInstance>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "SELECT {INSTANCE_COLS} FROM msg_scheduled_job_instances"
        ));
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(jid) = f.scheduled_job_id {
            push_where(&mut qb, &mut has_where);
            qb.push("scheduled_job_id = ").push_bind(jid.to_string());
        }
        if let Some(cid) = f.client_id {
            push_where(&mut qb, &mut has_where);
            qb.push("client_id = ").push_bind(cid.to_string());
        }
        if let Some(s) = f.status {
            push_where(&mut qb, &mut has_where);
            qb.push("status = ").push_bind(s.as_str().to_string());
        }
        if let Some(tk) = f.trigger_kind {
            push_where(&mut qb, &mut has_where);
            qb.push("trigger_kind = ")
                .push_bind(tk.as_str().to_string());
        }
        if let Some(from) = f.from {
            push_where(&mut qb, &mut has_where);
            qb.push("created_at >= ").push_bind(from);
        }
        if let Some(to) = f.to {
            push_where(&mut qb, &mut has_where);
            qb.push("created_at < ").push_bind(to);
        }

        qb.push(" ORDER BY created_at DESC");
        if let Some(l) = f.limit {
            qb.push(" LIMIT ").push_bind(l);
        }
        if let Some(o) = f.offset {
            qb.push(" OFFSET ").push_bind(o);
        }

        let rows: Vec<InstanceRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(ScheduledJobInstance::try_from)
            .collect()
    }

    pub async fn count(&self, f: &InstanceListFilters<'_>) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM msg_scheduled_job_instances");
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(jid) = f.scheduled_job_id {
            push_where(&mut qb, &mut has_where);
            qb.push("scheduled_job_id = ").push_bind(jid.to_string());
        }
        if let Some(cid) = f.client_id {
            push_where(&mut qb, &mut has_where);
            qb.push("client_id = ").push_bind(cid.to_string());
        }
        if let Some(s) = f.status {
            push_where(&mut qb, &mut has_where);
            qb.push("status = ").push_bind(s.as_str().to_string());
        }
        if let Some(tk) = f.trigger_kind {
            push_where(&mut qb, &mut has_where);
            qb.push("trigger_kind = ")
                .push_bind(tk.as_str().to_string());
        }
        if let Some(from) = f.from {
            push_where(&mut qb, &mut has_where);
            qb.push("created_at >= ").push_bind(from);
        }
        if let Some(to) = f.to {
            push_where(&mut qb, &mut has_where);
            qb.push("created_at < ").push_bind(to);
        }

        let row: (i64,) = qb.build_query_as().fetch_one(&self.pool).await?;
        Ok(row.0)
    }

    // ── Logs ────────────────────────────────────────────────────────────────

    /// Append a log entry to an instance. Called from the SDK callback path
    /// (`/api/scheduled-jobs/instances/:id/log`).
    pub async fn insert_log(&self, log: &ScheduledJobInstanceLog) -> Result<()> {
        sqlx::query(
            "INSERT INTO msg_scheduled_job_instance_logs \
                (id, instance_id, scheduled_job_id, client_id, level, message, metadata, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&log.id)
        .bind(&log.instance_id)
        .bind(&log.scheduled_job_id)
        .bind(&log.client_id)
        .bind(log.level.as_str())
        .bind(&log.message)
        .bind(&log.metadata)
        .bind(log.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_logs_for_instance(
        &self,
        instance_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<ScheduledJobInstanceLog>> {
        let limit = limit.unwrap_or(500);
        let rows = sqlx::query_as::<_, LogRow>(&format!(
            "SELECT {LOG_COLS} FROM msg_scheduled_job_instance_logs \
             WHERE instance_id = $1 ORDER BY created_at ASC LIMIT $2"
        ))
        .bind(instance_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(ScheduledJobInstanceLog::try_from)
            .collect()
    }
}
