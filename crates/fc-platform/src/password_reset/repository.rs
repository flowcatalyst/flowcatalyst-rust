//! PasswordResetToken Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::PasswordResetToken;
use crate::shared::enum_str::decode;
use crate::shared::error::{PlatformError, Result};

#[derive(sqlx::FromRow)]
struct PasswordResetTokenRow {
    id: String,
    principal_id: String,
    token_hash: String,
    purpose: String,
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
        let purpose = decode(&r.purpose, "iam_password_reset_tokens", "purpose", &r.id)?;
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

const TOKEN_COLUMNS: &str = "id, principal_id, token_hash, purpose, reset_2fa, requires_factor, \
     factor_attempts, redirect_uri, expires_at, created_at";

pub struct PasswordResetTokenRepository {
    pool: PgPool,
}

impl PasswordResetTokenRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create(&self, token: &PasswordResetToken) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO iam_password_reset_tokens
                (id, principal_id, token_hash, purpose, reset_2fa, requires_factor,
                 redirect_uri, expires_at, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())"#,
        )
        .bind(&token.id)
        .bind(&token.principal_id)
        .bind(&token.token_hash)
        .bind(token.purpose.as_str())
        .bind(token.reset_2fa)
        .bind(token.requires_factor)
        .bind(&token.redirect_uri)
        .bind(token.expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_token_hash(&self, hash: &str) -> Result<Option<PasswordResetToken>> {
        let row = sqlx::query_as::<_, PasswordResetTokenRow>(&format!(
            "SELECT {TOKEN_COLUMNS} FROM iam_password_reset_tokens WHERE token_hash = $1"
        ))
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;
        row.map(PasswordResetToken::try_from).transpose()
    }

    /// Count a wrong factor code against the token and return the new count.
    /// Atomic, so concurrent wrong guesses can't share a slot under the
    /// ceiling (Go `IncrementFactorAttempts`).
    pub async fn increment_factor_attempts(&self, id: &str) -> Result<Option<i32>> {
        let n = sqlx::query_scalar::<_, i32>(
            "UPDATE iam_password_reset_tokens SET factor_attempts = factor_attempts + 1 \
             WHERE id = $1 RETURNING factor_attempts",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(n)
    }

    pub async fn delete_by_principal_id(&self, principal_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM iam_password_reset_tokens WHERE principal_id = $1")
            .bind(principal_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_expired(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM iam_password_reset_tokens WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn delete_by_id(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM iam_password_reset_tokens WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
