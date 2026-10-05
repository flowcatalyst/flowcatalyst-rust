//! DispatchJob Repository — PostgreSQL via SQLx

use crate::dispatch_job::entity::{
    default_content_type, DispatchAttempt, DispatchAttemptStatus, DispatchMetadata, ErrorType,
};
use crate::dispatch_job::entity::{parse_dispatch_mode, parse_dispatch_status};
use crate::dispatch_job::entity::{DispatchJob, DispatchJobRead, DispatchStatus};
use crate::dispatch_job::lifecycle;
use chrono::{DateTime, Utc};
use fc_platform_core::shared::api_common::DecodedCursor;
use fc_platform_core::shared::enum_str::{corrupt_value, decode};
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::shared::tsid;
use sqlx::{PgPool, Postgres, QueryBuilder};

// ─── Row structs ─────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DispatchJobRow {
    id: String,
    external_id: Option<String>,
    source: Option<String>,
    kind: String,
    code: String,
    subject: Option<String>,
    event_id: Option<String>,
    correlation_id: Option<String>,
    metadata: serde_json::Value,
    target_url: String,
    protocol: String,
    payload: Option<String>,
    payload_content_type: Option<String>,
    data_only: bool,
    service_account_id: Option<String>,
    client_id: Option<String>,
    subscription_id: Option<String>,
    mode: String,
    dispatch_pool_id: Option<String>,
    message_group: Option<String>,
    sequence: i32,
    timeout_seconds: i32,
    schema_id: Option<String>,
    status: String,
    max_retries: i32,
    retry_strategy: String,
    scheduled_for: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    attempt_count: i32,
    last_attempt_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    duration_millis: Option<i64>,
    last_error: Option<String>,
    idempotency_key: Option<String>,
    descriptor: Option<String>,
    queue: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<DispatchJobRow> for DispatchJob {
    type Error = PlatformError;
    fn try_from(r: DispatchJobRow) -> Result<Self> {
        let kind = decode(&r.kind, "msg_dispatch_jobs", "kind", &r.id)?;
        let protocol = decode(&r.protocol, "msg_dispatch_jobs", "protocol", &r.id)?;
        let mode = parse_dispatch_mode(Some(&r.mode));
        let retry_strategy = decode(
            &r.retry_strategy,
            "msg_dispatch_jobs",
            "retry_strategy",
            &r.id,
        )?;
        let status = parse_dispatch_status(&r.status)
            .map_err(|_| corrupt_value("msg_dispatch_jobs", "status", &r.status, &r.id))?;
        let metadata: Vec<DispatchMetadata> =
            serde_json::from_value(r.metadata).unwrap_or_default();
        Ok(Self {
            id: r.id,
            external_id: r.external_id,
            kind,
            code: r.code,
            source: r.source,
            subject: r.subject,
            target_url: r.target_url,
            protocol,
            payload: r.payload,
            payload_content_type: r.payload_content_type.unwrap_or_else(default_content_type),
            data_only: r.data_only,
            event_id: r.event_id,
            correlation_id: r.correlation_id,
            client_id: r.client_id,
            subscription_id: r.subscription_id,
            service_account_id: r.service_account_id,
            dispatch_pool_id: r.dispatch_pool_id,
            message_group: r.message_group,
            mode,
            sequence: r.sequence,
            timeout_seconds: r.timeout_seconds as u32,
            schema_id: r.schema_id,
            max_retries: r.max_retries as u32,
            retry_strategy,
            status,
            attempt_count: r.attempt_count as u32,
            last_error: r.last_error,
            attempts: vec![],
            metadata,
            idempotency_key: r.idempotency_key,
            descriptor: r.descriptor,
            queue: r.queue,
            created_at: r.created_at,
            updated_at: r.updated_at,
            scheduled_for: r.scheduled_for,
            expires_at: r.expires_at,
            last_attempt_at: r.last_attempt_at,
            completed_at: r.completed_at,
            duration_millis: r.duration_millis,
        })
    }
}

#[derive(sqlx::FromRow)]
struct DispatchJobReadRow {
    id: String,
    external_id: Option<String>,
    source: Option<String>,
    kind: String,
    code: String,
    subject: Option<String>,
    event_id: Option<String>,
    correlation_id: Option<String>,
    target_url: String,
    protocol: String,
    client_id: Option<String>,
    subscription_id: Option<String>,
    service_account_id: Option<String>,
    dispatch_pool_id: Option<String>,
    message_group: Option<String>,
    mode: String,
    sequence: i32,
    status: String,
    attempt_count: i32,
    max_retries: i32,
    last_error: Option<String>,
    timeout_seconds: i32,
    retry_strategy: String,
    application: Option<String>,
    subdomain: Option<String>,
    aggregate: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    scheduled_for: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    last_attempt_at: Option<DateTime<Utc>>,
    duration_millis: Option<i64>,
    idempotency_key: Option<String>,
    descriptor: Option<String>,
    metadata: serde_json::Value,
    queue: Option<String>,
    is_completed: Option<bool>,
    is_terminal: Option<bool>,
    projected_at: Option<DateTime<Utc>>,
}

impl TryFrom<DispatchJobReadRow> for DispatchJobRead {
    type Error = PlatformError;
    fn try_from(r: DispatchJobReadRow) -> Result<Self> {
        let kind = decode(&r.kind, "msg_dispatch_jobs_read", "kind", &r.id)?;
        let protocol = decode(&r.protocol, "msg_dispatch_jobs_read", "protocol", &r.id)?;
        let mode = parse_dispatch_mode(Some(&r.mode));
        let status = parse_dispatch_status(&r.status)
            .map_err(|_| corrupt_value("msg_dispatch_jobs_read", "status", &r.status, &r.id))?;
        let retry_strategy = decode(
            &r.retry_strategy,
            "msg_dispatch_jobs_read",
            "retry_strategy",
            &r.id,
        )?;
        Ok(Self {
            id: r.id,
            external_id: r.external_id,
            source: r.source,
            kind,
            code: r.code,
            subject: r.subject,
            event_id: r.event_id,
            correlation_id: r.correlation_id,
            target_url: r.target_url,
            protocol,
            client_id: r.client_id,
            subscription_id: r.subscription_id,
            service_account_id: r.service_account_id,
            dispatch_pool_id: r.dispatch_pool_id,
            message_group: r.message_group,
            mode,
            sequence: r.sequence,
            status,
            attempt_count: r.attempt_count as u32,
            max_retries: r.max_retries as u32,
            last_error: r.last_error,
            timeout_seconds: r.timeout_seconds as u32,
            retry_strategy,
            application: r.application,
            subdomain: r.subdomain,
            aggregate: r.aggregate,
            created_at: r.created_at,
            updated_at: r.updated_at,
            scheduled_for: r.scheduled_for,
            expires_at: r.expires_at,
            completed_at: r.completed_at,
            last_attempt_at: r.last_attempt_at,
            duration_millis: r.duration_millis,
            idempotency_key: r.idempotency_key,
            descriptor: r.descriptor,
            // As Go's `rowToJob`: tags that are not `[{key, value}]` read as
            // none.
            metadata: serde_json::from_value(r.metadata).unwrap_or_default(),
            queue: r.queue,
            is_completed: r.is_completed.unwrap_or_default(),
            is_terminal: r.is_terminal.unwrap_or_default(),
            projected_at: r.projected_at,
        })
    }
}

// ─── Repository ──────────────────────────────────────────────────────────────

/// A delivery attempt as read back from `msg_dispatch_job_attempts`.
#[derive(Debug, Clone)]
pub struct RecordedAttempt {
    pub attempt: DispatchAttempt,
    /// What was sent: Go's `RequestSummary` JSON (`signedBy`, `signature`,
    /// `bearer`, `timestamp`, `headers`, `unsignedReason`, `target`).
    pub request_info: Option<serde_json::Value>,
}

#[derive(sqlx::FromRow)]
struct AttemptRow {
    attempt_number: Option<i32>,
    status: Option<String>,
    response_code: Option<i32>,
    response_body: Option<String>,
    error_message: Option<String>,
    error_type: Option<String>,
    duration_millis: Option<i64>,
    attempted_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    request_info: Option<serde_json::Value>,
}

impl From<AttemptRow> for RecordedAttempt {
    fn from(r: AttemptRow) -> Self {
        let success = r
            .status
            .as_deref()
            .and_then(|s| s.parse::<DispatchAttemptStatus>().ok())
            == Some(DispatchAttemptStatus::Success);
        Self {
            attempt: DispatchAttempt {
                attempt_number: r.attempt_number.unwrap_or(0).max(0) as u32,
                attempted_at: r.attempted_at.unwrap_or(r.created_at),
                completed_at: r.completed_at,
                duration_millis: r.duration_millis,
                response_code: r.response_code.and_then(|c| u16::try_from(c).ok()),
                response_body: r.response_body,
                success,
                error_message: r.error_message,
                error_type: r.error_type.as_deref().and_then(|t| t.parse().ok()),
            },
            request_info: r.request_info,
        }
    }
}

/// One webhook delivery attempt, as recorded in `msg_dispatch_job_attempts`.
#[derive(Debug, Clone, Copy)]
pub struct NewDispatchAttempt<'a> {
    pub dispatch_job_id: &'a str,
    pub attempt_number: u32,
    pub status: DispatchAttemptStatus,
    pub response_code: Option<u16>,
    pub response_body: Option<&'a str>,
    pub error_message: Option<&'a str>,
    pub error_type: Option<ErrorType>,
    pub error_stack_trace: Option<&'a str>,
    pub duration_millis: i64,
    /// When the attempt started and finished (Go's `NewAttempt` /
    /// `Complete*`).
    pub attempted_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    /// What was sent (`request_info`, Go's `RequestSummary`): never a secret.
    pub request_info: Option<&'a serde_json::Value>,
}

/// Recorded on the rows `/api/dispatch/settled` resets when the caller gives
/// no reason (Go `settled.defaultReason`).
pub const SETTLED_DEFAULT_REASON: &str =
    "settled: router ACKed as an untried buffered sibling behind a failed BLOCK_ON_ERROR head";

/// Recorded on the rows the stranded-sibling reaper resets (Go `reapReason`).
pub const REAP_REASON: &str =
    "reaper: sibling of a FAILED BLOCK_ON_ERROR head, stranded QUEUED/PROCESSING";

/// Go's dispatch-job list filters (`dispatchjob.FilterParams`): singular
/// equality filters for SDK callers, CSV multi-filters for the SPA, and
/// `accessible` scoping a non-anchor caller to platform jobs plus its
/// clients' jobs.
#[derive(Debug, Default)]
pub struct DispatchJobReadFilter<'a> {
    pub status: Option<&'a str>,
    pub statuses: &'a [String],
    pub client_id: Option<&'a str>,
    pub client_ids: &'a [String],
    pub accessible: Option<&'a [String]>,
    pub dispatch_pool_id: Option<&'a str>,
    pub subscription_id: Option<&'a str>,
    pub code: Option<&'a str>,
    pub codes: &'a [String],
    pub source: Option<&'a str>,
    pub message_group: Option<&'a str>,
    pub applications: &'a [String],
    pub subdomains: &'a [String],
    pub aggregates: &'a [String],
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub ascending: bool,
    pub limit: i64,
    pub offset: i64,
}

pub struct DispatchJobRepository {
    pool: PgPool,
}

impl DispatchJobRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Insert one job (the lifecycle's creation).
    pub async fn insert(&self, job: &DispatchJob) -> Result<()> {
        lifecycle::create_batch(&self.pool, &[lifecycle::new_job(job)]).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<DispatchJob>> {
        let row =
            sqlx::query_as::<_, DispatchJobRow>("SELECT * FROM msg_dispatch_jobs WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;

        row.map(DispatchJob::try_from).transpose()
    }

    pub async fn find_by_event_id(&self, event_id: &str) -> Result<Vec<DispatchJob>> {
        let rows = sqlx::query_as::<_, DispatchJobRow>(
            "SELECT * FROM msg_dispatch_jobs WHERE event_id = $1",
        )
        .bind(event_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(DispatchJob::try_from).collect()
    }

    pub async fn find_by_subscription_id(
        &self,
        subscription_id: &str,
        limit: i64,
    ) -> Result<Vec<DispatchJob>> {
        if limit > 0 {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs WHERE subscription_id = $1 LIMIT $2",
            )
            .bind(subscription_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        } else {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs WHERE subscription_id = $1",
            )
            .bind(subscription_id)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        }
    }

    pub async fn find_by_status(
        &self,
        status: DispatchStatus,
        limit: i64,
    ) -> Result<Vec<DispatchJob>> {
        if limit > 0 {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs WHERE status = $1 LIMIT $2",
            )
            .bind(status.as_str())
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        } else {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs WHERE status = $1",
            )
            .bind(status.as_str())
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        }
    }

    /// The due PENDING jobs, read through the queue table (one row per
    /// PENDING job), not by scanning the jobs table for the status.
    pub async fn find_pending_for_dispatch(&self, limit: i64) -> Result<Vec<DispatchJob>> {
        let now = Utc::now();
        let sql = "SELECT j.* FROM msg_dispatch_queue q \
                   JOIN msg_dispatch_jobs j ON j.id = q.job_id AND j.created_at = q.job_created_at \
                   WHERE (q.scheduled_for IS NULL OR q.scheduled_for <= $1)";
        let rows = if limit > 0 {
            sqlx::query_as::<_, DispatchJobRow>(&format!("{sql} LIMIT $2"))
                .bind(now)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query_as::<_, DispatchJobRow>(sql)
                .bind(now)
                .fetch_all(&self.pool)
                .await?
        };
        rows.into_iter().map(DispatchJob::try_from).collect()
    }

    pub async fn find_stale_in_progress(
        &self,
        stale_threshold: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<DispatchJob>> {
        if limit > 0 {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs \
                 WHERE status = 'PROCESSING' AND updated_at < $1 \
                 LIMIT $2",
            )
            .bind(stale_threshold)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        } else {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs \
                 WHERE status = 'PROCESSING' AND updated_at < $1",
            )
            .bind(stale_threshold)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        }
    }

    pub async fn find_by_client(&self, client_id: &str, limit: i64) -> Result<Vec<DispatchJob>> {
        if limit > 0 {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs WHERE client_id = $1 LIMIT $2",
            )
            .bind(client_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        } else {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs WHERE client_id = $1",
            )
            .bind(client_id)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        }
    }

    pub async fn find_by_correlation_id(&self, correlation_id: &str) -> Result<Vec<DispatchJob>> {
        let rows = sqlx::query_as::<_, DispatchJobRow>(
            "SELECT * FROM msg_dispatch_jobs WHERE correlation_id = $1",
        )
        .bind(correlation_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(DispatchJob::try_from).collect()
    }

    /// Find dispatch jobs with optional combined filters (AND logic).
    pub async fn find_with_filters(
        &self,
        event_id: Option<&str>,
        correlation_id: Option<&str>,
        subscription_id: Option<&str>,
        client_id: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<DispatchJob>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM msg_dispatch_jobs");
        let mut has_where = false;

        fn push_where(qb: &mut QueryBuilder<Postgres>, has_where: &mut bool) {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        }

        if let Some(v) = event_id {
            push_where(&mut qb, &mut has_where);
            qb.push("event_id = ").push_bind(v);
        }
        if let Some(v) = correlation_id {
            push_where(&mut qb, &mut has_where);
            qb.push("correlation_id = ").push_bind(v);
        }
        if let Some(v) = subscription_id {
            push_where(&mut qb, &mut has_where);
            qb.push("subscription_id = ").push_bind(v);
        }
        if let Some(v) = client_id {
            push_where(&mut qb, &mut has_where);
            qb.push("client_id = ").push_bind(v);
        }
        if let Some(v) = status {
            push_where(&mut qb, &mut has_where);
            qb.push("status = ").push_bind(v);
        }

        qb.push(" ORDER BY created_at DESC");
        if limit > 0 {
            qb.push(" LIMIT ").push_bind(limit);
        }

        let rows: Vec<DispatchJobRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        rows.into_iter().map(DispatchJob::try_from).collect()
    }

    /// Bulk insert multiple dispatch jobs using UNNEST against the pool.
    pub async fn insert_many(&self, jobs: &[DispatchJob]) -> Result<()> {
        Self::insert_many_inner(&self.pool, jobs).await
    }

    /// The advisory-lock class [`Self::insert_new`] serialises caller-supplied
    /// ids under (`pg_advisory_xact_lock(int, int)`'s first key, the second
    /// being the id's `hashtext`): "djid", as Java's
    /// `DispatchJobRepository.SUPPLIED_ID_LOCK_CLASS`.
    const SUPPLIED_ID_LOCK_CLASS: i32 = 0x646A_6964;

    /// Insert a batch in which some jobs carry caller-supplied ids (owner
    /// decision #24; Java `insertNew`, security-fixes-2026-09-24 S3.3).
    /// Refuses the whole batch when any of `supplied_ids` already names a
    /// job: the primary key is `(id, created_at)` on a partitioned table, so
    /// the database would happily store a second row with the same id and a
    /// later `created_at`, and every read by id would then see either. In
    /// one transaction each supplied id's advisory lock is taken (in hash
    /// order, so two batches sharing ids cannot deadlock, and two requests
    /// supplying the same new id serialise instead of both inserting), the
    /// ids are looked up across every partition, and only then is the batch
    /// inserted. `supplied_ids` must already be free of repeats.
    ///
    /// Returns the ids already taken, sorted; empty when the batch was
    /// inserted.
    pub async fn insert_new(
        &self,
        jobs: &[DispatchJob],
        supplied_ids: &[String],
    ) -> Result<Vec<String>> {
        if jobs.is_empty() {
            return Ok(Vec::new());
        }
        if supplied_ids.is_empty() {
            self.insert_many(jobs).await?;
            return Ok(Vec::new());
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock($1, k) \
             FROM (SELECT DISTINCT hashtext(x) AS k FROM unnest($2::text[]) AS x) s ORDER BY k",
        )
        .bind(Self::SUPPLIED_ID_LOCK_CLASS)
        .bind(supplied_ids)
        .execute(&mut *tx)
        .await?;
        let taken: Vec<String> = sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT id FROM msg_dispatch_jobs WHERE id = ANY($1) ORDER BY id",
        )
        .bind(supplied_ids)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|(id,)| id)
        .collect();
        if !taken.is_empty() {
            tx.rollback().await?;
            return Ok(taken);
        }
        Self::insert_many_inner(&mut *tx, jobs).await?;
        tx.commit().await?;
        Ok(Vec::new())
    }

    /// Bulk insert as part of an existing transaction. Used by services that
    /// need atomicity across the insert and another write (e.g. fan-out: claim
    /// events + create dispatch jobs in one txn).
    pub async fn insert_many_tx<'a>(
        &self,
        tx: &mut sqlx::Transaction<'a, sqlx::Postgres>,
        jobs: &[DispatchJob],
    ) -> Result<()> {
        Self::insert_many_inner(&mut **tx, jobs).await
    }

    async fn insert_many_inner<'e, E>(executor: E, jobs: &[DispatchJob]) -> Result<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows: Vec<lifecycle::NewJob> = jobs.iter().map(lifecycle::new_job).collect();
        lifecycle::create_batch(executor, &rows).await?;
        Ok(())
    }

    // ── Read projection methods ──────────────────────────────────────────

    pub async fn find_read_by_id(&self, id: &str) -> Result<Option<DispatchJobRead>> {
        let row = sqlx::query_as::<_, DispatchJobReadRow>(
            "SELECT * FROM msg_dispatch_jobs_read WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(DispatchJobRead::try_from).transpose()
    }

    /// The read projection filtered as Go's `FindWithFilters`.
    pub async fn find_read_filtered(
        &self,
        f: &DispatchJobReadFilter<'_>,
    ) -> Result<Vec<DispatchJobRead>> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT * FROM msg_dispatch_jobs_read WHERE TRUE");
        let eq = |qb: &mut QueryBuilder<Postgres>, col: &str, v: Option<&str>| {
            if let Some(v) = v {
                qb.push(format!(" AND {col} = ")).push_bind(v.to_string());
            }
        };
        let any = |qb: &mut QueryBuilder<Postgres>, col: &str, v: &[String]| {
            if !v.is_empty() {
                qb.push(format!(" AND {col} = ANY("))
                    .push_bind(v.to_vec())
                    .push(")");
            }
        };
        eq(&mut qb, "status", f.status);
        any(&mut qb, "status", f.statuses);
        eq(&mut qb, "client_id", f.client_id);
        any(&mut qb, "client_id", f.client_ids);
        if let Some(ids) = f.accessible {
            qb.push(" AND (client_id IS NULL OR client_id = ANY(")
                .push_bind(ids.to_vec())
                .push("))");
        }
        eq(&mut qb, "dispatch_pool_id", f.dispatch_pool_id);
        eq(&mut qb, "subscription_id", f.subscription_id);
        eq(&mut qb, "code", f.code);
        any(&mut qb, "code", f.codes);
        eq(&mut qb, "source", f.source);
        eq(&mut qb, "message_group", f.message_group);
        any(&mut qb, "application", f.applications);
        any(&mut qb, "subdomain", f.subdomains);
        any(&mut qb, "aggregate", f.aggregates);
        if let Some(t) = f.since {
            qb.push(" AND created_at >= ").push_bind(t);
        }
        if let Some(t) = f.until {
            qb.push(" AND created_at <= ").push_bind(t);
        }
        qb.push(if f.ascending {
            " ORDER BY created_at ASC"
        } else {
            " ORDER BY created_at DESC"
        });
        qb.push(" LIMIT ").push_bind(f.limit);
        if f.offset > 0 {
            qb.push(" OFFSET ").push_bind(f.offset);
        }
        let rows: Vec<DispatchJobReadRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        rows.into_iter().map(DispatchJobRead::try_from).collect()
    }

    /// An event's jobs from the read projection, newest first (Go
    /// `FindByEventID`).
    pub async fn find_read_by_event_id(&self, event_id: &str) -> Result<Vec<DispatchJobRead>> {
        let rows = sqlx::query_as::<_, DispatchJobReadRow>(
            "SELECT * FROM msg_dispatch_jobs_read WHERE event_id = $1 ORDER BY created_at DESC",
        )
        .bind(event_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(DispatchJobRead::try_from).collect()
    }

    /// Distinct non-null values of one whitelisted read-projection column,
    /// sorted, at most 200 (Go `DistinctValues`).
    pub async fn distinct_read_values(&self, column: &str) -> Result<Vec<String>> {
        const ALLOWED: &[&str] = &[
            "status",
            "code",
            "client_id",
            "dispatch_pool_id",
            "subscription_id",
            "kind",
        ];
        if !ALLOWED.contains(&column) {
            return Err(PlatformError::internal(format!(
                "dispatch_job repo: column {column:?} not allowed"
            )));
        }
        let rows: Vec<(String,)> = sqlx::query_as(&format!(
            "SELECT DISTINCT {column}::text FROM msg_dispatch_jobs_read \
             WHERE {column} IS NOT NULL ORDER BY 1 LIMIT 200"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(v,)| v).collect())
    }

    /// Cursor-paginated read of `msg_dispatch_jobs_read`. Drops `SELECT COUNT(*)` and the
    /// configurable sort — orders by `(created_at, id) DESC` so the keyset
    /// comparison is well-defined. Returns `fetch_limit` rows so the API
    /// layer can detect `hasMore`.
    #[allow(clippy::too_many_arguments)]
    pub async fn find_read_with_cursor(
        &self,
        client_ids: &[String],
        statuses: &[String],
        applications: &[String],
        subdomains: &[String],
        aggregates: &[String],
        codes: &[String],
        search: Option<&str>,
        cursor: Option<&DecodedCursor>,
        fetch_limit: i64,
    ) -> Result<Vec<DispatchJobRead>> {
        let search_pattern = search
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{}%", s));

        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT * FROM msg_dispatch_jobs_read");
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if !client_ids.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("client_id = ANY(").push_bind(client_ids).push(")");
        }
        if !statuses.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("status = ANY(").push_bind(statuses).push(")");
        }
        if !applications.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("application = ANY(")
                .push_bind(applications)
                .push(")");
        }
        if !subdomains.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("subdomain = ANY(").push_bind(subdomains).push(")");
        }
        if !aggregates.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("aggregate = ANY(").push_bind(aggregates).push(")");
        }
        if !codes.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("code = ANY(").push_bind(codes).push(")");
        }
        if let Some(pattern) = search_pattern {
            push_where(&mut qb, &mut has_where);
            qb.push("(code ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR subject ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR source ILIKE ")
                .push_bind(pattern.clone())
                .push(")");
        }
        if let Some(c) = cursor {
            push_where(&mut qb, &mut has_where);
            qb.push("(created_at, id) < (")
                .push_bind(c.created_at)
                .push(", ")
                .push_bind(c.id.clone())
                .push(")");
        }

        qb.push(" ORDER BY created_at DESC, id DESC LIMIT ")
            .push_bind(fetch_limit);
        let rows: Vec<DispatchJobReadRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        rows.into_iter().map(DispatchJobRead::try_from).collect()
    }

    pub async fn insert_read_projection(&self, p: &DispatchJobRead) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO msg_dispatch_jobs_read
                (id, external_id, source, kind, code, subject, event_id, correlation_id,
                 target_url, protocol, client_id, subscription_id, service_account_id,
                 dispatch_pool_id, message_group, mode, sequence, status, attempt_count,
                 max_retries, last_error, timeout_seconds, retry_strategy, application,
                 subdomain, aggregate, created_at, updated_at, scheduled_for, expires_at,
                 completed_at, last_attempt_at, duration_millis, idempotency_key,
                 is_completed, is_terminal, projected_at, descriptor, metadata, queue)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                    $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26,
                    $27, $28, $29, $30, $31, $32, $33, $34, $35, $36, $37, $38, $39, $40)"#,
        )
        .bind(&p.id)
        .bind(&p.external_id)
        .bind(&p.source)
        .bind(p.kind.as_str())
        .bind(&p.code)
        .bind(&p.subject)
        .bind(&p.event_id)
        .bind(&p.correlation_id)
        .bind(&p.target_url)
        .bind(p.protocol.as_str())
        .bind(&p.client_id)
        .bind(&p.subscription_id)
        .bind(&p.service_account_id)
        .bind(&p.dispatch_pool_id)
        .bind(&p.message_group)
        .bind(p.mode.as_str())
        .bind(p.sequence)
        .bind(p.status.as_str())
        .bind(p.attempt_count as i32)
        .bind(p.max_retries as i32)
        .bind(&p.last_error)
        .bind(p.timeout_seconds as i32)
        .bind(p.retry_strategy.as_str())
        .bind(&p.application)
        .bind(&p.subdomain)
        .bind(&p.aggregate)
        .bind(p.created_at)
        .bind(p.updated_at)
        .bind(p.scheduled_for)
        .bind(p.expires_at)
        .bind(p.completed_at)
        .bind(p.last_attempt_at)
        .bind(p.duration_millis)
        .bind(&p.idempotency_key)
        .bind(p.is_completed)
        .bind(p.is_terminal)
        .bind(p.projected_at)
        .bind(&p.descriptor)
        .bind(serde_json::to_value(&p.metadata).unwrap_or_else(|_| serde_json::json!([])))
        .bind(&p.queue)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_read_projection(&self, p: &DispatchJobRead) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO msg_dispatch_jobs_read
                (id, external_id, source, kind, code, subject, event_id, correlation_id,
                 target_url, protocol, client_id, subscription_id, service_account_id,
                 dispatch_pool_id, message_group, mode, sequence, status, attempt_count,
                 max_retries, last_error, timeout_seconds, retry_strategy, application,
                 subdomain, aggregate, created_at, updated_at, scheduled_for, expires_at,
                 completed_at, last_attempt_at, duration_millis, idempotency_key,
                 is_completed, is_terminal, projected_at, descriptor, metadata, queue)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                    $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26,
                    $27, $28, $29, $30, $31, $32, $33, $34, $35, $36, $37, $38, $39, $40)
            ON CONFLICT (id, created_at) DO UPDATE SET
                status = EXCLUDED.status,
                attempt_count = EXCLUDED.attempt_count,
                last_error = EXCLUDED.last_error,
                updated_at = EXCLUDED.updated_at,
                completed_at = EXCLUDED.completed_at,
                last_attempt_at = EXCLUDED.last_attempt_at,
                duration_millis = EXCLUDED.duration_millis,
                is_completed = EXCLUDED.is_completed,
                is_terminal = EXCLUDED.is_terminal,
                projected_at = EXCLUDED.projected_at"#,
        )
        .bind(&p.id)
        .bind(&p.external_id)
        .bind(&p.source)
        .bind(p.kind.as_str())
        .bind(&p.code)
        .bind(&p.subject)
        .bind(&p.event_id)
        .bind(&p.correlation_id)
        .bind(&p.target_url)
        .bind(p.protocol.as_str())
        .bind(&p.client_id)
        .bind(&p.subscription_id)
        .bind(&p.service_account_id)
        .bind(&p.dispatch_pool_id)
        .bind(&p.message_group)
        .bind(p.mode.as_str())
        .bind(p.sequence)
        .bind(p.status.as_str())
        .bind(p.attempt_count as i32)
        .bind(p.max_retries as i32)
        .bind(&p.last_error)
        .bind(p.timeout_seconds as i32)
        .bind(p.retry_strategy.as_str())
        .bind(&p.application)
        .bind(&p.subdomain)
        .bind(&p.aggregate)
        .bind(p.created_at)
        .bind(p.updated_at)
        .bind(p.scheduled_for)
        .bind(p.expires_at)
        .bind(p.completed_at)
        .bind(p.last_attempt_at)
        .bind(p.duration_millis)
        .bind(&p.idempotency_key)
        .bind(p.is_completed)
        .bind(p.is_terminal)
        .bind(p.projected_at)
        .bind(&p.descriptor)
        .bind(serde_json::to_value(&p.metadata).unwrap_or_else(|_| serde_json::json!([])))
        .bind(&p.queue)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ── Counts ───────────────────────────────────────────────────────────

    pub async fn count_by_status(&self, status: DispatchStatus) -> Result<u64> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM msg_dispatch_jobs WHERE status = $1")
                .bind(status.as_str())
                .fetch_one(&self.pool)
                .await?;

        Ok(count as u64)
    }

    pub async fn count_all(&self) -> Result<u64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM msg_dispatch_jobs")
            .fetch_one(&self.pool)
            .await?;

        Ok(count as u64)
    }

    // ── Distinct filter values ───────────────────────────────────────────

    pub async fn find_distinct_subscription_ids(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT subscription_id FROM msg_dispatch_jobs \
             WHERE subscription_id IS NOT NULL ORDER BY subscription_id",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn find_distinct_event_type_codes(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT code FROM msg_dispatch_jobs \
             WHERE code IS NOT NULL AND code != '' ORDER BY code",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    // ── Read projection filter queries ──────────────────────────────────

    pub async fn find_distinct_applications(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT application FROM msg_dispatch_jobs_read \
             WHERE application IS NOT NULL AND application != '' ORDER BY application",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn find_distinct_subdomains(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT subdomain FROM msg_dispatch_jobs_read \
             WHERE subdomain IS NOT NULL AND subdomain != '' ORDER BY subdomain",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn find_distinct_aggregates(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT aggregate FROM msg_dispatch_jobs_read \
             WHERE aggregate IS NOT NULL AND aggregate != '' ORDER BY aggregate",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn find_distinct_codes(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT code FROM msg_dispatch_jobs_read \
             WHERE code IS NOT NULL AND code != '' ORDER BY code",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn find_distinct_statuses(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT status FROM msg_dispatch_jobs_read \
             WHERE status IS NOT NULL AND status != '' ORDER BY status",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn find_distinct_client_ids(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT client_id FROM msg_dispatch_jobs_read \
             WHERE client_id IS NOT NULL AND client_id != '' ORDER BY client_id",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    // ── Pagination ───────────────────────────────────────────────────────

    /// Cursor-paginated raw dispatch jobs. Keyset on `(created_at, id) DESC`.
    pub async fn find_recent_with_cursor(
        &self,
        cursor: Option<&DecodedCursor>,
        fetch_limit: i64,
    ) -> Result<Vec<DispatchJob>> {
        let rows = if let Some(c) = cursor {
            sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs \
                 WHERE (created_at, id) < ($1, $2) \
                 ORDER BY created_at DESC, id DESC LIMIT $3",
            )
            .bind(c.created_at)
            .bind(&c.id)
            .bind(fetch_limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs ORDER BY created_at DESC, id DESC LIMIT $1",
            )
            .bind(fetch_limit)
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter().map(DispatchJob::try_from).collect()
    }

    // ── Attempt tracking ─────────────────────────────────────────────────

    /// A job's recorded delivery attempts, oldest first, each with what was
    /// sent (`request_info`, Go's `RequestSummary`). The job row itself
    /// carries none: `find_by_id` leaves `DispatchJob::attempts` empty.
    pub async fn find_attempts(&self, dispatch_job_id: &str) -> Result<Vec<RecordedAttempt>> {
        let rows = sqlx::query_as::<_, AttemptRow>(
            "SELECT attempt_number, status, response_code, response_body, error_message, \
             error_type, duration_millis, attempted_at, completed_at, created_at, request_info \
             FROM msg_dispatch_job_attempts WHERE dispatch_job_id = $1 \
             ORDER BY created_at, attempt_number",
        )
        .bind(dispatch_job_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(RecordedAttempt::from).collect())
    }

    /// Record one delivery attempt in `msg_dispatch_job_attempts` (Go
    /// `RecordAttempt`).
    pub async fn insert_attempt(&self, attempt: &NewDispatchAttempt<'_>) -> Result<()> {
        let NewDispatchAttempt {
            dispatch_job_id,
            attempt_number,
            status,
            response_code,
            response_body,
            error_message,
            error_type,
            error_stack_trace,
            duration_millis,
            attempted_at,
            completed_at,
            request_info,
        } = *attempt;
        let id = tsid::generate_untyped();

        sqlx::query(
            r#"INSERT INTO msg_dispatch_job_attempts
                (id, dispatch_job_id, attempt_number, status, response_code,
                 response_body, error_message, error_type, error_stack_trace,
                 duration_millis, attempted_at, completed_at, created_at, request_info)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, NOW(), $13)"#,
        )
        .bind(&id)
        .bind(dispatch_job_id)
        .bind(attempt_number as i32)
        .bind(status.as_str())
        .bind(response_code.map(|c| c as i32))
        .bind(response_body)
        .bind(error_message)
        .bind(error_type.map(|t| t.as_str()))
        .bind(error_stack_trace)
        .bind(duration_millis)
        .bind(attempted_at)
        .bind(completed_at)
        .bind(request_info)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ── Delivery lifecycle (platform infrastructure; Go's repository) ─────
    //
    // Every status flip carries `created_at` alongside the id: the table is
    // partitioned on it, and the equality prunes to the row's partition.

    /// Atomically claim a job for one delivery: `PENDING/QUEUED →
    /// PROCESSING`. `false` means another delivery holds it (or it
    /// finished) and the caller must not call the subscriber (Go
    /// `ClaimForDelivery`).
    pub async fn claim_for_delivery(&self, id: &str, created_at: DateTime<Utc>) -> Result<bool> {
        Ok(lifecycle::claim_for_delivery(&self.pool, id, created_at).await?)
    }

    /// Take over a delivery whose attempt died: `PROCESSING → PROCESSING`
    /// with a fresh claim time, only when the current claim was made before
    /// `claimed_before` (the attempt's lease has run out). Like
    /// [`Self::claim_for_delivery`], the answer is "did I win?".
    pub async fn reclaim_stale_delivery(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        claimed_before: DateTime<Utc>,
    ) -> Result<bool> {
        Ok(lifecycle::reclaim_stale_delivery(&self.pool, id, created_at, claimed_before).await?)
    }

    /// `→ COMPLETED`, stamping `completed_at` and the attempt's duration.
    /// A job already settled is left alone (counted as a refused transition).
    pub async fn mark_completed(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        duration_millis: i64,
    ) -> Result<()> {
        lifecycle::complete(&self.pool, id, created_at, duration_millis).await?;
        Ok(())
    }

    /// `→ FAILED` (terminal), stamping `last_error`, `completed_at` and the
    /// duration. A job already settled is left alone.
    pub async fn mark_failed(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        last_error: &str,
        duration_millis: i64,
    ) -> Result<()> {
        lifecycle::fail(&self.pool, id, created_at, last_error, duration_millis).await?;
        Ok(())
    }

    /// A retryable failure: back to PENDING at `scheduled_for`, spending one
    /// attempt of the budget (Go `ScheduleRetry`).
    pub async fn schedule_retry(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        scheduled_for: DateTime<Utc>,
        last_error: &str,
    ) -> Result<()> {
        lifecycle::schedule_retry(&self.pool, id, created_at, scheduled_for, last_error).await?;
        Ok(())
    }

    /// A subscriber's cooperative deferral (`ack:false`, 429): back to
    /// PENDING at `scheduled_for` WITHOUT spending the budget (Go
    /// `Reschedule`).
    pub async fn defer(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        scheduled_for: DateTime<Utc>,
    ) -> Result<()> {
        lifecycle::defer(&self.pool, id, created_at, scheduled_for).await?;
        Ok(())
    }

    /// A job held behind its group at delivery time: back to PENDING at
    /// `scheduled_for` WITHOUT spending the budget.
    pub async fn hold(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        scheduled_for: DateTime<Utc>,
    ) -> Result<()> {
        lifecycle::hold(&self.pool, id, created_at, scheduled_for).await?;
        Ok(())
    }

    /// Whether an EARLIER job of `group` is holding it (failed, or in a
    /// retry backoff) — the delivery-time half of the scheduler's hold (Go
    /// `GroupHeldBefore`). Positional, so the holder itself is never held
    /// by its own presence. FAILED/ERROR holders are read from the jobs
    /// table (`idx_dispatch_jobs_status_group`), backoff holders from the
    /// queue table.
    pub async fn group_held_before(
        &self,
        group: &str,
        sequence: i32,
        created_at: DateTime<Utc>,
        id: &str,
    ) -> Result<bool> {
        Ok(lifecycle::group_held_before(&self.pool, group, sequence, created_at, id).await?)
    }

    /// The router's settled-message hook: reset `ids` still QUEUED or
    /// PROCESSING to PENDING, recording `reason`. Returns the ids reset (Go
    /// `SettleAcked`). No `created_at` is known, so this scans by id.
    pub async fn settle_acked(&self, ids: &[String], reason: &str) -> Result<Vec<String>> {
        let rows = lifecycle::return_settled(&self.pool, ids, reason).await?;
        Ok(rows.into_iter().map(|r| r.id).collect())
    }

    /// The reaper backstop: reset to PENDING every QUEUED/PROCESSING
    /// BLOCK_ON_ERROR job whose group is headed by an earlier FAILED/ERROR
    /// job. A PROCESSING row updated since `live_before` is presumed in
    /// flight and left alone (Go `SweepStrandedGroupSiblings`).
    pub async fn sweep_stranded_group_siblings(
        &self,
        live_before: DateTime<Utc>,
    ) -> Result<Vec<String>> {
        let rows = lifecycle::reap_stranded_siblings(&self.pool, live_before, REAP_REASON).await?;
        Ok(rows.into_iter().map(|r| r.id).collect())
    }
}
