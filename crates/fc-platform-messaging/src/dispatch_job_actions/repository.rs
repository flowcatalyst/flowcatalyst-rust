//! The two writes an operator makes to `msg_dispatch_jobs` (Go
//! `Repository.Persist` for a reset or a status flip), each now an explicit
//! [`lifecycle`] operation. The read projection
//! follows through `updated_at`, as for every other dispatch-job write.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};
use std::collections::BTreeSet;

use crate::dispatch_job::entity::DispatchStatus;
use crate::dispatch_job::lifecycle;
use crate::dispatch_job::read_conditions::{push_read_conditions, ReadConditions};
use crate::dispatch_job::refs::{resolve_refs, Refs};
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::usecase::unit_of_work::HasId;
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::Persist;

/// The facts an action decides on.
#[derive(Debug, Clone)]
pub struct JobHead {
    pub id: String,
    pub client_id: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// Jobs back to PENDING with a fresh attempt budget (Go `ResetToPending`).
/// The ids in `redrive` are COMPLETED jobs re-delivered through the
/// lifecycle's separate redrive transition; every other job goes through the
/// ordinary requeue, which refuses a COMPLETED or in-flight one in SQL.
#[derive(Debug, Clone)]
pub struct JobsRequeue {
    pub jobs: Vec<(String, DateTime<Utc>)>,
    pub redrive: BTreeSet<String>,
}

impl HasId for JobsRequeue {
    fn id(&self) -> &str {
        "resent"
    }
}

/// A FAILED job settled by hand as CANCELLED or COMPLETED.
#[derive(Debug, Clone)]
pub struct JobStatusFlip {
    pub id: String,
    pub created_at: DateTime<Utc>,
    /// [`DispatchStatus::Cancelled`] or [`DispatchStatus::Completed`]: the two
    /// ways an operator settles a FAILED job.
    pub status: DispatchStatus,
}

impl HasId for JobStatusFlip {
    fn id(&self) -> &str {
        &self.id
    }
}

/// A job's payload and content type are replaced, its status untouched (Go
/// `EditPayloadPersister`).
#[derive(Debug, Clone)]
pub struct JobPayloadEdit {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub payload: String,
    pub content_type: Option<String>,
}

impl HasId for JobPayloadEdit {
    fn id(&self) -> &str {
        &self.id
    }
}

/// What the payload edit decides on: the stored status (spelled as stored:
/// `ERROR` is not `FAILED` here), scope and current payload.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JobEditView {
    pub id: String,
    pub client_id: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub payload: Option<String>,
    pub payload_content_type: String,
}

/// What a requeue-by-filter selects (Go `dispatchjob.FilterParams`): the
/// list's filter plus the two walk-only members. The match count and the id
/// walk build their WHERE in [`push_read_conditions`], the builder the list
/// shares, so they cannot
/// select different jobs.
#[derive(Debug, Default, Clone)]
pub struct MatchFilter {
    pub status: Option<String>,
    pub statuses: Vec<String>,
    /// Keep only jobs in one of these statuses, on top of `status`/`statuses`
    /// (both must hold). The id walk sets it to the statuses a requeue may
    /// reset, so the walk never hands over a job the status guard would skip.
    pub status_in: Vec<String>,
    pub client_id: Option<String>,
    pub client_ids: Vec<String>,
    /// Some: only platform-scoped jobs and these clients' (a non-anchor
    /// caller's visibility; `None` is an anchor: every job).
    pub accessible: Option<Vec<String>>,
    /// Drop platform-scoped jobs (`client_id IS NULL`): the id walk of a
    /// caller that may see those jobs but not requeue them.
    pub exclude_platform_scoped: bool,
    pub dispatch_pool_id: Option<String>,
    pub subscription_id: Option<String>,
    pub code: Option<String>,
    pub codes: Vec<String>,
    /// Jobs whose code is one of these (a job's code is its event type).
    pub event_types: Vec<String>,
    /// Jobs of any of these subscriptions.
    pub subscription_ids: Vec<String>,
    /// The by-name subscription and connection references, resolved to ids
    /// ([`DispatchJobActionsRepository::resolve_refs`]); unresolved ones match nothing.
    pub refs: Refs,
    pub source: Option<String>,
    pub message_group: Option<String>,
    pub applications: Vec<String>,
    pub subdomains: Vec<String>,
    pub aggregates: Vec<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    /// Keyset cursor of the id walk: only jobs whose id sorts after it.
    pub after_id: Option<String>,
}

/// One (status, client) group of the jobs a filter matches.
#[derive(Debug, Clone)]
pub struct MatchBucket {
    pub status: String,
    pub client_id: Option<String>,
    pub count: i64,
}

impl MatchFilter {
    /// The filter's WHERE conditions, in the form the shared builder takes
    /// (the dispatch-job list builds the same ones from its own filter): Go's
    /// `FilterParams.where`.
    fn conditions(&self) -> ReadConditions<'_> {
        ReadConditions {
            status: self.status.as_deref(),
            statuses: &self.statuses,
            status_in: &self.status_in,
            client_id: self.client_id.as_deref(),
            client_ids: &self.client_ids,
            accessible: self.accessible.as_deref(),
            exclude_platform_scoped: self.exclude_platform_scoped,
            dispatch_pool_id: self.dispatch_pool_id.as_deref(),
            subscription_id: self.subscription_id.as_deref(),
            code: self.code.as_deref(),
            codes: &self.codes,
            source: self.source.as_deref(),
            message_group: self.message_group.as_deref(),
            applications: &self.applications,
            subdomains: &self.subdomains,
            aggregates: &self.aggregates,
            event_types: &self.event_types,
            subscription_ids: &self.subscription_ids,
            refs: Some(&self.refs),
            since: self.since,
            until: self.until,
            after_id: self.after_id.as_deref(),
            ..ReadConditions::default()
        }
    }
}

pub struct DispatchJobActionsRepository {
    pool: PgPool,
}

impl DispatchJobActionsRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// The jobs `f` matches, grouped by status and client so a caller can apply
    /// its own per-client rule to each group (Go `CountMatches`). Reads the
    /// projection, like the list.
    pub async fn count_matches(&self, f: &MatchFilter) -> Result<Vec<MatchBucket>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT status, client_id, count(*) FROM msg_dispatch_jobs_read WHERE TRUE",
        );
        push_read_conditions(&mut qb, &f.conditions());
        qb.push(" GROUP BY status, client_id");
        let rows: Vec<(String, Option<String>, i64)> =
            qb.build_query_as().fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|(status, client_id, count)| MatchBucket {
                status,
                client_id,
                count,
            })
            .collect())
    }

    /// Up to `limit` ids of the jobs `f` matches, in id order, after
    /// `f.after_id` (Go `MatchingIDs`). Walking by id means a job that stops
    /// matching while the walk runs (it was just requeued) cannot shift the
    /// next page: every page starts strictly after the last id handed out.
    pub async fn matching_ids(&self, f: &MatchFilter, limit: i64) -> Result<Vec<String>> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT id FROM msg_dispatch_jobs_read WHERE TRUE");
        push_read_conditions(&mut qb, &f.conditions());
        qb.push(" ORDER BY id LIMIT ").push_bind(limit);
        let rows: Vec<(String,)> = qb.build_query_as().fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// The facts a payload edit decides on, read from the write table.
    pub async fn edit_view(&self, id: &str) -> Result<Option<JobEditView>> {
        Ok(sqlx::query_as::<_, JobEditView>(
            "SELECT id, client_id, status, created_at, payload, \
                    COALESCE(payload_content_type, 'application/json') AS payload_content_type \
               FROM msg_dispatch_jobs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Resolves the by-name subscription and connection references to ids,
    /// scoped to the platform's rows plus the caller's tenants (`None`: an anchor).
    pub async fn resolve_refs(&self, refs: Refs, scope: Option<&[String]>) -> Result<Refs> {
        resolve_refs(&self.pool, refs, scope).await
    }

    pub async fn heads(&self, ids: &[String]) -> Result<Vec<JobHead>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as!(
            JobHead,
            "SELECT id, client_id, status, created_at \
                    FROM msg_dispatch_jobs WHERE id = ANY($1)",
            ids
        )
        .fetch_all(&self.pool)
        .await?)
    }
}

impl Persist<JobsRequeue> for DispatchJobActionsRepository {
    async fn persist(&self, r: &JobsRequeue, tx: &mut DbTx<'_>) -> Result<()> {
        if r.jobs.is_empty() {
            return Ok(());
        }
        let (redrive, requeue): (Vec<_>, Vec<_>) = r
            .jobs
            .iter()
            .cloned()
            .partition(|(id, _)| r.redrive.contains(id));
        lifecycle::requeue(&mut **tx.inner, &requeue).await?;
        lifecycle::redrive_completed(&mut **tx.inner, &redrive).await?;
        Ok(())
    }

    async fn delete(&self, _r: &JobsRequeue, _tx: &mut DbTx<'_>) -> Result<()> {
        Err(PlatformError::internal("a requeue is not deleted"))
    }
}

impl Persist<JobStatusFlip> for DispatchJobActionsRepository {
    async fn persist(&self, f: &JobStatusFlip, tx: &mut DbTx<'_>) -> Result<()> {
        // The FAILED check is in the statement: a job that is not FAILED
        // any more (it moved between the use case's read and this write) is
        // left alone, and the unit of work rolls back rather than record an
        // event for a change that did not happen.
        let moved = match f.status {
            DispatchStatus::Cancelled => {
                lifecycle::operator_cancel(&mut **tx.inner, &f.id, f.created_at).await?
            }
            DispatchStatus::Completed => {
                lifecycle::operator_complete(&mut **tx.inner, &f.id, f.created_at).await?
            }
            other @ (DispatchStatus::Pending
            | DispatchStatus::Queued
            | DispatchStatus::Processing
            | DispatchStatus::Failed
            | DispatchStatus::Expired) => {
                return Err(PlatformError::internal(format!(
                    "a dispatch job is not settled by hand as {}",
                    other.as_str()
                )))
            }
        };
        if !moved {
            return Err(PlatformError::business_rule(
                "NOT_FAILED",
                "dispatch job is not FAILED; only a FAILED job can be overridden",
            ));
        }
        Ok(())
    }

    async fn delete(&self, _f: &JobStatusFlip, _tx: &mut DbTx<'_>) -> Result<()> {
        Err(PlatformError::internal("a status change is not deleted"))
    }
}

impl Persist<JobPayloadEdit> for DispatchJobActionsRepository {
    async fn persist(&self, e: &JobPayloadEdit, tx: &mut DbTx<'_>) -> Result<()> {
        // The status guard is in the statement: a job that moved (it was
        // claimed) between the use case's read and this write is left alone,
        // and the unit of work rolls back rather than record an event for a
        // change that did not happen.
        let done = lifecycle::edit_payload(
            &mut **tx.inner,
            &e.id,
            e.created_at,
            &e.payload,
            e.content_type.as_deref(),
        )
        .await?;
        if !done {
            return Err(PlatformError::business_rule(
                "NOT_EDITABLE",
                "dispatch job changed status while its payload was being edited; a payload can be edited only while the job is PENDING or FAILED",
            ));
        }
        Ok(())
    }

    async fn delete(&self, _e: &JobPayloadEdit, _tx: &mut DbTx<'_>) -> Result<()> {
        Err(PlatformError::internal("a payload edit is not deleted"))
    }
}
