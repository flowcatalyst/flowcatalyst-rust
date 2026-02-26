//! Authorization Code Repository
//!
//! MongoDB repository for OAuth2 authorization codes.

use chrono::Utc;
use mongodb::{bson::doc, Collection, Database};

use crate::AuthorizationCode;
use crate::shared::error::Result;

/// Repository for authorization codes.
pub struct AuthorizationCodeRepository {
    collection: Collection<AuthorizationCode>,
}

impl AuthorizationCodeRepository {
    const COLLECTION_NAME: &'static str = "authorization_codes";

    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection(Self::COLLECTION_NAME),
        }
    }

    /// Insert a new authorization code.
    pub async fn insert(&self, code: &AuthorizationCode) -> Result<()> {
        self.collection.insert_one(code).await?;
        Ok(())
    }

    /// Find an authorization code by its code value.
    pub async fn find_by_code(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let result = self.collection
            .find_one(doc! { "_id": code })
            .await?;
        Ok(result)
    }

    /// Find a valid (not used, not expired) authorization code.
    pub async fn find_valid_code(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let now = Utc::now();
        let result = self.collection
            .find_one(doc! {
                "_id": code,
                "used": false,
                "expires_at": { "$gt": now }
            })
            .await?;
        Ok(result)
    }

    /// Mark an authorization code as used.
    pub async fn mark_as_used(&self, code: &str) -> Result<bool> {
        let result = self.collection
            .update_one(
                doc! { "_id": code },
                doc! { "$set": { "used": true } },
            )
            .await?;
        Ok(result.modified_count > 0)
    }

    /// Delete an authorization code.
    pub async fn delete(&self, code: &str) -> Result<bool> {
        let result = self.collection
            .delete_one(doc! { "_id": code })
            .await?;
        Ok(result.deleted_count > 0)
    }

    /// Delete all expired authorization codes.
    pub async fn delete_expired(&self) -> Result<u64> {
        let now = Utc::now();
        let result = self.collection
            .delete_many(doc! { "expires_at": { "$lt": now } })
            .await?;
        Ok(result.deleted_count)
    }

    /// Delete all authorization codes for a principal.
    pub async fn delete_by_principal(&self, principal_id: &str) -> Result<u64> {
        let result = self.collection
            .delete_many(doc! { "principal_id": principal_id })
            .await?;
        Ok(result.deleted_count)
    }

    /// Delete all authorization codes for a client.
    pub async fn delete_by_client(&self, client_id: &str) -> Result<u64> {
        let result = self.collection
            .delete_many(doc! { "client_id": client_id })
            .await?;
        Ok(result.deleted_count)
    }

    /// Count all authorization codes.
    pub async fn count(&self) -> Result<u64> {
        let count = self.collection.count_documents(doc! {}).await?;
        Ok(count)
    }

    /// Count valid (not used, not expired) authorization codes.
    pub async fn count_valid(&self) -> Result<u64> {
        let now = Utc::now();
        let count = self.collection
            .count_documents(doc! {
                "used": false,
                "expires_at": { "$gt": now }
            })
            .await?;
        Ok(count)
    }
}
