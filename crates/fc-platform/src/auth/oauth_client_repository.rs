//! OAuth Client Repository

use mongodb::{Collection, Database, bson::doc};
use futures::TryStreamExt;
use crate::OAuthClient;
use crate::shared::error::Result;

pub struct OAuthClientRepository {
    collection: Collection<OAuthClient>,
}

impl OAuthClientRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("oauth_clients"),
        }
    }

    pub async fn insert(&self, client: &OAuthClient) -> Result<()> {
        self.collection.insert_one(client).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<OAuthClient>> {
        Ok(self.collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn find_by_client_id(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        Ok(self.collection.find_one(doc! { "clientId": client_id }).await?)
    }

    pub async fn find_active(&self) -> Result<Vec<OAuthClient>> {
        let cursor = self.collection
            .find(doc! { "active": true })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_all(&self) -> Result<Vec<OAuthClient>> {
        let cursor = self.collection.find(doc! {}).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_application(&self, application_id: &str) -> Result<Vec<OAuthClient>> {
        let cursor = self.collection
            .find(doc! { "applicationIds": application_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn exists_by_client_id(&self, client_id: &str) -> Result<bool> {
        let count = self.collection
            .count_documents(doc! { "clientId": client_id })
            .await?;
        Ok(count > 0)
    }

    pub async fn update(&self, client: &OAuthClient) -> Result<()> {
        self.collection
            .replace_one(doc! { "_id": &client.id }, client)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = self.collection.delete_one(doc! { "_id": id }).await?;
        Ok(result.deleted_count > 0)
    }
}
