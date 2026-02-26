//! Refresh Token Repository
//!
//! Repository for managing refresh tokens in MongoDB.
//! Supports token validation, rotation, and revocation.

use mongodb::{Collection, Database, bson::doc};
use futures::TryStreamExt;
use chrono::Utc;
use crate::RefreshToken;
use crate::shared::error::Result;

/// Repository for refresh token management
pub struct RefreshTokenRepository {
    collection: Collection<RefreshToken>,
}

impl RefreshTokenRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("refresh_tokens"),
        }
    }

    /// Insert a new refresh token
    pub async fn insert(&self, token: &RefreshToken) -> Result<()> {
        self.collection.insert_one(token).await?;
        Ok(())
    }

    /// Find a refresh token by its hash
    ///
    /// This is the primary lookup method. The raw token from the client
    /// is hashed and looked up.
    pub async fn find_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>> {
        Ok(self.collection.find_one(doc! { "tokenHash": token_hash }).await?)
    }

    /// Find a valid (non-expired, non-revoked) refresh token by its hash
    pub async fn find_valid_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        Ok(self.collection
            .find_one(doc! {
                "tokenHash": token_hash,
                "revoked": false,
                "expiresAt": { "$gt": now }
            })
            .await?)
    }

    /// Find all tokens for a principal
    pub async fn find_by_principal(&self, principal_id: &str) -> Result<Vec<RefreshToken>> {
        let cursor = self.collection
            .find(doc! { "principalId": principal_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    /// Find all active tokens for a principal
    pub async fn find_active_by_principal(&self, principal_id: &str) -> Result<Vec<RefreshToken>> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        let cursor = self.collection
            .find(doc! {
                "principalId": principal_id,
                "revoked": false,
                "expiresAt": { "$gt": now }
            })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    /// Revoke a token by its ID
    pub async fn revoke_by_id(&self, id: &str) -> Result<bool> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        let result = self.collection
            .update_one(
                doc! { "_id": id },
                doc! { "$set": { "revoked": true, "revokedAt": now } },
            )
            .await?;
        Ok(result.modified_count > 0)
    }

    /// Revoke a token by its hash (used when exchanging tokens)
    pub async fn revoke_by_hash(&self, token_hash: &str) -> Result<bool> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        let result = self.collection
            .update_one(
                doc! { "tokenHash": token_hash },
                doc! { "$set": { "revoked": true, "revokedAt": now } },
            )
            .await?;
        Ok(result.modified_count > 0)
    }

    /// Revoke all tokens for a principal (logout all devices)
    pub async fn revoke_all_for_principal(&self, principal_id: &str) -> Result<u64> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        let result = self.collection
            .update_many(
                doc! { "principalId": principal_id, "revoked": false },
                doc! { "$set": { "revoked": true, "revokedAt": now } },
            )
            .await?;
        Ok(result.modified_count)
    }

    /// Find all tokens in a token family
    pub async fn find_by_family(&self, family_id: &str) -> Result<Vec<RefreshToken>> {
        let cursor = self.collection
            .find(doc! { "tokenFamily": family_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    /// Revoke all tokens in a token family.
    /// Used when detecting token reuse attacks - if a rotated token is used again,
    /// the entire family is compromised and should be revoked.
    pub async fn revoke_all_in_family(&self, family_id: &str) -> Result<u64> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        let result = self.collection
            .update_many(
                doc! { "tokenFamily": family_id, "revoked": false },
                doc! { "$set": { "revoked": true, "revokedAt": now } },
            )
            .await?;
        Ok(result.modified_count)
    }

    /// Mark a token as replaced during token rotation.
    /// Records the hash of the new token and marks the old token for tracking.
    pub async fn mark_as_replaced(&self, token_hash: &str, new_token_hash: &str) -> Result<bool> {
        let result = self.collection
            .update_one(
                doc! { "tokenHash": token_hash },
                doc! { "$set": { "replacedBy": new_token_hash } },
            )
            .await?;
        Ok(result.modified_count > 0)
    }

    /// Check if a token was replaced (used to detect token reuse attacks)
    pub async fn was_replaced(&self, token_hash: &str) -> Result<bool> {
        let result = self.collection
            .find_one(doc! {
                "tokenHash": token_hash,
                "replacedBy": { "$exists": true, "$ne": null }
            })
            .await?;
        Ok(result.is_some())
    }

    /// Update last used timestamp
    pub async fn update_last_used(&self, id: &str) -> Result<bool> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        let result = self.collection
            .update_one(
                doc! { "_id": id },
                doc! { "$set": { "lastUsedAt": now } },
            )
            .await?;
        Ok(result.modified_count > 0)
    }

    /// Delete expired tokens (cleanup job)
    pub async fn delete_expired(&self) -> Result<u64> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        let result = self.collection
            .delete_many(doc! { "expiresAt": { "$lt": now } })
            .await?;
        Ok(result.deleted_count)
    }

    /// Delete revoked tokens older than a given date (cleanup job)
    pub async fn delete_revoked_before(&self, cutoff: chrono::DateTime<Utc>) -> Result<u64> {
        let cutoff_bson = mongodb::bson::DateTime::from_chrono(cutoff);
        let result = self.collection
            .delete_many(doc! {
                "revoked": true,
                "createdAt": { "$lt": cutoff_bson }
            })
            .await?;
        Ok(result.deleted_count)
    }

    /// Count active tokens for a principal (for rate limiting/monitoring)
    pub async fn count_active_for_principal(&self, principal_id: &str) -> Result<u64> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        Ok(self.collection
            .count_documents(doc! {
                "principalId": principal_id,
                "revoked": false,
                "expiresAt": { "$gt": now }
            })
            .await?)
    }

    /// Count all tokens (for monitoring)
    pub async fn count(&self) -> Result<u64> {
        Ok(self.collection.count_documents(doc! {}).await?)
    }

    /// Count expired tokens (for monitoring cleanup backlog)
    pub async fn count_expired(&self) -> Result<u64> {
        let now = mongodb::bson::DateTime::from_chrono(Utc::now());
        Ok(self.collection
            .count_documents(doc! { "expiresAt": { "$lt": now } })
            .await?)
    }
}

#[cfg(test)]
mod tests {
    // Repository tests require MongoDB connection
    // These would typically be integration tests
}
