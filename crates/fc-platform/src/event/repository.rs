//! Event Repository

use mongodb::{Collection, Database, bson::doc};
use futures::TryStreamExt;
use crate::{Event, EventRead};
use crate::shared::error::Result;

pub struct EventRepository {
    collection: Collection<Event>,
    read_collection: Collection<EventRead>,
}

impl EventRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("events"),
            read_collection: db.collection("events_read"),
        }
    }

    pub async fn insert(&self, event: &Event) -> Result<()> {
        self.collection.insert_one(event).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Event>> {
        Ok(self.collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn find_by_type(&self, event_type: &str, _limit: i64) -> Result<Vec<Event>> {
        let cursor = self.collection
            .find(doc! { "type": event_type })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_client(&self, client_id: &str, _limit: i64) -> Result<Vec<Event>> {
        let cursor = self.collection
            .find(doc! { "clientId": client_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_correlation_id(&self, correlation_id: &str) -> Result<Vec<Event>> {
        let cursor = self.collection
            .find(doc! { "correlationId": correlation_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    /// Find event by deduplication ID for exactly-once semantics
    pub async fn find_by_deduplication_id(&self, deduplication_id: &str) -> Result<Option<Event>> {
        Ok(self.collection.find_one(doc! { "deduplicationId": deduplication_id }).await?)
    }

    /// Bulk insert multiple events
    pub async fn insert_many(&self, events: &[Event]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        self.collection.insert_many(events).await?;
        Ok(())
    }

    // Read projection methods
    pub async fn find_read_by_id(&self, id: &str) -> Result<Option<EventRead>> {
        Ok(self.read_collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn insert_read_projection(&self, projection: &EventRead) -> Result<()> {
        self.read_collection.insert_one(projection).await?;
        Ok(())
    }

    pub async fn update_read_projection(&self, projection: &EventRead) -> Result<()> {
        self.read_collection
            .replace_one(doc! { "_id": &projection.id }, projection)
            .await?;
        Ok(())
    }

    /// Find recent events with pagination (for debug/admin)
    pub async fn find_recent_paged(&self, page: u32, size: u32) -> Result<Vec<Event>> {
        use mongodb::options::FindOptions;

        let skip = page as u64 * size as u64;
        let options = FindOptions::builder()
            .skip(skip)
            .limit(size as i64)
            .sort(doc! { "createdAt": -1 })
            .build();

        let cursor = self.collection.find(doc! {}).with_options(options).await?;
        Ok(cursor.try_collect().await?)
    }

    /// Count all events
    pub async fn count_all(&self) -> Result<u64> {
        let count = self.collection.count_documents(doc! {}).await?;
        Ok(count)
    }
}
