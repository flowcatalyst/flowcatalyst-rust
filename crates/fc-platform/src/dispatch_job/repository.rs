//! DispatchJob Repository

use mongodb::{Collection, Database, bson::doc};
use futures::TryStreamExt;
use chrono::{DateTime, Utc};
use crate::{DispatchJob, DispatchJobRead, DispatchStatus};
use crate::shared::error::Result;

pub struct DispatchJobRepository {
    collection: Collection<DispatchJob>,
    read_collection: Collection<DispatchJobRead>,
}

impl DispatchJobRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("dispatch_jobs"),
            read_collection: db.collection("dispatch_jobs_read"),
        }
    }

    pub async fn insert(&self, job: &DispatchJob) -> Result<()> {
        self.collection.insert_one(job).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<DispatchJob>> {
        Ok(self.collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn find_by_event_id(&self, event_id: &str) -> Result<Vec<DispatchJob>> {
        let cursor = self.collection
            .find(doc! { "eventId": event_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_subscription_id(&self, subscription_id: &str, _limit: i64) -> Result<Vec<DispatchJob>> {
        let cursor = self.collection
            .find(doc! { "subscriptionId": subscription_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_status(&self, status: DispatchStatus, _limit: i64) -> Result<Vec<DispatchJob>> {
        let status_str = serde_json::to_string(&status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let cursor = self.collection
            .find(doc! { "status": status_str })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_pending_for_dispatch(&self, _limit: i64) -> Result<Vec<DispatchJob>> {
        let cursor = self.collection
            .find(doc! {
                "status": "PENDING",
                "$or": [
                    { "nextRetryAt": { "$exists": false } },
                    { "nextRetryAt": null },
                    { "nextRetryAt": { "$lte": Utc::now() } }
                ]
            })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_stale_in_progress(&self, stale_threshold: DateTime<Utc>, _limit: i64) -> Result<Vec<DispatchJob>> {
        let cursor = self.collection
            .find(doc! {
                "status": "IN_PROGRESS",
                "updatedAt": { "$lt": stale_threshold }
            })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_client(&self, client_id: &str, _limit: i64) -> Result<Vec<DispatchJob>> {
        let cursor = self.collection
            .find(doc! { "clientId": client_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_correlation_id(&self, correlation_id: &str) -> Result<Vec<DispatchJob>> {
        let cursor = self.collection
            .find(doc! { "correlationId": correlation_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn update(&self, job: &DispatchJob) -> Result<()> {
        self.collection
            .replace_one(doc! { "_id": &job.id }, job)
            .await?;
        Ok(())
    }

    /// Bulk insert multiple dispatch jobs
    pub async fn insert_many(&self, jobs: &[DispatchJob]) -> Result<()> {
        if jobs.is_empty() {
            return Ok(());
        }
        self.collection.insert_many(jobs).await?;
        Ok(())
    }

    pub async fn update_status(&self, id: &str, status: DispatchStatus) -> Result<bool> {
        let status_str = serde_json::to_string(&status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let result = self.collection
            .update_one(
                doc! { "_id": id },
                doc! { "$set": { "status": status_str, "updatedAt": Utc::now() } },
            )
            .await?;
        Ok(result.modified_count > 0)
    }

    // Read projection methods
    pub async fn find_read_by_id(&self, id: &str) -> Result<Option<DispatchJobRead>> {
        Ok(self.read_collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn insert_read_projection(&self, projection: &DispatchJobRead) -> Result<()> {
        self.read_collection.insert_one(projection).await?;
        Ok(())
    }

    pub async fn update_read_projection(&self, projection: &DispatchJobRead) -> Result<()> {
        self.read_collection
            .replace_one(doc! { "_id": &projection.id }, projection)
            .await?;
        Ok(())
    }

    /// Count jobs by status
    pub async fn count_by_status(&self, status: DispatchStatus) -> Result<u64> {
        let status_str = serde_json::to_string(&status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let count = self.collection
            .count_documents(doc! { "status": status_str })
            .await?;
        Ok(count)
    }

    /// Count all jobs
    pub async fn count_all(&self) -> Result<u64> {
        let count = self.collection.count_documents(doc! {}).await?;
        Ok(count)
    }

    /// Find recent dispatch jobs with pagination (for debug/admin)
    pub async fn find_recent_paged(&self, page: u32, size: u32) -> Result<Vec<DispatchJob>> {
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
}
