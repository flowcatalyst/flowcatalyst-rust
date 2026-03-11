//! OIDC Login State Repository — PostgreSQL via SeaORM
//!
//! Repository for managing OIDC login state during the authorization code flow.
//! States are short-lived (10 minutes) and single-use for security.

use sea_orm::*;
use chrono::Utc;
use tracing::debug;
use crate::OidcLoginState;
use crate::entities::iam_oidc_login_states;
use crate::shared::error::Result;

/// Repository for OIDC login state management
pub struct OidcLoginStateRepository {
    db: DatabaseConnection,
}

impl OidcLoginStateRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    /// Insert a new login state
    pub async fn insert(&self, state: &OidcLoginState) -> Result<()> {
        let model = iam_oidc_login_states::ActiveModel {
            state: Set(state.state.clone()),
            email_domain: Set(state.email_domain.clone()),
            identity_provider_id: Set(state.identity_provider_id.clone()),
            email_domain_mapping_id: Set(state.email_domain_mapping_id.clone()),
            nonce: Set(state.nonce.clone()),
            code_verifier: Set(state.code_verifier.clone()),
            return_url: Set(state.return_url.clone()),
            oauth_client_id: Set(state.oauth_client_id.clone()),
            oauth_redirect_uri: Set(state.oauth_redirect_uri.clone()),
            oauth_scope: Set(state.oauth_scope.clone()),
            oauth_state: Set(state.oauth_state.clone()),
            oauth_code_challenge: Set(state.oauth_code_challenge.clone()),
            oauth_code_challenge_method: Set(state.oauth_code_challenge_method.clone()),
            oauth_nonce: Set(state.oauth_nonce.clone()),
            interaction_uid: Set(state.interaction_uid.clone()),
            created_at: Set(state.created_at.into()),
            expires_at: Set(state.expires_at.into()),
        };
        iam_oidc_login_states::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    /// Find a state by its state parameter (which is the primary key)
    pub async fn find_by_state(&self, state: &str) -> Result<Option<OidcLoginState>> {
        let result = iam_oidc_login_states::Entity::find_by_id(state)
            .one(&self.db)
            .await?;
        Ok(result.map(OidcLoginState::from))
    }

    /// Find a valid (non-expired) state by its state parameter
    ///
    /// This is the main method used during callback validation.
    /// Returns None if the state doesn't exist or has expired.
    pub async fn find_valid_state(&self, state: &str) -> Result<Option<OidcLoginState>> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let result = iam_oidc_login_states::Entity::find()
            .filter(iam_oidc_login_states::Column::State.eq(state))
            .filter(iam_oidc_login_states::Column::ExpiresAt.gt(now))
            .one(&self.db)
            .await?;
        Ok(result.map(OidcLoginState::from))
    }

    /// Atomically find and consume a valid (non-expired) state.
    ///
    /// Uses `DELETE ... WHERE state = $1 AND expires_at > NOW() RETURNING *`
    /// to prevent race conditions where two concurrent callbacks could both
    /// consume the same state. Returns None if the state doesn't exist,
    /// has expired, or was already consumed by another request.
    pub async fn find_and_consume_state(&self, state_param: &str) -> Result<Option<OidcLoginState>> {
        let sql = r#"
            DELETE FROM oauth_oidc_login_states
            WHERE state = $1 AND expires_at > NOW()
            RETURNING *
        "#;

        let result = iam_oidc_login_states::Entity::find()
            .from_raw_sql(Statement::from_sql_and_values(
                DbBackend::Postgres,
                sql,
                [state_param.into()],
            ))
            .one(&self.db)
            .await?;

        if let Some(ref _model) = result {
            debug!(state = %state_param, "OIDC login state atomically consumed");
        }

        Ok(result.map(OidcLoginState::from))
    }

    /// Delete a state by its state parameter (single-use enforcement)
    ///
    /// Should be called immediately after finding the state to ensure
    /// it cannot be reused.
    pub async fn delete_by_state(&self, state: &str) -> Result<bool> {
        let result = iam_oidc_login_states::Entity::delete_by_id(state)
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    /// Delete all expired states (cleanup job)
    ///
    /// Should be called periodically to clean up abandoned login attempts.
    /// Returns the number of deleted states.
    pub async fn delete_expired(&self) -> Result<u64> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let result = iam_oidc_login_states::Entity::delete_many()
            .filter(iam_oidc_login_states::Column::ExpiresAt.lt(now))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }

    /// Find all states (for debugging/admin purposes)
    pub async fn find_all(&self) -> Result<Vec<OidcLoginState>> {
        let rows = iam_oidc_login_states::Entity::find()
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(OidcLoginState::from).collect())
    }

    /// Count all states (for monitoring)
    pub async fn count(&self) -> Result<u64> {
        let count = iam_oidc_login_states::Entity::find()
            .count(&self.db)
            .await?;
        Ok(count)
    }

    /// Count expired states (for monitoring cleanup backlog)
    pub async fn count_expired(&self) -> Result<u64> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let count = iam_oidc_login_states::Entity::find()
            .filter(iam_oidc_login_states::Column::ExpiresAt.lt(now))
            .count(&self.db)
            .await?;
        Ok(count)
    }

    /// Delete states older than a specified duration (aggressive cleanup)
    ///
    /// Useful for cleaning up states that are much older than the normal 10-minute expiry.
    pub async fn delete_older_than(&self, cutoff: chrono::DateTime<Utc>) -> Result<u64> {
        let cutoff_fixed: chrono::DateTime<chrono::FixedOffset> = cutoff.into();
        let result = iam_oidc_login_states::Entity::delete_many()
            .filter(iam_oidc_login_states::Column::CreatedAt.lt(cutoff_fixed))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }
}

#[cfg(test)]
mod tests {
    // Repository tests require a PostgreSQL connection
    // These would typically be integration tests
}
