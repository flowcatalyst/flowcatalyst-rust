//! ApplicationClientConfig Repository

use mongodb::{Collection, Database, bson::doc};
use futures::TryStreamExt;
use crate::ApplicationClientConfig;
use crate::shared::error::Result;

pub struct ApplicationClientConfigRepository {
    collection: Collection<ApplicationClientConfig>,
}

impl ApplicationClientConfigRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("application_client_configs"),
        }
    }

    pub async fn insert(&self, config: &ApplicationClientConfig) -> Result<()> {
        self.collection.insert_one(config).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ApplicationClientConfig>> {
        Ok(self.collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn find_by_application(&self, application_id: &str) -> Result<Vec<ApplicationClientConfig>> {
        let cursor = self.collection
            .find(doc! { "applicationId": application_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_client(&self, client_id: &str) -> Result<Vec<ApplicationClientConfig>> {
        let cursor = self.collection
            .find(doc! { "clientId": client_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_application_and_client(
        &self,
        application_id: &str,
        client_id: &str
    ) -> Result<Option<ApplicationClientConfig>> {
        Ok(self.collection.find_one(doc! {
            "applicationId": application_id,
            "clientId": client_id
        }).await?)
    }

    pub async fn find_enabled_for_client(&self, client_id: &str) -> Result<Vec<ApplicationClientConfig>> {
        let cursor = self.collection
            .find(doc! { "clientId": client_id, "enabled": true })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn update(&self, config: &ApplicationClientConfig) -> Result<()> {
        self.collection
            .replace_one(doc! { "_id": &config.id }, config)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = self.collection.delete_one(doc! { "_id": id }).await?;
        Ok(result.deleted_count > 0)
    }

    pub async fn delete_by_application_and_client(
        &self,
        application_id: &str,
        client_id: &str
    ) -> Result<bool> {
        let result = self.collection.delete_one(doc! {
            "applicationId": application_id,
            "clientId": client_id
        }).await?;
        Ok(result.deleted_count > 0)
    }

    /// Enable an application for a client (upsert)
    pub async fn enable_for_client(&self, application_id: &str, client_id: &str) -> Result<()> {
        use mongodb::options::UpdateOptions;

        let options = UpdateOptions::builder().upsert(true).build();
        self.collection.update_one(
            doc! {
                "applicationId": application_id,
                "clientId": client_id
            },
            doc! {
                "$set": {
                    "enabled": true,
                    "updatedAt": mongodb::bson::DateTime::now()
                },
                "$setOnInsert": {
                    "_id": crate::shared::tsid::TsidGenerator::generate(),
                    "applicationId": application_id,
                    "clientId": client_id,
                    "createdAt": mongodb::bson::DateTime::now()
                }
            }
        ).with_options(options).await?;
        Ok(())
    }

    /// Disable an application for a client
    pub async fn disable_for_client(&self, application_id: &str, client_id: &str) -> Result<()> {
        self.collection.update_one(
            doc! {
                "applicationId": application_id,
                "clientId": client_id
            },
            doc! {
                "$set": {
                    "enabled": false,
                    "updatedAt": mongodb::bson::DateTime::now()
                }
            }
        ).await?;
        Ok(())
    }
}
