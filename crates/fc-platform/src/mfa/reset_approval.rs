//! The lost-device reset approval queue (Go `internal/platform/resetapproval`):
//! a self-service reset for a user with no strong factor, under the stricter
//! reset policy, waits here for a client administrator. Like Go, that
//! policy is off by default (Go's `RequireStrongFactorForReset` is never
//! set), so requests arrive only from a database Go wrote; the admin routes
//! decide them. Decisions are authentication state, written directly, as
//! Go does.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::shared::error::Result;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ResetApprovalRequest {
    pub id: String,
    pub principal_id: String,
    pub client_id: Option<String>,
    pub status: String,
    pub reset_2fa: bool,
    pub note: Option<String>,
    pub decided_by: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// A decision on a pending request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Decision {
    Approved,
    Denied,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Decision::Approved => "APPROVED",
            Decision::Denied => "DENIED",
        }
    }
}

const COLUMNS: &str = "id, principal_id, client_id, status, reset_2fa, note, decided_by, \
     decided_at, expires_at, created_at";

pub struct ResetApprovalRepository {
    pool: PgPool,
}

impl ResetApprovalRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ResetApprovalRequest>> {
        Ok(sqlx::query_as::<_, ResetApprovalRequest>(&format!(
            "SELECT {COLUMNS} FROM iam_reset_approval_requests WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Pending, unexpired requests, oldest first: all of them (`None`), or
    /// those of `client_ids`.
    pub async fn list_pending(
        &self,
        client_ids: Option<&[String]>,
    ) -> Result<Vec<ResetApprovalRequest>> {
        let rows = match client_ids {
            None => {
                sqlx::query_as::<_, ResetApprovalRequest>(&format!(
                    "SELECT {COLUMNS} FROM iam_reset_approval_requests \
                     WHERE status = 'PENDING' AND expires_at > NOW() ORDER BY created_at"
                ))
                .fetch_all(&self.pool)
                .await?
            }
            Some(ids) => {
                sqlx::query_as::<_, ResetApprovalRequest>(&format!(
                    "SELECT {COLUMNS} FROM iam_reset_approval_requests \
                     WHERE status = 'PENDING' AND expires_at > NOW() \
                       AND client_id = ANY($1::varchar[]) ORDER BY created_at"
                ))
                .bind(ids)
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows)
    }

    /// Decide a pending, unexpired request; `false` when it no longer was
    /// (one guarded statement: a request is decided once).
    pub async fn decide(&self, id: &str, decision: Decision, decided_by: &str) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE iam_reset_approval_requests \
             SET status = $2, decided_by = $3, decided_at = NOW() \
             WHERE id = $1 AND status = 'PENDING' AND expires_at > NOW()",
        )
        .bind(id)
        .bind(decision.as_str())
        .bind(decided_by)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }
}
