//! Two-factor storage (Go `internal/platform/mfa/repository.go`), in Go's
//! tables. Like Go, these are short-lived authentication state written
//! directly, not aggregates behind the unit of work (see the module docs).
//!
//! Every consuming write is a single guarded statement, so a code can be
//! spent once however many requests race for it: a TOTP time-step
//! ([`MfaRepository::claim_totp_step`] — Go's `TouchMethodUsed` is an
//! unconditional UPDATE after a separate read, which lets two concurrent
//! verifies of the same code both pass), a recovery code, and an email PIN.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{
    EmailPin, EmailPinPurpose, Method, MethodType, TrustedDevice, EMAIL_PIN_ID_PREFIX,
    RECOVERY_CODE_ID_PREFIX, TRUSTED_DEVICE_ID_PREFIX,
};
use crate::shared::enum_str::decode;
use crate::shared::error::{PlatformError, Result};

#[derive(sqlx::FromRow)]
struct MethodRow {
    id: String,
    principal_id: String,
    method: String,
    secret_encrypted: Option<String>,
    confirmed_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<MethodRow> for Method {
    type Error = PlatformError;
    fn try_from(r: MethodRow) -> Result<Self> {
        Ok(Self {
            method: decode(&r.method, "iam_user_mfa_methods", "method", &r.id)?,
            id: r.id,
            principal_id: r.principal_id,
            secret_encrypted: r.secret_encrypted,
            confirmed_at: r.confirmed_at,
            last_used_at: r.last_used_at,
            created_at: r.created_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct EmailPinRow {
    id: String,
    principal_id: String,
    purpose: String,
    pin_hash: String,
    attempts: i32,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl TryFrom<EmailPinRow> for EmailPin {
    type Error = PlatformError;
    fn try_from(r: EmailPinRow) -> Result<Self> {
        Ok(Self {
            purpose: decode(&r.purpose, "iam_mfa_email_pins", "purpose", &r.id)?,
            id: r.id,
            principal_id: r.principal_id,
            pin_hash: r.pin_hash,
            attempts: r.attempts,
            expires_at: r.expires_at,
            created_at: r.created_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct TrustedDeviceRow {
    id: String,
    principal_id: String,
    token_hash: String,
    label: Option<String>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

impl From<TrustedDeviceRow> for TrustedDevice {
    fn from(r: TrustedDeviceRow) -> Self {
        Self {
            id: r.id,
            principal_id: r.principal_id,
            token_hash: r.token_hash,
            label: r.label,
            expires_at: r.expires_at,
            created_at: r.created_at,
            last_used_at: r.last_used_at,
        }
    }
}

const METHOD_COLUMNS: &str =
    "id, principal_id, method, secret_encrypted, confirmed_at, last_used_at, created_at";
const PIN_COLUMNS: &str = "id, principal_id, purpose, pin_hash, attempts, expires_at, created_at";
const DEVICE_COLUMNS: &str =
    "id, principal_id, token_hash, label, expires_at, created_at, last_used_at";

pub struct MfaRepository {
    pool: PgPool,
}

impl MfaRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    // ── factors ─────────────────────────────────────────────────────────

    /// Store a (normally unconfirmed) factor, replacing an unconfirmed one of
    /// the same type. A confirmed one is never replaced: the insert then
    /// fails on the (principal, method) unique index.
    pub async fn replace_pending_method(&self, m: &Method) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM iam_user_mfa_methods \
             WHERE principal_id = $1 AND method = $2 AND confirmed_at IS NULL",
        )
        .bind(&m.principal_id)
        .bind(m.method.as_str())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO iam_user_mfa_methods \
               (id, principal_id, method, secret_encrypted, confirmed_at, last_used_at, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&m.id)
        .bind(&m.principal_id)
        .bind(m.method.as_str())
        .bind(&m.secret_encrypted)
        .bind(m.confirmed_at)
        .bind(m.last_used_at)
        .bind(m.created_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Store a factor only if the user has none of that type.
    pub async fn insert_method_if_absent(&self, m: &Method) -> Result<()> {
        sqlx::query(
            "INSERT INTO iam_user_mfa_methods \
               (id, principal_id, method, secret_encrypted, confirmed_at, last_used_at, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (principal_id, method) DO NOTHING",
        )
        .bind(&m.id)
        .bind(&m.principal_id)
        .bind(m.method.as_str())
        .bind(&m.secret_encrypted)
        .bind(m.confirmed_at)
        .bind(m.last_used_at)
        .bind(m.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Confirm a pending factor. `false` when it was already confirmed (or
    /// gone): the enrolment completes once.
    pub async fn confirm_method(&self, id: &str, at: DateTime<Utc>) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE iam_user_mfa_methods SET confirmed_at = $2 \
             WHERE id = $1 AND confirmed_at IS NULL",
        )
        .bind(id)
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Spend a TOTP time-step: record `step_start` as the factor's last used
    /// step, only if it is later than the one recorded. `false` means that
    /// step (or a later one) was already spent — a replay, refused. One
    /// statement, so two requests presenting the same code can't both win.
    pub async fn claim_totp_step(&self, id: &str, step_start: DateTime<Utc>) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE iam_user_mfa_methods SET last_used_at = $2 \
             WHERE id = $1 AND (last_used_at IS NULL OR last_used_at < $2)",
        )
        .bind(id)
        .bind(step_start)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn find_method(
        &self,
        principal_id: &str,
        method: MethodType,
    ) -> Result<Option<Method>> {
        let row = sqlx::query_as::<_, MethodRow>(&format!(
            "SELECT {METHOD_COLUMNS} FROM iam_user_mfa_methods \
             WHERE principal_id = $1 AND method = $2"
        ))
        .bind(principal_id)
        .bind(method.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(Method::try_from).transpose()
    }

    /// Every factor of the user, confirmed or not, oldest first.
    pub async fn find_methods(&self, principal_id: &str) -> Result<Vec<Method>> {
        let rows = sqlx::query_as::<_, MethodRow>(&format!(
            "SELECT {METHOD_COLUMNS} FROM iam_user_mfa_methods \
             WHERE principal_id = $1 ORDER BY created_at"
        ))
        .bind(principal_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Method::try_from).collect()
    }

    pub async fn delete_method(&self, principal_id: &str, method: MethodType) -> Result<u64> {
        let r =
            sqlx::query("DELETE FROM iam_user_mfa_methods WHERE principal_id = $1 AND method = $2")
                .bind(principal_id)
                .bind(method.as_str())
                .execute(&self.pool)
                .await?;
        Ok(r.rows_affected())
    }

    // ── recovery codes ──────────────────────────────────────────────────

    /// Replace the user's recovery-code set with `hashes`, in one
    /// transaction.
    pub async fn replace_recovery_codes(
        &self,
        principal_id: &str,
        hashes: &[String],
    ) -> Result<()> {
        let ids: Vec<String> = hashes
            .iter()
            .map(|_| fc_common::tsid::generate_with_prefix(RECOVERY_CODE_ID_PREFIX))
            .collect();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM iam_user_mfa_recovery_codes WHERE principal_id = $1")
            .bind(principal_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO iam_user_mfa_recovery_codes (id, principal_id, code_hash, created_at) \
             SELECT id, $1, code_hash, NOW() FROM UNNEST($2::text[], $3::text[]) AS t(id, code_hash)",
        )
        .bind(principal_id)
        .bind(&ids)
        .bind(hashes)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Spend an unused recovery code matching `code_hash`. `true` once; the
    /// code is then burned.
    pub async fn consume_recovery_code(&self, principal_id: &str, code_hash: &str) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE iam_user_mfa_recovery_codes SET used_at = NOW() \
             WHERE id = (SELECT id FROM iam_user_mfa_recovery_codes \
                         WHERE principal_id = $1 AND code_hash = $2 AND used_at IS NULL \
                         LIMIT 1) \
               AND used_at IS NULL",
        )
        .bind(principal_id)
        .bind(code_hash)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn count_unused_recovery_codes(&self, principal_id: &str) -> Result<i64> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM iam_user_mfa_recovery_codes \
             WHERE principal_id = $1 AND used_at IS NULL",
        )
        .bind(principal_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(n)
    }

    // ── email PINs ──────────────────────────────────────────────────────

    /// Replace the user's outstanding PINs of `purpose` with a fresh one.
    pub async fn replace_email_pin(
        &self,
        principal_id: &str,
        purpose: EmailPinPurpose,
        pin_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM iam_mfa_email_pins WHERE principal_id = $1 AND purpose = $2")
            .bind(principal_id)
            .bind(purpose.as_str())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO iam_mfa_email_pins \
               (id, principal_id, purpose, pin_hash, attempts, expires_at, created_at) \
             VALUES ($1, $2, $3, $4, 0, $5, NOW())",
        )
        .bind(fc_common::tsid::generate_with_prefix(EMAIL_PIN_ID_PREFIX))
        .bind(principal_id)
        .bind(purpose.as_str())
        .bind(pin_hash)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The user's most recent PIN of `purpose`.
    pub async fn find_latest_email_pin(
        &self,
        principal_id: &str,
        purpose: EmailPinPurpose,
    ) -> Result<Option<EmailPin>> {
        let row = sqlx::query_as::<_, EmailPinRow>(&format!(
            "SELECT {PIN_COLUMNS} FROM iam_mfa_email_pins \
             WHERE principal_id = $1 AND purpose = $2 \
             ORDER BY created_at DESC LIMIT 1"
        ))
        .bind(principal_id)
        .bind(purpose.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(EmailPin::try_from).transpose()
    }

    /// Count a wrong guess; the new count (`None` when the PIN is gone).
    pub async fn increment_email_pin_attempts(&self, id: &str) -> Result<Option<i32>> {
        let n = sqlx::query_scalar::<_, i32>(
            "UPDATE iam_mfa_email_pins SET attempts = attempts + 1 WHERE id = $1 RETURNING attempts",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(n)
    }

    /// Delete one PIN; `true` when this call removed it (so a correct PIN is
    /// spent by exactly one request).
    pub async fn delete_email_pin(&self, id: &str) -> Result<bool> {
        let r = sqlx::query("DELETE FROM iam_mfa_email_pins WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() == 1)
    }

    // ── trusted devices ─────────────────────────────────────────────────

    pub async fn insert_trusted_device(
        &self,
        principal_id: &str,
        token_hash: &str,
        label: Option<&str>,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO iam_mfa_trusted_devices \
               (id, principal_id, token_hash, label, expires_at, created_at) \
             VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(fc_common::tsid::generate_with_prefix(
            TRUSTED_DEVICE_ID_PREFIX,
        ))
        .bind(principal_id)
        .bind(token_hash)
        .bind(label)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Stamp use of an unexpired remembered device matching the token hash;
    /// `true` when there was one.
    pub async fn use_trusted_device(&self, principal_id: &str, token_hash: &str) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE iam_mfa_trusted_devices SET last_used_at = NOW() \
             WHERE principal_id = $1 AND token_hash = $2 AND expires_at > NOW()",
        )
        .bind(principal_id)
        .bind(token_hash)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    /// The user's remembered devices, newest first.
    pub async fn list_trusted_devices(&self, principal_id: &str) -> Result<Vec<TrustedDevice>> {
        let rows = sqlx::query_as::<_, TrustedDeviceRow>(&format!(
            "SELECT {DEVICE_COLUMNS} FROM iam_mfa_trusted_devices \
             WHERE principal_id = $1 ORDER BY created_at DESC"
        ))
        .bind(principal_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(TrustedDevice::from).collect())
    }

    /// Remove one of the user's remembered devices (owner-scoped).
    pub async fn delete_trusted_device(&self, principal_id: &str, id: &str) -> Result<u64> {
        let r =
            sqlx::query("DELETE FROM iam_mfa_trusted_devices WHERE id = $1 AND principal_id = $2")
                .bind(id)
                .bind(principal_id)
                .execute(&self.pool)
                .await?;
        Ok(r.rows_affected())
    }

    /// Forget every remembered device of the user (a password change).
    pub async fn delete_trusted_devices(&self, principal_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM iam_mfa_trusted_devices WHERE principal_id = $1")
            .bind(principal_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── reset ───────────────────────────────────────────────────────────

    /// Clear every factor, recovery code, pending PIN and remembered device
    /// of the user, in one transaction (Go `ResetAll`).
    pub async fn reset_all(&self, principal_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for table in [
            "iam_user_mfa_methods",
            "iam_user_mfa_recovery_codes",
            "iam_mfa_email_pins",
            "iam_mfa_trusted_devices",
        ] {
            sqlx::query(&format!("DELETE FROM {table} WHERE principal_id = $1"))
                .bind(principal_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Remove expired PINs and remembered devices; the rows removed.
    pub async fn purge_expired(&self) -> Result<u64> {
        let pins = sqlx::query("DELETE FROM iam_mfa_email_pins WHERE expires_at <= NOW()")
            .execute(&self.pool)
            .await?;
        let devices = sqlx::query("DELETE FROM iam_mfa_trusted_devices WHERE expires_at <= NOW()")
            .execute(&self.pool)
            .await?;
        Ok(pins.rows_affected() + devices.rows_affected())
    }
}
