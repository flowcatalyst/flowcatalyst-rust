//! Audit Log Repository

use mongodb::{Collection, Database, bson::doc, options::FindOptions};
use futures::TryStreamExt;
use crate::AuditLog;
use crate::shared::error::Result;

pub struct AuditLogRepository {
    collection: Collection<AuditLog>,
}

impl AuditLogRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("audit_logs"),
        }
    }

    pub async fn insert(&self, log: &AuditLog) -> Result<()> {
        self.collection.insert_one(log).await?;
        Ok(())
    }

    pub async fn insert_many(&self, logs: &[AuditLog]) -> Result<usize> {
        if logs.is_empty() {
            return Ok(0);
        }
        let result = self.collection.insert_many(logs).await?;
        Ok(result.inserted_ids.len())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<AuditLog>> {
        Ok(self.collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn find_by_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
        limit: i64,
    ) -> Result<Vec<AuditLog>> {
        let options = FindOptions::builder()
            .sort(doc! { "performedAt": -1 })
            .limit(limit)
            .build();

        let cursor = self.collection
            .find(doc! { "entityType": entity_type, "entityId": entity_id })
            .with_options(options)
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_principal(
        &self,
        principal_id: &str,
        limit: i64,
    ) -> Result<Vec<AuditLog>> {
        let options = FindOptions::builder()
            .sort(doc! { "performedAt": -1 })
            .limit(limit)
            .build();

        let cursor = self.collection
            .find(doc! { "principalId": principal_id })
            .with_options(options)
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_recent(&self, limit: i64) -> Result<Vec<AuditLog>> {
        let options = FindOptions::builder()
            .sort(doc! { "performedAt": -1 })
            .limit(limit)
            .build();

        let cursor = self.collection.find(doc! {}).with_options(options).await?;
        Ok(cursor.try_collect().await?)
    }

    /// Search audit logs with filters (matches Java schema)
    pub async fn search(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        operation: Option<&str>,
        principal_id: Option<&str>,
        skip: u64,
        limit: i64,
    ) -> Result<Vec<AuditLog>> {
        let mut filter = doc! {};

        if let Some(et) = entity_type {
            filter.insert("entityType", et);
        }
        if let Some(eid) = entity_id {
            filter.insert("entityId", eid);
        }
        if let Some(op) = operation {
            filter.insert("operation", op);
        }
        if let Some(pid) = principal_id {
            filter.insert("principalId", pid);
        }

        let options = FindOptions::builder()
            .sort(doc! { "performedAt": -1 })
            .skip(skip)
            .limit(limit)
            .build();

        let cursor = self.collection.find(filter).with_options(options).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn count(&self) -> Result<u64> {
        Ok(self.collection.count_documents(doc! {}).await?)
    }

    /// Count audit logs with filters (for pagination)
    pub async fn count_with_filters(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        operation: Option<&str>,
        principal_id: Option<&str>,
    ) -> Result<i64> {
        let mut filter = doc! {};

        if let Some(et) = entity_type {
            filter.insert("entityType", et);
        }
        if let Some(eid) = entity_id {
            filter.insert("entityId", eid);
        }
        if let Some(op) = operation {
            filter.insert("operation", op);
        }
        if let Some(pid) = principal_id {
            filter.insert("principalId", pid);
        }

        Ok(self.collection.count_documents(filter).await? as i64)
    }

    /// Find distinct entity types
    pub async fn find_distinct_entity_types(&self) -> Result<Vec<String>> {
        let values = self.collection.distinct("entityType", doc! {}).await?;
        Ok(values.into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect())
    }

    /// Find distinct operations (matches Java schema)
    pub async fn find_distinct_operations(&self) -> Result<Vec<String>> {
        let values = self.collection.distinct("operation", doc! {}).await?;
        Ok(values.into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect())
    }
}
