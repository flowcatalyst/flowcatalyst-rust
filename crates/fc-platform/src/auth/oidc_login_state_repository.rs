//! OIDC Login State Repository
//!
//! Repository for managing OIDC login state during the authorization code flow.
//! States are short-lived (10 minutes) and single-use for security.

use mongodb::{Collection, Database, bson::doc};
use futures::TryStreamExt;
use chrono::Utc;
use crate::OidcLoginState;
use crate::shared::error::Result;

/// Repository for OIDC login state management
pub struct OidcLoginStateRepository {
    collection: Collection<OidcLoginState>,
}

impl OidcLoginStateRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("oidc_login_state"),
        }
    }

    /// Insert a new login state
    pub async fn insert(&self, state: &OidcLoginState) -> Result<()> {
        self.collection.insert_one(state).await?;
        Ok(())
    }

    /// Find a state by its state parameter (which is also the _id)
    pub async fn find_by_state(&self, state: &str) -> Result<Option<OidcLoginState>> {
        Ok(self.collection.find_one(doc! { "_id": state }).await?)
    }

    /// Find a valid (non-expired) state by its state parameter
    ///
    /// This is the main method used during callback validation.
    /// Returns None if the state doesn't exist or has expired.
    pub async fn find_valid_state(&self, state: &str) -> Result<Option<OidcLoginState>> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        Ok(self.collection
            .find_one(doc! {
                "_id": state,
                "expiresAt": { "$gt": now }
            })
            .await?)
    }

    /// Delete a state by its state parameter (single-use enforcement)
    ///
    /// Should be called immediately after finding the state to ensure
    /// it cannot be reused.
    pub async fn delete_by_state(&self, state: &str) -> Result<bool> {
        let result = self.collection.delete_one(doc! { "_id": state }).await?;
        Ok(result.deleted_count > 0)
    }

    /// Delete all expired states (cleanup job)
    ///
    /// Should be called periodically to clean up abandoned login attempts.
    /// Returns the number of deleted states.
    pub async fn delete_expired(&self) -> Result<u64> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        let result = self.collection
            .delete_many(doc! { "expiresAt": { "$lt": now } })
            .await?;
        Ok(result.deleted_count)
    }

    /// Find all states (for debugging/admin purposes)
    pub async fn find_all(&self) -> Result<Vec<OidcLoginState>> {
        let cursor = self.collection.find(doc! {}).await?;
        Ok(cursor.try_collect().await?)
    }

    /// Count all states (for monitoring)
    pub async fn count(&self) -> Result<u64> {
        Ok(self.collection.count_documents(doc! {}).await?)
    }

    /// Count expired states (for monitoring cleanup backlog)
    pub async fn count_expired(&self) -> Result<u64> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        Ok(self.collection
            .count_documents(doc! { "expiresAt": { "$lt": now } })
            .await?)
    }

    /// Delete states older than a specified duration (aggressive cleanup)
    ///
    /// Useful for cleaning up states that are much older than the normal 10-minute expiry.
    pub async fn delete_older_than(&self, cutoff: chrono::DateTime<Utc>) -> Result<u64> {
        let cutoff_bson = mongodb::bson::DateTime::from_chrono(cutoff);
        let result = self.collection
            .delete_many(doc! { "createdAt": { "$lt": cutoff_bson } })
            .await?;
        Ok(result.deleted_count)
    }
}

#[cfg(test)]
mod tests {
    // Repository tests would require a MongoDB connection
    // These would typically be integration tests
}
