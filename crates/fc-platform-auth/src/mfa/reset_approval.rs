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

use fc_platform_core::shared::enum_str::{corrupt_value, decode, str_enum};
use fc_platform_core::shared::error::{PlatformError, Result};

/// Where a request stands (Go `resetapproval.Status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ResetApprovalStatus {
    Pending,
    Approved,
    Denied,
    Expired,
}

str_enum!(ResetApprovalStatus, "reset approval status", {
    Pending => "PENDING",
    Approved => "APPROVED",
    Denied => "DENIED",
    Expired => "EXPIRED",
});

/// Who decided a request, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    pub by: String,
    pub at: DateTime<Utc>,
}

/// A request's state. A decision carries the decider and time, so an
/// approved request with no decider cannot be built; pending and expired
/// requests carry neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetApprovalState {
    Pending,
    Approved(Decided),
    Denied(Decided),
    Expired,
}

impl ResetApprovalState {
    pub fn status(&self) -> ResetApprovalStatus {
        match self {
            Self::Pending => ResetApprovalStatus::Pending,
            Self::Approved(_) => ResetApprovalStatus::Approved,
            Self::Denied(_) => ResetApprovalStatus::Denied,
            Self::Expired => ResetApprovalStatus::Expired,
        }
    }
}

/// A queued lost-device reset. Fields are private and the only way to get one
/// is a stored row that passes [`ResetApprovalState`]'s invariants; a decision
/// is written by [`ResetApprovalRepository::decide`], never on the entity.
#[derive(Debug, Clone)]
pub struct ResetApprovalRequest {
    id: String,
    principal_id: String,
    client_id: Option<String>,
    reset_2fa: bool,
    note: Option<String>,
    state: ResetApprovalState,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl ResetApprovalRequest {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn principal_id(&self) -> &str {
        &self.principal_id
    }
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }
    pub fn reset_2fa(&self) -> bool {
        self.reset_2fa
    }
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
    pub fn state(&self) -> &ResetApprovalState {
        &self.state
    }
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

#[derive(sqlx::FromRow)]
struct ResetApprovalRow {
    id: String,
    principal_id: String,
    client_id: Option<String>,
    status: String,
    reset_2fa: bool,
    note: Option<String>,
    decided_by: Option<String>,
    decided_at: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl TryFrom<ResetApprovalRow> for ResetApprovalRequest {
    type Error = PlatformError;
    fn try_from(r: ResetApprovalRow) -> Result<Self> {
        let status: ResetApprovalStatus =
            decode(&r.status, "iam_reset_approval_requests", "status", &r.id)?;
        let state = match (status, r.decided_by, r.decided_at) {
            (ResetApprovalStatus::Pending, ..) => ResetApprovalState::Pending,
            (ResetApprovalStatus::Expired, ..) => ResetApprovalState::Expired,
            (ResetApprovalStatus::Approved, Some(by), Some(at)) => {
                ResetApprovalState::Approved(Decided { by, at })
            }
            (ResetApprovalStatus::Denied, Some(by), Some(at)) => {
                ResetApprovalState::Denied(Decided { by, at })
            }
            (decided, ..) => {
                return Err(corrupt_value(
                    "iam_reset_approval_requests",
                    "decided_by/decided_at",
                    &format!("{} without a decider and time", decided.as_str()),
                    &r.id,
                ))
            }
        };
        Ok(Self {
            id: r.id,
            principal_id: r.principal_id,
            client_id: r.client_id,
            reset_2fa: r.reset_2fa,
            note: r.note,
            state,
            expires_at: r.expires_at,
            created_at: r.created_at,
        })
    }
}

/// A decision on a pending request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Decision {
    Approved,
    Denied,
}

impl Decision {
    fn status(self) -> ResetApprovalStatus {
        match self {
            Decision::Approved => ResetApprovalStatus::Approved,
            Decision::Denied => ResetApprovalStatus::Denied,
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
        sqlx::query_as::<_, ResetApprovalRow>(&format!(
            "SELECT {COLUMNS} FROM iam_reset_approval_requests WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .map(ResetApprovalRequest::try_from)
        .transpose()
    }

    /// Pending, unexpired requests, oldest first: all of them (`None`), or
    /// those of `client_ids`.
    pub async fn list_pending(
        &self,
        client_ids: Option<&[String]>,
    ) -> Result<Vec<ResetApprovalRequest>> {
        let rows = match client_ids {
            None => {
                sqlx::query_as::<_, ResetApprovalRow>(&format!(
                    "SELECT {COLUMNS} FROM iam_reset_approval_requests \
                     WHERE status = 'PENDING' AND expires_at > NOW() ORDER BY created_at"
                ))
                .fetch_all(&self.pool)
                .await?
            }
            Some(ids) => {
                sqlx::query_as::<_, ResetApprovalRow>(&format!(
                    "SELECT {COLUMNS} FROM iam_reset_approval_requests \
                     WHERE status = 'PENDING' AND expires_at > NOW() \
                       AND client_id = ANY($1::varchar[]) ORDER BY created_at"
                ))
                .bind(ids)
                .fetch_all(&self.pool)
                .await?
            }
        };
        rows.into_iter()
            .map(ResetApprovalRequest::try_from)
            .collect()
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
        .bind(decision.status().as_str())
        .bind(decided_by)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        status: &str,
        decided_by: Option<&str>,
        decided_at: Option<DateTime<Utc>>,
    ) -> ResetApprovalRow {
        ResetApprovalRow {
            id: "rar_1".to_string(),
            principal_id: "prn_1".to_string(),
            client_id: None,
            status: status.to_string(),
            reset_2fa: false,
            note: None,
            decided_by: decided_by.map(str::to_string),
            decided_at,
            expires_at: Utc::now(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn pending_and_expired_need_no_decider() {
        let p = ResetApprovalRequest::try_from(row("PENDING", None, None)).unwrap();
        assert_eq!(p.state(), &ResetApprovalState::Pending);
        let e = ResetApprovalRequest::try_from(row("EXPIRED", None, None)).unwrap();
        assert_eq!(e.state().status(), ResetApprovalStatus::Expired);
    }

    #[test]
    fn a_decision_carries_its_decider_and_time() {
        let at = Utc::now();
        let a = ResetApprovalRequest::try_from(row("APPROVED", Some("prn_9"), Some(at))).unwrap();
        assert_eq!(
            a.state(),
            &ResetApprovalState::Approved(Decided {
                by: "prn_9".to_string(),
                at
            })
        );
        let d = ResetApprovalRequest::try_from(row("DENIED", Some("prn_9"), Some(at))).unwrap();
        assert_eq!(d.state().status(), ResetApprovalStatus::Denied);
    }

    #[test]
    fn a_decision_without_decider_or_time_is_a_corrupt_row() {
        assert!(ResetApprovalRequest::try_from(row("APPROVED", None, Some(Utc::now()))).is_err());
        assert!(ResetApprovalRequest::try_from(row("DENIED", Some("prn_9"), None)).is_err());
    }

    #[test]
    fn an_unknown_status_is_a_corrupt_row() {
        assert!(ResetApprovalRequest::try_from(row("MAYBE", None, None)).is_err());
    }
}
