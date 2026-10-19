//! Never demote the last all-applications administrator (Go
//! `principal/last_admin.go`, commit `9966a2fa`).
//!
//! An all-applications administrator is an ACTIVE principal of type USER,
//! tier ANCHOR, with the all-applications flag set. Service principals never
//! count: the point of the role is a human who can grant all-applications
//! access. A write that would leave none (assign application access,
//! deactivate, update, delete, scope change, platform user sync) is refused
//! with `409 LAST_ALL_APPLICATIONS_ADMIN`.
//!
//! Every principal write funnels through the repository's `Persist`, whichever
//! use case wraps it, so the rule is checked there, in the write's own
//! transaction, with one rule for all of them. The check is race-safe: after a
//! cheap unlocked probe it takes a transaction-scoped advisory lock, re-reads
//! the stored row and counts the others inside the same transaction, so two
//! administrators demoting each other concurrently serialise and the second is
//! refused (READ COMMITTED gives the statements after the lock a fresh
//! snapshot). The lock is taken only when the stored row IS an administrator and
//! the incoming state is not, so unrelated principal writes never contend on it.

use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::usecase::DbTx;

use super::entity::{Principal, PrincipalType, UserScope};

/// The `BUSINESS_RULE` code of the refusal.
pub const CODE_LAST_ALL_APPLICATIONS_ADMIN: &str = "LAST_ALL_APPLICATIONS_ADMIN";

/// The fixed `pg_advisory_xact_lock` key serialising every write that can
/// reduce the set of all-applications administrators ("FCADMIN" + 1).
const LAST_ADMIN_LOCK_KEY: i64 = 0x4643_4144_4d49_4e01;

/// The row predicate of an administrator (the same rule as
/// [`Principal::is_all_applications_admin`]).
const IS_ADMIN_SQL: &str = "type = 'USER' AND scope = 'ANCHOR' AND active AND all_applications";

impl Principal {
    /// Whether this is an all-applications administrator: an ACTIVE USER of
    /// tier ANCHOR with the all-applications flag.
    pub fn is_all_applications_admin(&self) -> bool {
        self.principal_type == PrincipalType::User
            && self.scope == UserScope::Anchor
            && self.active
            && self.all_applications
    }
}

fn refusal(verb: &str) -> PlatformError {
    PlatformError::business_rule(
        CODE_LAST_ALL_APPLICATIONS_ADMIN,
        format!(
            "This would {verb} the last active all-applications administrator, and nobody could then grant \
             all-applications access. Grant all-applications access to another active anchor user first."
        ),
    )
}

/// Refuse a write of `p` that would demote the last all-applications
/// administrator (Go `guardLastAdminOnPersist`). Not a reduction when `p` is
/// itself an administrator.
pub async fn guard_on_persist(tx: &mut DbTx<'_>, p: &Principal) -> Result<()> {
    if p.is_all_applications_admin() {
        return Ok(());
    }
    assert_another_admin(
        tx,
        p.id.as_str(),
        "remove all-applications administrator status from",
    )
    .await
}

/// Refuse the deletion of the last all-applications administrator (Go
/// `guardLastAdminOnDelete`).
pub async fn guard_on_delete(tx: &mut DbTx<'_>, p: &Principal) -> Result<()> {
    assert_another_admin(tx, p.id.as_str(), "delete").await
}

/// Refuse a platform user sync batch that would leave no administrator (Go
/// persists a sync's principals one at a time, so the second demotion of the
/// last two is refused; the batch is one statement here, so the end state is
/// checked instead: the administrators outside the batch plus the batch's own
/// incoming administrators).
pub async fn guard_on_batch(tx: &mut DbTx<'_>, batch: &[Principal]) -> Result<()> {
    let demoted: Vec<&str> = batch
        .iter()
        .filter(|p| !p.is_all_applications_admin())
        .map(|p| p.id.as_str())
        .collect();
    if demoted.is_empty() {
        return Ok(());
    }
    let probe = format!(
        "SELECT EXISTS (SELECT 1 FROM iam_principals WHERE id = ANY($1) AND {IS_ADMIN_SQL})"
    );
    let stored_admin: bool = sqlx::query_scalar(&probe)
        .bind(&demoted)
        .fetch_one(&mut **tx.inner)
        .await?;
    if !stored_admin {
        return Ok(());
    }
    lock(tx).await?;
    // Re-read under the lock: a concurrent writer may have demoted these rows.
    let stored_admin: bool = sqlx::query_scalar(&probe)
        .bind(&demoted)
        .fetch_one(&mut **tx.inner)
        .await?;
    if !stored_admin {
        return Ok(());
    }
    let batch_ids: Vec<&str> = batch.iter().map(|p| p.id.as_str()).collect();
    let outside: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM iam_principals WHERE id <> ALL($1) AND {IS_ADMIN_SQL}"
    ))
    .bind(&batch_ids)
    .fetch_one(&mut **tx.inner)
    .await?;
    let incoming = batch
        .iter()
        .filter(|p| p.is_all_applications_admin())
        .count() as i64;
    if outside + incoming > 0 {
        return Ok(());
    }
    Err(refusal("remove all-applications administrator status from"))
}

async fn lock(tx: &mut DbTx<'_>) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(LAST_ADMIN_LOCK_KEY)
        .execute(&mut **tx.inner)
        .await?;
    Ok(())
}

/// Refuse when the STORED principal `id` is an all-applications administrator
/// and no other one exists (Go `assertAnotherAdmin`).
async fn assert_another_admin(tx: &mut DbTx<'_>, id: &str, verb: &str) -> Result<()> {
    let is_admin = format!("SELECT {IS_ADMIN_SQL} FROM iam_principals WHERE id = $1");
    let admin: Option<bool> = sqlx::query_scalar(&is_admin)
        .bind(id)
        .fetch_optional(&mut **tx.inner)
        .await?;
    if admin != Some(true) {
        return Ok(());
    }
    lock(tx).await?;
    // Re-read under the lock: a concurrent writer may have demoted this row.
    let admin: Option<bool> = sqlx::query_scalar(&is_admin)
        .bind(id)
        .fetch_optional(&mut **tx.inner)
        .await?;
    if admin != Some(true) {
        return Ok(());
    }
    let others: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM iam_principals WHERE id <> $1 AND {IS_ADMIN_SQL}"
    ))
    .bind(id)
    .fetch_one(&mut **tx.inner)
    .await?;
    if others > 0 {
        return Ok(());
    }
    Err(refusal(verb))
}
