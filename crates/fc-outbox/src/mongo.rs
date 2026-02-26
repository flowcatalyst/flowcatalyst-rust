//! MongoDB Outbox Repository Implementation
//!
//! Implements the OutboxRepository trait for MongoDB with a single shared
//! `outbox_messages` collection using a `type` field, matching Java/TypeScript.

use async_trait::async_trait;
use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
use crate::repository::{OutboxRepository, OutboxTableConfig};
use anyhow::Result;
use mongodb::{Client, Collection, Database, IndexModel};
use mongodb::bson::{doc, Document};
use mongodb::options::{FindOptions, IndexOptions};
use chrono::{DateTime, Utc};
use futures::stream::TryStreamExt;
use std::time::Duration;
use tracing::{info, debug};

/// MongoDB implementation of OutboxRepository
pub struct MongoOutboxRepository {
    database: Database,
    table_config: OutboxTableConfig,
}

impl MongoOutboxRepository {
    /// Create a new MongoDB outbox repository with default table config
    pub fn new(client: Client, db_name: &str) -> Self {
        let database = client.database(db_name);
        Self {
            database,
            table_config: OutboxTableConfig::default(),
        }
    }

    /// Create with custom table configuration
    pub fn with_config(client: Client, db_name: &str, table_config: OutboxTableConfig) -> Self {
        let database = client.database(db_name);
        Self { database, table_config }
    }

    /// Get the database reference
    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Get collection for item type
    fn collection_for_type(&self, item_type: OutboxItemType) -> Collection<Document> {
        let name = self.table_config.table_for_type(item_type);
        self.database.collection(name)
    }

    /// Parse a document into an OutboxItem
    fn parse_doc(&self, doc: &Document, item_type: OutboxItemType) -> Result<OutboxItem> {
        let created_at_str = doc.get_str("created_at")?;
        let created_at: DateTime<Utc> = created_at_str.parse()
            .map_err(|e| anyhow::anyhow!("Invalid created_at: {}", e))?;

        let updated_at_str = doc.get_str("updated_at")?;
        let updated_at: DateTime<Utc> = updated_at_str.parse()
            .map_err(|e| anyhow::anyhow!("Invalid updated_at: {}", e))?;

        let status_code = doc.get_i32("status").unwrap_or(0);
        let status = OutboxStatus::from_code(status_code);

        let payload_str = doc.get_str("payload")?;
        let payload: serde_json::Value = serde_json::from_str(payload_str)?;

        Ok(OutboxItem {
            id: doc.get_str("id")?.to_string(),
            item_type,
            message_group: doc.get_str("message_group").ok().map(String::from),
            payload,
            status,
            retry_count: doc.get_i32("retry_count").unwrap_or(0),
            error_message: doc.get_str("error_message").ok().map(String::from),
            created_at,
            updated_at,
            client_id: doc.get_str("client_id").ok().map(String::from),
            payload_size: doc.get_i32("payload_size").ok(),
            headers: doc.get_str("headers").ok().and_then(|s| serde_json::from_str(s).ok()),
        })
    }

    /// Get current ISO 8601 timestamp string
    fn now_iso() -> String {
        Utc::now().to_rfc3339()
    }
}

#[async_trait]
impl OutboxRepository for MongoOutboxRepository {
    async fn fetch_pending_by_type(&self, item_type: OutboxItemType, limit: u32) -> Result<Vec<OutboxItem>> {
        let collection = self.collection_for_type(item_type);
        let filter = doc! {
            "status": OutboxStatus::PENDING.code(),
            "type": item_type.type_value()
        };
        let find_options = FindOptions::builder()
            .sort(doc! { "message_group": 1, "created_at": 1 })
            .limit(limit as i64)
            .build();

        let mut cursor = collection.find(filter).with_options(find_options).await?;
        let mut items = Vec::new();

        while let Some(doc) = cursor.try_next().await? {
            items.push(self.parse_doc(&doc, item_type)?);
        }

        debug!(
            collection = %self.table_config.table_for_type(item_type),
            item_type = %item_type,
            count = items.len(),
            "Fetched pending items"
        );

        Ok(items)
    }

    async fn mark_in_progress(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let collection = self.collection_for_type(item_type);

        let filter = doc! {
            "id": { "$in": &ids },
            "type": item_type.type_value()
        };
        let update = doc! {
            "$set": {
                "status": OutboxStatus::IN_PROGRESS.code(),
                "updated_at": Self::now_iso()
            }
        };

        collection.update_many(filter, update).await?;

        debug!(
            collection = %self.table_config.table_for_type(item_type),
            count = ids.len(),
            "Marked items as IN_PROGRESS"
        );

        Ok(())
    }

    async fn mark_with_status(
        &self,
        item_type: OutboxItemType,
        ids: Vec<String>,
        status: OutboxStatus,
        error_message: Option<String>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let collection = self.collection_for_type(item_type);

        let filter = doc! {
            "id": { "$in": &ids },
            "type": item_type.type_value()
        };
        let mut set_doc = doc! {
            "status": status.code(),
            "updated_at": Self::now_iso()
        };

        if let Some(err) = &error_message {
            set_doc.insert("error_message", err);
        }

        let update = doc! { "$set": set_doc };
        collection.update_many(filter, update).await?;

        debug!(
            collection = %self.table_config.table_for_type(item_type),
            status = ?status,
            count = ids.len(),
            "Marked items with status"
        );

        Ok(())
    }

    async fn increment_retry_count(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let collection = self.collection_for_type(item_type);

        let filter = doc! {
            "id": { "$in": &ids },
            "type": item_type.type_value()
        };
        let update = doc! {
            "$inc": { "retry_count": 1 },
            "$set": {
                "status": OutboxStatus::PENDING.code(),
                "updated_at": Self::now_iso()
            }
        };

        collection.update_many(filter, update).await?;

        debug!(
            collection = %self.table_config.table_for_type(item_type),
            count = ids.len(),
            "Incremented retry count"
        );

        Ok(())
    }

    async fn fetch_recoverable_items(
        &self,
        item_type: OutboxItemType,
        timeout: Duration,
        limit: u32,
    ) -> Result<Vec<OutboxItem>> {
        let collection = self.collection_for_type(item_type);
        let cutoff = (Utc::now() - chrono::Duration::from_std(timeout).unwrap_or_default()).to_rfc3339();

        let filter = doc! {
            "type": item_type.type_value(),
            "status": {
                "$in": [
                    OutboxStatus::IN_PROGRESS.code(),
                    OutboxStatus::BAD_REQUEST.code(),
                    OutboxStatus::INTERNAL_ERROR.code(),
                    OutboxStatus::UNAUTHORIZED.code(),
                    OutboxStatus::FORBIDDEN.code(),
                    OutboxStatus::GATEWAY_ERROR.code(),
                ]
            },
            "updated_at": { "$lt": cutoff }
        };

        let find_options = FindOptions::builder()
            .sort(doc! { "created_at": 1 })
            .limit(limit as i64)
            .build();

        let mut cursor = collection.find(filter).with_options(find_options).await?;
        let mut items = Vec::new();

        while let Some(doc) = cursor.try_next().await? {
            items.push(self.parse_doc(&doc, item_type)?);
        }

        Ok(items)
    }

    async fn reset_recoverable_items(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let collection = self.collection_for_type(item_type);

        let filter = doc! {
            "id": { "$in": &ids },
            "type": item_type.type_value()
        };
        let update = doc! {
            "$set": {
                "status": OutboxStatus::PENDING.code(),
                "updated_at": Self::now_iso()
            }
        };

        collection.update_many(filter, update).await?;

        info!(
            collection = %self.table_config.table_for_type(item_type),
            count = ids.len(),
            "Reset recoverable items to PENDING"
        );

        Ok(())
    }

    async fn fetch_stuck_items(
        &self,
        item_type: OutboxItemType,
        timeout: Duration,
        limit: u32,
    ) -> Result<Vec<OutboxItem>> {
        let collection = self.collection_for_type(item_type);
        let cutoff = (Utc::now() - chrono::Duration::from_std(timeout).unwrap_or_default()).to_rfc3339();

        let filter = doc! {
            "type": item_type.type_value(),
            "status": OutboxStatus::IN_PROGRESS.code(),
            "updated_at": { "$lt": cutoff }
        };

        let find_options = FindOptions::builder()
            .sort(doc! { "created_at": 1 })
            .limit(limit as i64)
            .build();

        let mut cursor = collection.find(filter).with_options(find_options).await?;
        let mut items = Vec::new();

        while let Some(doc) = cursor.try_next().await? {
            items.push(self.parse_doc(&doc, item_type)?);
        }

        Ok(items)
    }

    async fn reset_stuck_items(&self, item_type: OutboxItemType, ids: Vec<String>) -> Result<()> {
        self.reset_recoverable_items(item_type, ids).await
    }

    async fn init_schema(&self) -> Result<()> {
        // Create indexes for each unique collection
        for table_name in self.table_config.unique_tables() {
            let collection: Collection<Document> = self.database.collection(table_name);

            let pending_index = IndexModel::builder()
                .keys(doc! { "status": 1, "type": 1, "message_group": 1, "created_at": 1 })
                .options(IndexOptions::builder().name("idx_pending".to_string()).build())
                .build();
            let stuck_index = IndexModel::builder()
                .keys(doc! { "status": 1, "type": 1, "created_at": 1 })
                .options(IndexOptions::builder().name("idx_stuck".to_string()).build())
                .build();
            let client_index = IndexModel::builder()
                .keys(doc! { "client_id": 1, "status": 1, "created_at": 1 })
                .options(IndexOptions::builder().name("idx_client_pending".to_string()).build())
                .build();

            collection.create_indexes([pending_index, stuck_index, client_index]).await?;
        }

        info!(
            collections = ?self.table_config.unique_tables(),
            "Initialized MongoDB outbox indexes"
        );

        Ok(())
    }

    fn table_config(&self) -> &OutboxTableConfig {
        &self.table_config
    }
}
