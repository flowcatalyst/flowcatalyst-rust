//! OIDC Login State Repository — PostgreSQL via SQLx
//!
//! Repository for managing OIDC login state during the authorization code flow.
//! States are short-lived (10 minutes) and single-use for security.

use crate::auth::oidc_login_state::OidcLoginState;
use chrono::{DateTime, Utc};
use fc_platform_core::shared::error::Result;
use sqlx::PgPool;
use tracing::debug;

/// Row struct matching the `oauth_oidc_login_states` table
#[derive(Debug, sqlx::FromRow)]
struct OidcLoginStateRow {
    pub state: String,
    pub email_domain: String,
    pub identity_provider_id: String,
    pub email_domain_mapping_id: String,
    pub nonce: String,
    pub code_verifier: String,
    pub return_url: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub oauth_scope: Option<String>,
    pub oauth_state: Option<String>,
    pub oauth_code_challenge: Option<String>,
    pub oauth_code_challenge_method: Option<String>,
    pub oauth_nonce: Option<String>,
    pub interaction_uid: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl From<OidcLoginStateRow> for OidcLoginState {
    fn from(r: OidcLoginStateRow) -> Self {
        Self {
            state: r.state,
            email_domain: r.email_domain,
            identity_provider_id: r.identity_provider_id,
            email_domain_mapping_id: r.email_domain_mapping_id,
            nonce: r.nonce,
            code_verifier: r.code_verifier,
            return_url: r.return_url,
            oauth_client_id: r.oauth_client_id,
            oauth_redirect_uri: r.oauth_redirect_uri,
            oauth_scope: r.oauth_scope,
            oauth_state: r.oauth_state,
            oauth_code_challenge: r.oauth_code_challenge,
            oauth_code_challenge_method: r.oauth_code_challenge_method,
            oauth_nonce: r.oauth_nonce,
            interaction_uid: r.interaction_uid,
            created_at: r.created_at,
            expires_at: r.expires_at,
        }
    }
}

/// Repository for OIDC login state management
pub struct OidcLoginStateRepository {
    pool: PgPool,
}

impl OidcLoginStateRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Insert a new login state
    pub async fn insert(&self, state: &OidcLoginState) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO oauth_oidc_login_states (
                state, email_domain, identity_provider_id, email_domain_mapping_id,
                nonce, code_verifier, return_url,
                oauth_client_id, oauth_redirect_uri, oauth_scope, oauth_state,
                oauth_code_challenge, oauth_code_challenge_method, oauth_nonce,
                interaction_uid, created_at, expires_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)"#,
        )
        .bind(&state.state)
        .bind(&state.email_domain)
        .bind(&state.identity_provider_id)
        .bind(&state.email_domain_mapping_id)
        .bind(&state.nonce)
        .bind(&state.code_verifier)
        .bind(&state.return_url)
        .bind(&state.oauth_client_id)
        .bind(&state.oauth_redirect_uri)
        .bind(&state.oauth_scope)
        .bind(&state.oauth_state)
        .bind(&state.oauth_code_challenge)
        .bind(&state.oauth_code_challenge_method)
        .bind(&state.oauth_nonce)
        .bind(&state.interaction_uid)
        .bind(state.created_at)
        .bind(state.expires_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Find a state by its state parameter (which is the primary key)
    pub async fn find_by_state(&self, state: &str) -> Result<Option<OidcLoginState>> {
        let row = sqlx::query_as::<_, OidcLoginStateRow>(
            "SELECT * FROM oauth_oidc_login_states WHERE state = $1",
        )
        .bind(state)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(OidcLoginState::from))
    }

    /// Find a valid (non-expired) state by its state parameter
    ///
    /// This is the main method used during callback validation.
    /// Returns None if the state doesn't exist or has expired.
    pub async fn find_valid_state(&self, state: &str) -> Result<Option<OidcLoginState>> {
        let row = sqlx::query_as::<_, OidcLoginStateRow>(
            "SELECT * FROM oauth_oidc_login_states WHERE state = $1 AND expires_at > NOW()",
        )
        .bind(state)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(OidcLoginState::from))
    }

    /// Atomically find and consume a valid (non-expired) state.
    ///
    /// Uses `DELETE ... WHERE state = $1 AND expires_at > NOW() RETURNING *`
    /// to prevent race conditions where two concurrent callbacks could both
    /// consume the same state. Returns None if the state doesn't exist,
    /// has expired, or was already consumed by another request.
    pub async fn find_and_consume_state(
        &self,
        state_param: &str,
    ) -> Result<Option<OidcLoginState>> {
        let row = sqlx::query_as::<_, OidcLoginStateRow>(
            r#"DELETE FROM oauth_oidc_login_states
               WHERE state = $1 AND expires_at > NOW()
               RETURNING *"#,
        )
        .bind(state_param)
        .fetch_optional(&self.pool)
        .await?;

        if row.is_some() {
            debug!(state = %state_param, "OIDC login state atomically consumed");
        }

        Ok(row.map(OidcLoginState::from))
    }

    /// Delete a state by its state parameter (single-use enforcement)
    ///
    /// Should be called immediately after finding the state to ensure
    /// it cannot be reused.
    pub async fn delete_by_state(&self, state: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM oauth_oidc_login_states WHERE state = $1")
            .bind(state)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Delete all expired states (cleanup job)
    ///
    /// Should be called periodically to clean up abandoned login attempts.
    /// Returns the number of deleted states.
    pub async fn delete_expired(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM oauth_oidc_login_states WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Find all states (for debugging/admin purposes)
    pub async fn find_all(&self) -> Result<Vec<OidcLoginState>> {
        let rows = sqlx::query_as::<_, OidcLoginStateRow>("SELECT * FROM oauth_oidc_login_states")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(OidcLoginState::from).collect())
    }

    /// Count all states (for monitoring)
    pub async fn count(&self) -> Result<u64> {
        let (count,) = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM oauth_oidc_login_states")
            .fetch_one(&self.pool)
            .await?;

        Ok(count as u64)
    }

    /// Count expired states (for monitoring cleanup backlog)
    pub async fn count_expired(&self) -> Result<u64> {
        let (count,) = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM oauth_oidc_login_states WHERE expires_at < NOW()",
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(count as u64)
    }

    /// Delete states older than a specified duration (aggressive cleanup)
    ///
    /// Useful for cleaning up states that are much older than the normal 10-minute expiry.
    pub async fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<u64> {
        let result = sqlx::query("DELETE FROM oauth_oidc_login_states WHERE created_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    // Repository tests require a PostgreSQL connection
    // These would typically be integration tests
}
