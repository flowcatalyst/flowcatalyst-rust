//! Application Repository

use mongodb::{Collection, Database, bson::doc};
use futures::TryStreamExt;
use crate::Application;
use crate::shared::error::Result;

pub struct ApplicationRepository {
    collection: Collection<Application>,
}

impl ApplicationRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("applications"),
        }
    }

    pub async fn insert(&self, application: &Application) -> Result<()> {
        self.collection.insert_one(application).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Application>> {
        Ok(self.collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<Application>> {
        Ok(self.collection.find_one(doc! { "code": code }).await?)
    }

    pub async fn find_active(&self) -> Result<Vec<Application>> {
        let cursor = self.collection
            .find(doc! { "active": true })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_all(&self) -> Result<Vec<Application>> {
        let cursor = self.collection.find(doc! {}).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_applications(&self) -> Result<Vec<Application>> {
        let cursor = self.collection
            .find(doc! { "type": "APPLICATION", "active": true })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_integrations(&self) -> Result<Vec<Application>> {
        let cursor = self.collection
            .find(doc! { "type": "INTEGRATION", "active": true })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_service_account(&self, service_account_id: &str) -> Result<Option<Application>> {
        Ok(self.collection
            .find_one(doc! { "serviceAccountId": service_account_id })
            .await?)
    }

    pub async fn exists(&self, id: &str) -> Result<bool> {
        let count = self.collection
            .count_documents(doc! { "_id": id })
            .await?;
        Ok(count > 0)
    }

    pub async fn exists_by_code(&self, code: &str) -> Result<bool> {
        let count = self.collection
            .count_documents(doc! { "code": code })
            .await?;
        Ok(count > 0)
    }

    pub async fn update(&self, application: &Application) -> Result<()> {
        self.collection
            .replace_one(doc! { "_id": &application.id }, application)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = self.collection.delete_one(doc! { "_id": id }).await?;
        Ok(result.deleted_count > 0)
    }
}
