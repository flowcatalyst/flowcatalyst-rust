//! MongoDB Outbox Repository Implementation
//!
//! Go's document shape (`flowcatalyst-go/internal/outbox/mongo`): snake_case
//! fields, the row id in `id`, `status` and `retry_count` as integer codes,
//! `payload` as a JSON string, timestamps as RFC 3339 strings (UTC, `Z`).
//!
//! Go claims with a find followed by an update, which is not atomic. Here each
//! document is claimed with `findOneAndUpdate` (PENDING → IN_PROGRESS in one
//! step), so two processors never claim the same document.

use crate::repository::{parse_text_timestamp, ClaimedBatch, OutboxRepository, OutboxTableConfig};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use fc_common::{OutboxItemType, OutboxStatus};
use mongodb::bson::{doc, Bson, Document};
use mongodb::options::{FindOneAndUpdateOptions, IndexOptions, ReturnDocument};
use mongodb::{Client, Collection, Database, IndexModel};
use std::time::Duration;
use tracing::info;

/// MongoDB implementation of OutboxRepository
pub struct MongoOutboxRepository {
    database: Database,
    table_config: OutboxTableConfig,
}

/// Timestamps as Go writes them (`time.RFC3339`, UTC), so they compare as
/// text with Go-written documents.
fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn int_field(doc: &Document, key: &str) -> i64 {
    match doc.get(key) {
        Some(Bson::Int32(v)) => *v as i64,
        Some(Bson::Int64(v)) => *v,
        Some(Bson::Double(v)) => *v as i64,
        _ => 0,
    }
}

fn str_field(doc: &Document, key: &str) -> Option<String> {
    match doc.get(key) {
        Some(Bson::String(s)) => Some(s.clone()),
        _ => None,
    }
}

impl MongoOutboxRepository {
    /// Create a new MongoDB outbox repository with default table config
    pub fn new(client: Client, db_name: &str) -> Self {
        Self {
            database: client.database(db_name),
            table_config: OutboxTableConfig::default(),
        }
    }

    /// Create with custom table configuration
    pub fn with_config(client: Client, db_name: &str, table_config: OutboxTableConfig) -> Self {
        Self {
            database: client.database(db_name),
            table_config,
        }
    }

    /// Get the database reference
    pub fn database(&self) -> &Database {
        &self.database
    }

    fn collection(&self, name: &str) -> Collection<Document> {
        self.database.collection(name)
    }

    fn collection_for_type(&self, item_type: OutboxItemType) -> Collection<Document> {
        self.collection(self.table_config.table_for_type(item_type))
    }
}

#[async_trait]
impl OutboxRepository for MongoOutboxRepository {
    async fn claim_pending(&self, limit: u32) -> Result<ClaimedBatch> {
        let mut batch = ClaimedBatch::default();
        for (name, types) in self.table_config.tables_with_types() {
            let collection = self.collection(name);
            let type_names: Vec<&str> = types.iter().map(|t| t.as_str()).collect();
            let options = FindOneAndUpdateOptions::builder()
                .sort(doc! { "message_group": 1, "created_at": 1, "id": 1 })
                .return_document(ReturnDocument::After)
                .build();
            while batch.len() < limit as usize {
                let claimed = collection
                    .find_one_and_update(
                        doc! {
                            "status": OutboxStatus::Pending.code(),
                            "type": { "$in": &type_names },
                        },
                        doc! { "$set": {
                            "status": OutboxStatus::InProgress.code(),
                            "updated_at": now_iso(),
                        } },
                    )
                    .with_options(options.clone())
                    .await?;
                let Some(d) = claimed else { break };

                let payload = match d.get("payload") {
                    Some(Bson::String(s)) => s.clone(),
                    Some(other) => other.clone().into_relaxed_extjson().to_string(),
                    None => String::new(),
                };
                let type_str = str_field(&d, "type").unwrap_or_default();
                batch.push_row(
                    str_field(&d, "id").unwrap_or_default(),
                    type_str.parse()?,
                    str_field(&d, "message_group"),
                    &payload,
                    int_field(&d, "retry_count") as i32,
                    str_field(&d, "error_message").filter(|e| !e.is_empty()),
                    parse_text_timestamp(&str_field(&d, "created_at").unwrap_or_default()),
                    parse_text_timestamp(&str_field(&d, "updated_at").unwrap_or_default()),
                );
            }
        }
        Ok(batch)
    }

    async fn mark_success(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.collection_for_type(item_type)
            .delete_many(doc! { "id": { "$in": ids } })
            .await?;
        Ok(())
    }

    async fn mark_failed(
        &self,
        item_type: OutboxItemType,
        ids: &[String],
        status: OutboxStatus,
        error_message: &str,
        requeue: bool,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let new_status = if requeue {
            OutboxStatus::Pending
        } else {
            status
        };
        self.collection_for_type(item_type)
            .update_many(
                doc! { "id": { "$in": ids } },
                doc! {
                    "$set": {
                        "status": new_status.code(),
                        "error_message": error_message,
                        "updated_at": now_iso(),
                    },
                    "$inc": { "retry_count": 1 },
                },
            )
            .await?;
        Ok(())
    }

    async fn release(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.collection_for_type(item_type)
            .update_many(
                doc! { "id": { "$in": ids }, "status": OutboxStatus::InProgress.code() },
                doc! { "$set": { "status": OutboxStatus::Pending.code(), "updated_at": now_iso() } },
            )
            .await?;
        Ok(())
    }

    async fn requeue(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.collection_for_type(item_type)
            .update_many(
                doc! { "id": { "$in": ids } },
                doc! { "$set": {
                    "status": OutboxStatus::Pending.code(),
                    "retry_count": 0,
                    "error_message": "",
                    "updated_at": now_iso(),
                } },
            )
            .await?;
        Ok(())
    }

    async fn recover_stuck(&self, older_than: Duration) -> Result<u64> {
        // RFC 3339 UTC strings sort as text (Go compares them the same way).
        let cutoff = (Utc::now() - chrono::Duration::from_std(older_than)?)
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut total = 0;
        for name in self.table_config.unique_tables() {
            total += self
                .collection(name)
                .update_many(
                    doc! {
                        "status": OutboxStatus::InProgress.code(),
                        "updated_at": { "$lt": &cutoff },
                    },
                    doc! { "$set": { "status": OutboxStatus::Pending.code(), "updated_at": now_iso() } },
                )
                .await?
                .modified_count;
        }
        Ok(total)
    }

    async fn init_schema(&self) -> Result<()> {
        for name in self.table_config.unique_tables() {
            let index = |keys: Document, name: &str| {
                IndexModel::builder()
                    .keys(keys)
                    .options(IndexOptions::builder().name(name.to_string()).build())
                    .build()
            };
            self.collection(name)
                .create_indexes([
                    index(
                        doc! { "status": 1, "type": 1, "message_group": 1, "created_at": 1 },
                        "idx_pending",
                    ),
                    index(
                        doc! { "status": 1, "type": 1, "created_at": 1 },
                        "idx_stuck",
                    ),
                    index(
                        doc! { "client_id": 1, "status": 1, "created_at": 1 },
                        "idx_client_pending",
                    ),
                ])
                .await?;
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
