//! EventType Repository

use mongodb::{Collection, Database, bson::doc};
use futures::TryStreamExt;
use crate::EventType;
use crate::shared::error::Result;

pub struct EventTypeRepository {
    collection: Collection<EventType>,
}

impl EventTypeRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("event_types"),
        }
    }

    pub async fn insert(&self, event_type: &EventType) -> Result<()> {
        self.collection.insert_one(event_type).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<EventType>> {
        Ok(self.collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<EventType>> {
        Ok(self.collection.find_one(doc! { "code": code }).await?)
    }

    pub async fn find_active(&self) -> Result<Vec<EventType>> {
        let cursor = self.collection
            .find(doc! { "status": "ACTIVE" })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_application(&self, application: &str) -> Result<Vec<EventType>> {
        let cursor = self.collection
            .find(doc! { "application": application })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_client(&self, client_id: Option<&str>) -> Result<Vec<EventType>> {
        let filter = match client_id {
            Some(id) => doc! { "$or": [{ "clientId": id }, { "clientId": null }] },
            None => doc! { "clientId": null },
        };
        let cursor = self.collection.find(filter).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn update(&self, event_type: &EventType) -> Result<()> {
        self.collection
            .replace_one(doc! { "_id": &event_type.id }, event_type)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = self.collection.delete_one(doc! { "_id": id }).await?;
        Ok(result.deleted_count > 0)
    }
}
