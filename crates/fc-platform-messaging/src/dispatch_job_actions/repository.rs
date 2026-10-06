//! The two writes an operator makes to `msg_dispatch_jobs` (Go
//! `Repository.Persist` for a reset or a status flip), each now an explicit
//! [`lifecycle`] operation. The read projection
//! follows through `updated_at`, as for every other dispatch-job write.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::dispatch_job::entity::DispatchStatus;
use crate::dispatch_job::lifecycle;
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
#[derive(Debug, Clone)]
pub struct JobsRequeue {
    pub jobs: Vec<(String, DateTime<Utc>)>,
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

pub struct DispatchJobActionsRepository {
    pool: PgPool,
}

impl DispatchJobActionsRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
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
        lifecycle::requeue(&mut **tx.inner, &r.jobs).await?;
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
