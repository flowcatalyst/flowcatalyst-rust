//! The two writes an operator makes to `msg_dispatch_jobs` (Go
//! `Repository.Persist` for a reset or a status flip). The read projection
//! follows through `updated_at`, as for every other dispatch-job write.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::shared::error::{PlatformError, Result};
use crate::usecase::unit_of_work::HasId;

/// The facts an action decides on.
#[derive(Debug, Clone, sqlx::FromRow)]
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
    pub status: &'static str,
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
        Ok(sqlx::query_as::<_, JobHead>(
            "SELECT id, client_id, status, created_at FROM msg_dispatch_jobs WHERE id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?)
    }
}

#[async_trait]
impl crate::usecase::Persist<JobsRequeue> for DispatchJobActionsRepository {
    async fn persist(&self, r: &JobsRequeue, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        if r.jobs.is_empty() {
            return Ok(());
        }
        let ids: Vec<&str> = r.jobs.iter().map(|(id, _)| id.as_str()).collect();
        let created: Vec<DateTime<Utc>> = r.jobs.iter().map(|(_, c)| *c).collect();
        sqlx::query(
            "UPDATE msg_dispatch_jobs j SET status = 'PENDING', scheduled_for = NULL, \
                 attempt_count = 0, completed_at = NULL, duration_millis = NULL, \
                 last_error = NULL, queued_at = NULL, updated_at = NOW() \
             FROM UNNEST($1::text[], $2::timestamptz[]) AS u(id, created_at) \
             WHERE j.id = u.id AND j.created_at = u.created_at",
        )
        .bind(&ids)
        .bind(&created)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, _r: &JobsRequeue, _tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        Err(PlatformError::internal("a requeue is not deleted"))
    }
}

#[async_trait]
impl crate::usecase::Persist<JobStatusFlip> for DispatchJobActionsRepository {
    async fn persist(&self, f: &JobStatusFlip, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query(
            "UPDATE msg_dispatch_jobs SET status = $3, completed_at = NOW(), updated_at = NOW() \
             WHERE id = $1 AND created_at = $2",
        )
        .bind(&f.id)
        .bind(f.created_at)
        .bind(f.status)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, _f: &JobStatusFlip, _tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        Err(PlatformError::internal("a status change is not deleted"))
    }
}
