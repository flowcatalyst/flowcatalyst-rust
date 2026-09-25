//! DispatchJob Repository — PostgreSQL via SQLx

use crate::dispatch_job::entity::{
    default_content_type, DispatchAttemptStatus, DispatchMetadata, ErrorType,
};
use crate::dispatch_job::entity::{parse_dispatch_mode, parse_dispatch_status};
use crate::shared::enum_str::{corrupt_value, decode};
use crate::shared::error::{PlatformError, Result};
use crate::{DispatchJob, DispatchJobRead, DispatchStatus};
use chrono::{DateTime, Utc};
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
            is_completed: r.is_completed.unwrap_or_default(),
            is_terminal: r.is_terminal.unwrap_or_default(),
            projected_at: r.projected_at,
        })
    }
}

// ─── Repository ──────────────────────────────────────────────────────────────

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

pub struct DispatchJobRepository {
    pool: PgPool,
}

impl DispatchJobRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, job: &DispatchJob) -> Result<()> {
        let metadata_json = serde_json::to_value(&job.metadata).unwrap_or_default();

        sqlx::query(
            r#"INSERT INTO msg_dispatch_jobs
                (id, external_id, source, kind, code, subject, event_id, correlation_id,
                 metadata, target_url, protocol, payload, payload_content_type, data_only,
                 service_account_id, client_id, subscription_id, mode, dispatch_pool_id,
                 message_group, sequence, timeout_seconds, schema_id, status, max_retries,
                 retry_strategy, scheduled_for, expires_at, attempt_count, last_attempt_at,
                 completed_at, duration_millis, last_error, idempotency_key, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                    $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26,
                    $27, $28, $29, $30, $31, $32, $33, $34, $35, $36)"#,
        )
        .bind(&job.id)
        .bind(&job.external_id)
        .bind(&job.source)
        .bind(job.kind.as_str())
        .bind(&job.code)
        .bind(&job.subject)
        .bind(&job.event_id)
        .bind(&job.correlation_id)
        .bind(&metadata_json)
        .bind(&job.target_url)
        .bind(job.protocol.as_str())
        .bind(&job.payload)
        .bind(Some(&job.payload_content_type))
        .bind(job.data_only)
        .bind(&job.service_account_id)
        .bind(&job.client_id)
        .bind(&job.subscription_id)
        .bind(job.mode.as_str())
        .bind(&job.dispatch_pool_id)
        .bind(&job.message_group)
        .bind(job.sequence)
        .bind(job.timeout_seconds as i32)
        .bind(&job.schema_id)
        .bind(job.status.as_str())
        .bind(job.max_retries as i32)
        .bind(job.retry_strategy.as_str())
        .bind(job.scheduled_for)
        .bind(job.expires_at)
        .bind(job.attempt_count as i32)
        .bind(job.last_attempt_at)
        .bind(job.completed_at)
        .bind(job.duration_millis)
        .bind(&job.last_error)
        .bind(&job.idempotency_key)
        .bind(job.created_at)
        .bind(job.updated_at)
        .execute(&self.pool)
        .await?;

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

    pub async fn find_pending_for_dispatch(&self, limit: i64) -> Result<Vec<DispatchJob>> {
        let now = Utc::now();
        if limit > 0 {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs \
                 WHERE status = 'PENDING' AND (scheduled_for IS NULL OR scheduled_for <= $1) \
                 LIMIT $2",
            )
            .bind(now)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        } else {
            let rows = sqlx::query_as::<_, DispatchJobRow>(
                "SELECT * FROM msg_dispatch_jobs \
                 WHERE status = 'PENDING' AND (scheduled_for IS NULL OR scheduled_for <= $1)",
            )
            .bind(now)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(DispatchJob::try_from).collect()
        }
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

    pub async fn update(&self, job: &DispatchJob) -> Result<()> {
        let metadata_json = serde_json::to_value(&job.metadata).unwrap_or_default();

        sqlx::query(
            r#"UPDATE msg_dispatch_jobs SET
                external_id = $2, source = $3, kind = $4, code = $5, subject = $6,
                event_id = $7, correlation_id = $8, metadata = $9, target_url = $10,
                protocol = $11, payload = $12, payload_content_type = $13, data_only = $14,
                service_account_id = $15, client_id = $16, subscription_id = $17, mode = $18,
                dispatch_pool_id = $19, message_group = $20, sequence = $21,
                timeout_seconds = $22, schema_id = $23, status = $24, max_retries = $25,
                retry_strategy = $26, scheduled_for = $27, expires_at = $28,
                attempt_count = $29, last_attempt_at = $30, completed_at = $31,
                duration_millis = $32, last_error = $33, idempotency_key = $34,
                updated_at = $35
            WHERE id = $1 AND created_at = $36"#,
        )
        .bind(&job.id)
        .bind(&job.external_id)
        .bind(&job.source)
        .bind(job.kind.as_str())
        .bind(&job.code)
        .bind(&job.subject)
        .bind(&job.event_id)
        .bind(&job.correlation_id)
        .bind(&metadata_json)
        .bind(&job.target_url)
        .bind(job.protocol.as_str())
        .bind(&job.payload)
        .bind(Some(&job.payload_content_type))
        .bind(job.data_only)
        .bind(&job.service_account_id)
        .bind(&job.client_id)
        .bind(&job.subscription_id)
        .bind(job.mode.as_str())
        .bind(&job.dispatch_pool_id)
        .bind(&job.message_group)
        .bind(job.sequence)
        .bind(job.timeout_seconds as i32)
        .bind(&job.schema_id)
        .bind(job.status.as_str())
        .bind(job.max_retries as i32)
        .bind(job.retry_strategy.as_str())
        .bind(job.scheduled_for)
        .bind(job.expires_at)
        .bind(job.attempt_count as i32)
        .bind(job.last_attempt_at)
        .bind(job.completed_at)
        .bind(job.duration_millis)
        .bind(&job.last_error)
        .bind(&job.idempotency_key)
        .bind(job.updated_at)
        .bind(job.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
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
        if jobs.is_empty() {
            return Ok(());
        }

        let mut ids = Vec::with_capacity(jobs.len());
        let mut external_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut sources: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut kinds = Vec::with_capacity(jobs.len());
        let mut codes = Vec::with_capacity(jobs.len());
        let mut subjects: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut event_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut correlation_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut metadatas: Vec<serde_json::Value> = Vec::with_capacity(jobs.len());
        let mut target_urls = Vec::with_capacity(jobs.len());
        let mut protocols = Vec::with_capacity(jobs.len());
        let mut payloads: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut payload_content_types: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut data_onlys = Vec::with_capacity(jobs.len());
        let mut service_account_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut client_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut subscription_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut modes = Vec::with_capacity(jobs.len());
        let mut dispatch_pool_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut message_groups: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut sequences = Vec::with_capacity(jobs.len());
        let mut timeout_secs = Vec::with_capacity(jobs.len());
        let mut schema_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut statuses = Vec::with_capacity(jobs.len());
        let mut max_retries_vec = Vec::with_capacity(jobs.len());
        let mut retry_strategies = Vec::with_capacity(jobs.len());
        let mut scheduled_fors: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(jobs.len());
        let mut expires_ats: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(jobs.len());
        let mut attempt_counts = Vec::with_capacity(jobs.len());
        let mut last_attempt_ats: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(jobs.len());
        let mut completed_ats: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(jobs.len());
        let mut duration_milliss: Vec<Option<i64>> = Vec::with_capacity(jobs.len());
        let mut last_errors: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut idempotency_keys: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut created_ats = Vec::with_capacity(jobs.len());
        let mut updated_ats = Vec::with_capacity(jobs.len());

        for job in jobs {
            ids.push(job.id.as_str());
            external_ids.push(job.external_id.clone());
            sources.push(job.source.clone());
            kinds.push(job.kind.as_str());
            codes.push(job.code.as_str());
            subjects.push(job.subject.clone());
            event_ids.push(job.event_id.clone());
            correlation_ids.push(job.correlation_id.clone());
            metadatas.push(serde_json::to_value(&job.metadata).unwrap_or_default());
            target_urls.push(job.target_url.as_str());
            protocols.push(job.protocol.as_str());
            payloads.push(job.payload.clone());
            payload_content_types.push(Some(job.payload_content_type.clone()));
            data_onlys.push(job.data_only);
            service_account_ids.push(job.service_account_id.clone());
            client_ids.push(job.client_id.clone());
            subscription_ids.push(job.subscription_id.clone());
            modes.push(job.mode.as_str());
            dispatch_pool_ids.push(job.dispatch_pool_id.clone());
            message_groups.push(job.message_group.clone());
            sequences.push(job.sequence);
            timeout_secs.push(job.timeout_seconds as i32);
            schema_ids.push(job.schema_id.clone());
            statuses.push(job.status.as_str());
            max_retries_vec.push(job.max_retries as i32);
            retry_strategies.push(job.retry_strategy.as_str());
            scheduled_fors.push(job.scheduled_for);
            expires_ats.push(job.expires_at);
            attempt_counts.push(job.attempt_count as i32);
            last_attempt_ats.push(job.last_attempt_at);
            completed_ats.push(job.completed_at);
            duration_milliss.push(job.duration_millis);
            last_errors.push(job.last_error.clone());
            idempotency_keys.push(job.idempotency_key.clone());
            created_ats.push(job.created_at);
            updated_ats.push(job.updated_at);
        }

        sqlx::query(
            r#"INSERT INTO msg_dispatch_jobs
                (id, external_id, source, kind, code, subject, event_id, correlation_id,
                 metadata, target_url, protocol, payload, payload_content_type, data_only,
                 service_account_id, client_id, subscription_id, mode, dispatch_pool_id,
                 message_group, sequence, timeout_seconds, schema_id, status, max_retries,
                 retry_strategy, scheduled_for, expires_at, attempt_count, last_attempt_at,
                 completed_at, duration_millis, last_error, idempotency_key, created_at, updated_at)
            SELECT * FROM UNNEST(
                $1::varchar[], $2::varchar[], $3::varchar[], $4::varchar[], $5::varchar[],
                $6::varchar[], $7::varchar[], $8::varchar[], $9::jsonb[], $10::varchar[],
                $11::varchar[], $12::text[], $13::varchar[], $14::bool[],
                $15::varchar[], $16::varchar[], $17::varchar[], $18::varchar[], $19::varchar[],
                $20::varchar[], $21::int4[], $22::int4[], $23::varchar[], $24::varchar[],
                $25::int4[], $26::varchar[], $27::timestamptz[], $28::timestamptz[],
                $29::int4[], $30::timestamptz[], $31::timestamptz[], $32::int8[],
                $33::varchar[], $34::varchar[], $35::timestamptz[], $36::timestamptz[]
            )"#,
        )
        .bind(&ids)
        .bind(&external_ids as &[Option<String>])
        .bind(&sources as &[Option<String>])
        .bind(&kinds)
        .bind(&codes)
        .bind(&subjects as &[Option<String>])
        .bind(&event_ids as &[Option<String>])
        .bind(&correlation_ids as &[Option<String>])
        .bind(&metadatas)
        .bind(&target_urls)
        .bind(&protocols)
        .bind(&payloads as &[Option<String>])
        .bind(&payload_content_types as &[Option<String>])
        .bind(&data_onlys)
        .bind(&service_account_ids as &[Option<String>])
        .bind(&client_ids as &[Option<String>])
        .bind(&subscription_ids as &[Option<String>])
        .bind(&modes)
        .bind(&dispatch_pool_ids as &[Option<String>])
        .bind(&message_groups as &[Option<String>])
        .bind(&sequences)
        .bind(&timeout_secs)
        .bind(&schema_ids as &[Option<String>])
        .bind(&statuses)
        .bind(&max_retries_vec)
        .bind(&retry_strategies)
        .bind(&scheduled_fors as &[Option<DateTime<Utc>>])
        .bind(&expires_ats as &[Option<DateTime<Utc>>])
        .bind(&attempt_counts)
        .bind(&last_attempt_ats as &[Option<DateTime<Utc>>])
        .bind(&completed_ats as &[Option<DateTime<Utc>>])
        .bind(&duration_milliss as &[Option<i64>])
        .bind(&last_errors as &[Option<String>])
        .bind(&idempotency_keys as &[Option<String>])
        .bind(&created_ats)
        .bind(&updated_ats)
        .execute(executor)
        .await?;

        Ok(())
    }

    /// Update status by primary key. `created_at` is required because
    /// `msg_dispatch_jobs` is partitioned on it — including it in the WHERE
    /// clause lets PG prune to a single partition instead of scanning all
    /// active partition PK indexes.
    pub async fn update_status(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        status: DispatchStatus,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE msg_dispatch_jobs SET status = $1, updated_at = NOW() \
             WHERE id = $2 AND created_at = $3",
        )
        .bind(status.as_str())
        .bind(id)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
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
        cursor: Option<&crate::shared::api_common::DecodedCursor>,
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
                 is_completed, is_terminal, projected_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                    $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26,
                    $27, $28, $29, $30, $31, $32, $33, $34, $35, $36, $37)"#,
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
                 is_completed, is_terminal, projected_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                    $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26,
                    $27, $28, $29, $30, $31, $32, $33, $34, $35, $36, $37)
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
        cursor: Option<&crate::shared::api_common::DecodedCursor>,
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
        let id = crate::shared::tsid::generate_untyped();

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
        let r = sqlx::query(
            "UPDATE msg_dispatch_jobs \
                SET status = 'PROCESSING', last_attempt_at = NOW(), updated_at = NOW() \
              WHERE id = $1 AND created_at = $2 AND status IN ('PENDING', 'QUEUED')",
        )
        .bind(id)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Take over a delivery whose attempt died: `PROCESSING → PROCESSING`
    /// with a fresh claim time, only when the current claim was made before
    /// `claimed_before` (the attempt's lease has run out). Like
    /// [`Self::claim_for_delivery`], the affected-row count answers "did I
    /// win?": the winner's new claim time takes every other taker out of
    /// the condition.
    pub async fn reclaim_stale_delivery(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        claimed_before: DateTime<Utc>,
    ) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE msg_dispatch_jobs \
                SET last_attempt_at = NOW(), updated_at = NOW() \
              WHERE id = $1 AND created_at = $2 AND status = 'PROCESSING' \
                AND COALESCE(last_attempt_at, updated_at) < $3",
        )
        .bind(id)
        .bind(created_at)
        .bind(claimed_before)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }

    /// `→ COMPLETED`, stamping `completed_at` and the attempt's duration.
    pub async fn mark_completed(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        duration_millis: i64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE msg_dispatch_jobs \
                SET status = 'COMPLETED', completed_at = NOW(), duration_millis = $3, \
                    updated_at = NOW() \
              WHERE id = $1 AND created_at = $2",
        )
        .bind(id)
        .bind(created_at)
        .bind(duration_millis)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `→ FAILED` (terminal), stamping `last_error`, `completed_at` and the
    /// duration.
    pub async fn mark_failed(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        last_error: &str,
        duration_millis: i64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE msg_dispatch_jobs \
                SET status = 'FAILED', completed_at = NOW(), duration_millis = $3, \
                    last_error = $4, updated_at = NOW() \
              WHERE id = $1 AND created_at = $2",
        )
        .bind(id)
        .bind(created_at)
        .bind(duration_millis)
        .bind(last_error)
        .execute(&self.pool)
        .await?;
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
        sqlx::query(
            "UPDATE msg_dispatch_jobs \
                SET attempt_count = attempt_count + 1, scheduled_for = $3, last_error = $4, \
                    last_attempt_at = NOW(), status = 'PENDING', queued_at = NULL, \
                    updated_at = NOW() \
              WHERE id = $1 AND created_at = $2",
        )
        .bind(id)
        .bind(created_at)
        .bind(scheduled_for)
        .bind(last_error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Back to PENDING at `scheduled_for` WITHOUT spending the budget: a
    /// subscriber's cooperative deferral (`ack:false`, 429), or a job held
    /// behind its group at delivery time (Go `Reschedule`).
    pub async fn reschedule(
        &self,
        id: &str,
        created_at: DateTime<Utc>,
        scheduled_for: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE msg_dispatch_jobs \
                SET status = 'PENDING', scheduled_for = $3, queued_at = NULL, updated_at = NOW() \
              WHERE id = $1 AND created_at = $2",
        )
        .bind(id)
        .bind(created_at)
        .bind(scheduled_for)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Whether an EARLIER job of `group` is holding it (failed, or in a
    /// retry backoff) — the delivery-time half of the scheduler's hold (Go
    /// `GroupHeldBefore`). Positional, so the holder itself is never held
    /// by its own presence.
    pub async fn group_held_before(
        &self,
        group: &str,
        sequence: i32,
        created_at: DateTime<Utc>,
        id: &str,
    ) -> Result<bool> {
        let sql = format!(
            "SELECT EXISTS (SELECT 1 FROM msg_dispatch_jobs \
              WHERE message_group = $1 AND ({}) \
                AND (sequence, created_at, id) < ($2, $3, $4))",
            crate::scheduler::GROUP_HOLDING_STATUS_SQL
        );
        let (held,): (bool,) = sqlx::query_as(&sql)
            .bind(group)
            .bind(sequence)
            .bind(created_at)
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        Ok(held)
    }

    /// The router's settled-message hook: reset `ids` still QUEUED or
    /// PROCESSING to PENDING, recording `reason`. Returns the ids reset (Go
    /// `SettleAcked`). No `created_at` is known, so this scans by id.
    pub async fn settle_acked(&self, ids: &[String], reason: &str) -> Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(String,)> = sqlx::query_as(
            "UPDATE msg_dispatch_jobs \
                SET status = 'PENDING', scheduled_for = NULL, queued_at = NULL, \
                    last_error = $2, updated_at = NOW() \
              WHERE id = ANY($1) AND status IN ('QUEUED', 'PROCESSING') \
             RETURNING id",
        )
        .bind(ids)
        .bind(reason)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// The reaper backstop: reset to PENDING every QUEUED/PROCESSING
    /// BLOCK_ON_ERROR job whose group is headed by an earlier FAILED/ERROR
    /// job. A PROCESSING row updated since `live_before` is presumed in
    /// flight and left alone (Go `SweepStrandedGroupSiblings`).
    pub async fn sweep_stranded_group_siblings(
        &self,
        live_before: DateTime<Utc>,
    ) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "WITH stranded AS ( \
                 SELECT s.id, s.created_at \
                   FROM msg_dispatch_jobs s \
                   JOIN msg_dispatch_jobs h \
                     ON h.message_group = s.message_group \
                    AND h.status IN ('FAILED', 'ERROR') \
                    AND (h.sequence, h.created_at, h.id) < (s.sequence, s.created_at, s.id) \
                  WHERE s.mode = 'BLOCK_ON_ERROR' \
                    AND s.message_group IS NOT NULL \
                    AND s.status IN ('QUEUED', 'PROCESSING') \
                    AND (s.status <> 'PROCESSING' OR s.updated_at < $1) \
             ) \
             UPDATE msg_dispatch_jobs j \
                SET status = 'PENDING', scheduled_for = NULL, queued_at = NULL, \
                    last_error = $2, updated_at = NOW() \
               FROM stranded st \
              WHERE j.id = st.id AND j.created_at = st.created_at \
             RETURNING j.id",
        )
        .bind(live_before)
        .bind(REAP_REASON)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }
}
