//! `oauth_oidc_payloads` as a whole — PostgreSQL via SQLx.
//!
//! Each payload type (authorization codes, refresh tokens, pending
//! authorizations, WebAuthn ceremonies, …) has its own repository; this one
//! only serves the housekeeping purge that sweeps every type at once, as
//! Go's `payload.Repository.PurgeExpired` does
//! (`OAuthPayloadPurgeExpired`, sqlc/queries/payload.sql).

use sqlx::PgPool;

use fc_platform_core::shared::error::Result;

pub struct OidcPayloadRepository {
    pool: PgPool,
}

impl OidcPayloadRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Remove every payload past its expiry, whatever its type. A payload
    /// with no expiry is kept. Returns the rows removed.
    pub async fn purge_expired(&self) -> Result<u64> {
        let done = sqlx::query(
            "DELETE FROM oauth_oidc_payloads WHERE expires_at IS NOT NULL AND expires_at < NOW()",
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }
}
