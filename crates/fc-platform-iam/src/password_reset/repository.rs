//! PasswordResetToken Repository — PostgreSQL via SQLx

use crate::password_reset::entity::TokenPurpose;
use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::PasswordResetTokenId;
use fc_platform_core::shared::id::PrincipalId;
use sqlx::PgPool;

use super::entity::PasswordResetToken;
use fc_platform_core::shared::enum_str::Stored;
use fc_platform_core::shared::error::{PlatformError, Result};

struct PasswordResetTokenRow {
    id: PasswordResetTokenId,
    principal_id: PrincipalId,
    token_hash: String,
    purpose: Stored<TokenPurpose>,
    reset_2fa: bool,
    requires_factor: bool,
    factor_attempts: i32,
    redirect_uri: Option<String>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl TryFrom<PasswordResetTokenRow> for PasswordResetToken {
    type Error = PlatformError;
    /// An unknown purpose is a loud read error, never read as `reset`
    /// (X-06; Go `scanToken`).
    fn try_from(r: PasswordResetTokenRow) -> Result<Self> {
        let purpose = r
            .purpose
            .decode("iam_password_reset_tokens", "purpose", r.id.as_str())?;
        Ok(Self {
            id: r.id,
            principal_id: r.principal_id,
            token_hash: r.token_hash,
            purpose,
            reset_2fa: r.reset_2fa,
            requires_factor: r.requires_factor,
            factor_attempts: r.factor_attempts,
            redirect_uri: r.redirect_uri,
            expires_at: r.expires_at,
            created_at: r.created_at,
        })
    }
}

pub struct PasswordResetTokenRepository {
    pool: PgPool,
}

impl PasswordResetTokenRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create(&self, token: &PasswordResetToken) -> Result<()> {
        sqlx::query!(
            r#"INSERT INTO iam_password_reset_tokens
                (id, principal_id, token_hash, purpose, reset_2fa, requires_factor,
                 redirect_uri, expires_at, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())"#,
            &token.id as &PasswordResetTokenId,
            &token.principal_id as &PrincipalId,
            &token.token_hash,
            token.purpose as TokenPurpose,
            token.reset_2fa,
            token.requires_factor,
            token.redirect_uri.as_ref(),
            token.expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_token_hash(&self, hash: &str) -> Result<Option<PasswordResetToken>> {
        let row = sqlx::query_as!(
            PasswordResetTokenRow,
            "SELECT id AS \"id: PasswordResetTokenId\", \
                    principal_id AS \"principal_id: PrincipalId\", token_hash, \
                    purpose AS \"purpose: Stored<TokenPurpose>\", reset_2fa, \
                    requires_factor, factor_attempts, redirect_uri, expires_at, \
                    created_at \
                    FROM iam_password_reset_tokens WHERE token_hash = $1",
            hash
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(PasswordResetToken::try_from).transpose()
    }

    /// Count a wrong factor code against the token and return the new count.
    /// Atomic, so concurrent wrong guesses can't share a slot under the
    /// ceiling (Go `IncrementFactorAttempts`).
    pub async fn increment_factor_attempts(
        &self,
        id: &PasswordResetTokenId,
    ) -> Result<Option<i32>> {
        let n = sqlx::query_scalar!(
            "UPDATE iam_password_reset_tokens SET factor_attempts = factor_attempts + 1 \
             WHERE id = $1 RETURNING factor_attempts",
            id as &PasswordResetTokenId
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(n)
    }

    pub async fn delete_by_principal_id(&self, principal_id: &PrincipalId) -> Result<()> {
        sqlx::query!(
            "DELETE FROM iam_password_reset_tokens WHERE principal_id = $1",
            principal_id as &PrincipalId
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove the tokens that expired before `cutoff`. The housekeeping
    /// purge keeps an expired token for a grace period so a late click on the
    /// link still answers "expired" rather than "not found".
    pub async fn purge_expired_before(&self, cutoff: DateTime<Utc>) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM iam_password_reset_tokens WHERE expires_at <= $1",
            cutoff
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn delete_expired(&self) -> Result<u64> {
        let result = sqlx::query!("DELETE FROM iam_password_reset_tokens WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn delete_by_id(&self, id: &PasswordResetTokenId) -> Result<()> {
        sqlx::query!(
            "DELETE FROM iam_password_reset_tokens WHERE id = $1",
            id as &PasswordResetTokenId
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
